//! Log file at %APPDATA%\pane-pogo\pane-pogo.log. The app has no console, so this is
//! where registration results and errors go.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use windows::Win32::System::SystemInformation::GetLocalTime;

const MAX_BYTES: u64 = 1024 * 1024;

pub fn path() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    Some(
        PathBuf::from(appdata)
            .join("pane-pogo")
            .join("pane-pogo.log"),
    )
}

/// Starts a fresh log if the old one is over 1 MB.
pub fn rotate() {
    if let Some(p) = path()
        && fs::metadata(&p)
            .map(|m| m.len() > MAX_BYTES)
            .unwrap_or(false)
    {
        let _ = fs::rename(&p, p.with_extension("log.old"));
    }
}

pub fn write(msg: &str) {
    let Some(p) = path() else {
        return;
    };
    if let Some(dir) = p.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let t = unsafe { GetLocalTime() };
    let line = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} [{}] {msg}\n",
        t.wYear,
        t.wMonth,
        t.wDay,
        t.wHour,
        t.wMinute,
        t.wSecond,
        std::process::id()
    );
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&p) {
        let _ = f.write_all(line.as_bytes());
    }
}
