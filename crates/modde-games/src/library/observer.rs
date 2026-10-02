//! Minimal in-namespace descendant observer. Invoke only in an isolated helper
//! before starting threads or creating any unrelated child processes.
use std::ffi::OsString;
use std::process::ExitStatus;

#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::Result;

/// Wait for the launcher and its orphaned descendants, retaining the first
/// failure. A supervisor inside bubblewrap keeps its leader alive until the
/// game tree is empty; an observer outside the PID namespace cannot do this.
#[cfg(target_os = "linux")]
pub fn reap(arguments: &[OsString]) -> Result<ExitStatus> {
    reap_with_status(arguments, None)
}

/// The private pipe is inherited only by this observer. CLOEXEC prevents the
/// game and its descendants from holding it open or writing startup evidence.
#[cfg(target_os = "linux")]
pub fn reap_with_status(arguments: &[OsString], status_fd: Option<i32>) -> Result<ExitStatus> {
    use std::os::fd::FromRawFd;
    use std::os::unix::process::ExitStatusExt;
    let mut channel = if let Some(fd) = status_fd {
        anyhow::ensure!(fd == 3, "observer status must use reserved descriptor 3");
        // SAFETY: fcntl uses no pointers. The descriptor is supplied by the
        // supervisor solely for this observer, and is closed when it returns.
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(std::io::Error::last_os_error()).context("opening observer status pipe");
        }
        // SAFETY: fcntl established that this inherited owned descriptor exists.
        Some(unsafe { std::fs::File::from_raw_fd(fd) })
    } else {
        None
    };
    let (program, arguments) = arguments
        .split_first()
        .context("observer command missing")?;
    // SAFETY: prctl takes no pointers. The caller is an isolated, single-threaded
    // helper whose only children are those created for this launch.
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0 {
        return Err(std::io::Error::last_os_error())
            .context("enabling sandbox descendant observer");
    }
    report(&mut channel, &serde_json::json!({"phase": "starting"}))?;
    let mut child = match std::process::Command::new(program).args(arguments).spawn() {
        Ok(child) => child,
        Err(error) => {
            report(
                &mut channel,
                &serde_json::json!({"phase": "failed", "error": error.to_string()}),
            )?;
            return Err(error).context("starting sandbox game command");
        }
    };
    // A broken diagnostic channel after spawn must never stop descendant reaping.
    if let Err(error) = report(&mut channel, &serde_json::json!({"phase": "started"})) {
        eprintln!("observer startup evidence failed: {error}");
    }
    let mut outcome = child.wait().context("waiting for sandbox launcher")?;
    loop {
        let mut status = 0;
        // SAFETY: status is an initialized local integer; all children of this
        // helper belong to the one launch and no other thread reaps them.
        let pid = unsafe { libc::waitpid(-1, &raw mut status, 0) };
        if pid > 0 {
            let descendant = ExitStatus::from_raw(status);
            if outcome.success() && !descendant.success() {
                outcome = descendant;
            }
        } else {
            let error = std::io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::EINTR) => {}
                Some(libc::ECHILD) => {
                    report(
                        &mut channel,
                        &serde_json::json!({"phase": "completed", "raw_status": outcome.into_raw()}),
                    )?;
                    return Ok(outcome);
                }
                _ => return Err(error).context("waiting for sandbox descendants"),
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn report(channel: &mut Option<std::fs::File>, value: &serde_json::Value) -> Result<()> {
    use std::io::Write;
    if let Some(channel) = channel {
        serde_json::to_writer(&mut *channel, value)?;
        channel.write_all(b"\n")?;
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn reap(_arguments: &[OsString]) -> Result<ExitStatus> {
    anyhow::bail!("sandbox descendant observation requires Linux")
}

#[cfg(not(target_os = "linux"))]
pub fn reap_with_status(arguments: &[OsString], _status_fd: Option<i32>) -> Result<ExitStatus> {
    reap(arguments)
}
