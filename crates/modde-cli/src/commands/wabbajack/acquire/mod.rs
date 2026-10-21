#![allow(clippy::wildcard_imports)]
//! Wabbajack missing-archive acquisition commands.

use super::*;

mod browser;

use browser::acquire_missing_with_browser_controller;

pub(crate) async fn acquire_missing(
    manifest_path: PathBuf,
    download_dir: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    browser_profile: Option<PathBuf>,
    include_nexus: bool,
    browser_controller: bool,
    timeout_secs: u64,
    json: bool,
) -> Result<Vec<AcquireResult>> {
    let manifest = parse_wabbajack_manifest(&manifest_path)?;
    let data_dir = data_dir.unwrap_or_else(modde_core::paths::modde_data_dir);
    let store_dir = data_dir.join("store");
    let download_dir = download_dir.unwrap_or_else(|| data_dir.join("downloads"));
    tokio::fs::create_dir_all(&download_dir).await?;
    tokio::fs::create_dir_all(&store_dir).await?;

    if browser_profile.is_some() && !browser_controller && !json {
        eprintln!(
            "browser-profile is recorded for operator context; modde uses the system browser opener"
        );
    }

    let missing = missing_archives(&manifest, &store_dir, include_nexus);
    if browser_controller {
        let results = acquire_missing_with_browser_controller(
            &manifest,
            &store_dir,
            &download_dir,
            &data_dir,
            browser_profile.as_deref(),
            missing,
            Duration::from_secs(timeout_secs),
            json,
        )
        .await?;
        if json {
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        return Ok(results);
    }

    let mut results = Vec::with_capacity(missing.len());
    for archive in missing {
        let result = match archive.source_kind {
            MissingArchiveSourceKind::Manual => {
                match try_acquire_manual_direct(&manifest, &store_dir, &download_dir, &archive)
                    .await?
                {
                    DirectAcquireOutcome::Resolved(result)
                    | DirectAcquireOutcome::Final(result) => {
                        if !json {
                            print_acquire_result(&result);
                        }
                        results.push(result);
                        continue;
                    }
                    DirectAcquireOutcome::NeedsBrowser {
                        archive: _,
                        message,
                    } => {
                        if !json {
                            eprintln!(
                                "browser-required {:016x} {} message={}",
                                archive.hash, archive.name, message
                            );
                        }
                    }
                    DirectAcquireOutcome::Unsupported => {}
                }
                acquire_manual_archive(
                    &manifest,
                    &store_dir,
                    &download_dir,
                    &archive,
                    browser_profile.as_deref(),
                    Duration::from_secs(timeout_secs),
                    json,
                )
                .await?
            }
            MissingArchiveSourceKind::Nexus => {
                acquire_nexus_archive(&manifest, &store_dir, &download_dir, &archive, json).await?
            }
        };
        if !json {
            print_acquire_result(&result);
        }
        results.push(result);
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    }
    Ok(results)
}

async fn acquire_manual_archive(
    manifest: &WabbajackManifest,
    store_dir: &Path,
    download_dir: &Path,
    archive: &AcquireMissingArchive,
    browser_profile: Option<&Path>,
    timeout: Duration,
    json: bool,
) -> Result<AcquireResult> {
    let Some(url) = archive.url.as_deref() else {
        return Ok(acquire_message(
            archive,
            AcquireStatus::UnsupportedSource,
            "manual archive has no URL",
        ));
    };

    if !json {
        eprintln!(
            "opened-browser {:016x} {} -> {}",
            archive.hash, archive.name, url
        );
        if let Some(profile) = browser_profile {
            eprintln!("browser-profile hint: {}", profile.display());
        }
    }
    if let Err(err) = open::that(url) {
        eprintln!("browser-open-failed {url}: {err:#}");
        eprintln!("open this URL manually, then save the archive into the watched directory");
    }

    if !json {
        eprintln!(
            "waiting-for-download {:016x} {} in {}",
            archive.hash,
            archive.name,
            download_dir.display()
        );
    }

    match wait_for_matching_download(download_dir, archive, timeout).await {
        Ok(found) if found.matched => {
            import_acquired_archive(manifest, store_dir, archive, &found.path).await
        }
        Ok(found) => Ok(AcquireResult {
            archive: archive.clone(),
            status: AcquireStatus::Mismatched,
            path: Some(found.path),
            computed_xxh64: Some(found.computed_xxh64),
            message: Some("downloaded file name matched but hash did not".into()),
        }),
        Err(err) => Ok(AcquireResult {
            archive: archive.clone(),
            status: AcquireStatus::TimedOut,
            path: None,
            computed_xxh64: None,
            message: Some(err.to_string()),
        }),
    }
}

async fn acquire_nexus_archive(
    manifest: &WabbajackManifest,
    store_dir: &Path,
    download_dir: &Path,
    archive: &AcquireMissingArchive,
    json: bool,
) -> Result<AcquireResult> {
    let Some(directive) = manifest
        .download_directives()
        .into_iter()
        .find(|directive| directive.hash() == archive.hash)
    else {
        return Ok(acquire_message(
            archive,
            AcquireStatus::UnsupportedSource,
            "Nexus archive did not produce a download directive",
        ));
    };

    let client = reqwest::Client::new();
    let source = match modde_sources::nexus::NexusSource::new(client) {
        Ok(source) => source,
        Err(err) => {
            return Ok(acquire_message(
                archive,
                AcquireStatus::NexusCredentialsMissing,
                &format!("{err:#}"),
            ));
        }
    };

    if !json {
        eprintln!(
            "resolving-nexus {:016x} {}",
            archive.hash, archive.source_hint
        );
    }
    let handle = match source.resolve(&directive).await {
        Ok(handle) => handle,
        Err(err) => {
            return Ok(acquire_message(
                archive,
                AcquireStatus::NexusCredentialsMissing,
                &format!("{err:#}"),
            ));
        }
    };

    let dest = download_dir
        .join(modde_core::fs::safe_file_name(&archive.name).unwrap_or("archive.download"));
    let verified = match source.download(handle, &dest).await {
        Ok(verified) => verified,
        Err(err) => {
            return Ok(acquire_message(
                archive,
                AcquireStatus::Mismatched,
                &format!("{err:#}"),
            ));
        }
    };
    import_acquired_archive(manifest, store_dir, archive, &verified.path).await
}

fn acquire_message(
    archive: &AcquireMissingArchive,
    status: AcquireStatus,
    message: &str,
) -> AcquireResult {
    AcquireResult {
        archive: archive.clone(),
        status,
        path: None,
        computed_xxh64: None,
        message: Some(message.to_string()),
    }
}

fn print_acquire_result(result: &AcquireResult) {
    let archive = &result.archive;
    let path = result
        .path
        .as_ref()
        .map_or_else(String::new, |path| format!(" path={}", path.display()));
    let hash = result
        .computed_xxh64
        .map_or_else(String::new, |hash| format!(" computed={hash:016x}"));
    let message = result
        .message
        .as_ref()
        .map_or_else(String::new, |message| format!(" message={message}"));
    println!(
        "{} {:016x} {}{}{}{}",
        acquire_status_label(&result.status),
        archive.hash,
        archive.name,
        path,
        hash,
        message
    );
}

pub(crate) fn acquire_status_label(status: &AcquireStatus) -> &'static str {
    match status {
        AcquireStatus::AlreadyPresent => "already-present",
        AcquireStatus::OpenedBrowser => "opened-browser",
        AcquireStatus::WaitingForDownload => "waiting-for-download",
        AcquireStatus::DirectResolved => "direct-resolved",
        AcquireStatus::DirectFailed => "direct-failed",
        AcquireStatus::BrowserRequired => "browser-required",
        AcquireStatus::DnsUnresolved => "dns-unresolved",
        AcquireStatus::LoginRequired => "login-required",
        AcquireStatus::CaptchaRequired => "captcha-required",
        AcquireStatus::Imported => "imported",
        AcquireStatus::Mismatched => "mismatched",
        AcquireStatus::TimedOut => "timed-out",
        AcquireStatus::NexusCredentialsMissing => "nexus-credentials-missing",
        AcquireStatus::UnsupportedSource => "unsupported-source",
    }
}
