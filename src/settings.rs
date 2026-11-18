//! Settings window. Runs as a separate process (`pane-pogo.exe --settings`), so a crash here
//! never takes the tray app's hotkeys down with it.
//!
//! Plain Win32 and GDI, no GPU renderer, so the window costs a few MB of memory. It works like
//! an immediate-mode UI: one layout function per pane both paints and returns the clickable
//! items, so painting and hit-testing always agree.

use crate::config::{self, Action, Binding, Config, Focus, MonitorRole, Order};
use crate::hotkeys;
use crate::layout::Layout;
use std::cell::RefCell;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CLEARTYPE_QUALITY, ClientToScreen, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateFontIndirectW, CreatePen, DC_BRUSH, DC_PEN, DEFAULT_CHARSET,
    DRAW_TEXT_FORMAT, DT_CALCRECT, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_RIGHT,
    DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, DeleteDC, DeleteObject, DrawTextW, EndPaint,
    GetStockObject, HDC, HFONT, InvalidateRect, LOGFONTW, LineTo, MoveToEx, NULL_BRUSH,
    PAINTSTRUCT, PS_SOLID, Polyline, RoundRect, SRCCOPY, SelectObject, SetBkMode, SetDCBrushColor,
    SetDCPenColor, SetTextColor, SetViewportOrgEx, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    ICC_BAR_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, SetScrollInfo, TTF_SUBCLASS,
    TTM_ADDTOOLW, TTM_DELTOOLW, TTM_SETMAXTIPWIDTH, TTTOOLINFOW, WM_MOUSELEAVE,
};
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem,
    GetDpiForWindow, GetSystemMetricsForDpi, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyNameTextW, GetKeyState, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_CONTROL,
    VK_ESCAPE, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW, GetScrollInfo, HICON, IDC_ARROW,
    IDNO, IDOK, IDYES, IMAGE_ICON, KillTimer, LR_DEFAULTCOLOR, LoadCursorW, LoadImageW,
    MB_ICONWARNING, MB_OKCANCEL, MB_YESNOCANCEL, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    MINMAXINFO, MSG, MessageBoxW, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassExW,
    SB_BOTTOM, SB_LINEDOWN, SB_LINEUP, SB_PAGEDOWN, SB_PAGEUP, SB_THUMBTRACK, SB_TOP, SB_VERT,
    SCROLLINFO, SIF_ALL, SIF_DISABLENOSCROLL, SIF_TRACKPOS, SM_CXICON, SM_CXSMICON, SM_CXVSCROLL,
    SM_CYICON, SM_CYSMICON, SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetTimer,
    SetWindowPos, ShowWindow, TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_TOPALIGN,
    TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WM_ACTIVATE, WM_APP, WM_CHAR, WM_CLOSE,
    WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SIZE, WM_SYSCHAR, WM_SYSKEYDOWN, WM_TIMER,
    WM_VSCROLL, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW,
    WS_POPUP, WS_VISIBLE, WS_VSCROLL,
};
use windows::core::{PCWSTR, PWSTR, w};

/// Thread messages the tray app handles. Must match main.rs.
pub const MSG_RELOAD: u32 = WM_APP + 3;
pub const MSG_SUSPEND: u32 = WM_APP + 4;

const TWO_WINDOW_ACTIONS: [Action; 5] = [
    Action::TileWindows,
    Action::Tile2to1,
    Action::Tile1to2,
    Action::TileDualMonitors,
    Action::SwapWindows,
];

/// Collapsed-row columns, in 96-DPI pixels. Every row is cut into the same rectangles, so
/// label length never moves the shortcut field or the arrow.
const COL_PREVIEW: f32 = 60.0;
const COL_NAME: f32 = 190.0;
const COL_SHORTCUT: f32 = 200.0;
const COL_ARROW: f32 = 28.0;
const COL_GAP: f32 = 8.0;
const ROW_H: f32 = 40.0;
const PAD: f32 = 16.0;
const FOOTER_H: f32 = 52.0;
/// Width of the label column in an expanded row.
const LABEL_W: f32 = 140.0;

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

const BG: COLORREF = rgb(250, 250, 250);
const TEXT: COLORREF = rgb(27, 27, 27);
const WEAK: COLORREF = rgb(96, 96, 96);
const DISABLED: COLORREF = rgb(160, 160, 160);
const BORDER: COLORREF = rgb(204, 204, 204);
const BUTTON: COLORREF = rgb(240, 240, 240);
const HOVER: COLORREF = rgb(232, 232, 232);
const CHOSEN: COLORREF = rgb(214, 214, 214);
const ACCENT: COLORREF = rgb(0, 103, 192);
const ACCENT_HOVER: COLORREF = rgb(0, 88, 166);
/// Accent at 35% over the background: the previous window in a placement icon.
const ACCENT_LIGHT: COLORREF = rgb(163, 199, 231);
const WHITE: COLORREF = rgb(255, 255, 255);
const ERROR: COLORREF = rgb(196, 43, 28);
const WARNING: COLORREF = rgb(157, 93, 0);

/// Resource id of the app icon embedded by build.rs (assets/pane-pogo.rc).
const APP_ICON_ID: usize = 1;

const TIMER_TRAY: usize = 1;
const TIMER_STATUS: usize = 2;

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
const HELP_SHORTCUT: &str =
    "Click, then press the keys. Use Ctrl, Alt or Shift with a letter, number or F1–F12.";

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

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Tab {
    All,
    One,
    Two,
}

/// A choice button in an expanded row.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Pick {
    /// Monitor of a full-screen or half action.
    Monitor(MonitorRole),
    /// Side of a half action.
    Left(bool),
    Order(Order),
    PairMonitor(MonitorRole),
    Focus(Focus),
}

/// What a click on a layout item does.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Hit {
    /// Not clickable; carries a tooltip only.
    None,
    Tab(Tab),
    /// Expands or collapses a row.
    Toggle(usize),
    /// Starts recording a row's shortcut.
    Shortcut(usize),
    /// Opens the action drop-down of a row.
    Picker(usize),
    Pick(usize, Pick),
    Remove(usize),
    Add,
    Discard,
    Save,
    OpenFile,
    TryAgain,
    StartFresh,
}

struct Item {
    rect: RECT,
    hit: Hit,
    tip: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    /// The top-level window: header and footer.
    Main = 0,
    /// The scrolling child window with the shortcut rows.
    List = 1,
}

#[derive(Clone, Copy, Default)]
struct Wins {
    main: HWND,
    list: HWND,
    tip: HWND,
}

/// Fonts for one DPI, plus a memory DC used to measure text outside painting.
struct Gfx {
    dpi: u32,
    dc: HDC,
    normal: HFONT,
    bold: HFONT,
    heading: HFONT,
}

impl Gfx {
    fn new(dpi: u32) -> Self {
        let font = |points: i32, weight: i32| {
            let mut lf = LOGFONTW {
                lfHeight: -(points * dpi as i32 + 36) / 72,
                lfWeight: weight,
                lfCharSet: DEFAULT_CHARSET,
                lfQuality: CLEARTYPE_QUALITY,
                ..Default::default()
            };
            for (d, c) in lf.lfFaceName.iter_mut().zip("Segoe UI".encode_utf16()) {
                *d = c;
            }
            unsafe { CreateFontIndirectW(&lf) }
        };
        Self {
            dpi,
            dc: unsafe { CreateCompatibleDC(None) },
            normal: font(10, 400),
            bold: font(10, 600),
            heading: font(15, 600),
        }
    }

    /// Scales 96-DPI pixels to this DPI.
    fn s(&self, v: f32) -> i32 {
        (v * self.dpi as f32 / 96.0).round() as i32
    }
}

impl Drop for Gfx {
    fn drop(&mut self) {
        unsafe {
            for f in [self.normal, self.bold, self.heading] {
                let _ = DeleteObject(f.into());
            }
            let _ = DeleteDC(self.dc);
        }
    }
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

fn contains(r: RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Normal,
    Selected,
    Accent,
    /// Left-aligned text with a drop-down arrow.
    Picker,
}

/// Lays out (and, when `paint` is set, draws) one pane, collecting the items under the mouse.
struct Ui<'a> {
    g: &'a Gfx,
    dc: HDC,
    paint: bool,
    mouse: Option<POINT>,
    items: Vec<Item>,
}

impl Ui<'_> {
    fn s(&self, v: f32) -> i32 {
        self.g.s(v)
    }

    fn item(&mut self, rect: RECT, hit: Hit, tip: Option<&'static str>) {
        self.items.push(Item { rect, hit, tip });
    }

    fn hovered(&self, r: RECT) -> bool {
        self.mouse.is_some_and(|p| contains(r, p))
    }

    fn fill(&self, r: RECT, c: COLORREF, radius: i32) {
        if self.paint {
            unsafe {
                SetDCBrushColor(self.dc, c);
                SetDCPenColor(self.dc, c);
                let _ = RoundRect(
                    self.dc,
                    r.left,
                    r.top,
                    r.right,
                    r.bottom,
                    radius * 2,
                    radius * 2,
                );
            }
        }
    }

    fn frame(&self, r: RECT, c: COLORREF, radius: i32) {
        if self.paint {
            unsafe {
                let old = SelectObject(self.dc, GetStockObject(NULL_BRUSH));
                SetDCPenColor(self.dc, c);
                let _ = RoundRect(
                    self.dc,
                    r.left,
                    r.top,
                    r.right,
                    r.bottom,
                    radius * 2,
                    radius * 2,
                );
                SelectObject(self.dc, old);
            }
        }
    }

    fn line(&self, x0: i32, y0: i32, x1: i32, y1: i32, c: COLORREF) {
        if self.paint {
            unsafe {
                SetDCPenColor(self.dc, c);
                let _ = MoveToEx(self.dc, x0, y0, None);
                let _ = LineTo(self.dc, x1, y1);
            }
        }
    }

    /// Chevron centred on (cx, cy): pointing down, or right when collapsed.
    fn chevron(&self, cx: i32, cy: i32, down: bool, c: COLORREF) {
        if !self.paint {
            return;
        }
        let s = self.s(4.0);
        let points = if down {
            [
                POINT {
                    x: cx - s,
                    y: cy - s / 2,
                },
                POINT {
                    x: cx,
                    y: cy + s / 2,
                },
                POINT {
                    x: cx + s,
                    y: cy - s / 2,
                },
            ]
        } else {
            [
                POINT {
                    x: cx - s / 2,
                    y: cy - s,
                },
                POINT {
                    x: cx + s / 2,
                    y: cy,
                },
                POINT {
                    x: cx - s / 2,
                    y: cy + s,
                },
            ]
        };
        unsafe {
            let pen = CreatePen(PS_SOLID, self.s(1.5).max(1), c);
            let old = SelectObject(self.dc, pen.into());
            let _ = Polyline(self.dc, &points);
            SelectObject(self.dc, old);
            let _ = DeleteObject(pen.into());
        }
    }

    /// One line of text in `r`, cut with an ellipsis if too long.
    fn text(&self, r: RECT, s: &str, font: HFONT, c: COLORREF, align: DRAW_TEXT_FORMAT) {
        // DrawTextW faults on an empty buffer.
        if self.paint && !s.is_empty() {
            let mut buf: Vec<u16> = s.encode_utf16().collect();
            let mut r = r;
            unsafe {
                SelectObject(self.dc, font.into());
                SetTextColor(self.dc, c);
                DrawTextW(
                    self.dc,
                    &mut buf,
                    &mut r,
                    align | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
                );
            }
        }
    }

    fn measure(&self, s: &str, font: HFONT) -> i32 {
        if s.is_empty() {
            return 0;
        }
        let mut buf: Vec<u16> = s.encode_utf16().collect();
        let mut r = RECT::default();
        unsafe {
            SelectObject(self.dc, font.into());
            DrawTextW(
                self.dc,
                &mut buf,
                &mut r,
                DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
        r.right - r.left
    }

    /// Word-wrapped text at (x, y), `w` wide. Returns its height.
    fn wrap(&self, x: i32, y: i32, w: i32, s: &str, font: HFONT, c: COLORREF) -> i32 {
        if s.is_empty() {
            return 0;
        }
        let mut buf: Vec<u16> = s.encode_utf16().collect();
        let mut r = rect(x, y, w.max(1), 0);
        unsafe {
            SelectObject(self.dc, font.into());
            DrawTextW(
                self.dc,
                &mut buf,
                &mut r,
                DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
            );
            if self.paint {
                r.right = x + w.max(1);
                SetTextColor(self.dc, c);
                DrawTextW(self.dc, &mut buf, &mut r, DT_WORDBREAK | DT_NOPREFIX);
            }
        }
        r.bottom - r.top
    }

    /// A button that is clickable only when `enabled`.
    fn button(
        &mut self,
        r: RECT,
        label: &str,
        hit: Hit,
        tip: Option<&'static str>,
        style: Style,
        enabled: bool,
    ) {
        let hover = enabled && self.hovered(r);
        let radius = self.s(4.0);
        let (bg, fg) = match (enabled, style) {
            (false, Style::Accent) => (HOVER, DISABLED),
            (false, _) => (BUTTON, DISABLED),
            (true, Style::Accent) => (if hover { ACCENT_HOVER } else { ACCENT }, WHITE),
            (true, Style::Selected) => (CHOSEN, TEXT),
            (true, _) => (if hover { HOVER } else { BUTTON }, TEXT),
        };
        self.fill(r, bg, radius);
        if style != Style::Accent {
            self.frame(r, BORDER, radius);
        }
        let pad = self.s(8.0);
        if style == Style::Picker {
            let arrow = self.s(20.0);
            let t = RECT {
                left: r.left + pad,
                right: r.right - arrow,
                ..r
            };
            self.text(t, label, self.g.normal, fg, DT_LEFT);
            self.chevron(
                r.right - arrow / 2 - pad / 2,
                (r.top + r.bottom) / 2,
                true,
                WEAK,
            );
        } else {
            let t = RECT {
                left: r.left + pad,
                right: r.right - pad,
                ..r
            };
            self.text(t, label, self.g.normal, fg, DT_CENTER);
        }
        if enabled {
            self.item(r, hit, tip);
        } else if tip.is_some() {
            self.item(r, Hit::None, tip);
        }
    }

    /// Button sized to its label. Returns its width.
    #[allow(clippy::too_many_arguments)]
    fn button_auto(
        &mut self,
        x: i32,
        y: i32,
        label: &str,
        hit: Hit,
        tip: Option<&'static str>,
        style: Style,
        enabled: bool,
    ) -> i32 {
        let w = self.measure(label, self.g.normal) + self.s(24.0);
        self.button(rect(x, y, w, self.s(30.0)), label, hit, tip, style, enabled);
        w
    }
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
    allow_close: bool,
    /// True while the close prompt is open, so the tray check does not stack a second one.
    prompting: bool,
    /// True once the tray app has been seen running. Used to detect that it exited.
    tray_seen: bool,
    tray_running: bool,
    monitor_count: usize,
    /// Set when bindings.json could not be read. Saving is blocked so the file is
    /// never overwritten without the user choosing to.
    load_error: Option<String>,
    wins: Wins,
    gfx: Gfx,
    /// Scroll offset of the list pane, in pixels.
    scroll: i32,
    /// Mouse position per pane, in client pixels. None when the mouse is outside.
    mouse: [Option<POINT>; 2],
    /// Index of the item under the mouse per pane, to repaint only when it changes.
    hover: [Option<usize>; 2],
    /// Number of tooltip tools registered per pane.
    tools: [usize; 2],
}

impl App {
    fn new(gfx: Gfx) -> Self {
        let (bindings, load_error) = match config::load() {
            Ok(cfg) => (cfg.bindings, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        let running = app_running();
        let mut app = Self::with_bindings(bindings, gfx);
        app.load_error = load_error;
        app.status = config::load_status();
        app.tray_running = running;
        app.tray_seen = running;
        app.monitor_count = crate::win::monitors().len();
        app
    }

    fn with_bindings(bindings: Vec<Binding>, gfx: Gfx) -> Self {
        Self {
            tab: Tab::All,
            expanded: None,
            saved: bindings.clone(),
            bindings,
            status: config::Status::default(),
            message: String::new(),
            recording: None,
            record_hint: String::new(),
            allow_close: false,
            prompting: false,
            tray_seen: false,
            tray_running: false,
            monitor_count: 2,
            load_error: None,
            wins: Wins::default(),
            gfx,
            scroll: 0,
            mouse: [None; 2],
            hover: [None; 2],
            tools: [0; 2],
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
            && self.tray_running
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

    /// Keeps the broken file as bindings.broken.json and starts from the defaults.
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
            self.check_status_soon();
        } else {
            self.message = "Saved. Start Pane Pogo to use them.".into();
        }
    }

    /// Re-reads the status file once the tray app has registered the shortcuts.
    fn check_status_soon(&self) {
        unsafe { SetTimer(Some(self.wins.main), TIMER_STATUS, 500, None) };
    }

    fn start_recording(&mut self, i: usize) {
        self.recording = Some(i);
        self.record_hint.clear();
        notify_app(MSG_SUSPEND);
    }

    fn stop_recording(&mut self) {
        if self.recording.take().is_some() {
            notify_app(MSG_RELOAD);
            self.check_status_soon();
        }
    }

    /// Handles a key press. Returns true if it was used to record a shortcut.
    fn record_key(&mut self, vk: u32, lparam: LPARAM) -> bool {
        let Some(i) = self.recording else {
            return false;
        };
        if vk == u32::from(VK_ESCAPE.0) {
            self.stop_recording();
            return true;
        }
        // Modifiers alone, including the Windows keys: wait for the main key.
        if matches!(vk, 0x10..=0x12 | 0x5B | 0x5C | 0xA0..=0xA5) {
            return true;
        }
        let down = |k: u16| unsafe { GetKeyState(i32::from(k)) } < 0;
        let (ctrl, alt, shift) = (down(VK_CONTROL.0), down(VK_MENU.0), down(VK_SHIFT.0));
        if !(ctrl || alt || shift) {
            self.record_hint =
                "Hold Ctrl, Alt or Shift, then press a letter, number or F1–F12.".into();
            return true;
        }
        match recorded_shortcut(ctrl, alt, shift, vk) {
            Some(text) => {
                self.bindings[i].shortcut = text;
                self.stop_recording();
            }
            None => {
                let mut buf = [0u16; 64];
                let n = unsafe { GetKeyNameTextW(lparam.0 as i32, &mut buf) };
                let name = match n {
                    1.. => String::from_utf16_lossy(&buf[..n as usize]),
                    _ => "That key".into(),
                };
                self.record_hint = format!("{name} can't be used. Use a letter, number or F1–F12.");
            }
        }
        true
    }

    /// Applies a click. The action drop-down is handled by `pick_action`.
    fn apply(&mut self, hit: Hit) {
        match hit {
            Hit::None | Hit::Picker(_) => {}
            Hit::Tab(t) => self.tab = t,
            Hit::Toggle(i) => {
                self.expanded = if self.expanded == Some(i) {
                    None
                } else {
                    Some(i)
                }
            }
            Hit::Shortcut(i) => {
                if self.recording != Some(i) {
                    self.start_recording(i);
                }
            }
            Hit::Pick(i, p) => self.pick(i, p),
            Hit::Remove(i) => {
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
            Hit::Add => {
                let action = if self.tab == Tab::Two {
                    Action::TileWindows
                } else {
                    Action::TileLeft
                };
                self.bindings.push(Binding {
                    shortcut: String::new(),
                    action,
                    pair: None,
                });
                let i = self.bindings.len() - 1;
                self.expanded = Some(i);
                self.start_recording(i);
            }
            Hit::Discard => {
                self.stop_recording();
                self.bindings = self.saved.clone();
                self.message = "Changes undone.".into();
            }
            Hit::Save => self.save(),
            Hit::OpenFile => {
                if let Ok(p) = config::path() {
                    let _ = std::process::Command::new("notepad.exe").arg(p).spawn();
                }
            }
            Hit::TryAgain => self.reload_file(),
            Hit::StartFresh => self.start_fresh(),
        }
    }

    fn pick(&mut self, i: usize, pick: Pick) {
        let b = &mut self.bindings[i];
        let mut p = b.pair_settings();
        match pick {
            Pick::Monitor(m) => {
                b.action = if is_full(b.action) {
                    full_action(m)
                } else {
                    half_action(m, side_is_left(b.action))
                };
                return;
            }
            Pick::Left(left) => {
                b.action = half_action(monitor_of(b.action), left);
                return;
            }
            Pick::Order(o) => p.order = o,
            Pick::PairMonitor(m) => p.monitor = m,
            Pick::Focus(f) => p.focus = f,
        }
        // Write only on a real change: a row saved without `pair` uses the action's
        // default, and storing that default would mark it unsaved.
        if p != b.pair_settings() {
            b.pair = Some(p);
        }
    }

    /// Applies a command from the action drop-down (see `pick_action`).
    fn set_action(&mut self, i: usize, cmd: usize) {
        let b = &mut self.bindings[i];
        b.action = match cmd {
            MENU_FULL if is_full(b.action) => return,
            MENU_FULL => Action::FullPrimary,
            MENU_HALF if is_half(b.action) => return,
            MENU_HALF => Action::TileLeft,
            MENU_NEXT => Action::MoveToNextMonitor,
            c if (MENU_TWO..MENU_TWO + TWO_WINDOW_ACTIONS.len()).contains(&c) => {
                TWO_WINDOW_ACTIONS[c - MENU_TWO]
            }
            _ => return,
        };
        if !b.action.is_pair() {
            b.pair = None;
        }
    }

    fn mouse_in(&self, pane: Pane) -> Option<POINT> {
        let p = self.mouse[pane as usize]?;
        Some(match pane {
            Pane::Main => p,
            Pane::List => POINT {
                x: p.x,
                y: p.y + self.scroll,
            },
        })
    }

    /// Lays out a pane `w` by `h` pixels. Returns its items (list items in content
    /// coordinates) and the header height (main) or content height (list).
    fn layout(&self, pane: Pane, dc: HDC, paint: bool, w: i32, h: i32) -> (Vec<Item>, i32) {
        let mut ui = Ui {
            g: &self.gfx,
            dc,
            paint,
            mouse: self.mouse_in(pane),
            items: Vec::new(),
        };
        let height = match pane {
            Pane::Main => {
                let header = header(self, &mut ui, w);
                if self.load_error.is_none() {
                    let top = h - ui.s(FOOTER_H);
                    footer(self, &mut ui, top, w);
                }
                header
            }
            Pane::List => list(self, &mut ui, w),
        };
        (ui.items, height)
    }

    fn client(&self, pane: Pane) -> (i32, i32) {
        let hwnd = match pane {
            Pane::Main => self.wins.main,
            Pane::List => self.wins.list,
        };
        let mut r = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut r);
        }
        (r.right, r.bottom)
    }

    /// Topmost item under a client point of a pane, with its index and client rect.
    fn item_at(&self, pane: Pane, pt: POINT) -> Option<(usize, Hit, RECT)> {
        let (w, h) = self.client(pane);
        let (items, _) = self.layout(pane, self.gfx.dc, false, w, h);
        let dy = if pane == Pane::List { self.scroll } else { 0 };
        let p = POINT {
            x: pt.x,
            y: pt.y + dy,
        };
        items
            .iter()
            .enumerate()
            .rev()
            .find(|(_, it)| contains(it.rect, p))
            .map(|(n, it)| {
                let mut r = it.rect;
                r.top -= dy;
                r.bottom -= dy;
                (n, it.hit, r)
            })
    }
}

/// Builds the shortcut text for a recorded key press. Virtual-key codes name the key, not the
/// character, so Shift+1 records as `1` on any layout. Returns None for keys that cannot be used.
fn recorded_shortcut(ctrl: bool, alt: bool, shift: bool, vk: u32) -> Option<String> {
    let key = match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk)?.to_string(),
        0x70..=0x7B => format!("F{}", vk - 0x6F),
        _ => return None,
    };
    let mut parts = Vec::new();
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    parts.push(&key);
    let text = parts.join("+");
    hotkeys::parse(&text).is_ok().then_some(text)
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
    let class = wide("PanePogoTray");
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

/// Small monitor diagram, so the user sees where the window goes.
fn placement_icon(ui: &Ui, r: RECT, b: &Binding) {
    let gap = ui.s(3.0);
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    let primary_w = (w - gap) * 3 / 5;
    let monitors = [
        rect(r.left, r.top, primary_w, h),
        rect(r.left + primary_w + gap, r.top, w - gap - primary_w, h),
    ];
    for m in monitors {
        ui.frame(m, WEAK, ui.s(2.0));
    }
    let inset = ui.s(2.0);
    for (m, x0, x1, current) in placement_regions(b) {
        let mon = monitors[m];
        let mw = (mon.right - mon.left) as f32;
        let region = RECT {
            left: mon.left + (mw * x0).round() as i32 + inset,
            right: mon.left + (mw * x1).round() as i32 - inset,
            top: mon.top + inset,
            bottom: mon.bottom - inset,
        };
        ui.fill(region, if current { ACCENT } else { ACCENT_LIGHT }, 1);
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

/// Title, intro, file error or warning, and tabs. Returns the height used.
fn header(app: &App, ui: &mut Ui, w: i32) -> i32 {
    let g = ui.g;
    let pad = ui.s(PAD);
    let tw = w - 2 * pad;
    let mut y = pad;
    y += ui.wrap(pad, y, tw, "Pane Pogo shortcuts", g.heading, TEXT) + ui.s(6.0);
    y += ui.wrap(
        pad,
        y,
        tw,
        "The current window is the one you are using when you press a shortcut. \
         The previous window is the one you used just before it.",
        g.normal,
        TEXT,
    ) + ui.s(10.0);

    if let Some(err) = &app.load_error {
        let top = y;
        let x = pad + ui.s(12.0);
        let iw = tw - ui.s(24.0);
        y += ui.s(10.0);
        y += ui.wrap(
            x,
            y,
            iw,
            "Your shortcuts file could not be read, so no shortcuts are active.",
            g.bold,
            ERROR,
        ) + ui.s(4.0);
        y += ui.wrap(x, y, iw, err, g.normal, WEAK) + ui.s(10.0);
        let mut bx = x;
        for (label, hit, tip) in [
            (
                "Open file",
                Hit::OpenFile,
                "Opens bindings.json in Notepad so you can fix it.",
            ),
            (
                "Try again",
                Hit::TryAgain,
                "Reads the file again after you fix it.",
            ),
            (
                "Start fresh",
                Hit::StartFresh,
                "Keeps the broken file as bindings.broken.json and starts from the default shortcuts.",
            ),
        ] {
            bx += ui.button_auto(bx, y, label, hit, Some(tip), Style::Normal, true) + ui.s(8.0);
        }
        y += ui.s(30.0) + ui.s(10.0);
        ui.frame(rect(pad, top, tw, y - top), BORDER, ui.s(4.0));
        if !app.message.is_empty() {
            y += ui.s(8.0);
            y += ui.wrap(pad, y, tw, &app.message, g.normal, TEXT);
        }
        return y + ui.s(8.0);
    }

    if !app.tray_running {
        y += ui.wrap(
            pad,
            y,
            tw,
            "Pane Pogo is not running, so shortcuts do nothing yet. Run pane-pogo.exe to start it.",
            g.normal,
            WARNING,
        ) + ui.s(8.0);
    }
    let mut x = pad;
    for (tab, label) in [
        (Tab::All, "All"),
        (Tab::One, "One window"),
        (Tab::Two, "Two windows"),
    ] {
        let style = if app.tab == tab {
            Style::Selected
        } else {
            Style::Normal
        };
        x += ui.button_auto(x, y, label, Hit::Tab(tab), None, style, true) + ui.s(4.0);
    }
    y += ui.s(30.0) + ui.s(8.0);
    ui.line(pad, y, w - pad, y, BORDER);
    y + 1
}

/// Add, status, Discard and Save, along the bottom edge from `top`.
fn footer(app: &App, ui: &mut Ui, top: i32, w: i32) {
    let pad = ui.s(PAD);
    ui.line(pad, top, w - pad, top, BORDER);
    let bh = ui.s(30.0);
    let y = top + (ui.s(FOOTER_H) - bh) / 2;
    let add_w = ui.button_auto(
        pad,
        y,
        "+  Add shortcut",
        Hit::Add,
        None,
        Style::Normal,
        true,
    );

    // Right to left: Save is the primary action, Discard sits beside it, and the
    // status reads next to them.
    let dirty = app.dirty();
    let can_save = dirty && !app.any_blocking();
    let mut x = w - pad;
    let save_w = ui.measure("Save changes", ui.g.normal) + ui.s(24.0);
    x -= save_w;
    ui.button_auto(
        x,
        y,
        "Save changes",
        Hit::Save,
        None,
        Style::Accent,
        can_save,
    );
    let discard_w = ui.measure("Discard changes", ui.g.normal) + ui.s(24.0);
    x -= ui.s(8.0) + discard_w;
    ui.button_auto(
        x,
        y,
        "Discard changes",
        Hit::Discard,
        None,
        Style::Normal,
        dirty,
    );

    let (text, color) = if dirty {
        match (0..app.bindings.len())
            .filter(|&i| app.blocking_problem(i).is_some())
            .count()
        {
            0 => ("Unsaved changes".to_string(), WARNING),
            1 => ("1 row needs fixing before you can save.".to_string(), ERROR),
            n => (format!("{n} rows need fixing before you can save."), ERROR),
        }
    } else {
        (app.message.clone(), TEXT)
    };
    let left = pad + add_w + ui.s(12.0);
    let status = RECT {
        left,
        top: y,
        right: (x - ui.s(12.0)).max(left),
        bottom: y + bh,
    };
    ui.text(status, &text, ui.g.normal, color, DT_RIGHT);
}

/// The shortcut rows. Returns the content height.
fn list(app: &App, ui: &mut Ui, w: i32) -> i32 {
    let g = ui.g;
    let pad = ui.s(PAD);
    let mut y = ui.s(6.0);
    let tab = app.tab;
    let rows: Vec<usize> = (0..app.bindings.len())
        .filter(|&i| match tab {
            Tab::All => true,
            Tab::One => !app.bindings[i].action.is_pair(),
            Tab::Two => app.bindings[i].action.is_pair(),
        })
        .collect();
    if rows.is_empty() {
        let text = match tab {
            Tab::All => "No shortcuts yet. Click “Add shortcut” to create one.",
            Tab::One => "No one-window shortcuts yet.",
            Tab::Two => "No two-window shortcuts yet.",
        };
        y += ui.wrap(pad, y, w - 2 * pad, text, g.normal, TEXT) + ui.s(8.0);
    }
    for i in rows {
        y = row(app, ui, i, y, w);
    }
    y + ui.s(8.0)
}

/// One shortcut row: preview | name and monitor | shortcut | arrow, then its messages and,
/// when expanded, its settings. Returns the y below it.
fn row(app: &App, ui: &mut Ui, i: usize, mut y: i32, w: i32) -> i32 {
    let g = ui.g;
    let b = &app.bindings[i];
    let expanded = app.expanded == Some(i);
    let recording = app.recording == Some(i);
    let pad = ui.s(PAD);
    let gap = ui.s(COL_GAP);
    let row_h = ui.s(ROW_H);
    // The name column takes the width the fixed columns leave, so titles are cut only when the
    // window is narrower than the minimum.
    let fixed = ui.s(COL_PREVIEW) + ui.s(COL_SHORTCUT) + ui.s(COL_ARROW) + gap * 3;
    let name_w = (w - 2 * pad - fixed).max(ui.s(COL_NAME));
    let widths = [
        ui.s(COL_PREVIEW),
        name_w,
        ui.s(COL_SHORTCUT),
        ui.s(COL_ARROW),
    ];
    let total = widths.iter().sum::<i32>() + gap * 3;

    // Pushed before its contents, so the shortcut field sits on top and keeps its own
    // clicks. A click anywhere else on the row toggles the settings.
    let row = rect(pad, y, total, row_h);
    ui.item(row, Hit::Toggle(i), None);
    let row_hover = ui.hovered(row);
    if row_hover {
        ui.fill(row, HOVER, ui.s(4.0));
    }
    let mut x = pad;
    let [preview, name, shortcut_col, arrow_col] = widths.map(|cw| {
        let r = rect(x, y, cw, row_h);
        x += cw + gap;
        r
    });

    let cx = (preview.left + preview.right) / 2;
    let cy = (preview.top + preview.bottom) / 2;
    let (iw, ih) = (ui.s(56.0), ui.s(34.0));
    placement_icon(ui, rect(cx - iw / 2, cy - ih / 2, iw, ih), b);

    let title = picker_label(b.action);
    match monitor_text(b) {
        Some(m) => {
            let mid = name.top + row_h / 2;
            ui.text(
                RECT {
                    bottom: mid,
                    ..name
                },
                title,
                g.bold,
                TEXT,
                DT_LEFT,
            );
            ui.text(RECT { top: mid, ..name }, m, g.normal, WEAK, DT_LEFT);
        }
        None => ui.text(name, title, g.bold, TEXT, DT_LEFT),
    }

    let text = if recording {
        "Press keys…  (Esc to cancel)"
    } else if b.shortcut.is_empty() {
        "Click to set"
    } else {
        &b.shortcut
    };
    let fh = ui.s(28.0);
    let field = rect(
        shortcut_col.left,
        y + (row_h - fh) / 2,
        shortcut_col.right - shortcut_col.left,
        fh,
    );
    let style = if recording {
        Style::Selected
    } else {
        Style::Normal
    };
    ui.button(
        field,
        text,
        Hit::Shortcut(i),
        Some(HELP_SHORTCUT),
        style,
        true,
    );

    let arrow = rect(
        arrow_col.left,
        y + (row_h - fh) / 2,
        arrow_col.right - arrow_col.left,
        fh,
    );
    ui.item(arrow, Hit::Toggle(i), None);
    let color = if row_hover || ui.hovered(arrow) {
        TEXT
    } else {
        WEAK
    };
    ui.chevron(
        (arrow.left + arrow.right) / 2,
        (arrow.top + arrow.bottom) / 2,
        expanded,
        color,
    );
    y += row_h + ui.s(2.0);

    let tw = w - 2 * pad;
    if recording && !app.record_hint.is_empty() {
        y += ui.wrap(pad, y, tw, &app.record_hint, g.normal, WARNING) + ui.s(2.0);
    }
    if let Some(p) = app.blocking_problem(i) {
        y += ui.wrap(pad, y, tw, &p, g.normal, ERROR) + ui.s(2.0);
    } else if let Some(wn) = app.warning(i) {
        y += ui.wrap(pad, y, tw, &wn, g.normal, WARNING) + ui.s(2.0);
    }

    if expanded {
        y = settings_body(ui, b, i, y + ui.s(4.0));
    }
    y += ui.s(6.0);
    ui.line(pad, y, w - pad, y, BORDER);
    y + ui.s(6.0)
}

/// The expanded settings of row `i`. Returns the y below them.
fn settings_body(ui: &mut Ui, b: &Binding, i: usize, mut y: i32) -> i32 {
    let x = ui.s(PAD + 20.0);
    let line_h = ui.s(36.0);
    let ch = ui.s(28.0);
    let cx = x + ui.s(LABEL_W);
    let label = |ui: &mut Ui, y: i32, text: &str, tip: &'static str| {
        let w = ui.measure(text, ui.g.normal);
        let r = rect(x, y, w, line_h);
        ui.text(r, text, ui.g.normal, TEXT, DT_LEFT);
        ui.item(r, Hit::None, Some(tip));
    };

    label(ui, y, "Action:", action_help(b.action));
    ui.button(
        rect(cx, y + (line_h - ch) / 2, ui.s(290.0), ch),
        picker_label(b.action),
        Hit::Picker(i),
        None,
        Style::Picker,
        true,
    );
    y += line_h;

    let mut choice_row =
        |ui: &mut Ui, text: &str, tip: &'static str, opts: [(Pick, &str, bool); 2]| {
            label(ui, y, text, tip);
            choice(ui, i, cx, y + (line_h - ch) / 2, ch, opts);
            y += line_h;
        };
    let a = b.action;
    if is_full(a) || is_half(a) {
        let m = monitor_of(a);
        choice_row(
            ui,
            "Monitor:",
            HELP_MONITOR,
            [
                (
                    Pick::Monitor(MonitorRole::Primary),
                    "Primary monitor",
                    m == MonitorRole::Primary,
                ),
                (
                    Pick::Monitor(MonitorRole::Secondary),
                    "Secondary monitor",
                    m == MonitorRole::Secondary,
                ),
            ],
        );
        if is_half(a) {
            let left = side_is_left(a);
            choice_row(
                ui,
                "Side:",
                HELP_SIDE,
                [
                    (Pick::Left(true), "Left half", left),
                    (Pick::Left(false), "Right half", !left),
                ],
            );
        }
    }
    if a.is_pair() {
        let p = b.pair_settings();
        let monitors = [
            (
                Pick::PairMonitor(MonitorRole::Primary),
                "Primary monitor",
                p.monitor == MonitorRole::Primary,
            ),
            (
                Pick::PairMonitor(MonitorRole::Secondary),
                "Secondary monitor",
                p.monitor == MonitorRole::Secondary,
            ),
        ];
        if matches!(a.layout(), Layout::Split { .. }) {
            choice_row(
                ui,
                "Current window on:",
                HELP_ORDER,
                [
                    (
                        Pick::Order(Order::CurrentLeft),
                        "Left",
                        p.order == Order::CurrentLeft,
                    ),
                    (
                        Pick::Order(Order::CurrentRight),
                        "Right",
                        p.order == Order::CurrentRight,
                    ),
                ],
            );
            choice_row(ui, "Monitor:", HELP_TILE_ON, monitors);
        } else {
            choice_row(ui, "Current window on:", HELP_GOES_TO, monitors);
        }
        choice_row(
            ui,
            "Final focus:",
            HELP_FOCUS,
            [
                (
                    Pick::Focus(Focus::Current),
                    "Current window",
                    p.focus == Focus::Current,
                ),
                (
                    Pick::Focus(Focus::Previous),
                    "Previous window",
                    p.focus == Focus::Previous,
                ),
            ],
        );
    }

    ui.button_auto(
        x,
        y + ui.s(4.0),
        "Remove",
        Hit::Remove(i),
        None,
        Style::Normal,
        true,
    );
    y + ui.s(4.0) + ui.s(30.0) + ui.s(4.0)
}

/// Two-option segmented control. The chosen option gets a neutral fill.
fn choice(ui: &mut Ui, i: usize, x: i32, y: i32, h: i32, opts: [(Pick, &str, bool); 2]) {
    let inner = ui.s(2.0);
    let widths = opts.map(|(_, label, _)| ui.measure(label, ui.g.normal) + ui.s(20.0));
    let total = widths[0] + widths[1] + inner * 3;
    ui.frame(rect(x, y, total, h), BORDER, ui.s(4.0));
    let mut bx = x + inner;
    for ((pick, label, chosen), bw) in opts.into_iter().zip(widths) {
        let r = rect(bx, y + inner, bw, h - 2 * inner);
        if chosen {
            ui.fill(r, CHOSEN, ui.s(3.0));
        } else if ui.hovered(r) {
            ui.fill(r, HOVER, ui.s(3.0));
        }
        ui.text(r, label, ui.g.normal, TEXT, DT_CENTER);
        ui.item(r, Hit::Pick(i, pick), None);
        bx += bw + inner;
    }
}

// Action drop-down command ids.
const MENU_FULL: usize = 1;
const MENU_HALF: usize = 2;
const MENU_NEXT: usize = 3;
const MENU_TWO: usize = 10;

thread_local! {
    // Already const; the lint misfires on this form.
    #[allow(clippy::missing_const_for_thread_local)]
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Runs `f` on the app state. Returns None if there is no state or it is already in use,
/// which happens only if a nested message loop re-enters a handler.
fn with<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

/// Re-lays out both panes: places the list, sets its scroll bar and tooltips, and repaints.
/// Call after any state change. Never call it while `with` is running.
fn refresh() {
    let Some((wins, list_rect, show_list, content_h, scroll, tools, old_tools)) = with(|a| {
        if a.wins.list.0.is_null() {
            return None;
        }
        let (w, h) = a.client(Pane::Main);
        let (main_items, header_h) = a.layout(Pane::Main, a.gfx.dc, false, w, h);
        let show_list = a.load_error.is_none();
        let list_h = (h - header_h - a.gfx.s(FOOTER_H)).max(0);
        let list_w = w - unsafe { GetSystemMetricsForDpi(SM_CXVSCROLL, a.gfx.dpi) };
        let (list_items, content_h) = a.layout(Pane::List, a.gfx.dc, false, list_w, list_h);
        a.scroll = a.scroll.clamp(0, (content_h - list_h).max(0));
        let tips = |items: &[Item], dy: i32| -> Vec<(RECT, &'static str)> {
            items
                .iter()
                .filter_map(|it| {
                    let mut r = it.rect;
                    r.top -= dy;
                    r.bottom -= dy;
                    it.tip.map(|t| (r, t))
                })
                .collect()
        };
        let tools = [tips(&main_items, 0), tips(&list_items, a.scroll)];
        let old = a.tools;
        a.tools = [tools[0].len(), tools[1].len()];
        Some((
            a.wins,
            rect(0, header_h, w, list_h),
            show_list,
            content_h,
            a.scroll,
            tools,
            old,
        ))
    })
    .flatten() else {
        return;
    };
    unsafe {
        let r = list_rect;
        let _ = MoveWindow(
            wins.list,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            true,
        );
        let _ = ShowWindow(wins.list, if show_list { SW_SHOW } else { SW_HIDE });
        let si = SCROLLINFO {
            cbSize: size_of::<SCROLLINFO>() as u32,
            fMask: SIF_ALL | SIF_DISABLENOSCROLL,
            nMin: 0,
            nMax: (content_h - 1).max(0),
            nPage: (r.bottom - r.top).max(0) as u32,
            nPos: scroll,
            nTrackPos: 0,
        };
        SetScrollInfo(wins.list, SB_VERT, &si, true);
        set_tools(wins.tip, wins.main, old_tools[0], &tools[0]);
        set_tools(wins.tip, wins.list, old_tools[1], &tools[1]);
        let _ = InvalidateRect(Some(wins.main), None, false);
        let _ = InvalidateRect(Some(wins.list), None, false);
    }
}

/// Replaces the tooltip tools of `target`: one per item with help text.
fn set_tools(tip: HWND, target: HWND, old: usize, tools: &[(RECT, &'static str)]) {
    let info = |id: usize, r: RECT, text: &mut Vec<u16>| TTTOOLINFOW {
        cbSize: size_of::<TTTOOLINFOW>() as u32,
        uFlags: TTF_SUBCLASS,
        hwnd: target,
        uId: id,
        rect: r,
        lpszText: PWSTR(text.as_mut_ptr()),
        ..Default::default()
    };
    unsafe {
        for id in 0..old {
            let ti = info(id, RECT::default(), &mut vec![0]);
            SendMessageW(
                tip,
                TTM_DELTOOLW,
                None,
                Some(LPARAM(&ti as *const _ as isize)),
            );
        }
        for (id, (r, text)) in tools.iter().enumerate() {
            let mut buf = wide(text);
            let ti = info(id, *r, &mut buf);
            SendMessageW(
                tip,
                TTM_ADDTOOLW,
                None,
                Some(LPARAM(&ti as *const _ as isize)),
            );
        }
    }
}

/// Opens the action drop-down under the picker of row `i` and applies the choice.
fn pick_action(i: usize, r: RECT) {
    let Some((wins, current)) = with(|a| (a.wins, a.bindings[i].action)) else {
        return;
    };
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let add = |flags, id: usize, text: &str| {
            let flags = if id != 0 && menu_id(current) == id {
                flags | MF_CHECKED
            } else {
                flags
            };
            let _ = AppendMenuW(menu, flags, id, PCWSTR(wide(text).as_ptr()));
        };
        add(MF_STRING | MF_GRAYED, 0, "One window");
        add(MF_STRING, MENU_FULL, "Full screen on one monitor");
        add(MF_STRING, MENU_HALF, "Half of one monitor");
        add(
            MF_STRING,
            MENU_NEXT,
            action_label(Action::MoveToNextMonitor),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        add(MF_STRING | MF_GRAYED, 0, "Two windows");
        for (n, a) in TWO_WINDOW_ACTIONS.iter().enumerate() {
            add(MF_STRING, MENU_TWO + n, action_label(*a));
        }
        let mut pt = POINT {
            x: r.left,
            y: r.bottom,
        };
        let _ = ClientToScreen(wins.list, &mut pt);
        let cmd = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RETURNCMD | TPM_NONOTIFY,
            pt.x,
            pt.y,
            Some(0),
            wins.main,
            None,
        );
        let _ = DestroyMenu(menu);
        if cmd.0 > 0 {
            with(|a| a.set_action(i, cmd.0 as usize));
        }
    }
}

/// Drop-down command that matches an action, for the check mark.
fn menu_id(a: Action) -> usize {
    if is_full(a) {
        MENU_FULL
    } else if is_half(a) {
        MENU_HALF
    } else if a == Action::MoveToNextMonitor {
        MENU_NEXT
    } else {
        TWO_WINDOW_ACTIONS
            .iter()
            .position(|t| *t == a)
            .map_or(0, |n| MENU_TWO + n)
    }
}

fn point(lparam: LPARAM) -> POINT {
    POINT {
        x: (lparam.0 & 0xFFFF) as i16 as i32,
        y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    }
}

fn on_click(pane: Pane, pt: POINT) {
    let Some(Some((_, hit, r))) = with(|a| a.item_at(pane, pt)) else {
        return;
    };
    match hit {
        Hit::None => return,
        Hit::Picker(i) => pick_action(i, r),
        _ => {
            with(|a| a.apply(hit));
        }
    }
    refresh();
}

fn on_mouse_move(pane: Pane, hwnd: HWND, pt: POINT) {
    unsafe {
        let mut tme = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        let _ = TrackMouseEvent(&mut tme);
    }
    let changed = with(|a| {
        a.mouse[pane as usize] = Some(pt);
        let idx = a.item_at(pane, pt).map(|(n, ..)| n);
        let changed = a.hover[pane as usize] != idx;
        a.hover[pane as usize] = idx;
        changed
    });
    if changed == Some(true) {
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

fn on_mouse_leave(pane: Pane, hwnd: HWND) {
    with(|a| {
        a.mouse[pane as usize] = None;
        a.hover[pane as usize] = None;
    });
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn scroll_by(dy: i32) {
    with(|a| a.scroll += dy);
    refresh();
}

fn on_wheel(wparam: WPARAM) {
    let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as i32;
    let step = with(|a| a.gfx.s(ROW_H)).unwrap_or(40);
    scroll_by(-delta * step * 3 / 2 / 120);
}

/// Double-buffered paint of one pane.
fn paint(pane: Pane, hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut cr = RECT::default();
        let _ = GetClientRect(hwnd, &mut cr);
        let (w, h) = (cr.right, cr.bottom);
        let mem = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, w.max(1), h.max(1));
        let old_bmp = SelectObject(mem, bmp.into());
        let old_pen = SelectObject(mem, GetStockObject(DC_PEN));
        let old_brush = SelectObject(mem, GetStockObject(DC_BRUSH));
        SetBkMode(mem, TRANSPARENT);
        SetDCBrushColor(mem, BG);
        SetDCPenColor(mem, BG);
        let _ = RoundRect(mem, -1, -1, w + 1, h + 1, 0, 0);
        with(|a| {
            if pane == Pane::List {
                let _ = SetViewportOrgEx(mem, 0, -a.scroll, None);
            }
            a.layout(pane, mem, true, w, h);
            let _ = SetViewportOrgEx(mem, 0, 0, None);
        });
        let _ = BitBlt(hdc, 0, 0, w, h, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old_brush);
        SelectObject(mem, old_pen);
        SelectObject(mem, old_bmp);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        let _ = EndPaint(hwnd, &ps);
    }
}

/// Asks before closing with unsaved changes. Returns true if the window may close.
fn confirm_close(hwnd: HWND) -> bool {
    let Some((dirty, blocking, allow)) = with(|a| (a.dirty(), a.any_blocking(), a.allow_close))
    else {
        return false;
    };
    if !dirty || allow {
        return true;
    }
    with(|a| a.prompting = true);
    let title = w!("Pane Pogo shortcuts");
    let close = unsafe {
        if blocking {
            let text = wide(
                "You have unsaved changes, but some rows need fixing before they can be saved.\n\nClose without saving?",
            );
            MessageBoxW(
                Some(hwnd),
                PCWSTR(text.as_ptr()),
                title,
                MB_OKCANCEL | MB_ICONWARNING,
            ) == IDOK
        } else {
            let text = wide("You have unsaved changes. Save them before closing?");
            match MessageBoxW(
                Some(hwnd),
                PCWSTR(text.as_ptr()),
                title,
                MB_YESNOCANCEL | MB_ICONWARNING,
            ) {
                IDYES => with(|a| {
                    a.save();
                    !a.dirty()
                })
                .unwrap_or(false),
                IDNO => true,
                _ => false,
            }
        }
    };
    with(|a| {
        a.prompting = false;
        a.allow_close = close;
    });
    if !close {
        refresh();
    }
    close
}

/// Close when the tray app exits. Settings is its own process, so it only learns this by
/// polling. Only a tray app seen running counts, so a standalone Settings window stays open.
fn on_tray_timer(hwnd: HWND) {
    let Some((close, changed)) = with(|a| {
        let running = app_running();
        let changed = running != a.tray_running;
        a.tray_running = running;
        let close = if running {
            a.tray_seen = true;
            false
        } else if a.tray_seen && !a.prompting {
            a.tray_seen = false;
            true
        } else {
            false
        };
        (close, changed)
    }) else {
        return;
    };
    if changed {
        refresh();
    }
    if close {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

unsafe extern "system" fn main_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => paint(Pane::Main, hwnd),
        WM_ERASEBKGND => return LRESULT(1),
        WM_SIZE => refresh(),
        WM_GETMINMAXINFO => {
            let dpi = with(|a| a.gfx.dpi).unwrap_or(96) as i32;
            let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            info.ptMinTrackSize = POINT {
                x: 716 * dpi / 96,
                y: 440 * dpi / 96,
            };
        }
        WM_DPICHANGED => {
            let dpi = (wparam.0 & 0xFFFF) as u32;
            with(|a| a.gfx = Gfx::new(dpi));
            let r = unsafe { *(lparam.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            refresh();
        }
        WM_MOUSEMOVE => on_mouse_move(Pane::Main, hwnd, point(lparam)),
        WM_MOUSELEAVE => on_mouse_leave(Pane::Main, hwnd),
        WM_LBUTTONDOWN => on_click(Pane::Main, point(lparam)),
        WM_MOUSEWHEEL => on_wheel(wparam),
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if with(|a| a.record_key(wparam.0 as u32, lparam)) == Some(true) {
                refresh();
                return LRESULT(0);
            }
        }
        // Swallow the character a recorded Alt+key produces, which would otherwise beep.
        // Alt+Space still opens the window menu.
        WM_SYSCHAR if wparam.0 != 0x20 => return LRESULT(0),
        WM_CHAR if with(|a| a.recording.is_some()) == Some(true) => return LRESULT(0),
        WM_ACTIVATE if wparam.0 & 0xFFFF == 0 => {
            with(|a| a.stop_recording());
            refresh();
        }
        WM_TIMER if wparam.0 == TIMER_TRAY => on_tray_timer(hwnd),
        WM_TIMER if wparam.0 == TIMER_STATUS => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_STATUS);
            }
            with(|a| a.status = config::load_status());
            refresh();
        }
        WM_CLOSE => {
            if confirm_close(hwnd) {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
            return LRESULT(0);
        }
        WM_DESTROY => {
            with(|a| a.stop_recording());
            unsafe { PostQuitMessage(0) };
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe extern "system" fn list_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => paint(Pane::List, hwnd),
        WM_ERASEBKGND => return LRESULT(1),
        WM_MOUSEMOVE => on_mouse_move(Pane::List, hwnd, point(lparam)),
        WM_MOUSELEAVE => on_mouse_leave(Pane::List, hwnd),
        WM_LBUTTONDOWN => on_click(Pane::List, point(lparam)),
        WM_MOUSEWHEEL => {
            on_wheel(wparam);
            return LRESULT(0);
        }
        WM_VSCROLL => {
            let page = with(|a| a.client(Pane::List).1).unwrap_or(0);
            let line = with(|a| a.gfx.s(ROW_H)).unwrap_or(40);
            let code = (wparam.0 & 0xFFFF) as i32;
            match code {
                c if c == SB_LINEUP.0 => scroll_by(-line),
                c if c == SB_LINEDOWN.0 => scroll_by(line),
                c if c == SB_PAGEUP.0 => scroll_by(-page),
                c if c == SB_PAGEDOWN.0 => scroll_by(page),
                c if c == SB_TOP.0 => scroll_by(i32::MIN / 2),
                c if c == SB_BOTTOM.0 => scroll_by(i32::MAX / 2),
                c if c == SB_THUMBTRACK.0 => {
                    let mut si = SCROLLINFO {
                        cbSize: size_of::<SCROLLINFO>() as u32,
                        fMask: SIF_TRACKPOS,
                        ..Default::default()
                    };
                    if unsafe { GetScrollInfo(hwnd, SB_VERT, &mut si) }.is_ok() {
                        with(|a| a.scroll = si.nTrackPos);
                        refresh();
                    }
                }
                _ => {}
            }
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub fn run() -> Result<(), String> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let icc = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES,
        };
        let _ = InitCommonControlsEx(&icc);
    }
    let hinstance = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    let dpi = unsafe { GetDpiForSystem() };
    let icon = |cx, cy| unsafe {
        LoadImageW(
            Some(hinstance.into()),
            PCWSTR(APP_ICON_ID as *const u16),
            IMAGE_ICON,
            GetSystemMetricsForDpi(cx, dpi),
            GetSystemMetricsForDpi(cy, dpi),
            LR_DEFAULTCOLOR,
        )
        .map(|h| HICON(h.0))
        .unwrap_or_default()
    };
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let main_class = w!("PanePogoSettings");
    let list_class = w!("PanePogoSettingsList");
    unsafe {
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(main_proc),
            hInstance: hinstance.into(),
            hIcon: icon(SM_CXICON, SM_CYICON),
            hIconSm: icon(SM_CXSMICON, SM_CYSMICON),
            hCursor: cursor,
            lpszClassName: main_class,
            ..Default::default()
        });
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(list_proc),
            hInstance: hinstance.into(),
            hCursor: cursor,
            lpszClassName: list_class,
            ..Default::default()
        });
    }

    APP.with(|cell| *cell.borrow_mut() = Some(App::new(Gfx::new(dpi))));

    let style = WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN;
    let mut outer = rect(0, 0, 860 * dpi as i32 / 96, 600 * dpi as i32 / 96);
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut outer, style, false, WINDOW_EX_STYLE(0), dpi);
    }
    let main = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            main_class,
            w!("Pane Pogo shortcuts"),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            outer.right - outer.left,
            outer.bottom - outer.top,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    let list = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            list_class,
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_VSCROLL,
            0,
            0,
            0,
            0,
            Some(main),
            None,
            Some(hinstance.into()),
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    let tip = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST,
            w!("tooltips_class32"),
            PCWSTR::null(),
            WS_POPUP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            Some(main),
            None,
            Some(hinstance.into()),
            None,
        )
    }
    .unwrap_or_default();

    let window_dpi = unsafe { GetDpiForWindow(main) };
    with(|a| {
        a.wins = Wins { main, list, tip };
        if window_dpi != 0 && window_dpi != a.gfx.dpi {
            a.gfx = Gfx::new(window_dpi);
        }
        unsafe {
            SendMessageW(
                tip,
                TTM_SETMAXTIPWIDTH,
                None,
                Some(LPARAM(a.gfx.s(360.0) as isize)),
            );
            SetTimer(Some(main), TIMER_TRAY, 1000, None);
        }
    });
    refresh();
    unsafe {
        let _ = ShowWindow(main, SW_SHOW);
    }

    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    // Dropping the state resumes the tray app's hotkeys if a recording was still open.
    APP.with(|cell| cell.borrow_mut().take());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_shortcut_names_keys() {
        // Shift+1: the virtual key is `1` on any layout, never `!`.
        assert_eq!(
            recorded_shortcut(false, true, true, u32::from(b'1')),
            Some("Alt+Shift+1".to_string())
        );
        assert_eq!(
            recorded_shortcut(true, false, false, 0x74),
            Some("Ctrl+F5".to_string())
        );
        // A key the tray app cannot register (VK_OEM_2, `/?` on US layouts).
        assert_eq!(recorded_shortcut(false, true, true, 0xBF), None);
    }

    fn sample_app() -> App {
        let actions = [
            Action::TileLeft,
            Action::TileDualMonitors,
            Action::Tile2to1,
            Action::MoveToNextMonitor,
            Action::FullSecondary,
            Action::SwapWindows,
            Action::TileRightSecondary,
        ];
        let bindings = actions
            .iter()
            .enumerate()
            .map(|(n, a)| Binding {
                shortcut: if n == 3 {
                    String::new()
                } else {
                    format!("Ctrl+Alt+{n}")
                },
                action: *a,
                pair: None,
            })
            .collect();
        let mut app = App::with_bindings(bindings, Gfx::new(96));
        app.expanded = Some(1);
        app.recording = Some(5);
        app
    }

    /// Clears the recording before drop, so dropping does not message a running tray app.
    fn finish(mut app: App) {
        app.recording = None;
    }

    /// Lays out seven rows at the minimum window width, with an expanded body, an error line
    /// and a recording row between them, and checks that every shortcut field and arrow lands
    /// on the same x.
    #[test]
    fn row_columns_align() {
        let app = sample_app();
        let (items, height) = app.layout(Pane::List, app.gfx.dc, false, 700, 400);
        let fields: Vec<RECT> = items
            .iter()
            .filter(|it| matches!(it.hit, Hit::Shortcut(_)))
            .map(|it| it.rect)
            .collect();
        // Each row pushes two Toggle items: the whole row, then the arrow.
        let arrows: Vec<RECT> = items
            .iter()
            .filter(|it| matches!(it.hit, Hit::Toggle(_)))
            .skip(1)
            .step_by(2)
            .map(|it| it.rect)
            .collect();
        assert_eq!(fields.len(), 7);
        assert_eq!(arrows.len(), 7);
        for (n, (f, a)) in fields.iter().zip(&arrows).enumerate() {
            assert_eq!(f.left, fields[0].left, "shortcut x, row {n}");
            assert_eq!(
                f.right - f.left,
                COL_SHORTCUT as i32,
                "shortcut width, row {n}"
            );
            assert_eq!(a.left, arrows[0].left, "arrow x, row {n}");
        }
        // Rows stack top to bottom and stay inside the minimum window width.
        assert!(fields.windows(2).all(|w| w[1].top > w[0].bottom));
        assert!(arrows[0].right <= 700);
        assert!(height > fields[6].bottom);
        // The expanded row shows its settings.
        assert!(items.iter().any(|it| it.hit == Hit::Picker(1)));
        assert!(items.iter().any(|it| it.hit == Hit::Remove(1)));
        finish(app);
    }

    fn hit_at(app: &App, p: POINT) -> Hit {
        let (items, _) = app.layout(Pane::List, app.gfx.dc, false, 700, 400);
        items
            .iter()
            .rev()
            .find(|it| contains(it.rect, p))
            .map_or(Hit::None, |it| it.hit)
    }

    #[test]
    fn row_click_targets() {
        let app = sample_app();
        let (items, _) = app.layout(Pane::List, app.gfx.dc, false, 700, 400);
        let row = items[0].rect;
        let field = items
            .iter()
            .find(|it| it.hit == Hit::Shortcut(0))
            .unwrap()
            .rect;
        let arrow = items
            .iter()
            .filter(|it| it.hit == Hit::Toggle(0))
            .nth(1)
            .unwrap()
            .rect;
        let mid = |r: RECT| POINT {
            x: (r.left + r.right) / 2,
            y: (r.top + r.bottom) / 2,
        };
        // Name column: toggles the row.
        let name = POINT {
            x: row.left + (COL_PREVIEW + COL_GAP + 20.0) as i32,
            y: mid(row).y,
        };
        assert_eq!(hit_at(&app, name), Hit::Toggle(0), "click on name");
        // Shortcut field: records only.
        assert_eq!(
            hit_at(&app, mid(field)),
            Hit::Shortcut(0),
            "click on shortcut"
        );
        // Chevron: toggles.
        assert_eq!(hit_at(&app, mid(arrow)), Hit::Toggle(0), "click on chevron");
        finish(app);
    }

    #[test]
    fn menu_round_trips_actions() {
        let mut app = App::with_bindings(
            vec![Binding {
                shortcut: "Alt+Shift+W".into(),
                action: Action::TileRightSecondary,
                pair: None,
            }],
            Gfx::new(96),
        );
        // Picking the kind already chosen keeps the monitor and side.
        app.set_action(0, menu_id(Action::TileRightSecondary));
        assert_eq!(app.bindings[0].action, Action::TileRightSecondary);
        app.set_action(0, menu_id(Action::Tile1to2));
        assert_eq!(app.bindings[0].action, Action::Tile1to2);
        app.pick(0, Pick::Focus(Focus::Current));
        assert!(app.bindings[0].pair.is_some());
        app.set_action(0, MENU_FULL);
        assert_eq!(app.bindings[0].action, Action::FullPrimary);
        assert_eq!(app.bindings[0].pair, None);
    }
}
