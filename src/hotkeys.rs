//! Shortcut parsing ("Alt+Shift+E") and RegisterHotKey wrappers.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub modifiers: u32,
    pub vk: u32,
}

/// Parses a shortcut such as "Alt+Shift+E". Keys: A–Z, 0–9, F1–F12.
pub fn parse(s: &str) -> Result<Chord, String> {
    let mut modifiers = 0u32;
    let mut vk = None;
    for part in s.split('+').map(|p| p.trim().to_ascii_uppercase()) {
        match part.as_str() {
            "ALT" => modifiers |= MOD_ALT.0,
            "SHIFT" => modifiers |= MOD_SHIFT.0,
            "CTRL" | "CONTROL" => modifiers |= MOD_CONTROL.0,
            "WIN" => modifiers |= MOD_WIN.0,
            key => {
                if vk.is_some() {
                    return Err(format!("'{s}' has more than one key"));
                }
                vk = Some(key_to_vk(key).ok_or_else(|| format!("unknown key '{key}' in '{s}'"))?);
            }
        }
    }
    let vk = vk.ok_or_else(|| format!("'{s}' has no key"))?;
    if modifiers == 0 {
        return Err(format!("'{s}' needs at least one modifier"));
    }
    Ok(Chord { modifiers, vk })
}

fn key_to_vk(key: &str) -> Option<u32> {
    let b = key.as_bytes();
    match b {
        [c] if c.is_ascii_uppercase() || c.is_ascii_digit() => Some(*c as u32),
        _ if key.starts_with('F') => {
            let n: u32 = key[1..].parse().ok()?;
            (1..=12).contains(&n).then(|| 0x70 + n - 1)
        }
        _ => None,
    }
}

/// Registers a chord on the current thread. Returns Err with the Win32 error code on failure,
/// which usually means another app or binding already holds the shortcut.
pub fn register(id: i32, chord: Chord) -> Result<(), String> {
    unsafe {
        RegisterHotKey(
            None,
            id,
            HOT_KEY_MODIFIERS(chord.modifiers | MOD_NOREPEAT.0),
            chord.vk,
        )
        .map_err(|e| format!("already in use or reserved (Win32: {e})"))
    }
}

pub fn unregister(id: i32) {
    unsafe {
        let _ = UnregisterHotKey(None, id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_letter_chord() {
        let c = parse("Alt+Shift+E").unwrap();
        assert_eq!(c.vk, 'E' as u32);
        assert_eq!(c.modifiers, MOD_ALT.0 | MOD_SHIFT.0);
    }

    #[test]
    fn parses_function_key() {
        assert_eq!(parse("Ctrl+F5").unwrap().vk, 0x74);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("Alt+Shift+").is_err());
        assert!(parse("E").is_err());
        assert!(parse("Alt+Foo").is_err());
        assert!(parse("Alt+E+R").is_err());
    }
}
