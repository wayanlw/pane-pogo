# pane-pogo specification

This file defines how pane-pogo behaves.

## Terms

- **Current window**: the foreground window when a shortcut is pressed.
- **Previous window**: the most recently focused window other than the current one, taken from focus history recorded since pane-pogo started. A window only counts as current or previous if it is **valid**: visible, not minimised, has a title, and is not cloaked (cloaked covers windows on other virtual desktops and suspended Store apps).
- **Primary monitor**: the monitor Windows marks as primary.
- **Secondary monitor**: the first non-primary monitor in Windows' enumeration order.
- **Work area**: a monitor's area minus the taskbar. All placement uses work areas, in each monitor's own size.

## Actions

| Config name | Label in Settings | Behaviour | Pair settings |
|---|---|---|---|
| `tile_windows` | Side by side (½ + ½) | Splits a monitor in halves between the current and previous window. | yes |
| `tile2to1` | Side by side (2/3 left + 1/3 right) | Splits a monitor 2/3 on the left and 1/3 on the right. | yes |
| `tile1to2` | Side by side (1/3 left + 2/3 right) | Splits a monitor 1/3 on the left and 2/3 on the right. | yes |
| `tile_dual_monitors` | Full screen on separate monitors | The current window fills one monitor and the previous window fills the other. | yes |
| `swap_windows` | Swap places | The current and previous window exchange position and size. Focus moves to the previous window. | no |
| `full_primary` | Full screen on primary monitor | The current window fills the primary work area. | no |
| `full_secondary` | Full screen on secondary monitor | The current window fills the secondary work area. | no |
| `tile_left` / `tile_right` | Left / Right half of primary monitor | The current window takes that half of the primary work area. | no |
| `tile_left_secondary` / `tile_right_secondary` | Left / Right half of secondary monitor | The current window takes that half of the secondary work area. | no |
| `move_to_next_monitor` | Move to next monitor | Moves the current window to the next monitor in enumeration order, keeping its size and its position relative to the work area, clamped to the target. | no |

Every action restores a maximised window before moving it.

**Settings presentation.** The Action drop-down lists **One window** first, then **Two windows**. **Full screen on one monitor** and **Half of one monitor** are single entries. A **Monitor** row (and, for halves, a **Side** row) below the action picks the config name in the table above. The saved file still stores each config name.

## Pair settings

Only the four "Side by side" and "Full screen on separate monitors" actions use these.

| Setting | Values | Meaning |
|---|---|---|
| `order` | `current_left`, `current_right` | Which side of the split the current window takes. Splits are positional, so in `tile2to1` the current window gets the 2/3 when it is on the left. Not used by `tile_dual_monitors`. |
| `monitor` | `primary`, `secondary` | Split actions: the monitor that is split. `tile_dual_monitors`: the monitor the current window fills. |
| `focus` | `current`, `previous` | The window that is active after the action. |

**Defaults** when `pair` is missing: `current_right`, `previous` focus, and the `primary` monitor. `tile_dual_monitors` defaults to the `secondary` monitor.

## Fallbacks

- **No valid previous window:** the current window alone takes the place it would have in the pair, and keeps focus.
- **Split action set to `secondary` with no secondary monitor:** the primary is split.
- **`tile_dual_monitors` with no secondary monitor:** both windows share a half split on the primary.
- **`full_secondary`, `tile_left_secondary` or `tile_right_secondary` with no secondary monitor:** nothing happens.
- **`move_to_next_monitor` with one monitor:** nothing happens.

## Shortcuts

- **Format:** `Modifier+...+Key`, case-insensitive. For example `Alt+Shift+E`.
- **Modifiers:** `Ctrl` (or `Control`), `Alt`, `Shift`, `Win`. At least one is required.
- **Keys:** `A`–`Z`, `0`–`9`, `F1`–`F12`. Exactly one key.
- **Recording:** the Settings recorder captures Ctrl, Alt and Shift. `Win` works only when written in `bindings.json`.
- **Key repeat:** holding the keys fires the action once.
- **Uniqueness:** each shortcut may appear once in the list.
- **Conflicts:** a shortcut another program holds cannot register. pane-pogo skips it, logs it, lists it in `status.json`, and Settings marks the row.
- **Defaults:** when `bindings.json` does not exist, pane-pogo creates it with eight default shortcuts: `Alt+Shift+1` → `full_primary`, `Alt+Shift+2` → `full_secondary`, `Alt+Shift+E` → `tile_left`, `Alt+Shift+R` → `tile_right`, `Alt+Shift+W` → `tile_windows` (current window left, primary monitor, focus stays), `Alt+Shift+D` → `tile_dual_monitors` (current window on the primary monitor, focus stays), `Alt+Shift+S` → `swap_windows` and `Alt+Shift+Z` → `move_to_next_monitor`. The user can change or remove every one in Settings. If another program already holds a default, pane-pogo skips it, marks the row in Settings and shows one tray notification on that first run.

## Files

| Path | Written by | Content |
|---|---|---|
| `%APPDATA%\pane-pogo\bindings.json` | Settings, or first run | The user's bindings. See schema below. |
| `%APPDATA%\pane-pogo\status.json` | Tray app, after each (re)load | `{ "in_use": [shortcuts held by other programs] }` |
| `%APPDATA%\pane-pogo\pane-pogo.log` | Both processes | Timestamped events. Renamed to `pane-pogo.log.old` when over 1 MB at start. |
| `%APPDATA%\pane-pogo\bindings.broken.json` | Settings, "Start fresh" | The unreadable file, kept as a backup. |
| `%LOCALAPPDATA%\Programs\pane-pogo\pane-pogo.exe` | Tray, "Start with Windows" on | Installed copy that starts at login. |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `PanePogo` | Tray, "Start with Windows" | Quoted path of the installed copy. Removed when turned off. |

**`bindings.json` schema**

```json
{
  "bindings": [
    { "shortcut": "Alt+Shift+W", "action": "tile_windows",
      "pair": { "order": "current_right", "monitor": "primary", "focus": "previous" } },
    { "shortcut": "Alt+Shift+E", "action": "tile_left", "pair": null }
  ]
}
```

`pair` is optional. Single-window actions ignore it.

## Process model

- **`pane-pogo.exe`:** the tray app. It holds the hotkeys, tracks focus, and has a tray menu with Settings..., Start with Windows, and Exit. It is a GUI-subsystem program, so no console opens.
- **`pane-pogo.exe --settings`:** the settings window, in its own process. It closes within about a second of the tray app exiting. If there are unsaved changes, it asks first.
- **A second launch** while the tray app runs opens Settings instead of a second tray app.
- **Settings → tray signals:** thread messages to the tray app's thread. `WM_APP+3` reloads `bindings.json` and re-registers the hotkeys. `WM_APP+4` releases all hotkeys while a shortcut is recorded. Recording ends with a reload.
- **Broken `bindings.json`:**
  - The tray app runs with no shortcuts and shows an error notification.
  - Settings blocks Save until you choose Open file, Try again, or Start fresh.
  - If a reload fails, the tray app keeps the previous bindings.
- **Explorer restart:** the tray icon is re-added when Explorer broadcasts `TaskbarCreated`.
- **Graphics:** Settings renders with wgpu on DirectX 12, falling back to OpenGL. Vulkan is excluded unless the `WGPU_BACKEND` environment variable is set.

## Known limitations

- Monitors with different scaling (DPI) can place windows off-target. pane-pogo does not declare DPI awareness.
- Windows' invisible resize borders leave visible gaps of about 7 px between tiled windows.
- Elevated (administrator) windows cannot be moved.
- Apps with a minimum width larger than the target part, such as a 1/3 split on a small screen, overlap the other window.
- `move_to_next_monitor` keeps the pixel size, so a large window can overflow a smaller monitor.
- With three or more monitors, which one counts as "secondary" can change after docking or a reboot.
- The installed copy does not update itself after a rebuild. Turn Start with Windows off and on again to refresh it.
- Global shortcuts take those keys away from every other app, and `Alt+Shift` also switches the input language when several keyboard layouts are installed.
- The exe is unsigned, so Windows SmartScreen may warn on first run.
