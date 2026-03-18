//! # `isartor stop` — Graceful server shutdown
//!
//! Reads the PID from `~/.isartor/isartor.pid` and terminates the
//! running Isartor server (SIGTERM on Unix, TerminateProcess on Windows).

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;

/// Default PID file location: `~/.isartor/isartor.pid`
pub fn pid_file_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    Ok(home.join(".isartor").join("isartor.pid"))
}

/// Write the current process PID to `~/.isartor/isartor.pid`.
pub fn write_pid_file() -> Result<()> {
    let path = pid_file_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, std::process::id().to_string())?;
    tracing::debug!(path = %path.display(), pid = std::process::id(), "PID file written");
    Ok(())
}

/// Remove the PID file (best-effort, called on shutdown).
pub fn remove_pid_file() {
    if let Ok(path) = pid_file_path() {
        let _ = std::fs::remove_file(path);
    }
}

// ── Platform helpers ─────────────────────────────────────────────────

/// Check whether a process with the given PID is alive.
#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
fn is_process_alive(pid: u32) -> bool {
    use std::process::Command;
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

/// Send a termination signal to the process.
/// On Unix: SIGTERM (graceful) or SIGKILL (force).
/// On Windows: always TerminateProcess via `taskkill`.
#[cfg(unix)]
fn terminate_process(pid: u32, force: bool) -> Result<()> {
    let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
    let result = unsafe { libc::kill(pid as i32, signal) };
    if result != 0 {
        let err = std::io::Error::last_os_error();
        let name = if force { "SIGKILL" } else { "SIGTERM" };
        bail!("Failed to send {} to PID {}: {}", name, pid, err);
    }
    Ok(())
}

#[cfg(windows)]
fn terminate_process(pid: u32, force: bool) -> Result<()> {
    use std::process::Command;
    let mut cmd = Command::new("taskkill");
    if force {
        cmd.arg("/F");
    }
    cmd.args(["/PID", &pid.to_string()]);
    let output = cmd.output().context("failed to run taskkill")?;
    if !output.status.success() {
        bail!(
            "taskkill failed for PID {}: {}",
            pid,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

// ── CLI ──────────────────────────────────────────────────────────────

#[derive(Parser, Debug, Clone)]
pub struct StopArgs {
    /// Path to PID file (default: ~/.isartor/isartor.pid).
    #[arg(long)]
    pub pid_file: Option<PathBuf>,

    /// Force kill the process (SIGKILL on Unix, /F on Windows).
    #[arg(long)]
    pub force: bool,
}

pub fn handle_stop(args: StopArgs) -> Result<()> {
    let path = match args.pid_file {
        Some(p) => p,
        None => pid_file_path()?,
    };

    if !path.exists() {
        bail!(
            "No PID file found at {}. Is Isartor running?",
            path.display()
        );
    }

    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read PID file: {}", path.display()))?;
    let pid: u32 = contents
        .trim()
        .parse()
        .with_context(|| format!("invalid PID in {}: {:?}", path.display(), contents.trim()))?;

    // Verify the process is actually running.
    if !is_process_alive(pid) {
        let _ = std::fs::remove_file(&path);
        bail!(
            "Process {} is not running (stale PID file removed). Isartor may have already stopped.",
            pid
        );
    }

    let signal_name = if args.force { "SIGKILL" } else { "SIGTERM" };
    terminate_process(pid, args.force)?;
    eprintln!("  ✓ Sent {} to Isartor (PID {})", signal_name, pid);

    // Wait briefly for the process to exit, then clean up the PID file.
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if !is_process_alive(pid) {
            let _ = std::fs::remove_file(&path);
            eprintln!("  ✓ Isartor stopped.");
            return Ok(());
        }
    }

    if args.force {
        eprintln!("  ⚠ Process {} did not exit within 2 seconds.", pid);
    } else {
        eprintln!(
            "  ⚠ Process {} still running after 2 seconds. Try: isartor stop --force",
            pid
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_file_path_returns_expected_location() {
        let path = pid_file_path().unwrap();
        assert!(path.ends_with(".isartor/isartor.pid"));
    }

    #[test]
    fn write_and_remove_pid_file() {
        write_pid_file().unwrap();
        let path = pid_file_path().unwrap();
        assert!(path.exists());

        let content = std::fs::read_to_string(&path).unwrap();
        let pid: u32 = content.trim().parse().unwrap();
        assert_eq!(pid, std::process::id());

        remove_pid_file();
        assert!(!path.exists());
    }

    #[test]
    fn stop_with_no_pid_file_returns_error() {
        let args = StopArgs {
            pid_file: Some(PathBuf::from("/tmp/isartor-nonexistent-test.pid")),
            force: false,
        };
        let result = handle_stop(args);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No PID file found"));
    }

    #[cfg(unix)]
    #[test]
    fn stop_with_stale_pid_file() {
        let path = PathBuf::from("/tmp/isartor-stale-test.pid");
        std::fs::write(&path, "999999999").unwrap();

        let args = StopArgs {
            pid_file: Some(path.clone()),
            force: false,
        };
        let result = handle_stop(args);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not running"));
        assert!(!path.exists());
    }
}
