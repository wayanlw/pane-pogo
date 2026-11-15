//! Runs a binding's action against the current window.

use crate::config::{Action, Binding, Focus, PairSettings};
use crate::layout::{self, Rect};
use crate::{focus, win};
use windows::Win32::Foundation::HWND;

pub fn run(binding: &Binding) {
    let Some(cur) = win::foreground().filter(|&h| win::is_valid(h)) else {
        return;
    };
    let monitors = win::monitors();
    let Some(primary) = monitors.iter().find(|m| m.primary).map(|m| m.work) else {
        return;
    };
    let secondary = monitors.iter().find(|m| !m.primary).map(|m| m.work);

    match binding.action {
        action if action.is_pair() => {
            run_pair(cur, action, binding.pair_settings(), primary, secondary);
        }
        Action::TileLeft => place_half(cur, primary, true),
        Action::TileRight => place_half(cur, primary, false),
        Action::TileLeftSecondary => {
            if let Some(sec) = secondary {
                place_half(cur, sec, true);
            }
        }
        Action::TileRightSecondary => {
            if let Some(sec) = secondary {
                place_half(cur, sec, false);
            }
        }
        Action::FullPrimary => win::place(cur, primary),
        Action::FullSecondary => {
            if let Some(sec) = secondary {
                win::place(cur, sec);
            }
        }
        Action::SwapWindows => swap_with_previous(cur),
        Action::MoveToNextMonitor => move_to_next_monitor(cur, &monitors),
        _ => {}
    }
}

fn run_pair(cur: HWND, action: Action, s: PairSettings, primary: Rect, secondary: Option<Rect>) {
    let arrangement = action.layout();
    match previous(cur) {
        Some(other) => {
            let p = layout::pair(arrangement, primary, secondary, s.order, s.monitor);
            win::place(cur, p.current);
            win::place(other, p.other);
            win::activate(match s.focus {
                Focus::Current => cur,
                Focus::Previous => other,
            });
        }
        None => {
            // No valid previous window: the current window alone takes its usual place.
            let r = layout::solo(arrangement, primary, secondary, s.order, s.monitor);
            win::place(cur, r);
            win::activate(cur);
        }
    }
}

fn place_half(hwnd: HWND, area: Rect, left: bool) {
    let w = area.w / 2;
    let x = if left { area.x } else { area.x + w };
    win::place(
        hwnd,
        Rect {
            x,
            y: area.y,
            w,
            h: area.h,
        },
    );
}

fn swap_with_previous(cur: HWND) {
    let Some(other) = previous(cur) else {
        return;
    };
    let (Some(a), Some(b)) = (win::rect_of(cur), win::rect_of(other)) else {
        return;
    };
    win::place(cur, b);
    win::place(other, a);
    win::activate(other);
}

fn move_to_next_monitor(cur: HWND, monitors: &[win::Monitor]) {
    if monitors.len() < 2 {
        return;
    }
    let (Some(from), Some(rect)) = (win::monitor_index_of(cur, monitors), win::rect_of(cur)) else {
        return;
    };
    let to = &monitors[(from + 1) % monitors.len()].work;
    let src = monitors[from].work;

    let mut x = to.x + (rect.x - src.x);
    let mut y = to.y + (rect.y - src.y);
    x = x.min(to.x + to.w - rect.w).max(to.x);
    y = y.min(to.y + to.h - rect.h).max(to.y);
    win::place(
        cur,
        Rect {
            x,
            y,
            w: rect.w,
            h: rect.h,
        },
    );
}

/// Last valid window before `cur` in real focus history.
fn previous(cur: HWND) -> Option<HWND> {
    focus::recent()
        .into_iter()
        .find(|&h| h != cur && win::is_valid(h))
}
