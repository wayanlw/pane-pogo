//! Tray icon with a context menu: Settings, Start with Windows, Exit.
//! A hidden top-level window receives the tray callbacks. It must be top-level, not
//! message-only, so it receives the TaskbarCreated broadcast after Explorer restarts.

use crate::{applog, startup};
use std::iter::once;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFY_ICON_INFOTIP_FLAGS, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, GetCursorPos,
    GetSystemMetrics, HICON, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTCOLOR, LoadIconW, LoadImageW,
    MF_CHECKED, MF_STRING, MF_UNCHECKED, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SM_CXSMICON, SM_CYSMICON, SetForegroundWindow, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON,
    TrackPopupMenu, WINDOW_EX_STYLE, WM_APP, WM_COMMAND, WM_DESTROY, WM_RBUTTONUP, WNDCLASSW,
    WS_OVERLAPPED,
};
use windows::core::PCWSTR;

const WM_TRAY: u32 = WM_APP + 1;
const CMD_STARTUP: usize = 1;
const CMD_EXIT: usize = 2;
const CMD_SETTINGS: usize = 3;
const TOOLTIP: &str = "pane-pogo";

/// Id of the "TaskbarCreated" message, registered at startup.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(once(0)).collect()
}

fn copy_into(dst: &mut [u16], s: &str) {
    let max = dst.len() - 1;
    for (d, c) in dst
        .iter_mut()
        .zip(s.encode_utf16().take(max).chain(once(0)))
    {
        *d = c;
    }
}

fn base_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    }
}

/// Creates the hidden window and adds the tray icon. Returns the window handle.
pub fn create() -> Result<HWND, String> {
    let class = wide("PanePogoTray");
    let hinstance = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance.into(),
        lpszClassName: PCWSTR(class.as_ptr()),
        ..Default::default()
    };
    unsafe { RegisterClassW(&wc) };
    TASKBAR_CREATED.store(
        unsafe { RegisterWindowMessageW(PCWSTR(wide("TaskbarCreated").as_ptr())) },
        Ordering::Relaxed,
    );

    // Never shown: no WS_VISIBLE.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            PCWSTR(wide("pane-pogo").as_ptr()),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
    }
    .map_err(|e| e.to_string())?;

    add_icon(hwnd)?;
    Ok(hwnd)
}

/// Resource id of the app icon embedded by build.rs (assets/pane-pogo.rc).
const APP_ICON_ID: usize = 1;

/// The embedded app icon at the tray's small-icon size, or the generic Windows icon
/// if it cannot be loaded, so the tray never ends up without an icon.
fn tray_icon() -> Result<HICON, String> {
    let app = unsafe {
        let hinstance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        LoadImageW(
            Some(hinstance.into()),
            PCWSTR(APP_ICON_ID as *const u16),
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_DEFAULTCOLOR,
        )
    };
    match app {
        Ok(h) => Ok(HICON(h.0)),
        Err(e) => {
            applog::write(&format!(
                "Tray icon: embedded icon not loaded ({e}), using the default"
            ));
            unsafe { LoadIconW(None, IDI_APPLICATION) }.map_err(|e| e.to_string())
        }
    }
}

fn add_icon(hwnd: HWND) -> Result<(), String> {
    let icon = tray_icon()?;
    let mut data = base_data(hwnd);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_TRAY;
    data.hIcon = icon;
    copy_into(&mut data.szTip, TOOLTIP);
    if unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool() {
        Ok(())
    } else {
        Err("could not add the tray icon".into())
    }
}

/// Removes the tray icon. Call before the process exits.
pub fn remove(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &base_data(hwnd));
    }
}

/// Shows a balloon notification from the tray icon.
pub fn notify(hwnd: HWND, title: &str, text: &str, error: bool) {
    let mut data = base_data(hwnd);
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = if error { NIIF_ERROR } else { NIIF_INFO } | NOTIFY_ICON_INFOTIP_FLAGS(0);
    copy_into(&mut data.szInfoTitle, title);
    copy_into(&mut data.szInfo, text);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
    }
}

fn show_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let startup_flags = if startup::is_enabled() {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            CMD_SETTINGS,
            PCWSTR(wide("Settings...").as_ptr()),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING | startup_flags,
            CMD_STARTUP,
            PCWSTR(wide("Start with Windows").as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT, PCWSTR(wide("Exit").as_ptr()));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Required so the menu closes when the user clicks elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            Some(0),
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
    }
}

pub fn open_settings() {
    match std::env::current_exe() {
        Ok(exe) => {
            if let Err(e) = std::process::Command::new(exe).arg("--settings").spawn() {
                applog::write(&format!("Could not open settings: {e}"));
            }
        }
        Err(e) => applog::write(&format!("Could not open settings: {e}")),
    }
}

fn toggle_startup(hwnd: HWND) {
    let on = !startup::is_enabled();
    match startup::set_enabled(on) {
        Ok(Some(path)) => {
            applog::write(&format!("Start with Windows on: {}", path.display()));
            notify(
                hwnd,
                "Starts with Windows",
                &format!("pane-pogo will start from {}", path.display()),
                false,
            );
        }
        Ok(None) => applog::write("Start with Windows off"),
        Err(e) => {
            applog::write(&format!("Start with Windows: {e}"));
            notify(hwnd, "Could not turn on Start with Windows", &e, true);
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar_created = TASKBAR_CREATED.load(Ordering::Relaxed);
    if taskbar_created != 0 && msg == taskbar_created {
        // Explorer restarted and lost our icon. Delete first in case it still exists,
        // so NIM_ADD cannot fail on a duplicate.
        remove(hwnd);
        if let Err(e) = add_icon(hwnd) {
            applog::write(&format!("Re-adding tray icon failed: {e}"));
        } else {
            applog::write("Explorer restarted; tray icon re-added");
        }
        return LRESULT(0);
    }
    match msg {
        WM_TRAY if lparam.0 as u32 == WM_RBUTTONUP => show_menu(hwnd),
        WM_COMMAND => match wparam.0 & 0xFFFF {
            CMD_SETTINGS => open_settings(),
            CMD_STARTUP => toggle_startup(hwnd),
            CMD_EXIT => unsafe { PostQuitMessage(0) },
            _ => {}
        },
        WM_DESTROY => unsafe { PostQuitMessage(0) },
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
