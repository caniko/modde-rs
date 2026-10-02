//! Private launch bundles. Requests/raw output stay local; exports contain only
//! the explicitly redacted structured files. Journals remain the authority.
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use super::{LaunchSettings, PendingSession, atomic_json};

pub fn root() -> PathBuf {
    crate::paths::modde_data_dir().join("logs/launches")
}

pub fn private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn log_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let file = options.open(path)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        Ok(file)
    }
    #[cfg(not(unix))]
    Ok(options.open(path)?)
}

/// Rotate the GUI's process log at startup. Game logs belong to launch bundles
/// and are retained with their lifecycle evidence instead of being renamed live.
pub fn gui_log() -> Result<File> {
    let directory = crate::paths::modde_data_dir().join("logs");
    private_directory(&directory)?;
    let _lock = super::lock_file(&directory.join("gui-log.lock"))?;
    let path = directory.join("gui.log");
    if path.metadata().is_ok_and(|m| m.len() > 5 * 1024 * 1024) {
        for index in (1..=3).rev() {
            let from = if index == 1 {
                path.clone()
            } else {
                directory.join(format!("gui.log.{}", index - 1))
            };
            if from.exists() {
                std::fs::rename(from, directory.join(format!("gui.log.{index}")))?;
            }
        }
    }
    log_file(&path)
}

fn now_ms() -> Result<u128> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis())
}

pub fn begin(installation: &str, entry: &str) -> Result<PathBuf> {
    private_directory(&root())?;
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| anyhow::anyhow!("launch ID: {error}"))?;
    let started = now_ms()?;
    let id = format!("run-{started:020}-{:032x}", u128::from_ne_bytes(random));
    let directory = root().join(&id);
    private_directory(&directory)?;
    atomic_json(
        &directory.join("launch.json"),
        &json!({
            "version": 1, "id": id, "installation": installation, "entry": entry,
            "started_unix_ms": started, "modde_version": env!("CARGO_PKG_VERSION"),
            "revision": option_env!("MODDE_BUILD_REVISION"),
            "binary": std::env::current_exe().ok(),
        }),
    )?;
    event(&directory, "requested", &json!({}))?;
    Ok(directory)
}

pub fn event(directory: &Path, phase: &str, details: &Value) -> Result<()> {
    let line =
        serde_json::to_vec(&json!({"unix_ms": now_ms()?, "phase": phase, "details": details}))?;
    let _lock = super::lock_file(&directory.join("events.lock"))?;
    let mut file = log_file(&directory.join("events.jsonl"))?;
    file.write_all(&[line.as_slice(), b"\n"].concat())?;
    file.sync_data()?;
    Ok(())
}

/// Diagnostics must not undo a successfully written recovery journal.
pub fn note(session: &PendingSession, phase: &str, details: &Value) {
    if let Some(directory) = &session.diagnostics
        && let Err(error) = event(directory, phase, details)
    {
        eprintln!("launch diagnostics {}: {error}", directory.display());
    }
}

pub fn settings(directory: &Path, settings: &LaunchSettings) -> Result<()> {
    let mut settings = settings.clone();
    for value in settings.environment.values_mut() {
        *value = "<redacted>".into();
    }
    settings.arguments.fill("<redacted>".into());
    for wrapper in &mut settings.wrappers {
        if wrapper.len() > 1 {
            wrapper[1..].fill("<redacted>".into());
        }
    }
    atomic_json(&directory.join("settings.json"), &settings)
}

pub fn finish(directory: &Path, outcome: &str) -> Result<()> {
    event(directory, outcome, &json!({}))?;
    atomic_json(
        &directory.join("finished.json"),
        &json!({"outcome": outcome, "finished_unix_ms": now_ms()?}),
    )
}

pub fn resolve(id: Option<&str>) -> Result<PathBuf> {
    let id = match id {
        Some(id) => id.to_owned(),
        None => recent(1)?
            .first()
            .and_then(|v| v["id"].as_str())
            .context("no launch diagnostics")?
            .to_owned(),
    };
    ensure!(
        id.starts_with("run-") && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'),
        "invalid launch ID"
    );
    let directory = root().join(id);
    ensure!(
        directory.symlink_metadata()?.file_type().is_dir(),
        "launch bundle must be a directory"
    );
    Ok(directory)
}

pub fn recent(limit: usize) -> Result<Vec<Value>> {
    let entries = match std::fs::read_dir(root()) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut directories: Vec<_> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    directories.sort();
    let mut records = Vec::new();
    for directory in directories.into_iter().rev().take(limit) {
        let Ok(bytes) = std::fs::read(directory.join("launch.json")) else {
            continue;
        };
        let mut record: Value = serde_json::from_slice(&bytes)?;
        if let Ok(bytes) = std::fs::read(directory.join("finished.json")) {
            let finished: Value = serde_json::from_slice(&bytes)?;
            record["outcome"] = finished["outcome"].clone();
        }
        record["directory"] = json!(directory);
        records.push(record);
    }
    Ok(records)
}

pub fn export(directory: &Path) -> Result<Value> {
    let mut result = json!({"version": 1, "raw_output_included": false});
    for name in [
        "launch",
        "settings",
        "command",
        "evidence",
        "finished",
        "completion",
    ] {
        match std::fs::read(directory.join(format!("{name}.json"))) {
            Ok(bytes) => {
                let mut value: Value = serde_json::from_slice(&bytes)?;
                // Errors can contain provider response text or command arguments.
                if let Some(object) = value.as_object_mut() {
                    object.remove("launch_error");
                }
                result[name] = value;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    if let Ok(bytes) = std::fs::read_to_string(directory.join("events.jsonl")) {
        let events: Vec<Value> = bytes
            .lines()
            .map(|line| {
                let value: Value = serde_json::from_str(line)?;
                Ok(json!({"unix_ms": value["unix_ms"], "phase": value["phase"]}))
            })
            .collect::<Result<_>>()?;
        result["events"] = json!(events);
    }
    Ok(result)
}

pub fn tail(path: &Path, bytes: u64) -> Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(bytes)))?;
    let mut buffer = Vec::new();
    file.take(bytes).read_to_end(&mut buffer)?;
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

/// Preserve pending and explicitly retained experiment bundles. Delete only
/// finished unreferenced directories, never journals, vaults or runtime data.
pub fn prune(keep: usize) -> Result<()> {
    let pending = PendingSession::load_blocking()?.and_then(|s| s.diagnostics);
    let mut retained = 0;
    for record in recent(usize::MAX)? {
        let directory = PathBuf::from(
            record["directory"]
                .as_str()
                .context("launch directory missing")?,
        );
        if pending.as_ref() == Some(&directory)
            || directory.join("retained").exists()
            || !directory.join("finished.json").exists()
        {
            continue;
        }
        retained += 1;
        if retained > keep {
            std::fs::remove_dir_all(directory)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_export_excludes_request_raw_logs_errors_and_argument_secrets() {
        let directory = tempfile::tempdir().unwrap();
        let mut launch = LaunchSettings::default();
        launch
            .environment
            .insert("STEAM_SECRET".into(), "private-token".into());
        launch.arguments.push("private-token".into());
        launch
            .wrappers
            .push(vec!["/bin/wrapper".into(), "private-token".into()]);
        settings(directory.path(), &launch).unwrap();
        atomic_json(
            &directory.path().join("request.json"),
            &json!({"secret": "private-token"}),
        )
        .unwrap();
        atomic_json(
            &directory.path().join("evidence.json"),
            &json!({"launch_error": "private-token", "completed": true}),
        )
        .unwrap();
        std::fs::write(directory.path().join("game.log"), "private-token").unwrap();
        event(
            directory.path(),
            "failed",
            &json!({"error": "private-token"}),
        )
        .unwrap();
        let exported = export(directory.path()).unwrap().to_string();
        assert!(!exported.contains("private-token"));
        assert!(exported.contains("STEAM_SECRET"));
        assert!(exported.contains("redacted"));
    }

    #[cfg(unix)]
    #[test]
    fn a_log_symlink_cannot_redirect_output_into_another_file() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("important");
        std::fs::write(&target, "unchanged").unwrap();
        let link = directory.path().join("game.log");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(log_file(&link).is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "unchanged");
    }
}
