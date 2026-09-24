//! Phase 2 (roadmap 2026-09-16, §5) — process lifecycle for timed commands.
//!
//! `tokio::time::timeout(d, cmd.output())` only drops the future: the OS
//! process keeps running while the agent moves on to Repairing. This helper
//! spawns the child explicitly and, on timeout, kills it and WAITS for it
//! (`tokio::process::Child::kill` = start_kill + wait) so no zombie is left.
//!
//! On Unix, the direct child and descendants that remain in its process
//! group are terminated before this function returns. Other platforms retain
//! the direct-child guarantee.

#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{Output, Stdio};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command as TCmd;

#[cfg(unix)]
fn kill_process_group(pid: u32) {
    let pgid = match libc::pid_t::try_from(pid) {
        Ok(pgid) => pgid,
        Err(_) => return,
    };

    // SAFETY: the child was placed in a process group whose id is its own pid.
    unsafe {
        let _ = libc::kill(-pgid, libc::SIGKILL);
    }
}

/// Run `cmd` with a deadline.
///
/// * `Ok(Some(output))` — the process exited on its own.
/// * `Ok(None)`         — deadline hit; the process was killed and reaped.
/// * `Err(e)`           — spawn failed (e.g. `ErrorKind::NotFound`).
pub async fn output_with_timeout(cmd: &mut TCmd, dur: Duration) -> std::io::Result<Option<Output>> {
    cmd.kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    cmd.as_std_mut().process_group(0);

    let mut child = cmd.spawn()?;
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();

    let out_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(s) = so.as_mut() {
            let _ = s.read_to_end(&mut buf).await;
        }
        buf
    });
    let err_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(s) = se.as_mut() {
            let _ = s.read_to_end(&mut buf).await;
        }
        buf
    });

    match tokio::time::timeout(dur, child.wait()).await {
        Ok(Ok(status)) => {
            let stdout = out_task.await.unwrap_or_default();
            let stderr = err_task.await.unwrap_or_default();
            Ok(Some(Output {
                status,
                stdout,
                stderr,
            }))
        }
        Ok(Err(e)) => Err(e),
        Err(_) => {
            #[cfg(unix)]
            {
                if let Some(pid) = child.id() {
                    kill_process_group(pid);
                } else {
                    let _ = child.kill().await;
                }

                // Reap the direct child before returning to the caller.
                let _ = child.wait().await;
            }

            #[cfg(not(unix))]
            {
                let _ = child.kill().await;
            }

            out_task.abort();
            err_task.abort();
            Ok(None)
        }
    }
}
