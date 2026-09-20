//! Phase 2 (roadmap 2026-09-16, §5) — process lifecycle for timed commands.
//!
//! `tokio::time::timeout(d, cmd.output())` only drops the future: the OS
//! process keeps running while the agent moves on to Repairing. This helper
//! spawns the child explicitly and, on timeout, kills it and WAITS for it
//! (`tokio::process::Child::kill` = start_kill + wait) so no zombie is left.
//!
//! Guarantee scope: the direct child only. Grandchildren spawned by the child
//! are NOT covered — that needs process groups and is tracked separately.

use std::process::{Output, Stdio};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command as TCmd;

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
            // kill() awaits the child: terminated AND reaped before we return.
            let _ = child.kill().await;
            out_task.abort();
            err_task.abort();
            Ok(None)
        }
    }
}
