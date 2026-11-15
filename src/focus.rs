//! Tracks real focus history so "previous window" means the last window that had focus.
//! Starts at app launch. Entries are window handles stored as isize (HWND is not Send).

use std::sync::Mutex;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};

const MAX_HISTORY: usize = 16;

static HISTORY: Mutex<Vec<isize>> = Mutex::new(Vec::new());

fn push(hwnd: isize) {
    if let Ok(mut h) = HISTORY.lock() {
        h.retain(|&x| x != hwnd);
        h.insert(0, hwnd);
        h.truncate(MAX_HISTORY);
    }
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if !hwnd.0.is_null() {
        push(hwnd.0 as isize);
    }
}

/// Installs the foreground hook. Must be called on the thread that runs the message loop.
pub fn install() -> HWINEVENTHOOK {
    if let Some(h) = crate::win::foreground() {
        push(h.0 as isize);
    }
    unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    }
}

pub fn uninstall(hook: HWINEVENTHOOK) {
    unsafe {
        let _ = UnhookWinEvent(hook);
    }
}

/// Recent focus history, most recent first.
pub fn recent() -> Vec<HWND> {
    HISTORY
        .lock()
        .map(|h| h.iter().map(|&x| HWND(x as *mut _)).collect())
        .unwrap_or_default()
}
