use crate::layout::Layout;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    TileWindows,
    /// 2/3 left + 1/3 right. Name kept for existing bindings.json files.
    Tile2to1,
    /// 1/3 left + 2/3 right.
    Tile1to2,
    TileDualMonitors,
    TileLeft,
    TileRight,
    TileLeftSecondary,
    TileRightSecondary,
    SwapWindows,
    MoveToNextMonitor,
    /// Fill the primary monitor's work area (taskbar stays visible).
    FullPrimary,
    /// Fill the secondary monitor's work area (taskbar stays visible).
    FullSecondary,
}

impl Action {
    /// Actions that place two windows and take the pair settings.
    pub fn is_pair(self) -> bool {
        matches!(
            self,
            Action::TileWindows | Action::Tile2to1 | Action::Tile1to2 | Action::TileDualMonitors
        )
    }

    /// Arrangement used by pair actions.
    pub fn layout(self) -> Layout {
        match self {
            Action::Tile2to1 => Layout::Split {
                left_share: 2.0 / 3.0,
            },
            Action::Tile1to2 => Layout::Split {
                left_share: 1.0 / 3.0,
            },
            Action::TileDualMonitors => Layout::Across,
            _ => Layout::Split { left_share: 0.5 },
        }
    }

    /// Settings a new pair binding starts with, matching the original script.
    pub fn default_pair(self) -> PairSettings {
        PairSettings {
            order: Order::CurrentRight,
            monitor: if self == Action::TileDualMonitors {
                MonitorRole::Secondary
            } else {
                MonitorRole::Primary
            },
            focus: Focus::Previous,
        }
    }
}

/// Which side of the split the current window takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    CurrentRight,
    CurrentLeft,
}

/// Which monitor the current window goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorRole {
    Primary,
    Secondary,
}

/// Which window keeps focus at the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Focus {
    Current,
    Previous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairSettings {
    pub order: Order,
    pub monitor: MonitorRole,
    pub focus: Focus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub shortcut: String,
    pub action: Action,
    /// Only used by pair actions. Missing means the action's default.
    #[serde(default)]
    pub pair: Option<PairSettings>,
}

impl Binding {
    pub fn pair_settings(&self) -> PairSettings {
        self.pair.unwrap_or_else(|| self.action.default_pair())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub bindings: Vec<Binding>,
}

fn dir() -> Result<PathBuf, String> {
    let appdata = std::env::var("APPDATA").map_err(|e| format!("APPDATA not set: {e}"))?;
    Ok(PathBuf::from(appdata).join("pane-pogo"))
}

pub fn path() -> Result<PathBuf, String> {
    Ok(dir()?.join("bindings.json"))
}

/// What the tray app could not register, written after each (re)load so the
/// settings window can show it next to the binding.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    /// Shortcuts that another program already holds.
    pub in_use: Vec<String>,
}

pub fn save_status(status: &Status) {
    if let Ok(dir) = dir() {
        let _ = fs::create_dir_all(&dir);
        if let Ok(text) = serde_json::to_string_pretty(status) {
            let _ = fs::write(dir.join("status.json"), text);
        }
    }
}

pub fn load_status() -> Status {
    dir()
        .ok()
        .and_then(|d| fs::read_to_string(d.join("status.json")).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Loads the config. If the file is missing, writes the default one (TileLeft and TileRight only).
pub fn load() -> Result<Config, String> {
    let path = path()?;
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let cfg = default_config();
            save(&cfg)?;
            Ok(cfg)
        }
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn save(cfg: &Config) -> Result<(), String> {
    let path = path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn default_config() -> Config {
    Config {
        bindings: vec![
            Binding {
                shortcut: "Alt+Shift+E".into(),
                action: Action::TileLeft,
                pair: None,
            },
            Binding {
                shortcut: "Alt+Shift+R".into(),
                action: Action::TileRight,
                pair: None,
            },
        ],
    }
}
