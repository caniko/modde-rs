use super::*;

pub(super) fn requests_by_normalized_path(
    requests: &[ArchiveRequest],
) -> HashMap<String, Vec<ArchiveRequest>> {
    let mut by_path: HashMap<String, Vec<ArchiveRequest>> = HashMap::new();
    for request in requests {
        by_path
            .entry(normalize_path(&request.from).to_lowercase())
            .or_default()
            .push(request.clone());
    }
    by_path
}

pub(super) fn satisfy_requests_from_reader(
    input: &mut dyn std::io::Read,
    requests: &[ArchiveRequest],
    output: &mut ArchiveBatchOutput,
) -> std::io::Result<()> {
    if requests.iter().any(|request| request.inner_path.is_some()) {
        let mut data = Vec::new();
        input.read_to_end(&mut data)?;
        satisfy_maybe_nested_requests_from_bytes(&data, requests, output)?;
        return Ok(());
    }

    if requests
        .iter()
        .any(|request| matches!(request.kind, ArchiveRequestKind::Bytes))
    {
        let data = read_to_vec(input, expected_write_size(requests)?)?;
        satisfy_requests_from_bytes(&data, requests, output)?;
        return Ok(());
    }

    let mut writers = Vec::new();
    for request in requests {
        if let ArchiveRequestKind::WriteFile { to, .. } = &request.kind {
            writers.push(File::create(to)?);
        }
    }
    copy_streaming_to_many(input, &mut writers, expected_write_size(requests)?)?;
    Ok(())
}

pub(super) fn satisfy_requests_from_bytes(
    data: &[u8],
    requests: &[ArchiveRequest],
    output: &mut ArchiveBatchOutput,
) -> std::io::Result<()> {
    if requests.iter().any(|request| request.inner_path.is_some()) {
        satisfy_maybe_nested_requests_from_bytes(data, requests, output)?;
        return Ok(());
    }

    for request in requests {
        match &request.kind {
            ArchiveRequestKind::WriteFile { to, expected_size } => {
                validate_final_output_size(data.len() as u64, *expected_size)?;
                std::fs::write(to, data)?;
            }
            ArchiveRequestKind::Bytes => {
                output.bytes.insert(request.directive_index, data.to_vec());
            }
        }
    }
    Ok(())
}

pub(super) fn satisfy_maybe_nested_requests_from_bytes(
    data: &[u8],
    requests: &[ArchiveRequest],
    output: &mut ArchiveBatchOutput,
) -> std::io::Result<()> {
    let mut direct_requests = Vec::new();
    let mut nested_requests = Vec::new();

    for request in requests {
        if let Some(inner_path) = &request.inner_path {
            nested_requests.push(ArchiveRequest {
                directive_index: request.directive_index,
                from: inner_path.clone(),
                inner_path: None,
                kind: request.kind.clone(),
            });
        } else {
            direct_requests.push(request.clone());
        }
    }

    if !direct_requests.is_empty() {
        satisfy_requests_from_bytes(data, &direct_requests, output)?;
    }

    if nested_requests.is_empty() {
        return Ok(());
    }

    let nested_output = if bytes_have_bethesda_magic(data) {
        let mut temp = tempfile::NamedTempFile::new()?;
        temp.write_all(data)?;
        temp.flush()?;
        ArchiveBatchExtractor::extract_selected(temp.path(), &nested_requests)
    } else {
        ArchiveBatchExtractor::extract_selected_from(
            ArchiveInput::Bytes {
                name: "nested archive",
                bytes: data,
            },
            &nested_requests,
        )
    }
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e:#}")))?;

    output.bytes.extend(nested_output.bytes);
    Ok(())
}

pub(super) fn copy_streaming_to_many(
    input: &mut dyn std::io::Read,
    outputs: &mut [File],
    expected_size: Option<u64>,
) -> std::io::Result<u64> {
    let mut buf = vec![0_u8; COPY_BUFFER];
    let mut total = 0_u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        validate_output_size(total, expected_size)?;
        for output in outputs.iter_mut() {
            output.write_all(&buf[..n])?;
        }
    }
    for output in outputs {
        output.flush()?;
    }
    validate_final_output_size(total, expected_size)?;
    Ok(total)
}

pub(super) fn read_to_vec(
    input: &mut dyn std::io::Read,
    expected_size: Option<u64>,
) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0_u8; COPY_BUFFER];
    let mut data = Vec::new();
    let mut total = 0_u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        validate_output_size(total, expected_size)?;
        data.extend_from_slice(&buf[..n]);
    }
    validate_final_output_size(total, expected_size)?;
    Ok(data)
}

pub(super) fn expected_write_size(requests: &[ArchiveRequest]) -> std::io::Result<Option<u64>> {
    let mut expected = None;
    for request in requests {
        let ArchiveRequestKind::WriteFile {
            expected_size: Some(size),
            ..
        } = request.kind
        else {
            continue;
        };
        if let Some(previous) = expected
            && previous != size
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "duplicate archive entry has inconsistent expected sizes: {previous} and {size}"
                ),
            ));
        }
        expected = Some(size);
    }
    Ok(expected)
}

pub(super) fn validate_declared_entry_size(
    actual: u64,
    requests: &[ArchiveRequest],
) -> std::io::Result<()> {
    if let Some(expected) = expected_write_size(requests)?
        && actual != expected
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("archive entry size mismatch: expected {expected}, got {actual}"),
        ));
    }
    Ok(())
}

pub(super) fn validate_output_size(actual: u64, expected: Option<u64>) -> std::io::Result<()> {
    if let Some(expected) = expected
        && actual > expected
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "archive entry output exceeds expected size: expected {expected}, got {actual}"
            ),
        ));
    }
    Ok(())
}

pub(super) fn validate_final_output_size(
    actual: u64,
    expected: Option<u64>,
) -> std::io::Result<()> {
    validate_output_size(actual, expected)?;
    if let Some(expected) = expected
        && actual != expected
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("archive entry output size mismatch: expected {expected}, got {actual}"),
        ));
    }
    Ok(())
}

#[cfg(any(feature = "bethesda-archives", feature = "rar"))]
pub(super) struct SizeCheckedWriter<W> {
    inner: W,
    expected_size: Option<u64>,
    written: u64,
}

#[cfg(any(feature = "bethesda-archives", feature = "rar"))]
impl<W> SizeCheckedWriter<W> {
    pub(super) fn new(inner: W, expected_size: Option<u64>) -> Self {
        Self {
            inner,
            expected_size,
            written: 0,
        }
    }

    pub(super) fn finish(&self) -> std::io::Result<()> {
        validate_final_output_size(self.written, self.expected_size)
    }
}

#[cfg(any(feature = "bethesda-archives", feature = "rar"))]
impl<W: std::io::Write> std::io::Write for SizeCheckedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let next = self.written.saturating_add(buf.len() as u64);
        validate_output_size(next, self.expected_size)?;
        let written = self.inner.write(buf)?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub(super) fn ensure_all_found(
    label: &str,
    requests: &[ArchiveRequest],
    found: &HashSet<usize>,
) -> Result<()> {
    let missing: Vec<&ArchiveRequest> = requests
        .iter()
        .filter(|request| !found.contains(&request.directive_index))
        .collect();
    if !missing.is_empty() {
        let mut unique = BTreeMap::<&str, usize>::new();
        for request in &missing {
            *unique.entry(request.from.as_str()).or_default() += 1;
        }
        let shown = unique
            .into_iter()
            .map(|(path, count)| {
                if count == 1 {
                    path.to_string()
                } else {
                    format!("{path} (x{count})")
                }
            })
            .collect::<Vec<_>>();
        bail!(
            "{} requested entr{} missing from {} ({} unique): {}",
            missing.len(),
            if missing.len() == 1 { "y" } else { "ies" },
            label,
            shown.len(),
            shown.join(", ")
        );
    }
    Ok(())
}

pub(super) fn has_rar_magic(path: &Path) -> std::io::Result<bool> {
    let mut file = File::open(path)?;
    let mut magic = [0_u8; 8];
    let len = file.read(&mut magic)?;
    Ok(magic[..len].starts_with(b"Rar!\x1A\x07\x00")
        || magic[..len].starts_with(b"Rar!\x1A\x07\x01\x00"))
}

pub(super) fn has_zip_magic(path: &Path) -> std::io::Result<bool> {
    let mut file = File::open(path)?;
    let mut magic = [0_u8; 4];
    let len = file.read(&mut magic)?;
    Ok(bytes_have_zip_magic(&magic[..len]))
}

pub(super) fn bytes_have_zip_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
}

pub(super) fn bytes_have_bethesda_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"BSA\0") || bytes.starts_with(b"BTDX")
}

pub(super) fn validate_zip_entry<R: std::io::Read + ?Sized>(
    entry: &zip::read::ZipFile<'_, R>,
) -> Result<()> {
    validate_archive_entry(entry.name())?;
    if entry.is_symlink() {
        bail!("archive entry is a symlink (rejected): {}", entry.name());
    }
    Ok(())
}

pub(super) fn validate_archive_entry(name: &str) -> Result<()> {
    let normalized = normalize_path(name);
    if normalized.starts_with('/') {
        bail!("archive entry contains absolute path: {name}");
    }
    if normalized.split('/').any(|component| component == "..") {
        bail!("archive entry contains path traversal: {name}");
    }
    Ok(())
}

pub(super) fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}
