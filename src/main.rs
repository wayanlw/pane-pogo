// GUI app: no console window. Output goes to the log file (see applog.rs).
#![windows_subsystem = "windows"]

mod actions;
mod applog;
mod config;
mod focus;
mod hotkeys;
mod layout;
mod settings;
mod startup;
mod tray;
mod win;

use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MB_ICONERROR, MB_OK, MSG, MessageBoxW, TranslateMessage,
    WM_HOTKEY,
};
use windows::core::PCWSTR;

fn main() {
    applog::rotate();

    // A second launch opens Settings instead of starting a second tray app,
    // whose hotkeys would all clash with the first.
    if std::env::args().any(|a| a == "--settings") || settings::app_running() {
        if let Err(e) = settings::run() {
            applog::write(&format!("Settings: {e}"));
            error_box(&format!("The settings window could not open.\n\n{e}"));
        }
        return;
    }

    let tray_hwnd = match tray::create() {
        Ok(h) => h,
        Err(e) => {
            applog::write(&format!("Tray icon: {e}"));
            error_box(&format!("Pane Pogo could not start its tray icon.\n\n{e}"));
            std::process::exit(1);
        }
    };
    applog::write("Started");

    // A broken bindings.json must not stop the app: start with no shortcuts and say so.
    let mut cfg = load_or_report(tray_hwnd).unwrap_or_default();
    let hook = focus::install();
    let mut registered = register_all(&cfg);
    if config::took_created_defaults() {
        warn_taken_defaults(tray_hwnd);
    }

    let mut msg = MSG::default();
    loop {
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if r.0 <= 0 {
            break; // 0 = WM_QUIT, -1 = error
        }
        if msg.message == WM_HOTKEY {
            let id = msg.wParam.0;
            if let Some(binding) = id.checked_sub(1).and_then(|i| cfg.bindings.get(i)) {
                actions::run(binding);
            }
        } else if msg.message == settings::MSG_SUSPEND {
            // Settings window is recording a shortcut: release ours so the key press reaches it.
            unregister_all(&registered);
            registered.clear();
        } else if msg.message == settings::MSG_RELOAD {
            unregister_all(&registered);
            if let Some(new_cfg) = load_or_report(tray_hwnd) {
                cfg = new_cfg;
                applog::write("Reloaded bindings");
            }
            registered = register_all(&cfg);
        } else {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    unregister_all(&registered);
    tray::remove(tray_hwnd);
    focus::uninstall(hook);
}

/// Registers every binding and writes the status file. Returns the ids that succeeded.
/// Hotkey id = index into cfg.bindings + 1.
fn register_all(cfg: &config::Config) -> Vec<i32> {
    let mut registered = Vec::new();
    let mut status = config::Status::default();
    for (i, binding) in cfg.bindings.iter().enumerate() {
        let id = i as i32 + 1;
        let chord = match hotkeys::parse(&binding.shortcut) {
            Ok(c) => c,
            Err(e) => {
                applog::write(&format!(
                    "Skipped {} ({:?}): {e}",
                    binding.shortcut, binding.action
                ));
                continue;
            }
        };
        match hotkeys::register(id, chord) {
            Ok(()) => {
                applog::write(&format!(
                    "Registered {} -> {:?}",
                    binding.shortcut, binding.action
                ));
                registered.push(id);
            }
            Err(e) => {
                applog::write(&format!(
                    "Skipped {} ({:?}): {e}",
                    binding.shortcut, binding.action
                ));
                status.in_use.push(binding.shortcut.clone());
            }
        }
    }
    config::save_status(&status);
    registered
}

/// Loads bindings.json. On error, logs it and shows a tray notification, then returns None.
fn load_or_report(tray_hwnd: windows::Win32::Foundation::HWND) -> Option<config::Config> {
    match config::load() {
        Ok(c) => Some(c),
        Err(e) => {
            applog::write(&format!("Config error: {e}"));
            tray::notify(
                tray_hwnd,
                "Pane Pogo: shortcuts file has an error",
                "Your shortcuts are not active. Open Settings from the tray icon to fix it.",
                true,
            );
            None
        }
    }
}

/// On a first run, tells the user which default shortcuts another program already holds.
/// Settings also marks each of those rows, so this only points the user there.
fn warn_taken_defaults(tray_hwnd: windows::Win32::Foundation::HWND) {
    let taken = config::load_status().in_use;
    if let Some(text) = taken_message(&taken) {
        tray::notify(
            tray_hwnd,
            "Pane Pogo: some shortcuts are not active",
            &text,
            false,
        );
    }
}

fn taken_message(taken: &[String]) -> Option<String> {
    if taken.is_empty() {
        return None;
    }
    Some(format!(
        "Another program already uses: {}. Open Settings from the tray icon and pick different shortcuts.",
        taken.join(", ")
    ))
}

fn error_box(text: &str) {
    let text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let title: Vec<u16> = "Pane Pogo"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn unregister_all(ids: &[i32]) {
    for &id in ids {
        hotkeys::unregister(id);
    }
}

#[cfg(test)]
mod tests {
    use super::taken_message;

    #[test]
    fn no_message_when_nothing_is_taken() {
        assert!(taken_message(&[]).is_none());
    }

    #[test]
    fn message_lists_taken_shortcuts() {
        let msg = taken_message(&["Alt+Shift+W".to_string(), "Alt+Shift+D".to_string()]).unwrap();
        assert!(msg.contains("Alt+Shift+W, Alt+Shift+D"));
        assert!(msg.len() < 256, "balloon text must fit");
    }
}
