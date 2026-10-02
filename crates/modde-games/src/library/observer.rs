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
    use std::os::unix::process::ExitStatusExt;
    let (program, arguments) = arguments
        .split_first()
        .context("observer command missing")?;
    // SAFETY: prctl takes no pointers. The caller is an isolated, single-threaded
    // helper whose only children are those created for this launch.
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0 {
        return Err(std::io::Error::last_os_error())
            .context("enabling sandbox descendant observer");
    }
    let mut child = std::process::Command::new(program)
        .args(arguments)
        .spawn()
        .context("starting sandbox game command")?;
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
                Some(libc::ECHILD) => return Ok(outcome),
                _ => return Err(error).context("waiting for sandbox descendants"),
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn reap(_arguments: &[OsString]) -> Result<ExitStatus> {
    anyhow::bail!("sandbox descendant observation requires Linux")
}
