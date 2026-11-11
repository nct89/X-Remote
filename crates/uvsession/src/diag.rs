//! One-line events for host registration.
//!
//! Written to `~/.config/x-remote/host.log` (Windows: `%APPDATA%\X-Remote\host.log`).
//! Each line is `[unix] [local YYYY-MM-DD HH:MM:SS] text`. The text never includes
//! the host password or a `60:` challenge body.

use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Append one diagnostic line. Failures are ignored so logging cannot break login.
pub fn event(s: &str) {
    let Some(p) = log_path() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
        }
    }
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
        }
        let _ = writeln!(f, "[{secs}] [{}] {s}", local_stamp(secs));
    }
}

fn log_path() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        return env("APPDATA").map(|d| d.join("X-Remote").join("host.log"));
    }
    env("XDG_CONFIG_HOME")
        .or_else(|| env("HOME").map(|h| h.join(".config")))
        .map(|d| d.join("x-remote").join("host.log"))
}

#[cfg(unix)]
fn local_stamp(secs: u64) -> String {
    let t = secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return format!("{secs}");
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

#[cfg(not(unix))]
fn local_stamp(secs: u64) -> String {
    format!("{secs}")
}
