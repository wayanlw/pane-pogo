//! "Start with Windows" via the per-user Run key. No admin rights needed.

use std::fs;
use std::iter::once;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::PCWSTR;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "PanePogo";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(once(0)).collect()
}

fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    let key = wide(RUN_KEY);
    let mut hkey = HKEY::default();
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            Some(0),
            access,
            &mut hkey,
        )
    };
    (rc == ERROR_SUCCESS).then_some(hkey)
}

pub fn is_enabled() -> bool {
    let Some(hkey) = open(KEY_QUERY_VALUE) else {
        return false;
    };
    let name = wide(VALUE_NAME);
    let rc = unsafe { RegQueryValueExW(hkey, PCWSTR(name.as_ptr()), None, None, None, None) };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    rc == ERROR_SUCCESS
}

/// Stable location for the copy that starts with Windows, so rebuilding or
/// cleaning a development folder cannot break startup.
pub fn install_path() -> Result<PathBuf, String> {
    let base = std::env::var("LOCALAPPDATA").map_err(|e| format!("LOCALAPPDATA not set: {e}"))?;
    Ok(PathBuf::from(base)
        .join("Programs")
        .join("pane-pogo")
        .join("pane-pogo.exe"))
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Copies the running exe to `install_path` unless it already runs from there.
fn install_copy() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = install_path()?;
    if same_file(&exe, &target) {
        return Ok(target);
    }
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::copy(&exe, &target).map_err(|e| {
        format!("could not copy to {} ({e}). If another pane-pogo is running from there, exit it first.", target.display())
    })?;
    Ok(target)
}

/// On: installs a copy and points the Run entry at it. Returns the path it will start from.
/// Off: removes the Run entry. The installed copy is left in place.
pub fn set_enabled(on: bool) -> Result<Option<PathBuf>, String> {
    let target = if on { Some(install_copy()?) } else { None };
    let hkey = open(KEY_SET_VALUE).ok_or("could not open the Run key")?;
    let name = wide(VALUE_NAME);
    let rc = if let Some(exe) = &target {
        let data = wide(&format!("\"{}\"", exe.display()));
        let bytes: Vec<u8> = data.iter().flat_map(|c| c.to_le_bytes()).collect();
        unsafe { RegSetValueExW(hkey, PCWSTR(name.as_ptr()), Some(0), REG_SZ, Some(&bytes)) }
    } else {
        unsafe { RegDeleteValueW(hkey, PCWSTR(name.as_ptr())) }
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if rc == ERROR_SUCCESS || (!on && rc.0 == 2) {
        Ok(target) // 2 = ERROR_FILE_NOT_FOUND: already off
    } else {
        Err(format!("registry write failed (Win32 {})", rc.0))
    }
}
