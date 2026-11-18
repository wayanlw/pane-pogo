# How to use Pane Pogo

Task steps. For the exact rules behind each action, setting and file, see [spec.md](spec.md).

## Run it

- **From source:** install [Rust](https://rustup.rs). Then run this in the project folder:

  ```powershell
  cargo run
  ```

  The app starts in the tray. If the icon isn't visible, look under the `^` overflow arrow.
- **Installed copy:** run `%LOCALAPPDATA%\Programs\pane-pogo\pane-pogo.exe`. It exists after you turn on Start with Windows.

## Default shortcuts

On first run, Pane Pogo creates these shortcuts. They are only defaults: change or remove any of them in Settings.

| Shortcut | What happens |
|---|---|
| `Alt+Shift+1` | Current window fills the primary monitor. |
| `Alt+Shift+2` | Current window fills the secondary monitor. |
| `Alt+Shift+E` | Current window takes the left half of the primary monitor. |
| `Alt+Shift+R` | Current window takes the right half of the primary monitor. |
| `Alt+Shift+W` | Side by side on the primary monitor. Current window left, previous window right. You keep typing in the current window. |
| `Alt+Shift+D` | Each window fills its own monitor. Current window on the primary monitor. You keep typing in the current window. |
| `Alt+Shift+S` | Swap places: the two windows trade position and size. |
| `Alt+Shift+Z` | Move the current window to the next monitor, keeping its size. |

If another program already uses one of them, Pane Pogo shows a notification on first run and Settings marks that row. Pick a different shortcut there.

## Add or change a shortcut

1. Right-click the tray icon and choose **Settings...**
2. Click **Add shortcut**, or edit an existing row.
3. Pick the **Action**. Hover over an action or an ℹ icon to see what it does.
4. Click the **Shortcut** box and press the keys. Hold Ctrl, Alt or Shift, then press a letter, number or F1–F12. **Esc** cancels.
5. Set the rows under the action. **Full screen on one monitor**: **Monitor**. **Half of one monitor**: **Monitor** and **Side**. Two-window actions: **Current window on** (Left or Right for side-by-side layouts, or a monitor for Full screen on separate monitors), **Monitor** (side-by-side layouts only), and **Final focus**.
6. Click **Save**. The shortcuts work at once. You don't need to restart.

A red message on a row blocks saving until you fix it. An orange message is only a warning.

## Start with Windows

1. Right-click the tray icon and tick **Start with Windows**.
2. A notification shows where it installed the copy that will start at login.

**After rebuilding from source:**
1. Exit any copy running from `%LOCALAPPDATA%\Programs\pane-pogo\`.
2. Run the new build.
3. Untick, then re-tick **Start with Windows**. This refreshes the installed copy.

## A shortcut does nothing

Check these in order:

1. **Is Pane Pogo running?** Look for the tray icon. Settings shows a banner when it isn't running.
2. **Does the row show "Another program already uses this shortcut"?** Pick a different shortcut, or close the program that holds it.
3. **Was the current window minimised, cloaked or elevated?** Those windows are skipped. See the Known limitations in [spec.md](spec.md).
4. **Read the log** at `%APPDATA%\pane-pogo\pane-pogo.log`. Each start lists every shortcut as `Registered` or `Skipped`, with the reason.

## "Shortcuts file has an error"

`bindings.json` could not be read, so no shortcuts are active. Open **Settings...** from the tray, then choose one:

- **Open file:** fix it in Notepad, then click **Try again**.
- **Start fresh:** keeps the broken file as `bindings.broken.json` and starts with no shortcuts.

## Stop or uninstall

1. **Stop:** right-click the tray icon and choose **Exit**.
2. **Remove startup:** untick **Start with Windows** before exiting.
3. **Uninstall:** delete `%LOCALAPPDATA%\Programs\pane-pogo\`.
4. **Remove your settings and logs:** delete `%APPDATA%\pane-pogo\`.
