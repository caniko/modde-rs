//! Run the game in a separate subreaper, optionally owned by a systemd cgroup.
//! The CLI never infers session completion from the first launcher exiting.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{Context, Result, bail, ensure};
use modde_core::library::diagnostics;
use modde_core::library::{
    PendingSession, SessionObservation, SessionPhase, atomic_json, lock_file,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Request {
    program: OsString,
    arguments: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
    directory: PathBuf,
    journal: PathBuf,
    sample_directory: Option<PathBuf>,
    #[serde(default)]
    gpu: Option<modde_games::library::gpu::Snapshot>,
    #[serde(default)]
    sandboxed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Evidence {
    pub supervisor: u32,
    pub started: bool,
    /// The outer boundary spawned; this alone does not establish game exec.
    #[serde(default)]
    pub boundary_started: bool,
    pub completed: bool,
    pub raw_status: Option<i32>,
    #[serde(default)]
    pub leader_status: Option<i32>,
    #[serde(default)]
    pub descendant_failures: usize,
    #[serde(default)]
    pub launch_error: Option<String>,
    pub elapsed_ms: u128,
    /// Launch-to-first-parseable-MangoHud-sample, sampled every 100 ms.
    #[serde(default)]
    pub first_sample_ms: Option<u128>,
    #[serde(default)]
    pub gpu: Option<modde_games::library::gpu::Snapshot>,
    #[serde(default)]
    pub inner_raw_status: Option<i32>,
}

pub(super) fn evidence(observation: &SessionObservation) -> Result<Option<Evidence>> {
    match std::fs::read(observation.directory.join("evidence.json")) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn boot_id() -> Result<String> {
    #[cfg(target_os = "linux")]
    {
        Ok(std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?
            .trim()
            .into())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(String::new())
    }
}

pub(super) fn remove_request(session: &PendingSession) -> Result<()> {
    if let Some(observation) = &session.observation {
        match std::fs::remove_file(observation.directory.join("request.json")) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("removing the consumed launch environment"),
        }
    }
    Ok(())
}

pub(super) fn run(
    mut command: Command,
    session: &mut PendingSession,
    sample_directory: Option<&Path>,
    settings: &modde_core::library::LaunchSettings,
) -> Result<(ExitStatus, Option<std::fs::File>)> {
    let gpu = modde_games::library::gpu::snapshot(settings)?;
    let directory = if let Some(directory) = &session.diagnostics {
        directory.clone()
    } else {
        tempfile::Builder::new()
            .prefix("run-")
            .tempdir_in(
                PendingSession::path()
                    .parent()
                    .context("session directory missing")?,
            )?
            .keep()
    };
    diagnostics::private_directory(&directory)?;
    let mut environment: BTreeMap<_, _> = std::env::vars_os().collect();
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            environment.insert(key.to_owned(), value.to_owned());
        } else {
            environment.remove(key);
        }
    }
    environment.remove(&OsString::from("MODDE_STEAM_API_KEY"));
    let mut arguments: Vec<OsString> = command.get_args().map(OsString::from).collect();
    if settings.sandbox.enabled {
        let index = arguments
            .windows(3)
            .position(|args| args == ["library", "reap", "--"].map(OsString::from))
            .context("sandbox command has no in-namespace observer")?;
        arguments.splice(
            index + 2..index + 2,
            ["--status-fd", "3"].map(OsString::from),
        );
        // Bubblewrap forwards inherited descriptors to the command (its PID-1
        // helper closes its copy). There is no --preserve-fds option in bwrap.
    }
    let mounts: Vec<_> = arguments
        .windows(3)
        .filter(|args| {
            ["--bind", "--ro-bind", "--dev-bind"]
                .iter()
                .any(|flag| args[0] == *flag)
        })
        .map(|args| serde_json::json!({"mode": args[0], "source": args[1], "destination": args[2]}))
        .collect();
    atomic_json(
        &directory.join("command.json"),
        &serde_json::json!({
            "program": command.get_program(), "argument_count": arguments.len(),
            "directory": command.get_current_dir(), "sandboxed": settings.sandbox.enabled,
            "mounts": mounts,
        }),
    )?;
    let request = Request {
        program: command.get_program().to_owned(),
        arguments,
        environment: environment.into_iter().collect(),
        directory: command
            .get_current_dir()
            .map(Path::to_path_buf)
            .unwrap_or(std::env::current_dir()?),
        journal: PendingSession::path(),
        sample_directory: sample_directory.map(Path::to_path_buf),
        gpu: Some(gpu),
        sandboxed: settings.sandbox.enabled,
    };
    let request_path = directory.join("request.json");
    atomic_json(&request_path, &request)?;
    let systemd = host_service_is_compatible()
        && Command::new("systemctl")
            .args(["--user", "show-environment"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
    let unit = systemd.then(|| {
        format!(
            "modde-{}.service",
            directory.file_name().unwrap_or_default().to_string_lossy()
        )
    });
    session.observation = Some(SessionObservation {
        directory: directory.clone(),
        boot_id: boot_id()?,
        unit: unit.clone(),
    });
    session.advance(SessionPhase::Launching)?;
    let binary = std::env::current_exe()?;
    let log_path = directory.join("completion.log");
    let log = diagnostics::log_file(&log_path)?;
    let mut completion = Command::new(&binary);
    completion
        .arg("--config-dir")
        .arg(modde_core::paths::config_dir())
        .arg("--data-dir")
        .arg(modde_core::paths::modde_data_dir())
        .args(["library", "complete-observed"])
        .arg(&directory)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        completion.process_group(0);
    }
    // Intentionally detached. Its only authority is this observation pin and
    // the global mutation lease; failures remain in completion.log + journal.
    let _completion = completion
        .spawn()
        .context("starting session completion worker")?;
    command = if let Some(unit) = unit {
        let mut service = Command::new("systemd-run");
        service
            .args([
                "--user",
                "--quiet",
                "--wait",
                "--pipe",
                "--collect",
                "--service-type=exec",
            ])
            .arg(format!("--unit={unit}"))
            .args([
                "--property=ExitType=cgroup",
                "--property=KillMode=control-group",
            ]);
        for key in ["PATH", "LD_LIBRARY_PATH"] {
            if let Some(value) = std::env::var_os(key) {
                let mut argument = OsString::from(format!("--setenv={key}="));
                argument.push(value);
                service.arg(argument);
            }
        }
        service.arg("--").arg(&binary);
        service
    } else {
        let mut helper = Command::new(&binary);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Remain in the caller's mount namespace, but separate terminal
            // signals from the CLI so an interrupted client retains observation.
            helper.process_group(0);
        }
        helper
    };
    command.arg("library").arg("supervise").arg(&request_path);
    // This is deliberately blocking on the CLI's runtime, never on the GUI.
    // An unsuccessful service launch is never retried outside its cgroup.
    let status = command
        .status()
        .context("starting isolated game supervisor")?;
    let observation = session
        .observation
        .as_ref()
        .context("missing process observation")?;
    let result =
        evidence(observation)?.context("supervisor did not write evidence; session retained")?;
    if let Some(error) = result.launch_error {
        bail!("game did not start: {error}; use `modde library recover`");
    }
    ensure!(
        result.completed,
        "supervisor exited {status} without proving game exit; session retained"
    );
    let observer = require_exit(session, false)?;
    let status = from_raw(result.raw_status.context("missing game exit status")?);
    session.advance(SessionPhase::Exited)?;
    Ok((status, observer))
}

/// Retain this lease until capture and journal removal complete. Otherwise a
/// delayed helper could start between the exit check and journal removal.
pub(super) fn require_exit(
    session: &PendingSession,
    confirm: bool,
) -> Result<Option<std::fs::File>> {
    let Some(observation) = &session.observation else {
        ensure!(
            confirm || session.phase == SessionPhase::Captured,
            "exit is unobserved; confirm the game is closed with `modde library finish --confirm-exited`"
        );
        return Ok(None);
    };
    let guard = lock_file(&observation.directory.join("observer.lock"))
        .context("the game supervisor is still running")?;
    if boot_id()? != observation.boot_id {
        return Ok(Some(guard));
    }
    if let Some(unit) = &observation.unit {
        let output = Command::new("systemctl")
            .args([
                "--user",
                "show",
                unit,
                "--property=ActiveState",
                "--property=LoadState",
            ])
            .output()
            .context("querying the session cgroup")?;
        let state = String::from_utf8(output.stdout)?;
        ensure!(
            state.lines().any(|line| line == "LoadState=not-found")
                || (output.status.success()
                    && state
                        .lines()
                        .any(|line| matches!(line, "ActiveState=inactive" | "ActiveState=failed"))),
            "session cgroup is active or unavailable; completion cannot be established"
        );
    }
    ensure!(
        confirm
            || session.phase == SessionPhase::Captured
            || evidence(observation)?.is_some_and(|e| e.completed),
        "process evidence was interrupted; confirm all game/Wine processes exited with `modde library finish --confirm-exited`"
    );
    Ok(Some(guard))
}

fn host_service_is_compatible() -> bool {
    #[cfg(target_os = "linux")]
    {
        if Path::new("/.flatpak-info").exists()
            || std::env::var_os("FLATPAK_ID").is_some()
            || std::env::var_os("container").is_some()
            || std::env::var_os("PRESSURE_VESSEL_RUNTIME").is_some()
        {
            return false;
        }
        // A host user service cannot inherit a container's mount/user/PID
        // namespaces. Missing namespace evidence also selects the local helper.
        ["mnt", "pid", "user", "net", "ipc", "uts", "cgroup"]
            .iter()
            .all(|namespace| {
                match (
                    std::fs::read_link(format!("/proc/self/ns/{namespace}")),
                    std::fs::read_link(format!("/proc/1/ns/{namespace}")),
                ) {
                    (Ok(ours), Ok(init)) => ours == init,
                    _ => false,
                }
            })
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// This helper has no database or global mutation lock. The pending journal
/// blocks mutations if its parent dies; observer.lock blocks premature Finish.
pub(super) fn supervise(path: &Path) -> Result<()> {
    let directory = path.parent().context("request directory missing")?;
    let _guard = lock_file(&directory.join("observer.lock"))?;
    let request: Request = serde_json::from_slice(&std::fs::read(path)?)?;
    let mut session: PendingSession = serde_json::from_slice(
        &std::fs::read(&request.journal).context("launch request was cancelled")?,
    )?;
    ensure!(
        session.phase == SessionPhase::Launching
            && session
                .observation
                .as_ref()
                .is_some_and(|o| o.directory == directory),
        "launch request no longer owns the pending session"
    );
    let evidence_path = directory.join("evidence.json");
    ensure!(
        !evidence_path.exists(),
        "supervisor request has already been consumed"
    );
    #[cfg(target_os = "linux")]
    // SAFETY: prctl changes only this isolated helper's child-reaping policy;
    // it takes no pointers and is called before any game process is created.
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0 {
        return Err(std::io::Error::last_os_error()).context("enabling descendant observation");
    }
    let mut record = Evidence {
        supervisor: std::process::id(),
        started: false,
        boundary_started: false,
        completed: false,
        raw_status: None,
        leader_status: None,
        descendant_failures: 0,
        launch_error: None,
        elapsed_ms: 0,
        first_sample_ms: None,
        gpu: request.gpu,
        inner_raw_status: None,
    };
    atomic_json(&evidence_path, &record)?;
    // Environment belongs only to this launch; do not retain inherited secrets
    // for the whole gaming session or in a completed request on disk.
    std::fs::remove_file(path)?;
    let start = std::time::Instant::now();
    let log = diagnostics::log_file(&directory.join("game.log"))?;
    let mut game = Command::new(request.program);
    game.args(request.arguments)
        .env_clear()
        .envs(request.environment)
        .current_dir(request.directory)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(target_os = "linux")]
    let mut inner = if request.sandboxed {
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::process::CommandExt;
        let mut descriptors = [0; 2];
        // SAFETY: pipe2 writes two initialized descriptors into this local array.
        ensure!(
            unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC) } == 0,
            "creating observer status pipe: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: successful pipe2 transfers exclusive ownership of both ends.
        let read = unsafe { std::fs::File::from_raw_fd(descriptors[0]) };
        let write = unsafe { std::fs::File::from_raw_fd(descriptors[1]) };
        let fd = write.as_raw_fd();
        // SAFETY: only async-signal-safe syscalls run between fork and exec.
        unsafe {
            game.pre_exec(move || {
                if libc::dup2(fd, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Some((std::io::BufReader::new(read), write))
    } else {
        None
    };
    let mut child = match game.spawn() {
        Ok(child) => child,
        Err(error) => {
            record.launch_error = Some(error.to_string());
            record.completed = true;
            atomic_json(&evidence_path, &record)?;
            // spawn returned an error: the executable never ran, so the earlier
            // save transition can be restored instead of capturing it as play.
            session.phase = SessionPhase::Ready;
            atomic_json(&request.journal, &session)?;
            return Err(error).context("starting game command");
        }
    };
    record.boundary_started = true;
    record.started = !request.sandboxed;
    session.phase = SessionPhase::Running;
    #[cfg(target_os = "linux")]
    let mut inner_reader = if let Some((mut reader, write)) = inner.take() {
        drop(write);
        if let Err(error) = inner_startup(&mut reader, &mut record) {
            eprintln!(
                "sandbox startup evidence unavailable: {error}; continuing descendant observation"
            );
        }
        Some(reader)
    } else {
        None
    };
    // A status-write failure after spawn must not orphan an otherwise healthy
    // observer. Keep reaping; the final durable evidence is the completion gate.
    let progress =
        atomic_json(&evidence_path, &record).and_then(|()| atomic_json(&request.journal, &session));
    let status = if request.sample_directory.is_none() {
        child.wait()?
    } else {
        loop {
            observe_sample(request.sample_directory.as_deref(), start, &mut record);
            if let Some(status) = child.try_wait()? {
                break status;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    record.leader_status = Some(into_raw(status));
    record.raw_status = record.leader_status;
    #[cfg(target_os = "linux")]
    loop {
        let mut status = 0;
        // SAFETY: status points to an initialized local integer. All descendants
        // of this helper belong to this one launch. No unrelated child is reaped.
        let pid = unsafe {
            libc::waitpid(
                -1,
                &raw mut status,
                if request.sample_directory.is_some() {
                    libc::WNOHANG
                } else {
                    0
                },
            )
        };
        if pid > 0 {
            let descendant = from_raw(status);
            if !descendant.success() {
                record.descendant_failures += 1;
                if record.raw_status.is_some_and(|raw| from_raw(raw).success()) {
                    record.raw_status = Some(status);
                }
            }
            continue;
        }
        if pid == 0 {
            observe_sample(request.sample_directory.as_deref(), start, &mut record);
            std::thread::sleep(std::time::Duration::from_millis(100));
            continue;
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINTR) => {}
            Some(libc::ECHILD) => break,
            _ => return Err(error).context("waiting for detached game descendants"),
        }
    }
    observe_sample(request.sample_directory.as_deref(), start, &mut record);
    #[cfg(target_os = "linux")]
    if let Some(reader) = &mut inner_reader {
        use std::io::BufRead;
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok_and(|bytes| bytes > 0) {
            let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
                eprintln!("invalid inner completion evidence; session retained");
                break;
            };
            if message["phase"] == "completed" {
                record.inner_raw_status = message["raw_status"]
                    .as_i64()
                    .and_then(|v| i32::try_from(v).ok());
            }
            line.clear();
        }
        // An inner signal retains its real wait status rather than the CLI's
        // normalized 128+signal exit code. Preserve outer boundary failures too.
        if let Some(raw) = record.inner_raw_status
            && !from_raw(raw).success()
        {
            record.raw_status = Some(raw);
        }
    }
    record.elapsed_ms = start.elapsed().as_millis();
    // Other platforms cannot infer descendant lifetime from a child handle.
    record.completed = cfg!(target_os = "linux")
        && (!request.sandboxed
            || record.launch_error.is_some()
            || record.inner_raw_status.is_some());
    if record.launch_error.is_some() {
        session.phase = SessionPhase::Ready;
        atomic_json(&request.journal, &session)?;
    }
    atomic_json(&evidence_path, &record)?;
    if let Err(error) = progress {
        eprintln!("session progress could not be recorded: {error}; final exit evidence was saved");
    }
    if !record.completed {
        bail!("game child exited; confirm descendant exit manually on this platform");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn inner_startup(reader: &mut impl std::io::BufRead, record: &mut Evidence) -> Result<()> {
    let mut attempted = false;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            if !attempted {
                record.launch_error =
                    Some("sandbox observer did not start; inspect game.log".into());
            }
            return Ok(());
        }
        let message: serde_json::Value = serde_json::from_str(&line)?;
        match message["phase"].as_str() {
            Some("starting") => attempted = true,
            Some("started") => {
                record.started = true;
                return Ok(());
            }
            Some("failed") => {
                record.launch_error = Some(
                    message["error"]
                        .as_str()
                        .unwrap_or("inner command did not start")
                        .into(),
                );
                return Ok(());
            }
            _ => bail!("invalid sandbox startup evidence"),
        }
    }
}

fn observe_sample(directory: Option<&Path>, start: std::time::Instant, record: &mut Evidence) {
    use std::io::Read;
    if record.first_sample_ms.is_some() {
        return;
    }
    let Some(entries) = directory
        .as_ref()
        .and_then(|dir| std::fs::read_dir(dir).ok())
    else {
        return;
    };
    for entry in entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "csv"))
    {
        let Ok(file) = std::fs::File::open(entry.path()) else {
            continue;
        };
        let mut text = String::new();
        if file.take(65_536).read_to_string(&mut text).is_ok()
            && modde_core::performance::parse_mangohud_csv(&text)
                .is_ok_and(|parsed| !parsed.samples.is_empty())
        {
            record.first_sample_ms = Some(start.elapsed().as_millis());
            break;
        }
    }
}

#[cfg(unix)]
fn into_raw(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status.into_raw()
}
#[cfg(unix)]
pub(super) fn from_raw(status: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(status)
}
#[cfg(windows)]
fn into_raw(status: ExitStatus) -> i32 {
    use std::os::windows::process::ExitStatusExt;
    status.into_raw() as i32
}
#[cfg(windows)]
pub(super) fn from_raw(status: i32) -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(status as u32)
}
