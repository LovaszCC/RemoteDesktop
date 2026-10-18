//! System keyboard capture for the embedded RDP canvas, so key combinations the OS would take (the
//! Windows key, Alt+Tab, Ctrl+Esc…) reach the remote session while the canvas has focus.
//!
//! - **Windows:** a low-level keyboard hook (`WH_KEYBOARD_LL`) on its own thread. Only the combinations
//!   the OS would handle itself are swallowed and forwarded ([`windows_should_capture`]); every other key
//!   still reaches the webview, whose canvas sends it as usual.
//! - **Linux on X11:** a best-effort keyboard grab on the root window (x11rb). While it is held the
//!   webview gets no keys at all, so every key is forwarded as a scancode (evdev code →
//!   [`evdev_to_scancode`]).
//! - **Linux on Wayland:** not possible for a client (no global grabs); [`support`] says so and nothing
//!   is captured. The canvas keeps working; only the compositor's own shortcuts stay local.
//! - **macOS:** not needed: ⌘ shortcuts arrive in the webview and the translator maps them.
//!
//! Captured keys go to a [`KeySink`] as RDP scancodes (`0x100` = extended).

#[cfg(windows)]
mod windows;
#[cfg(all(target_os = "linux", not(target_os = "android")))]
mod x11;

use std::sync::Arc;

/// A key the capture took from the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturedKey {
    /// RDP scancode, `0x100` set for extended keys.
    pub scancode: u16,
    /// Press or release.
    pub down: bool,
}

/// Receives captured keys (on the capture thread).
pub type KeySink = Arc<dyn Fn(CapturedKey) + Send + Sync>;

/// What capture means on this system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureMode {
    /// Windows: system combinations only, through the low-level hook.
    Hook,
    /// X11: every key, through a keyboard grab.
    Grab,
    /// Nothing can or needs to be captured (macOS, Wayland, no display).
    None,
}

/// Why capturing failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureError(pub String);

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CaptureError {}

/// A running capture; dropping it lets go.
pub struct KeyboardCapture {
    #[cfg(windows)]
    _inner: windows::Capture,
    #[cfg(all(target_os = "linux", not(target_os = "android")))]
    _inner: x11::Capture,
}

impl std::fmt::Debug for KeyboardCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KeyboardCapture(..)")
    }
}

/// What [`KeyboardCapture::start`] would do here.
#[must_use]
pub fn support() -> CaptureMode {
    if cfg!(windows) {
        CaptureMode::Hook
    } else if cfg!(target_os = "linux") {
        x11_mode(
            std::env::var_os("WAYLAND_DISPLAY").is_some(),
            std::env::var_os("DISPLAY").is_some(),
        )
    } else {
        CaptureMode::None
    }
}

/// Linux: X11 can grab; a Wayland session can't (even with Xwayland the webview is a Wayland client).
fn x11_mode(wayland: bool, display: bool) -> CaptureMode {
    if !wayland && display {
        CaptureMode::Grab
    } else {
        CaptureMode::None
    }
}

impl KeyboardCapture {
    /// Starts capturing; `Ok(None)` where nothing can be captured ([`support`] is `None`).
    ///
    /// # Errors
    /// When the hook or the grab can't be set up.
    #[allow(clippy::unnecessary_wraps, clippy::needless_pass_by_value)]
    pub fn start(sink: KeySink) -> Result<Option<Self>, CaptureError> {
        #[cfg(windows)]
        {
            return windows::Capture::start(sink).map(|inner| Some(Self { _inner: inner }));
        }
        #[cfg(all(target_os = "linux", not(target_os = "android")))]
        {
            if support() == CaptureMode::Grab {
                return x11::Capture::start(sink).map(|inner| Some(Self { _inner: inner }));
            }
            return Ok(None);
        }
        #[allow(unreachable_code)]
        {
            let _ = sink;
            Ok(None)
        }
    }
}

/// Virtual-key codes the Windows rule looks at.
pub mod vk {
    /// Tab.
    pub const TAB: u32 = 0x09;
    /// Esc.
    pub const ESCAPE: u32 = 0x1B;
    /// Space.
    pub const SPACE: u32 = 0x20;
    /// Left Windows key.
    pub const LWIN: u32 = 0x5B;
    /// Right Windows key.
    pub const RWIN: u32 = 0x5C;
}

/// The modifiers held when a key event arrives.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Held {
    /// Either Alt.
    pub alt: bool,
    /// Either Ctrl.
    pub ctrl: bool,
    /// Either Windows key.
    pub win: bool,
}

/// Windows: whether a key event belongs to a combination the OS would handle itself, so the hook takes
/// it for the remote session: the Windows keys and anything pressed with them, Alt+Tab, Alt+Esc,
/// Alt+Space and Ctrl+Esc. Alt+F4 stays local so the app can always be closed.
#[must_use]
pub fn windows_should_capture(vk: u32, held: Held) -> bool {
    matches!(vk, vk::LWIN | vk::RWIN)
        || held.win
        || (held.alt && matches!(vk, vk::TAB | vk::ESCAPE | vk::SPACE))
        || (held.ctrl && vk == vk::ESCAPE)
}

/// The RDP scancode of a Linux evdev key code (X keycode − 8); `None` for keys RDP has no code for.
#[must_use]
pub fn evdev_to_scancode(code: u16) -> Option<u16> {
    Some(match code {
        // KEY_ESC … KEY_F10, the keypad and KEY_102ND, KEY_F11, KEY_F12: set 1 uses the same numbers.
        1..=83 | 86..=88 => code,
        96 => 0x11C,  // KP Enter
        97 => 0x11D,  // Right Ctrl
        98 => 0x135,  // KP /
        99 => 0x137,  // SysRq / Print Screen
        100 => 0x138, // Right Alt
        102 => 0x147, // Home
        103 => 0x148, // Up
        104 => 0x149, // Page Up
        105 => 0x14B, // Left
        106 => 0x14D, // Right
        107 => 0x14F, // End
        108 => 0x150, // Down
        109 => 0x151, // Page Down
        110 => 0x152, // Insert
        111 => 0x153, // Delete
        113 => 0x120, // Mute
        114 => 0x12E, // Volume down
        115 => 0x130, // Volume up
        117 => 0x59,  // KP =
        119 => 0x146, // Pause
        125 => 0x15B, // Left Meta
        126 => 0x15C, // Right Meta
        127 => 0x15D, // Menu
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_takes_only_system_combinations() {
        let none = Held::default();
        let alt = Held {
            alt: true,
            ..Held::default()
        };
        let ctrl = Held {
            ctrl: true,
            ..Held::default()
        };
        let win = Held {
            win: true,
            ..Held::default()
        };
        assert!(windows_should_capture(vk::LWIN, none));
        assert!(windows_should_capture(vk::RWIN, none));
        assert!(windows_should_capture(0x52, win)); // Win+R
        assert!(windows_should_capture(vk::TAB, alt));
        assert!(windows_should_capture(vk::ESCAPE, alt));
        assert!(windows_should_capture(vk::SPACE, alt));
        assert!(windows_should_capture(vk::ESCAPE, ctrl));
        assert!(!windows_should_capture(0x73, alt)); // Alt+F4 stays local
        assert!(!windows_should_capture(vk::TAB, none));
        assert!(!windows_should_capture(0x43, ctrl)); // Ctrl+C reaches the webview
        assert!(!windows_should_capture(0x41, none));
    }

    #[test]
    fn evdev_codes_map_to_set_one_scancodes() {
        assert_eq!(evdev_to_scancode(1), Some(0x01)); // Esc
        assert_eq!(evdev_to_scancode(30), Some(0x1E)); // A
        assert_eq!(evdev_to_scancode(88), Some(0x58)); // F12
        assert_eq!(evdev_to_scancode(86), Some(0x56)); // 102nd key
        assert_eq!(evdev_to_scancode(105), Some(0x14B)); // Left
        assert_eq!(evdev_to_scancode(125), Some(0x15B)); // Meta
        assert_eq!(evdev_to_scancode(84), None);
        assert_eq!(evdev_to_scancode(0), None);
        assert_eq!(evdev_to_scancode(300), None);
    }

    #[test]
    fn wayland_means_no_capture() {
        assert_eq!(x11_mode(false, true), CaptureMode::Grab);
        assert_eq!(x11_mode(true, true), CaptureMode::None);
        assert_eq!(x11_mode(false, false), CaptureMode::None);
        if cfg!(target_os = "macos") {
            assert_eq!(support(), CaptureMode::None);
            assert!(KeyboardCapture::start(Arc::new(|_| {})).unwrap().is_none());
        }
        assert_eq!(
            serde_json::to_value(CaptureMode::Hook).unwrap(),
            serde_json::json!("hook")
        );
        assert_eq!(CaptureError("x".into()).to_string(), "x");
    }
}
