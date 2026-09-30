#![allow(clippy::wildcard_imports)]
//! Selective archive extraction: pull a chosen set of entries out of `zip`,
//! `7z`, Bethesda (`BSA`/`BA2`), and (optionally) `rar` archives in one pass,
//! validating sizes and rejecting unsafe entry paths.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{Cursor, Read as _, Seek, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const COPY_BUFFER: usize = 1 << 20;

mod requests;
use requests::*;

/// How a single extracted entry should be delivered: written to disk or
/// returned as in-memory bytes.
#[derive(Debug, Clone)]
pub enum ArchiveRequestKind {
    WriteFile {
        to: PathBuf,
        expected_size: Option<u64>,
    },
    Bytes,
}

/// One requested entry: the archive path to extract, an optional nested inner
/// path, and how to deliver it. `directive_index` ties the result back to the
/// caller's request.
#[derive(Debug, Clone)]
pub struct ArchiveRequest {
    pub directive_index: usize,
    pub from: String,
    pub inner_path: Option<String>,
    pub kind: ArchiveRequestKind,
}

/// Results of a batch extraction: in-memory bytes keyed by `directive_index`
/// for entries requested as [`ArchiveRequestKind::Bytes`].
#[derive(Debug, Default)]
pub struct ArchiveBatchOutput {
    pub bytes: HashMap<usize, Vec<u8>>,
}

/// Source of an archive to extract from: a file path or in-memory bytes.
pub enum ArchiveInput<'a> {
    Path(&'a Path),
    Bytes { name: &'a str, bytes: &'a [u8] },
}

/// Stateless entry point for selective, single-pass archive extraction.
pub struct ArchiveBatchExtractor;

impl ArchiveBatchExtractor {
    /// Extract the requested entries from the archive at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported formats, unsafe entry paths, size
    /// mismatches, or any requested entry that is missing.
    pub fn extract_selected(
        path: &Path,
        requests: &[ArchiveRequest],
    ) -> Result<ArchiveBatchOutput> {
        Self::extract_selected_from(ArchiveInput::Path(path), requests)
    }

    /// Extract the requested entries from `input` (a path or in-memory bytes).
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported formats, unsafe entry paths, size
    /// mismatches, or any requested entry that is missing.
    pub fn extract_selected_from(
        input: ArchiveInput<'_>,
        requests: &[ArchiveRequest],
    ) -> Result<ArchiveBatchOutput> {
        for request in requests {
            validate_archive_entry(&request.from)?;
            if let Some(inner_path) = &request.inner_path {
                validate_archive_entry(inner_path)?;
            }
            if let ArchiveRequestKind::WriteFile { to, .. } = &request.kind
                && let Some(parent) = to.parent()
            {
                std::fs::create_dir_all(parent)?;
            }
        }

        match input {
            ArchiveInput::Path(path) => {
                if has_zip_magic(path).unwrap_or(false) {
                    return extract_zip(File::open(path)?, path.display().to_string(), requests);
                }

                let mut magic = [0; 4];
                if File::open(path)?.read_exact(&mut magic).is_ok()
                    && requests::bytes_have_bethesda_magic(&magic) {
                    #[cfg(feature = "bethesda-archives")]
                    return extract_bethesda(path, requests);
                    #[cfg(not(feature = "bethesda-archives"))]
                    bail!("BSA/BA2 extraction requires the bethesda-archives feature");
                }

                #[cfg(feature = "rar")]
                if has_rar_magic(path).unwrap_or(false) {
                    return extract_rar(path, requests);
                }

                #[cfg(not(feature = "rar"))]
                if has_rar_magic(path).unwrap_or(false) {
                    bail!(
                        "RAR archive detected but modde-sources was built without the rar feature"
                    );
                }

                if sevenz_rust2::Archive::open(path).is_ok() {
                    return extract_seven_z(
                        File::open(path)?,
                        path.display().to_string(),
                        requests,
                    );
                }

                bail!(
                    "unsupported archive format for {}; supported: zip, 7z{}{}",
                    path.display(),
                    if cfg!(feature = "bethesda-archives") { ", BSA, BA2" } else { "" },
                    if cfg!(feature = "rar") { ", rar" } else { "" }
                )
            }
            ArchiveInput::Bytes { name, bytes } => {
                if bytes_have_zip_magic(bytes) {
                    return extract_zip(Cursor::new(bytes), name.to_string(), requests);
                }

                if let Ok(output) = extract_seven_z(Cursor::new(bytes), name.to_string(), requests)
                {
                    return Ok(output);
                }

                bail!(
                    "unsupported in-memory archive format for {name}; supported in memory: zip, 7z"
                )
            }
        }
    }
}

fn extract_zip<R: std::io::Read + Seek>(
    reader: R,
    label: String,
    requests: &[ArchiveRequest],
) -> Result<ArchiveBatchOutput> {
    let by_path = requests_by_normalized_path(requests);
    let mut archive = zip::ZipArchive::new(reader)
        .with_context(|| format!("failed to read zip archive {label}"))?;
    let mut output = ArchiveBatchOutput::default();
    let mut found = HashSet::new();

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        validate_zip_entry(&entry)?;
        let key = normalize_path(entry.name()).to_lowercase();
        let Some(matched_requests) = by_path.get(&key) else {
            continue;
        };

        if matched_requests
            .iter()
            .all(|request| request.inner_path.is_none())
        {
            validate_declared_entry_size(entry.size(), matched_requests)?;
        }

        if matched_requests
            .iter()
            .any(|request| request.inner_path.is_some())
        {
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;
            satisfy_maybe_nested_requests_from_bytes(&data, matched_requests, &mut output)?;
        } else if matched_requests
            .iter()
            .any(|request| matches!(request.kind, ArchiveRequestKind::Bytes))
        {
            let data = read_to_vec(&mut entry, expected_write_size(matched_requests)?)?;
            satisfy_requests_from_bytes(&data, matched_requests, &mut output)?;
        } else {
            let mut writers = Vec::new();
            for request in matched_requests {
                if let ArchiveRequestKind::WriteFile { to, .. } = &request.kind {
                    writers
                        .push(File::create(to).with_context(|| {
                            format!("failed to create output {}", to.display())
                        })?);
                }
            }
            copy_streaming_to_many(
                &mut entry,
                &mut writers,
                expected_write_size(matched_requests)?,
            )?;
        }

        found.extend(
            matched_requests
                .iter()
                .map(|request| request.directive_index),
        );
        if found.len() == requests.len() {
            break;
        }
    }

    ensure_all_found(&label, requests, &found)?;
    Ok(output)
}

#[cfg(feature = "bethesda-archives")]
fn extract_bethesda(path: &Path, requests: &[ArchiveRequest]) -> Result<ArchiveBatchOutput> {
    let index = modde_core::bethesda_archive::ArchiveIndex::read(path)
        .with_context(|| format!("failed to read Bethesda archive {}", path.display()))?;
    let mut output = ArchiveBatchOutput::default();
    for request in requests {
        if request.inner_path.is_some() {
            let data = index.extract_file(&request.from)?;
            satisfy_maybe_nested_requests_from_bytes(
                &data,
                std::slice::from_ref(request),
                &mut output,
            )?;
            continue;
        }
        match &request.kind {
            ArchiveRequestKind::WriteFile { to, expected_size } => {
                let mut out = File::create(to)
                    .with_context(|| format!("failed to create output {}", to.display()))?;
                let mut checked = SizeCheckedWriter::new(&mut out, *expected_size);
                index.extract_file_to_writer(&request.from, &mut checked)?;
                checked.finish()?;
            }
            ArchiveRequestKind::Bytes => {
                let data = index.extract_file(&request.from)?;
                output.bytes.insert(request.directive_index, data);
            }
        }
    }
    Ok(output)
}

fn extract_seven_z<R: std::io::Read + Seek>(
    reader: R,
    label: String,
    requests: &[ArchiveRequest],
) -> Result<ArchiveBatchOutput> {
    let by_path = requests_by_normalized_path(requests);
    let mut output = ArchiveBatchOutput::default();
    let mut found = HashSet::new();
    let mut reader = sevenz_rust2::ArchiveReader::new(reader, sevenz_rust2::Password::empty())
        .with_context(|| format!("failed to open 7z archive {label}"))?;

    reader.for_each_entries(|entry, input| {
        let key = normalize_path(entry.name()).to_lowercase();
        let Some(matched_requests) = by_path.get(&key) else {
            std::io::copy(input, &mut std::io::sink())
                .map_err(|e| sevenz_rust2::Error::Io(e, "drain skipped entry".into()))?;
            return Ok(true);
        };
        if matched_requests
            .iter()
            .all(|request| request.inner_path.is_none())
        {
            validate_declared_entry_size(entry.size(), matched_requests)
                .map_err(|e| sevenz_rust2::Error::Io(e, "validate matched entry size".into()))?;
        }
        satisfy_requests_from_reader(input, matched_requests, &mut output)
            .map_err(|e| sevenz_rust2::Error::Io(e, "extract matched entry".into()))?;
        found.extend(
            matched_requests
                .iter()
                .map(|request| request.directive_index),
        );
        Ok(true)
    })?;

    ensure_all_found(&label, requests, &found)?;
    Ok(output)
}

#[cfg(feature = "rar")]
fn extract_rar(path: &Path, requests: &[ArchiveRequest]) -> Result<ArchiveBatchOutput> {
    let by_path = requests_by_normalized_path(requests);
    let mut output = ArchiveBatchOutput::default();
    let mut found = HashSet::new();
    let mut archive = unrar::Archive::new(path)
        .open_for_processing()
        .with_context(|| format!("failed to open RAR archive {}", path.display()))?;

    while let Some(header) = archive.read_header()? {
        let key = normalize_path(&header.entry().filename.to_string_lossy()).to_lowercase();
        let Some(matched_requests) = by_path.get(&key) else {
            archive = header.skip()?;
            continue;
        };

        let (data, next) = header.read()?;
        satisfy_requests_from_bytes(&data, matched_requests, &mut output)?;
        found.extend(
            matched_requests
                .iter()
                .map(|request| request.directive_index),
        );
        archive = next;
        if found.len() == requests.len() {
            break;
        }
    }

    ensure_all_found(&path.display().to_string(), requests, &found)?;
    Ok(output)
}

#[cfg(test)]
mod tests;
