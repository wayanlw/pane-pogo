//! Thin wrappers over the Win32 calls the actions need.

use crate::layout::Rect;
use std::mem::size_of;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromWindow,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect, GetWindowTextLengthW, IsIconic, IsWindow, IsWindowVisible,
    IsZoomed, MONITORINFOF_PRIMARY, SW_RESTORE, SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow,
    SetWindowPos, ShowWindow,
};
use windows::core::BOOL;

pub struct Monitor {
    pub handle: HMONITOR,
    pub work: Rect,
    pub primary: bool,
}

pub fn foreground() -> Option<HWND> {
    let h = unsafe { GetForegroundWindow() };
    if h.0.is_null() { None } else { Some(h) }
}

/// Visible, not minimised, not cloaked, titled window.
pub fn is_valid(hwnd: HWND) -> bool {
    unsafe {
        IsWindow(Some(hwnd)).as_bool()
            && IsWindowVisible(hwnd).as_bool()
            && !IsIconic(hwnd).as_bool()
            && GetWindowTextLengthW(hwnd) > 0
            && !is_cloaked(hwnd)
    }
}

/// Cloaked windows report as visible but are not shown: windows on other virtual
/// desktops, suspended Store apps, the closed Start menu.
fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    let ok = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut core::ffi::c_void,
            size_of::<u32>() as u32,
        )
    };
    ok.is_ok() && cloaked != 0
}

pub fn rect_of(hwnd: HWND) -> Option<Rect> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r).ok()? };
    Some(Rect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    })
}

/// All monitors in enumeration order, with work areas (taskbar excluded).
pub fn monitors() -> Vec<Monitor> {
    let mut out: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_proc),
            LPARAM(&mut out as *mut Vec<Monitor> as isize),
        );
    }
    out
}

unsafe extern "system" fn enum_proc(h: HMONITOR, _dc: HDC, _r: *mut RECT, lp: LPARAM) -> BOOL {
    let out = unsafe { &mut *(lp.0 as *mut Vec<Monitor>) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(h, &mut info) }.as_bool() {
        let w = info.rcWork;
        out.push(Monitor {
            handle: h,
            work: Rect {
                x: w.left,
                y: w.top,
                w: w.right - w.left,
                h: w.bottom - w.top,
            },
            primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    BOOL(1)
}

/// Index into `monitors` of the monitor holding most of the window.
pub fn monitor_index_of(hwnd: HWND, monitors: &[Monitor]) -> Option<usize> {
    let h = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    monitors.iter().position(|m| m.handle == h)
}

/// Moves and resizes a window. Restores it first if maximised.
pub fn place(hwnd: HWND, r: Rect) {
    unsafe {
        if IsZoomed(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetWindowPos(
            hwnd,
            None,
            r.x,
            r.y,
            r.w,
            r.h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

pub fn activate(hwnd: HWND) {
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
}
