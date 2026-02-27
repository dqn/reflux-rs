//! Keyboard input simulation via SendInput API.
//!
//! Uses scan codes with `KEYEVENTF_SCANCODE` for DirectInput compatibility,
//! which is required for INFINITAS to recognize the input.

use std::time::Duration;

/// Game-relevant keys for navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
}

impl GameKey {
    /// Virtual key code for `MapVirtualKeyW` lookup.
    #[cfg(target_os = "windows")]
    fn virtual_key(self) -> u16 {
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        match self {
            Self::Up => VK_UP.0,
            Self::Down => VK_DOWN.0,
            Self::Left => VK_LEFT.0,
            Self::Right => VK_RIGHT.0,
            Self::Enter => VK_RETURN.0,
            Self::Escape => VK_ESCAPE.0,
        }
    }
}

/// Send a key press (down + delay + up) for the given key.
///
/// Uses scan codes so that DirectInput-based games (like INFINITAS) can
/// recognise the input. Extended keys (arrows) get the `KEYEVENTF_EXTENDEDKEY`
/// flag automatically.
#[cfg(target_os = "windows")]
pub fn send_key_press(key: GameKey, hold: Duration) -> anyhow::Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;

    let scan = unsafe { MapVirtualKeyW(key.virtual_key() as u32, MAPVK_VK_TO_VSC) } as u16;

    let is_extended = matches!(
        key,
        GameKey::Up | GameKey::Down | GameKey::Left | GameKey::Right
    );

    let mut flags_down = KEYEVENTF_SCANCODE;
    let mut flags_up = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;
    if is_extended {
        flags_down |= KEYEVENTF_EXTENDEDKEY;
        flags_up |= KEYEVENTF_EXTENDEDKEY;
    }

    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: scan,
                dwFlags: flags_down,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: scan,
                dwFlags: flags_up,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    // SAFETY: SendInput is called with properly initialized INPUT structs.
    // The array contains exactly the number of elements indicated by the count parameter.
    let sent = unsafe { SendInput(&[down], std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        anyhow::bail!(
            "SendInput (key down) failed: {}",
            std::io::Error::last_os_error()
        );
    }

    std::thread::sleep(hold);

    let sent = unsafe { SendInput(&[up], std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        anyhow::bail!(
            "SendInput (key up) failed: {}",
            std::io::Error::last_os_error()
        );
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn send_key_press(_key: GameKey, _hold: Duration) -> anyhow::Result<()> {
    anyhow::bail!("SendInput is only supported on Windows")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_key_debug() {
        // Ensure all variants are representable
        let keys = [
            GameKey::Up,
            GameKey::Down,
            GameKey::Left,
            GameKey::Right,
            GameKey::Enter,
            GameKey::Escape,
        ];
        for key in &keys {
            let _ = format!("{:?}", key);
        }
    }
}
