//! Settings window. Runs as a separate process (`pane-pogo.exe --settings`), so its
//! event loop does not clash with the tray app's Win32 message loop.

use crate::config::{self, Action, Binding, Config, Focus, MonitorRole, Order, PairSettings};
use crate::hotkeys;
use crate::layout::Layout;
use eframe::egui::{self, Color32, RichText};
use std::time::{Duration, Instant};

/// Thread messages the tray app handles. Must match main.rs.
pub const MSG_RELOAD: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 3;
pub const MSG_SUSPEND: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 4;

const TWO_WINDOW_ACTIONS: [Action; 5] = [
    Action::TileWindows,
    Action::Tile2to1,
    Action::Tile1to2,
    Action::TileDualMonitors,
    Action::SwapWindows,
];

const ONE_WINDOW_ACTIONS: [Action; 1] = [Action::MoveToNextMonitor];

/// Collapsed-row columns. Every row is cut into the same rectangles, so label length never
/// moves the shortcut field or the arrow.
const COL_PREVIEW: f32 = 60.0;
const COL_NAME: f32 = 190.0;
const COL_SHORTCUT: f32 = 200.0;
const COL_ARROW: f32 = 28.0;
const COL_GAP: f32 = 8.0;
const ROW_H: f32 = 40.0;

const ERROR: Color32 = Color32::from_rgb(220, 80, 80);
const WARNING: Color32 = Color32::from_rgb(220, 160, 40);

pub fn action_label(a: Action) -> &'static str {
    match a {
        Action::TileWindows => "Side by side (½ + ½)",
        Action::Tile2to1 => "Side by side (2/3 left + 1/3 right)",
        Action::Tile1to2 => "Side by side (1/3 left + 2/3 right)",
        Action::TileDualMonitors => "Full screen on separate monitors",
        Action::SwapWindows => "Swap places",
        Action::TileLeft => "Left half of primary monitor",
        Action::TileRight => "Right half of primary monitor",
        Action::TileLeftSecondary => "Left half of secondary monitor",
        Action::TileRightSecondary => "Right half of secondary monitor",
        Action::MoveToNextMonitor => "Move to next monitor",
        Action::FullPrimary => "Full screen on primary monitor",
        Action::FullSecondary => "Full screen on secondary monitor",
    }
}

fn action_help(a: Action) -> &'static str {
    match a {
        Action::TileWindows => "Puts the current and previous window side by side, half each.",
        Action::Tile2to1 => "Side by side. The left part is 2/3 wide, the right part 1/3.",
        Action::Tile1to2 => "Side by side. The left part is 1/3 wide, the right part 2/3.",
        Action::TileDualMonitors => "Each window fills a different monitor. Needs two monitors.",
        Action::SwapWindows => "The current and previous window swap position and size.",
        Action::TileLeft => "Moves the current window to the left half of the primary monitor.",
        Action::TileRight => "Moves the current window to the right half of the primary monitor.",
        Action::TileLeftSecondary => {
            "Moves the current window to the left half of the secondary monitor."
        }
        Action::TileRightSecondary => {
            "Moves the current window to the right half of the secondary monitor."
        }
        Action::MoveToNextMonitor => "Moves the current window to the next monitor, same size.",
        Action::FullPrimary => {
            "The current window fills the primary monitor. The taskbar stays visible."
        }
        Action::FullSecondary => {
            "The current window fills the secondary monitor. The taskbar stays visible."
        }
    }
}

const HELP_ORDER: &str =
    "Which side the current window goes to. The previous window takes the other side.";
const HELP_TILE_ON: &str =
    "Which monitor the two windows are tiled on. Secondary is your first non-primary monitor.";
const HELP_GOES_TO: &str =
    "The monitor the current window fills. The previous window fills the other monitor.";
const HELP_FOCUS: &str = "Which window you can type in after the shortcut runs.";
const HELP_MONITOR: &str =
    "The monitor the current window goes to. Secondary is your first non-primary monitor.";
const HELP_SIDE: &str = "Which half of that monitor the current window fills.";

// Full screen and half are stored as one action per monitor (and side). The drop-down picks the kind,
// and the buttons below pick the monitor and side. This keeps existing bindings.json files valid.
fn is_full(a: Action) -> bool {
    matches!(a, Action::FullPrimary | Action::FullSecondary)
}

fn is_half(a: Action) -> bool {
    matches!(
        a,
        Action::TileLeft
            | Action::TileRight
            | Action::TileLeftSecondary
            | Action::TileRightSecondary
    )
}

fn monitor_of(a: Action) -> MonitorRole {
    match a {
        Action::FullSecondary | Action::TileLeftSecondary | Action::TileRightSecondary => {
            MonitorRole::Secondary
        }
        _ => MonitorRole::Primary,
    }
}

fn side_is_left(a: Action) -> bool {
    !matches!(a, Action::TileRight | Action::TileRightSecondary)
}

fn full_action(m: MonitorRole) -> Action {
    match m {
        MonitorRole::Primary => Action::FullPrimary,
        MonitorRole::Secondary => Action::FullSecondary,
    }
}

fn half_action(m: MonitorRole, left: bool) -> Action {
    match (m, left) {
        (MonitorRole::Primary, true) => Action::TileLeft,
        (MonitorRole::Primary, false) => Action::TileRight,
        (MonitorRole::Secondary, true) => Action::TileLeftSecondary,
        (MonitorRole::Secondary, false) => Action::TileRightSecondary,
    }
}

/// Label shown in the closed drop-down.
fn picker_label(a: Action) -> &'static str {
    if is_full(a) {
        "Full screen on one monitor"
    } else if is_half(a) {
        "Half of one monitor"
    } else {
        action_label(a)
    }
}
const HELP_SHORTCUT: &str =
    "Click, then press the keys. Use Ctrl, Alt or Shift with a letter, number or F1–F12.";

/// Settings shown under a two-window action. Swap has none.
fn has_pair_settings(a: Action) -> bool {
    a.is_pair()
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    All,
    One,
    Two,
}

struct App {
    tab: Tab,
    /// Row whose settings are expanded. At most one (accordion).
    expanded: Option<usize>,
    bindings: Vec<Binding>,
    saved: Vec<Binding>,
    status: config::Status,
    message: String,
    /// Row whose shortcut is being recorded.
    recording: Option<usize>,
    record_hint: String,
    /// When to re-read the status file after saving.
    status_check_at: Option<Instant>,
    confirm_close: bool,
    allow_close: bool,
    /// True once the tray app has been seen running. Used to detect that it exited.
    tray_seen: bool,
    next_tray_check: Instant,
    monitor_count: usize,
    /// Set when bindings.json could not be read. Saving is blocked so the file is
    /// never overwritten without the user choosing to.
    load_error: Option<String>,
}

impl App {
    fn new() -> Self {
        let (bindings, load_error) = match config::load() {
            Ok(cfg) => (cfg.bindings, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        Self {
            tab: Tab::All,
            expanded: None,
            saved: bindings.clone(),
            bindings,
            status: config::load_status(),
            message: String::new(),
            load_error,
            recording: None,
            record_hint: String::new(),
            status_check_at: None,
            confirm_close: false,
            allow_close: false,
            tray_seen: false,
            next_tray_check: Instant::now(),
            monitor_count: crate::win::monitors().len(),
        }
    }

    fn dirty(&self) -> bool {
        self.bindings != self.saved
    }

    /// Problem that blocks saving, for row `i`.
    fn blocking_problem(&self, i: usize) -> Option<String> {
        let b = &self.bindings[i];
        if b.shortcut.trim().is_empty() {
            return Some("Click the shortcut box and press the keys you want.".into());
        }
        if let Err(e) = hotkeys::parse(&b.shortcut) {
            return Some(format!("Invalid shortcut: {e}"));
        }
        if self.bindings[..i]
            .iter()
            .any(|o| o.shortcut.eq_ignore_ascii_case(&b.shortcut))
        {
            return Some(
                "Same shortcut as another row. Each shortcut can do only one thing.".into(),
            );
        }
        None
    }

    /// Problem that does not block saving, for row `i`.
    fn warning(&self, i: usize) -> Option<String> {
        let b = &self.bindings[i];
        let saved_unchanged = self
            .saved
            .iter()
            .any(|s| s.shortcut.eq_ignore_ascii_case(&b.shortcut));
        if saved_unchanged
            && app_running()
            && self
                .status
                .in_use
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&b.shortcut))
        {
            return Some("Another program already uses this shortcut, so it does nothing. Pick a different one.".into());
        }
        if self.monitor_count < 2 && needs_secondary(b) {
            return Some(
                "Only one monitor is connected. This falls back to the primary monitor.".into(),
            );
        }
        None
    }

    fn any_blocking(&self) -> bool {
        (0..self.bindings.len()).any(|i| self.blocking_problem(i).is_some())
    }

    /// Re-reads bindings.json, replacing the list shown.
    fn reload_file(&mut self) {
        match config::load() {
            Ok(cfg) => {
                self.bindings = cfg.bindings.clone();
                self.saved = cfg.bindings;
                self.load_error = None;
                self.message = "Shortcuts file loaded.".into();
                notify_app(MSG_RELOAD);
            }
            Err(e) => self.load_error = Some(e),
        }
    }

    /// Keeps the broken file as bindings.broken.json and starts with no shortcuts.
    fn start_fresh(&mut self) {
        let Ok(path) = config::path() else {
            return;
        };
        let backup = path.with_file_name("bindings.broken.json");
        if let Err(e) = std::fs::rename(&path, &backup) {
            self.message = format!("Could not move the broken file aside: {e}");
            return;
        }
        self.reload_file();
        if self.load_error.is_none() {
            self.message = format!(
                "Started fresh. Your old file is kept as {}.",
                backup.display()
            );
        }
    }

    fn save(&mut self) {
        if self.load_error.is_some() {
            self.message = "Not saved. Fix or replace the broken shortcuts file first.".into();
            return;
        }
        if self.any_blocking() {
            self.message = "Not saved. Fix the rows marked in red first.".into();
            return;
        }
        if let Err(e) = config::save(&Config {
            bindings: self.bindings.clone(),
        }) {
            self.message = format!("Not saved: {e}");
            return;
        }
        self.saved = self.bindings.clone();
        if notify_app(MSG_RELOAD) {
            self.message = "Saved. Your shortcuts are active now.".into();
            self.status_check_at = Some(Instant::now() + Duration::from_millis(500));
        } else {
            self.message = "Saved. Start pane-pogo to use them.".into();
        }
    }

    fn start_recording(&mut self, i: usize) {
        self.recording = Some(i);
        self.record_hint.clear();
        notify_app(MSG_SUSPEND);
    }

    fn stop_recording(&mut self) {
        if self.recording.take().is_some() {
            notify_app(MSG_RELOAD);
            self.status_check_at = Some(Instant::now() + Duration::from_millis(500));
        }
    }

    /// Reads key presses while a row is recording.
    fn handle_recording(&mut self, ctx: &egui::Context) {
        let Some(i) = self.recording else {
            return;
        };
        if !ctx.input(|inp| inp.viewport().focused.unwrap_or(true)) {
            self.stop_recording();
            return;
        }
        let events = ctx.input(|inp| inp.events.clone());
        for ev in events {
            let egui::Event::Key {
                key,
                physical_key,
                pressed: true,
                modifiers,
                ..
            } = ev
            else {
                continue;
            };
            if key == egui::Key::Escape {
                self.stop_recording();
                return;
            }
            if !(modifiers.ctrl || modifiers.alt || modifiers.shift) {
                self.record_hint =
                    "Hold Ctrl, Alt or Shift, then press a letter, number or F1–F12.".into();
                continue;
            }
            match recorded_shortcut(modifiers, key, physical_key) {
                Ok(text) => {
                    self.bindings[i].shortcut = text;
                    self.stop_recording();
                    return;
                }
                Err(name) => {
                    self.record_hint =
                        format!("{name} can't be used. Use a letter, number or F1–F12.");
                }
            }
        }
    }
}

/// Builds the shortcut text for a recorded key press. Tries the key egui names first, which
/// follows the keyboard layout, then the physical key. Shift+1 on a US layout arrives as `!`
/// (`Key::Exclamationmark`), so the physical key turns it back into `1`.
/// Returns the named key's name when neither makes a valid shortcut.
fn recorded_shortcut(
    modifiers: egui::Modifiers,
    key: egui::Key,
    physical_key: Option<egui::Key>,
) -> Result<String, &'static str> {
    let mut parts = Vec::new();
    if modifiers.ctrl {
        parts.push("Ctrl");
    }
    if modifiers.alt {
        parts.push("Alt");
    }
    if modifiers.shift {
        parts.push("Shift");
    }
    let prefix = parts.join("+");
    std::iter::once(key)
        .chain(physical_key)
        .map(|k| format!("{prefix}+{}", k.name()))
        .find(|text| hotkeys::parse(text).is_ok())
        .ok_or(key.name())
}

impl Drop for App {
    fn drop(&mut self) {
        // Never leave the tray app with its hotkeys paused.
        self.stop_recording();
    }
}

fn needs_secondary(b: &Binding) -> bool {
    match b.action {
        Action::TileLeftSecondary
        | Action::TileRightSecondary
        | Action::MoveToNextMonitor
        | Action::FullSecondary => true,
        Action::TileDualMonitors => true,
        a if a.is_pair() => b.pair_settings().monitor == MonitorRole::Secondary,
        _ => false,
    }
}

fn tray_thread() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};
    use windows::core::PCWSTR;
    let class: Vec<u16> = "PanePogoTray"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let hwnd = FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()).ok()?;
        if hwnd.0.is_null() {
            return None;
        }
        Some(GetWindowThreadProcessId(hwnd, None))
    }
}

pub fn app_running() -> bool {
    tray_thread().is_some()
}

/// Posts a thread message to the running tray app. Returns false if it is not running.
fn notify_app(msg: u32) -> bool {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
    match tray_thread() {
        Some(tid) => unsafe { PostThreadMessageW(tid, msg, WPARAM(0), LPARAM(0)).is_ok() },
        None => false,
    }
}

/// Where a binding puts windows: (monitor 0 = primary / 1 = secondary, start, end as fractions
/// of that monitor's width, current window or not). Drawn by `placement_icon`. Two-window actions
/// use the same monitor and side choices as the layout (`layout.rs`, `pair`).
fn placement_regions(b: &Binding) -> Vec<(usize, f32, f32, bool)> {
    let a = b.action;
    if a.is_pair() {
        let p = b.pair_settings();
        let chosen = usize::from(p.monitor == MonitorRole::Secondary);
        return match a.layout() {
            Layout::Across => vec![(0, 0.0, 1.0, chosen == 0), (1, 0.0, 1.0, chosen == 1)],
            Layout::Split { left_share } => {
                let left_current = p.order == Order::CurrentLeft;
                vec![
                    (chosen, 0.0, left_share, left_current),
                    (chosen, left_share, 1.0, !left_current),
                ]
            }
        };
    }
    match a {
        Action::TileLeft => vec![(0, 0.0, 0.5, true)],
        Action::TileRight => vec![(0, 0.5, 1.0, true)],
        Action::TileLeftSecondary => vec![(1, 0.0, 0.5, true)],
        Action::TileRightSecondary => vec![(1, 0.5, 1.0, true)],
        Action::FullPrimary => vec![(0, 0.0, 1.0, true)],
        Action::FullSecondary => vec![(1, 0.0, 1.0, true)],
        Action::MoveToNextMonitor => vec![(0, 0.0, 1.0, false), (1, 0.0, 1.0, true)],
        _ => vec![],
    }
}

/// What one collapsed row reports back. The rects are where the shortcut field and arrow landed.
struct RowOutput {
    /// The whole row; a click outside the shortcut field toggles the settings.
    row: egui::Response,
    shortcut: egui::Response,
    /// The expand/collapse control.
    arrow: egui::Response,
}

/// Runs `add` inside `rect`, left-aligned and vertically centred.
fn in_rect<R>(ui: &mut egui::Ui, rect: egui::Rect, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        add,
    )
    .inner
}

/// Draws one collapsed shortcut row: preview | name and monitor | shortcut | arrow.
/// The row is allocated once at a fixed width, then cut into fixed column rects. Settings,
/// recording hints and errors are drawn below the row by the caller.
fn shortcut_row(ui: &mut egui::Ui, b: &Binding, expanded: bool, recording: bool) -> RowOutput {
    let widths = [COL_PREVIEW, COL_NAME, COL_SHORTCUT, COL_ARROW];
    let total = widths.iter().sum::<f32>() + COL_GAP * (widths.len() - 1) as f32;
    // Allocated before its contents, so the shortcut button sits on top and keeps its own
    // clicks. A click anywhere else on the row toggles the settings.
    let (row, row_resp) = ui.allocate_exact_size(egui::vec2(total, ROW_H), egui::Sense::click());
    if row_resp.hovered() {
        ui.painter()
            .rect_filled(row, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let mut x = row.min.x;
    let [preview, name, shortcut_col, arrow_col] = widths.map(|w| {
        let r = egui::Rect::from_min_size(egui::pos2(x, row.min.y), egui::vec2(w, ROW_H));
        x += w + COL_GAP;
        r
    });

    placement_icon(
        ui,
        egui::Rect::from_center_size(preview.center(), egui::vec2(56.0, 34.0)),
        b,
    );

    let title = egui::Label::new(RichText::new(picker_label(b.action)).strong())
        .truncate()
        .selectable(false);
    match monitor_text(b) {
        Some(m) => {
            let half = egui::vec2(name.width(), ROW_H / 2.0);
            in_rect(ui, egui::Rect::from_min_size(name.min, half), |ui| {
                ui.add(title)
            });
            let sub = egui::Label::new(RichText::new(m).weak())
                .truncate()
                .selectable(false);
            let low = egui::pos2(name.min.x, name.min.y + ROW_H / 2.0);
            in_rect(ui, egui::Rect::from_min_size(low, half), |ui| ui.add(sub));
        }
        None => {
            in_rect(ui, name, |ui| ui.add(title));
        }
    }

    let text = if recording {
        "Press keys…  (Esc to cancel)".to_string()
    } else if b.shortcut.is_empty() {
        "Click to set".to_string()
    } else {
        b.shortcut.clone()
    };
    let field = egui::Rect::from_center_size(shortcut_col.center(), egui::vec2(COL_SHORTCUT, 28.0));
    let resp = ui
        .put(
            field,
            egui::Button::new(text).selected(recording).truncate(),
        )
        .on_hover_text(HELP_SHORTCUT);

    // Expand/collapse chevron, painted as lines: a text glyph like "▶" falls back to the
    // coloured emoji font.
    let arrow_rect = egui::Rect::from_center_size(arrow_col.center(), egui::vec2(COL_ARROW, 28.0));
    let arrow_resp = ui.interact(arrow_rect, row_resp.id.with("arrow"), egui::Sense::click());
    let c = arrow_rect.center();
    let s = 4.0;
    let points = if expanded {
        vec![
            c + egui::vec2(-s, -s / 2.0),
            c + egui::vec2(0.0, s / 2.0),
            c + egui::vec2(s, -s / 2.0),
        ]
    } else {
        vec![
            c + egui::vec2(-s / 2.0, -s),
            c + egui::vec2(s / 2.0, 0.0),
            c + egui::vec2(-s / 2.0, s),
        ]
    };
    let color = if arrow_resp.hovered() || row_resp.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    ui.painter().line(points, egui::Stroke::new(1.5, color));

    RowOutput {
        row: row_resp,
        shortcut: resp,
        arrow: arrow_resp,
    }
}

/// Small monitor diagram, so the user sees where the window goes. Paints into `rect`.
fn placement_icon(ui: &egui::Ui, rect: egui::Rect, b: &Binding) {
    let painter = ui.painter_at(rect);
    let gap = 3.0;
    let primary_w = (rect.width() - gap) * 0.6;
    let secondary_w = rect.width() - gap - primary_w;
    let monitors = [
        egui::Rect::from_min_size(rect.min, egui::vec2(primary_w, rect.height())),
        egui::Rect::from_min_size(
            egui::pos2(rect.min.x + primary_w + gap, rect.min.y),
            egui::vec2(secondary_w, rect.height()),
        ),
    ];
    let visuals = ui.visuals();
    let outline = visuals.weak_text_color();
    let fill = visuals.selection.bg_fill;
    for m in monitors {
        painter.rect_stroke(
            m,
            2.0,
            egui::Stroke::new(1.0, outline),
            egui::StrokeKind::Inside,
        );
    }
    for (m, x0, x1, current) in placement_regions(b) {
        let mon = monitors[m];
        let r = egui::Rect::from_min_max(
            egui::pos2(mon.min.x + mon.width() * x0, mon.min.y),
            egui::pos2(mon.min.x + mon.width() * x1, mon.max.y),
        )
        .shrink(1.5);
        let color = if current {
            fill
        } else {
            fill.linear_multiply(0.35)
        };
        painter.rect_filled(r, 1.0, color);
    }
}

/// Short line under a row's name: the monitor the window goes to, if the action has one.
fn monitor_text(b: &Binding) -> Option<&'static str> {
    let monitor = if b.action.is_pair() {
        b.pair_settings().monitor
    } else if is_full(b.action) || is_half(b.action) {
        monitor_of(b.action)
    } else if b.action == Action::MoveToNextMonitor {
        return Some("Next monitor");
    } else {
        return None;
    };
    Some(match monitor {
        MonitorRole::Primary => "Primary monitor",
        MonitorRole::Secondary => "Secondary monitor",
    })
}

/// Two-option segmented control. The chosen option gets a neutral fill, not the selection blue.
fn choice<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, options: [(T, &str); 2]) {
    let stroke = ui.visuals().widgets.inactive.bg_stroke;
    let chosen = ui.visuals().widgets.active.bg_fill;
    egui::Frame::new()
        .stroke(stroke)
        .corner_radius(4)
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (v, label) in options {
                    let fill = if *value == v {
                        chosen
                    } else {
                        Color32::TRANSPARENT
                    };
                    if ui
                        .add(egui::Button::new(label).fill(fill).corner_radius(3))
                        .clicked()
                    {
                        *value = v;
                    }
                }
            });
        });
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.handle_recording(&ctx);

        if let Some(at) = self.status_check_at {
            if Instant::now() >= at {
                self.status = config::load_status();
                self.status_check_at = None;
            } else {
                ctx.request_repaint_after(at - Instant::now());
            }
        }

        // Close when the tray app exits. Settings is its own process, so it only learns this by polling.
        // Only a tray app seen running counts, so a standalone Settings window stays open.
        let now = Instant::now();
        if now >= self.next_tray_check {
            self.next_tray_check = now + Duration::from_secs(1);
            if app_running() {
                self.tray_seen = true;
            } else if self.tray_seen {
                self.tray_seen = false;
                if self.dirty() {
                    self.confirm_close = true;
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        ctx.request_repaint_after(self.next_tray_check.saturating_duration_since(now));

        if ctx.input(|i| i.viewport().close_requested()) && self.dirty() && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }

        egui::CentralPanel::default().show(root, |ui| {
            ui.heading("pane-pogo shortcuts");
            ui.label(
                "The current window is the one you are using when you press a shortcut. \
                 The previous window is the one you used just before it.",
            );
            if let Some(err) = self.load_error.clone() {
                ui.add_space(4.0);
                ui.group(|ui| {
                    ui.label(RichText::new("Your shortcuts file could not be read, so no shortcuts are active.").color(ERROR).strong());
                    ui.label(RichText::new(err).weak());
                    ui.horizontal(|ui| {
                        if ui.button("Open file").on_hover_text("Opens bindings.json in Notepad so you can fix it.").clicked()
                            && let Ok(p) = config::path()
                        {
                            let _ = std::process::Command::new("notepad.exe").arg(p).spawn();
                        }
                        if ui.button("Try again").on_hover_text("Reads the file again after you fix it.").clicked() {
                            self.reload_file();
                        }
                        if ui.button("Start fresh").on_hover_text("Keeps the broken file as bindings.broken.json and starts from the default shortcuts.").clicked() {
                            self.start_fresh();
                        }
                    });
                });
                ui.add_space(8.0);
                return;
            }
            if !app_running() {
                ui.add_space(4.0);
                ui.label(RichText::new("pane-pogo is not running, so shortcuts do nothing yet. Run pane-pogo.exe to start it.").color(WARNING));
            }
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::All, "All");
                ui.selectable_value(&mut self.tab, Tab::One, "One window");
                ui.selectable_value(&mut self.tab, Tab::Two, "Two windows");
            });
            ui.add_space(4.0);
            let list_height = (ui.available_height() - 60.0).max(100.0);
            let mut remove = None;
            let mut record = None;
            let mut toggle = None;
            egui::ScrollArea::vertical().max_height(list_height).auto_shrink([false, false]).show(ui, |ui| {
                if self.bindings.is_empty() {
                    ui.label("No shortcuts yet. Click “Add shortcut” to create one.");
                }
                let tab = self.tab;
                let rows: Vec<usize> = (0..self.bindings.len())
                    .filter(|&i| match tab {
                        Tab::All => true,
                        Tab::One => !self.bindings[i].action.is_pair(),
                        Tab::Two => self.bindings[i].action.is_pair(),
                    })
                    .collect();
                if rows.is_empty() {
                    ui.label(match tab {
                        Tab::All => "No shortcuts yet. Click “Add shortcut” to create one.",
                        Tab::One => "No one-window shortcuts yet.",
                        Tab::Two => "No two-window shortcuts yet.",
                    });
                }
                for i in rows {
                    let blocking = self.blocking_problem(i);
                    let warning = self.warning(i);
                    let recording = self.recording == Some(i);
                    let b = &mut self.bindings[i];

                    let out = shortcut_row(ui, b, self.expanded == Some(i), recording);
                    if out.shortcut.clicked() && !recording {
                        record = Some(i);
                    }
                    if out.row.clicked() || out.arrow.clicked() {
                        toggle = Some(i);
                    }
                    if recording && !self.record_hint.is_empty() {
                        ui.label(RichText::new(&self.record_hint).color(WARNING));
                    }
                    if let Some(p) = &blocking {
                        ui.label(RichText::new(p).color(ERROR));
                    } else if let Some(w) = &warning {
                        ui.label(RichText::new(w).color(WARNING));
                    }

                    if self.expanded == Some(i) {
                        ui.indent(("body", i), |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Action:").on_hover_text(action_help(b.action));
                            egui::ComboBox::from_id_salt(("action", i))
                                .selected_text(picker_label(b.action))
                                .width(270.0)
                                .show_ui(ui, |ui| {
                                    ui.label(RichText::new("One window").weak());
                                    let full = is_full(b.action);
                                    if ui.selectable_label(full, "Full screen on one monitor").on_hover_text(action_help(Action::FullPrimary)).clicked() && !full {
                                        b.action = Action::FullPrimary;
                                    }
                                    let half = is_half(b.action);
                                    if ui.selectable_label(half, "Half of one monitor").on_hover_text(action_help(Action::TileLeft)).clicked() && !half {
                                        b.action = Action::TileLeft;
                                    }
                                    for a in ONE_WINDOW_ACTIONS {
                                        ui.selectable_value(&mut b.action, a, action_label(a)).on_hover_text(action_help(a));
                                    }
                                    ui.separator();
                                    ui.label(RichText::new("Two windows").weak());
                                    for a in TWO_WINDOW_ACTIONS {
                                        ui.selectable_value(&mut b.action, a, action_label(a)).on_hover_text(action_help(a));
                                    }
                                });
                        });

                        if is_full(b.action) || is_half(b.action) {
                            let full = is_full(b.action);
                            let mut monitor = monitor_of(b.action);
                            let mut left = side_is_left(b.action);
                            egui::Grid::new(("single", i)).num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                                ui.label("Monitor:").on_hover_text(HELP_MONITOR);
                                ui.horizontal(|ui| {
                                    choice(ui, &mut monitor, [
                                        (MonitorRole::Primary, "Primary monitor"),
                                        (MonitorRole::Secondary, "Secondary monitor"),
                                    ]);
                                });
                                ui.end_row();
                                if !full {
                                    ui.label("Side:").on_hover_text(HELP_SIDE);
                                    ui.horizontal(|ui| {
                                        choice(ui, &mut left, [(true, "Left half"), (false, "Right half")]);
                                    });
                                    ui.end_row();
                                }
                            });
                            b.action = if full {
                                full_action(monitor)
                            } else {
                                half_action(monitor, left)
                            };
                        }

                        if has_pair_settings(b.action) {
                            let mut p: PairSettings = b.pair_settings();
                            egui::Grid::new(("pair", i)).num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                                let split = matches!(b.action.layout(), Layout::Split { .. });
                                if split {
                                    ui.label("Current window on:").on_hover_text(HELP_ORDER);
                                    ui.horizontal(|ui| {
                                        choice(ui, &mut p.order, [(Order::CurrentLeft, "Left"), (Order::CurrentRight, "Right")]);
                                    });
                                    ui.end_row();
                                    ui.label("Monitor:").on_hover_text(HELP_TILE_ON);
                                } else {
                                    ui.label("Current window on:").on_hover_text(HELP_GOES_TO);
                                }
                                ui.horizontal(|ui| {
                                    choice(ui, &mut p.monitor, [
                                        (MonitorRole::Primary, "Primary monitor"),
                                        (MonitorRole::Secondary, "Secondary monitor"),
                                    ]);
                                });
                                ui.end_row();
                                ui.label("Final focus:").on_hover_text(HELP_FOCUS);
                                ui.horizontal(|ui| {
                                    choice(ui, &mut p.focus, [
                                        (Focus::Current, "Current window"),
                                        (Focus::Previous, "Previous window"),
                                    ]);
                                });
                                ui.end_row();
                            });
                            // Write only on a real change: a row saved without `pair` uses the
                            // action's default, and storing that default would mark it unsaved.
                            if p != b.pair_settings() {
                                b.pair = Some(p);
                            }
                        } else {
                            b.pair = None;
                        }

                        ui.horizontal(|ui| {
                            if ui.button("Remove").clicked() {
                                remove = Some(i);
                            }
                        });
                        });
                    }
                    ui.separator();
                }
            });

            if let Some(i) = toggle {
                self.expanded = if self.expanded == Some(i) { None } else { Some(i) };
                // The rows were already drawn this frame; draw again so the change shows now.
                ui.ctx().request_repaint();
            }
            if let Some(i) = remove {
                if self.recording.is_some() {
                    self.stop_recording();
                }
                self.bindings.remove(i);
                self.expanded = match self.expanded {
                    Some(e) if e == i => None,
                    Some(e) if e > i => Some(e - 1),
                    other => other,
                };
            }
            if let Some(i) = record {
                self.start_recording(i);
            }

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("➕ Add shortcut").clicked() {
                    let action = if self.tab == Tab::Two { Action::TileWindows } else { Action::TileLeft };
                    self.bindings.push(Binding { shortcut: String::new(), action, pair: None });
                    let i = self.bindings.len() - 1;
                    self.expanded = Some(i);
                    self.start_recording(i);
                }
                // Right to left: Save is the primary action, Discard sits beside it,
                // and the status reads next to them.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can_save = self.dirty() && !self.any_blocking();
                    let accent = ui.visuals().selection.bg_fill;
                    let save = egui::Button::new(RichText::new("Save changes").color(Color32::WHITE)).fill(accent);
                    if ui.add_enabled(can_save, save).clicked() {
                        self.save();
                    }
                    if ui.add_enabled(self.dirty(), egui::Button::new("Discard changes")).clicked() {
                        self.stop_recording();
                        self.bindings = self.saved.clone();
                        self.message = "Changes undone.".into();
                    }
                    if self.dirty() {
                        let blocked = (0..self.bindings.len()).filter(|&i| self.blocking_problem(i).is_some()).count();
                        if blocked == 0 {
                            ui.label(RichText::new("Unsaved changes").color(WARNING));
                        } else if blocked == 1 {
                            ui.label(RichText::new("1 row needs fixing before you can save.").color(ERROR));
                        } else {
                            ui.label(RichText::new(format!("{blocked} rows need fixing before you can save.")).color(ERROR));
                        }
                    } else {
                        ui.label(&self.message);
                    }
                });
            });
        });

        if self.confirm_close {
            egui::Modal::new(egui::Id::unique("confirm_close")).show(&ctx, |ui| {
                ui.label("You have unsaved changes.");
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.any_blocking(), egui::Button::new("Save and close"))
                        .clicked()
                    {
                        self.save();
                        self.allow_close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Close without saving").clicked() {
                        self.allow_close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Keep editing").clicked() {
                        self.confirm_close = false;
                    }
                });
            });
        }
    }
}

pub fn run() -> eframe::Result {
    use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};

    // Intel UHD 620 Vulkan driver (igvk64.dll) crashes in vkCreateDescriptorPool, so skip
    // Vulkan and use DirectX 12, with OpenGL as fallback. WGPU_BACKEND still overrides this.
    let mut create_new = WgpuSetupCreateNew::without_display_handle();
    if std::env::var_os("WGPU_BACKEND").is_none() {
        create_new.instance_descriptor.backends =
            eframe::wgpu::Backends::DX12 | eframe::wgpu::Backends::GL;
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("pane-pogo shortcuts")
            .with_inner_size([860.0, 600.0])
            .with_min_inner_size([700.0, 400.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-64.png"))
                    .expect("assets/icon-64.png is a valid PNG"),
            ),
        wgpu_options: WgpuConfiguration {
            wgpu_setup: WgpuSetup::CreateNew(create_new),
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "pane-pogo settings",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_shortcut_falls_back_to_physical_key() {
        let alt_shift = egui::Modifiers {
            alt: true,
            shift: true,
            ..Default::default()
        };
        // Shift+1 on a US layout: egui names the key `!`.
        assert_eq!(
            recorded_shortcut(alt_shift, egui::Key::Exclamationmark, Some(egui::Key::Num1)),
            Ok("Alt+Shift+1".to_string())
        );
        // A letter keeps the layout's name, even if the physical key differs (AZERTY A/Q).
        assert_eq!(
            recorded_shortcut(alt_shift, egui::Key::A, Some(egui::Key::Q)),
            Ok("Alt+Shift+A".to_string())
        );
        // Neither key usable: report the named key.
        assert_eq!(
            recorded_shortcut(alt_shift, egui::Key::Questionmark, Some(egui::Key::Slash)),
            Err("Questionmark")
        );
    }

    /// Lays out seven rows through `shortcut_row` at the minimum window width, with an
    /// expanded body, an error line and a recording row between them, and checks that every
    /// shortcut field and arrow lands on the same x.
    #[test]
    fn row_columns_align() {
        let actions = [
            Action::TileLeft,
            Action::TileDualMonitors,
            Action::Tile2to1,
            Action::MoveToNextMonitor,
            Action::FullSecondary,
            Action::SwapWindows,
            Action::TileRightSecondary,
        ];
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700.0, 600.0),
            )),
            ..Default::default()
        };
        let mut fields = Vec::new();
        let mut arrows = Vec::new();
        let mut output = ctx.run_ui(input, |ui| {
            fields.clear();
            arrows.clear();
            for (n, a) in actions.iter().enumerate() {
                let b = Binding {
                    shortcut: if n == 3 {
                        String::new()
                    } else {
                        format!("Ctrl+Alt+{n}")
                    },
                    action: *a,
                    pair: None,
                };
                let out = shortcut_row(ui, &b, n == 1, n == 5);
                fields.push(out.shortcut.rect);
                arrows.push(out.arrow.rect);
                if n == 1 {
                    ui.indent(("body", n), |ui| {
                        ui.label("Monitor:");
                        ui.label("Expanded settings for this row.");
                    });
                }
                if n == 3 {
                    ui.label("Click the shortcut box and press the keys you want.");
                }
            }
        });

        // No renderer here, so drop the font texture upload deliberately.
        output.textures_delta.clear();

        assert_eq!(fields.len(), 7);
        for (n, (f, a)) in fields.iter().zip(&arrows).enumerate() {
            assert_eq!(f.min.x, fields[0].min.x, "shortcut x, row {n}");
            assert_eq!(f.width(), COL_SHORTCUT, "shortcut width, row {n}");
            assert_eq!(a.min.x, arrows[0].min.x, "arrow x, row {n}");
        }
        // Rows stack top to bottom and stay inside the minimum window width.
        assert!(fields.windows(2).all(|w| w[1].min.y > w[0].max.y));
        assert!(arrows[0].max.x <= 700.0);
    }

    /// Clicks at a point on one row over three frames (move, press, release) and returns
    /// which of row / shortcut / arrow reported the click.
    fn click_at(point: impl Fn(&RowOutput) -> egui::Pos2) -> (bool, bool, bool) {
        let ctx = egui::Context::default();
        let b = Binding {
            shortcut: "Ctrl+Alt+T".into(),
            action: Action::TileWindows,
            pair: None,
        };
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 600.0));
        let target = std::cell::Cell::new(egui::Pos2::ZERO);
        let result = std::cell::Cell::new((false, false, false));
        let frame = |events: Vec<egui::Event>, find: bool| {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                // Same container as the app: rows live inside a vertical scroll area.
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let o = shortcut_row(ui, &b, false, false);
                        if find {
                            target.set(point(&o));
                        }
                        result.set((o.row.clicked(), o.shortcut.clicked(), o.arrow.clicked()));
                    });
            });
            out.textures_delta.clear();
        };
        frame(vec![], true);
        let pos = target.get();
        frame(vec![egui::Event::PointerMoved(pos)], false);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        frame(vec![button(true)], false);
        frame(vec![button(false)], false);
        result.get()
    }

    /// `run` unwraps this decode, so a broken PNG must fail here rather than at startup.
    #[test]
    fn window_icon_decodes() {
        let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-64.png"))
            .expect("assets/icon-64.png decodes");
        assert_eq!((icon.width, icon.height), (64, 64));
    }

    #[test]
    fn row_click_targets() {
        // Name column: toggles the row only.
        let name = click_at(|o| {
            egui::pos2(
                o.row.rect.min.x + COL_PREVIEW + COL_GAP + 20.0,
                o.row.rect.center().y,
            )
        });
        assert_eq!(name, (true, false, false), "click on name");
        // Shortcut field: records only.
        let field = click_at(|o| o.shortcut.rect.center());
        assert!(field.1, "click on shortcut");
        assert!(!field.0 && !field.2, "shortcut click must not toggle");
        // Chevron: toggles.
        let arrow = click_at(|o| o.arrow.rect.center());
        assert!(arrow.0 || arrow.2, "click on chevron");
    }
}
