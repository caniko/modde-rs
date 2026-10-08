//! Run the game in a separate subreaper, optionally owned by a systemd cgroup.
//! The CLI never infers session completion from the first launcher exiting.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{Context, Result, bail, ensure};
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
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct Evidence {
    pub supervisor: u32,
    pub started: bool,
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
    sandboxed: bool,
) -> Result<(ExitStatus, Option<std::fs::File>)> {
    if sandboxed {
        command = sandbox_tree(&command, &super::cli_binary()?.canonicalize()?, false)?;
    }
    let directory = tempfile::Builder::new()
        .prefix("run-")
        .tempdir_in(
            PendingSession::path()
                .parent()
                .context("session directory missing")?,
        )?
        .keep();
    let mut environment: BTreeMap<_, _> = std::env::vars_os().collect();
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            environment.insert(key.to_owned(), value.to_owned());
        } else {
            environment.remove(key);
        }
    }
    environment.remove(&OsString::from("MODDE_STEAM_API_KEY"));
    let request = Request {
        program: command.get_program().to_owned(),
        arguments: command.get_args().map(OsString::from).collect(),
        environment: environment.into_iter().collect(),
        directory: command
            .get_current_dir()
            .map(Path::to_path_buf)
            .unwrap_or(std::env::current_dir()?),
        journal: PendingSession::path(),
        sample_directory: sample_directory.map(Path::to_path_buf),
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
    let binary = super::cli_binary()?;
    let log_path = directory.join("completion.log");
    let log = std::fs::File::create(&log_path)?;
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
    enable_subreaper()?;
    let mut record = Evidence {
        supervisor: std::process::id(),
        ..Default::default()
    };
    atomic_json(&evidence_path, &record)?;
    // Environment belongs only to this launch; do not retain inherited secrets
    // for the whole gaming session or in a completed request on disk.
    std::fs::remove_file(path)?;
    let start = std::time::Instant::now();
    let mut child = match Command::new(request.program)
        .args(request.arguments)
        .env_clear()
        .envs(request.environment)
        .current_dir(request.directory)
        .spawn()
    {
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
    record.started = true;
    session.phase = SessionPhase::Running;
    // A status-write failure after spawn must not orphan an otherwise healthy
    // observer. Keep reaping; the final durable evidence is the completion gate.
    let progress =
        atomic_json(&evidence_path, &record).and_then(|()| atomic_json(&request.journal, &session));
    let status = if request.sample_directory.is_none() {
        child.wait()?
    } else {
        loop {
            observe_sample(&request.sample_directory, start, &mut record);
            if let Some(status) = child.try_wait()? {
                break status;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    record.leader_status = Some(into_raw(status));
    record.raw_status = record.leader_status;
    reap_descendants(&request.sample_directory, start, &mut record)?;
    observe_sample(&request.sample_directory, start, &mut record);
    record.elapsed_ms = start.elapsed().as_millis();
    // Other platforms cannot infer descendant lifetime from a child handle.
    record.completed = cfg!(target_os = "linux");
    atomic_json(&evidence_path, &record)?;
    if let Err(error) = progress {
        eprintln!("session progress could not be recorded: {error}; final exit evidence was saved");
    }
    if !record.completed {
        bail!("game child exited; confirm descendant exit manually on this platform");
    }
    Ok(())
}

fn enable_subreaper() -> Result<()> {
    #[cfg(target_os = "linux")]
    // SAFETY: called only by isolated, single-threaded helpers before spawning.
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0 {
        return Err(std::io::Error::last_os_error()).context("enabling descendant observation");
    }
    Ok(())
}

fn reap_descendants(
    sample_directory: &Option<PathBuf>,
    start: std::time::Instant,
    record: &mut Evidence,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    loop {
        let mut status = 0;
        // SAFETY: status points to an initialized local integer. All descendants
        // of this helper belong to this one launch. No unrelated child is reaped.
        let pid = unsafe {
            libc::waitpid(
                -1,
                &mut status,
                if sample_directory.is_some() {
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
            observe_sample(sample_directory, start, record);
            std::thread::sleep(std::time::Duration::from_millis(100));
            continue;
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINTR) => continue,
            Some(libc::ECHILD) => break,
            _ => return Err(error).context("waiting for detached game descendants"),
        }
    }
    Ok(())
}

/// Bubblewrap's built-in init exits with its first child, killing remaining
/// namespace processes. Our init retains the namespace until all children exit.
pub(super) fn wait_tree(command: &[OsString]) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    bail!("sandbox descendant observation requires Linux");
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::ExitStatusExt;
        let (program, args) = command.split_first().context("sandbox command is empty")?;
        enable_subreaper()?;
        let status = Command::new(program).args(args).spawn()?.wait()?;
        let mut record = Evidence {
            raw_status: Some(into_raw(status)),
            ..Default::default()
        };
        reap_descendants(&None, std::time::Instant::now(), &mut record)?;
        let status = from_raw(
            record
                .raw_status
                .context("sandbox exit status is missing")?,
        );
        // PID 1 ignores default terminating signals; carry their conventional
        // nonzero exit code instead. A failed child must never become success.
        std::process::exit(
            status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)),
        );
    }
}

pub(super) fn preflight_sandbox(command: &Command) -> Result<()> {
    let status = sandbox_tree(command, &super::cli_binary()?.canonicalize()?, true)?
        .stdout(Stdio::null())
        .status()
        .context("probing sandbox namespace init")?;
    ensure!(
        status.success(),
        "sandbox namespace init probe failed: {status}"
    );
    Ok(())
}

fn sandbox_tree(command: &Command, binary: &Path, probe: bool) -> Result<Command> {
    let args: Vec<_> = command.get_args().collect();
    let mut boundary = 0;
    // ponytail: parse only modde-games::library::sandbox's bounded option set;
    // new builder options must extend this table. Never split on an arbitrary
    // '--' that might be an environment value or argv. Unknown options fail closed.
    loop {
        let option = args
            .get(boundary)
            .context("sandbox command boundary is missing")?;
        let values = match option.to_str() {
            Some("--") => break,
            Some(
                "--die-with-parent" | "--new-session" | "--unshare-user" | "--unshare-pid"
                | "--unshare-ipc" | "--unshare-uts" | "--unshare-net" | "--clearenv",
            ) => 0,
            Some("--cap-drop" | "--proc" | "--dev" | "--tmpfs" | "--dir" | "--chdir") => 1,
            Some("--ro-bind" | "--bind" | "--dev-bind" | "--setenv") => 2,
            _ => bail!("unsupported sandbox supervisor option: {option:?}"),
        };
        boundary += values + 1;
    }
    ensure!(boundary + 1 < args.len(), "sandbox game command is empty");
    let mut wrapped = Command::new(command.get_program());
    wrapped
        .args(&args[..boundary])
        .arg("--as-pid-1")
        .arg("--ro-bind")
        .arg(binary)
        .arg(binary)
        .arg("--")
        .arg(binary)
        .args(["library", "wait-tree", "--"]);
    if probe {
        // Exercise the real inner helper and its packaged runtime, never the
        // game or its wrappers, before deployment or live save replacement.
        wrapped.arg(binary).arg("--help");
    } else {
        wrapped.args(&args[boundary + 1..]);
    }
    if let Some(path) = command.get_current_dir() {
        wrapped.current_dir(path);
    }
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            wrapped.env(key, value);
        } else {
            wrapped.env_remove(key);
        }
    }
    Ok(wrapped)
}

fn observe_sample(directory: &Option<PathBuf>, start: std::time::Instant, record: &mut Evidence) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_init_keeps_option_values_and_game_argv_literal() {
        let binary = Path::new("/bin/modde");
        let mut command = Command::new("bwrap");
        command.args([
            "--unshare-pid",
            "--setenv",
            "LITERAL",
            "--",
            "--chdir",
            "/game",
            "--",
            "/game/launcher",
            "--",
            "$(not-a-shell)",
        ]);
        let wrapped = sandbox_tree(&command, binary, false).unwrap();
        assert_eq!(
            wrapped.get_args().collect::<Vec<_>>(),
            [
                "--unshare-pid",
                "--setenv",
                "LITERAL",
                "--",
                "--chdir",
                "/game",
                "--as-pid-1",
                "--ro-bind",
                "/bin/modde",
                "/bin/modde",
                "--",
                "/bin/modde",
                "library",
                "wait-tree",
                "--",
                "/game/launcher",
                "--",
                "$(not-a-shell)"
            ]
        );
        let probe = sandbox_tree(&command, binary, true).unwrap();
        let args: Vec<_> = probe.get_args().collect();
        assert!(args.ends_with(&[binary.as_os_str(), std::ffi::OsStr::new("--help")]));
        assert!(!args.iter().any(|arg| *arg == "/game/launcher"));
        let mut command = Command::new("bwrap");
        command.args(["--unexpected", "--", "true"]);
        assert!(sandbox_tree(&command, binary, false).is_err());
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
