//! Keyboard input for the embedded RDP canvas: `KeyboardEvent.code` → RDP scancode, and the translator
//! that decides between scancodes and Unicode key events (a port of Swift `RDPKeyboardTranslator`, keyed
//! by the physical `code` instead of the Mac virtual key code).
//!
//! Scancodes are PS/2 set 1 codes as RDP uses them; `0x100` marks the extended (`E0`) ones, as in
//! `FreeRDP`'s `scancode.h`.
//!
//! The rules:
//! - Plain keys go out as scancodes, so the remote keyboard layout applies.
//! - **macOS:** Option (either side) is held back. With a key that types a character it types that Mac
//!   character as Unicode (so `⌥2` gives what the Mac layout gives); with a key that types none it becomes
//!   Alt (left or right). A dead key (`⌥E`) is swallowed and the next key's composed character is typed.
//!   ⌘ becomes Ctrl, and keys pressed with ⌘ are released at once (`WebKit` sends no keyup for them); ⌘
//!   tapped alone is the Windows key. Control stays Control. Caps Lock toggles on each change.
//! - **Windows and Linux:** Ctrl, Alt and Win go out as they are. AltGr (`AltRight`) follows the Option
//!   rule (layout characters are typed as Unicode); the fake left Ctrl Windows sends with AltGr is
//!   released first.

use std::collections::HashSet;

/// Left Shift.
pub const SC_LSHIFT: u16 = 0x2A;
/// Right Shift.
pub const SC_RSHIFT: u16 = 0x36;
/// Left Ctrl.
pub const SC_LCONTROL: u16 = 0x1D;
/// Right Ctrl (extended).
pub const SC_RCONTROL: u16 = 0x11D;
/// Left Alt.
pub const SC_LMENU: u16 = 0x38;
/// Right Alt / AltGr (extended).
pub const SC_RMENU: u16 = 0x138;
/// Left Windows key (extended).
pub const SC_LWIN: u16 = 0x15B;
/// Right Windows key (extended).
pub const SC_RWIN: u16 = 0x15C;
/// Caps Lock.
pub const SC_CAPSLOCK: u16 = 0x3A;
/// Enter.
pub const SC_RETURN: u16 = 0x1C;
/// Tab.
pub const SC_TAB: u16 = 0x0F;
/// The extended flag inside a scancode value.
pub const EXTENDED: u16 = 0x100;

/// The keys outside the letter, digit and F-key runs.
const TABLE: &[(&str, u16)] = &[
    ("Escape", 0x01),
    ("Minus", 0x0C),
    ("Equal", 0x0D),
    ("Backspace", 0x0E),
    ("Tab", SC_TAB),
    ("BracketLeft", 0x1A),
    ("BracketRight", 0x1B),
    ("Enter", SC_RETURN),
    ("ControlLeft", SC_LCONTROL),
    ("Semicolon", 0x27),
    ("Quote", 0x28),
    ("Backquote", 0x29),
    ("ShiftLeft", SC_LSHIFT),
    ("Backslash", 0x2B),
    ("Comma", 0x33),
    ("Period", 0x34),
    ("Slash", 0x35),
    ("ShiftRight", SC_RSHIFT),
    ("NumpadMultiply", 0x37),
    ("AltLeft", SC_LMENU),
    ("Space", 0x39),
    ("CapsLock", SC_CAPSLOCK),
    ("NumLock", 0x45),
    ("ScrollLock", 0x46),
    ("Numpad7", 0x47),
    ("Numpad8", 0x48),
    ("Numpad9", 0x49),
    ("NumpadSubtract", 0x4A),
    ("Numpad4", 0x4B),
    ("Numpad5", 0x4C),
    ("Numpad6", 0x4D),
    ("NumpadAdd", 0x4E),
    ("Numpad1", 0x4F),
    ("Numpad2", 0x50),
    ("Numpad3", 0x51),
    ("Numpad0", 0x52),
    ("NumpadDecimal", 0x53),
    ("IntlBackslash", 0x56),
    ("NumpadEqual", 0x59),
    ("KanaMode", 0x70),
    ("Lang2", 0x71),
    ("Lang1", 0x72),
    ("IntlRo", 0x73),
    ("Convert", 0x79),
    ("NonConvert", 0x7B),
    ("IntlYen", 0x7D),
    ("NumpadComma", 0x7E),
    ("MediaTrackPrevious", 0x110),
    ("MediaTrackNext", 0x119),
    ("NumpadEnter", 0x11C),
    ("ControlRight", SC_RCONTROL),
    ("AudioVolumeMute", 0x120),
    ("LaunchApp2", 0x121),
    ("MediaPlayPause", 0x122),
    ("MediaStop", 0x124),
    ("AudioVolumeDown", 0x12E),
    ("AudioVolumeUp", 0x130),
    ("BrowserHome", 0x132),
    ("NumpadDivide", 0x135),
    ("PrintScreen", 0x137),
    ("AltRight", SC_RMENU),
    ("Pause", 0x146),
    ("Home", 0x147),
    ("ArrowUp", 0x148),
    ("PageUp", 0x149),
    ("ArrowLeft", 0x14B),
    ("ArrowRight", 0x14D),
    ("End", 0x14F),
    ("ArrowDown", 0x150),
    ("PageDown", 0x151),
    ("Insert", 0x152),
    ("Help", 0x152),
    ("Delete", 0x153),
    ("MetaLeft", SC_LWIN),
    ("OSLeft", SC_LWIN),
    ("MetaRight", SC_RWIN),
    ("OSRight", SC_RWIN),
    ("ContextMenu", 0x15D),
    ("Power", 0x15E),
    ("Sleep", 0x15F),
    ("WakeUp", 0x163),
    ("BrowserSearch", 0x165),
    ("BrowserFavorites", 0x166),
    ("BrowserRefresh", 0x167),
    ("BrowserStop", 0x168),
    ("BrowserForward", 0x169),
    ("BrowserBack", 0x16A),
    ("LaunchApp1", 0x16B),
    ("LaunchMail", 0x16C),
    ("MediaSelect", 0x16D),
];

/// The RDP scancode of a `KeyboardEvent.code`, `0x100` set for extended keys; `None` for codes RDP has
/// no key for.
#[must_use]
pub fn scancode(code: &str) -> Option<u16> {
    let letters = "QWERTYUIOPASDFGHJKLZXCVBNM";
    let letter_codes: [u16; 26] = [
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24,
        0x25, 0x26, 0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32,
    ];
    if let Some(letter) = code.strip_prefix("Key")
        && letter.len() == 1
    {
        return letters.find(letter).map(|i| letter_codes[i]);
    }
    if let Some(digit) = code.strip_prefix("Digit")
        && let Ok(n) = digit.parse::<u16>()
        && n <= 9
    {
        return Some(if n == 0 { 0x0B } else { 0x01 + n });
    }
    if let Some(n) = code.strip_prefix('F').and_then(|n| n.parse::<u16>().ok()) {
        return match n {
            1..=10 => Some(0x3A + n),
            11 => Some(0x57),
            12 => Some(0x58),
            13..=23 => Some(0x64 + n - 13),
            24 => Some(0x76),
            _ => None,
        };
    }
    TABLE.iter().find(|(c, _)| *c == code).map(|&(_, sc)| sc)
}

/// Which modifier rules apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardPlatform {
    /// Option/⌘ rules.
    MacOs,
    /// Ctrl/Alt/Win as they are, AltGr types layout characters.
    Other,
}

impl KeyboardPlatform {
    /// The platform this build runs on.
    #[must_use]
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Other
        }
    }
}

/// One key event from the canvas (a DOM `KeyboardEvent`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInput {
    /// `KeyboardEvent.code`: the physical key.
    pub code: String,
    /// `KeyboardEvent.key`: the character it types, `"Dead"` for a dead key, or a key name.
    pub key: String,
    /// `keydown` (`true`) or `keyup`.
    pub down: bool,
}

impl KeyInput {
    /// A key event.
    #[must_use]
    pub fn new(code: &str, key: &str, down: bool) -> Self {
        Self {
            code: code.to_owned(),
            key: key.to_owned(),
            down,
        }
    }
}

/// What goes to the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// A scancode press or release (`0x100` = extended).
    Scancode {
        /// The scancode.
        code: u16,
        /// Press or release.
        down: bool,
    },
    /// A UTF-16 unit typed as a Unicode key event (press and release).
    Unicode(u16),
}

impl KeyAction {
    /// A press.
    #[must_use]
    pub fn down(code: u16) -> Self {
        Self::Scancode { code, down: true }
    }

    /// A release.
    #[must_use]
    pub fn up(code: u16) -> Self {
        Self::Scancode { code, down: false }
    }

    /// The units of `text` as Unicode events.
    #[must_use]
    pub fn text(text: &str) -> Vec<Self> {
        text.encode_utf16().map(Self::Unicode).collect()
    }
}

/// What a key event's `key` says about the character it types.
enum Typed<'a> {
    /// A dead key: the next key brings the composed character.
    Dead,
    /// Text to type (no control characters).
    Text(&'a str),
    /// A named key (`Enter`, `F4`, `ArrowLeft`…) or a control character.
    Nothing,
}

fn typed(key: &str) -> Typed<'_> {
    if key == "Dead" {
        return Typed::Dead;
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_control() && !('\u{F700}'..='\u{F8FF}').contains(&c) => Typed::Text(key),
        _ => Typed::Nothing,
    }
}

/// Translates canvas key events into RDP key actions, keeping track of what is held.
#[derive(Debug, Clone)]
pub struct KeyboardTranslator {
    platform: KeyboardPlatform,
    /// Modifier scancodes sent down and not yet released, in press order.
    held_modifiers: Vec<u16>,
    /// Ordinary keys sent down and not yet released.
    held_keys: Vec<u16>,
    /// Codes whose press went out as Unicode (or was swallowed): their release sends nothing.
    swallowed: HashSet<String>,
    /// Physical keys held, by code (both Ctrl, both Shift, both ⌘ count separately).
    physical: HashSet<String>,
    command_used: bool,
    dead_key_pending: bool,
    /// The left Ctrl held now is the fake one Windows sends with AltGr.
    altgr_control: bool,
}

impl KeyboardTranslator {
    /// A translator with nothing held.
    #[must_use]
    pub fn new(platform: KeyboardPlatform) -> Self {
        Self {
            platform,
            held_modifiers: Vec::new(),
            held_keys: Vec::new(),
            swallowed: HashSet::new(),
            physical: HashSet::new(),
            command_used: false,
            dead_key_pending: false,
            altgr_control: false,
        }
    }

    fn held(&self, code: &str) -> bool {
        self.physical.contains(code)
    }

    fn command_held(&self) -> bool {
        self.platform == KeyboardPlatform::MacOs && (self.held("MetaLeft") || self.held("MetaRight"))
    }

    fn left_option_held(&self) -> bool {
        self.platform == KeyboardPlatform::MacOs && self.held("AltLeft")
    }

    /// Right Option on macOS, AltGr elsewhere.
    fn right_option_held(&self) -> bool {
        self.held("AltRight")
    }

    fn control_held(&self) -> bool {
        (self.held("ControlLeft") && !self.altgr_control) || self.held("ControlRight")
    }

    /// Sends a modifier's press or release once (a repeated press or a release of an unsent key is
    /// dropped).
    fn set(&mut self, code: u16, down: bool, actions: &mut Vec<KeyAction>) {
        let position = self.held_modifiers.iter().position(|&c| c == code);
        match (down, position) {
            (true, None) => self.held_modifiers.push(code),
            (false, Some(i)) => {
                self.held_modifiers.remove(i);
            }
            _ => return,
        }
        actions.push(KeyAction::Scancode { code, down });
    }

    /// One key event; returns what to send.
    pub fn key(&mut self, input: &KeyInput) -> Vec<KeyAction> {
        let code = input.code.as_str();
        let was_held = self.physical.contains(code);
        if input.down {
            self.physical.insert(code.to_owned());
        } else {
            self.physical.remove(code);
        }
        if let Some(actions) = self.modifier(code, input.down, was_held) {
            return actions;
        }
        if input.down {
            self.key_down(code, &input.key)
        } else {
            self.key_up(code)
        }
    }

    /// Modifier keys; `None` for ordinary keys.
    fn modifier(&mut self, code: &str, down: bool, was_held: bool) -> Option<Vec<KeyAction>> {
        let mac = self.platform == KeyboardPlatform::MacOs;
        let mut actions = Vec::new();
        match code {
            "ShiftLeft" => self.set(SC_LSHIFT, down, &mut actions),
            "ShiftRight" => self.set(SC_RSHIFT, down, &mut actions),
            "ControlLeft" | "ControlRight" if mac => {
                self.set(SC_LCONTROL, self.control_held(), &mut actions);
            }
            "ControlLeft" => {
                if !down {
                    self.altgr_control = false;
                }
                self.set(SC_LCONTROL, down, &mut actions);
            }
            "ControlRight" => self.set(SC_RCONTROL, down, &mut actions),
            // Held back until a key shows whether it types a character.
            "AltLeft" if mac => {
                if !down {
                    self.set(SC_LMENU, false, &mut actions);
                }
            }
            "AltLeft" => self.set(SC_LMENU, down, &mut actions),
            "AltRight" => {
                if down && !mac && !was_held && self.held("ControlLeft") {
                    // Windows sends a fake left Ctrl with AltGr: let go of it on the server.
                    self.altgr_control = true;
                    self.set(SC_LCONTROL, false, &mut actions);
                }
                if !down {
                    self.set(SC_RMENU, false, &mut actions);
                }
            }
            "MetaLeft" | "MetaRight" if mac => {
                let command = self.command_held();
                if down && !was_held {
                    let other = if code == "MetaLeft" {
                        "MetaRight"
                    } else {
                        "MetaLeft"
                    };
                    if !self.held(other) {
                        self.command_used = false;
                    }
                } else if !down && !command {
                    if self.held_modifiers.contains(&SC_RCONTROL) {
                        self.set(SC_RCONTROL, false, &mut actions);
                    } else if !self.command_used {
                        actions.push(KeyAction::down(SC_LWIN));
                        actions.push(KeyAction::up(SC_LWIN));
                    }
                }
            }
            "MetaLeft" => self.set(SC_LWIN, down, &mut actions),
            "MetaRight" => self.set(SC_RWIN, down, &mut actions),
            // WebKit on macOS reports Caps Lock turning on as keydown and off as keyup: toggle each time.
            "CapsLock" if mac => {
                actions.push(KeyAction::down(SC_CAPSLOCK));
                actions.push(KeyAction::up(SC_CAPSLOCK));
            }
            _ => return None,
        }
        Some(actions)
    }

    fn type_text(&mut self, text: &str, code: &str) -> Vec<KeyAction> {
        self.swallowed.insert(code.to_owned());
        KeyAction::text(text)
    }

    fn key_down(&mut self, code: &str, key: &str) -> Vec<KeyAction> {
        let control = self.control_held();
        let command = self.command_held();
        let option = self.left_option_held() || self.right_option_held();
        let typed = typed(key);
        if command {
            self.command_used = true;
        } else if option && !control {
            match typed {
                Typed::Dead => {
                    self.dead_key_pending = true;
                    self.swallowed.insert(code.to_owned());
                    return Vec::new();
                }
                Typed::Text(text) => {
                    self.dead_key_pending = false;
                    return self.type_text(text, code);
                }
                Typed::Nothing => {}
            }
        } else if self.dead_key_pending
            && !control
            && let Typed::Text(text) = typed
        {
            self.dead_key_pending = false;
            return self.type_text(text, code);
        }
        self.dead_key_pending = false;

        let Some(scancode) = scancode(code) else {
            // No physical key RDP knows: type what it produces, if anything.
            return match typed {
                Typed::Text(text) if !command => self.type_text(text, code),
                _ => Vec::new(),
            };
        };
        let mut actions = Vec::new();
        if command {
            self.set(SC_RCONTROL, true, &mut actions);
        }
        if self.left_option_held() {
            self.set(SC_LMENU, true, &mut actions);
        }
        if self.right_option_held() {
            self.set(SC_RMENU, true, &mut actions);
        }
        actions.push(KeyAction::down(scancode));
        if command {
            // WebKit sends no keyup for keys pressed with ⌘ held: release right away (repeats press again).
            self.swallowed.insert(code.to_owned());
            actions.push(KeyAction::up(scancode));
        } else if !self.held_keys.contains(&scancode) {
            self.held_keys.push(scancode);
        }
        actions
    }

    fn key_up(&mut self, code: &str) -> Vec<KeyAction> {
        if self.swallowed.remove(code) {
            return Vec::new();
        }
        let Some(scancode) = scancode(code) else {
            return Vec::new();
        };
        self.held_keys.retain(|&c| c != scancode);
        vec![KeyAction::up(scancode)]
    }

    /// Lets go of everything held (focus lost): keys first, then modifiers, latest first.
    pub fn release_all(&mut self) -> Vec<KeyAction> {
        let actions = self
            .held_keys
            .iter()
            .rev()
            .chain(self.held_modifiers.iter().rev())
            .map(|&code| KeyAction::up(code))
            .collect();
        *self = Self::new(self.platform);
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_C: u16 = 0x2E;
    const KEY_E: u16 = 0x12;
    const KEY_V: u16 = 0x2F;
    const KEY_Q: u16 = 0x10;
    const F4: u16 = 0x3E;

    fn down(code: u16) -> KeyAction {
        KeyAction::down(code)
    }

    fn up(code: u16) -> KeyAction {
        KeyAction::up(code)
    }

    fn mac() -> KeyboardTranslator {
        KeyboardTranslator::new(KeyboardPlatform::MacOs)
    }

    fn other() -> KeyboardTranslator {
        KeyboardTranslator::new(KeyboardPlatform::Other)
    }

    fn press(t: &mut KeyboardTranslator, code: &str, key: &str) -> Vec<KeyAction> {
        t.key(&KeyInput::new(code, key, true))
    }

    fn release(t: &mut KeyboardTranslator, code: &str, key: &str) -> Vec<KeyAction> {
        t.key(&KeyInput::new(code, key, false))
    }

    #[test]
    fn table_covers_letters_digits_function_and_extended_keys() {
        assert_eq!(scancode("KeyA"), Some(0x1E));
        assert_eq!(scancode("KeyM"), Some(0x32));
        assert_eq!(scancode("KeyQ"), Some(KEY_Q));
        assert_eq!(scancode("Digit1"), Some(0x02));
        assert_eq!(scancode("Digit0"), Some(0x0B));
        assert_eq!(scancode("F1"), Some(0x3B));
        assert_eq!(scancode("F4"), Some(F4));
        assert_eq!(scancode("F10"), Some(0x44));
        assert_eq!(scancode("F11"), Some(0x57));
        assert_eq!(scancode("F13"), Some(0x64));
        assert_eq!(scancode("F24"), Some(0x76));
        assert_eq!(scancode("ArrowLeft"), Some(0x14B));
        assert_eq!(scancode("NumpadEnter"), Some(0x11C));
        assert_eq!(scancode("Delete"), Some(0x153));
        assert_eq!(scancode("IntlBackslash"), Some(0x56));
        assert_eq!(scancode("Keyboard"), None);
        assert_eq!(scancode("KeyAA"), None);
        assert_eq!(scancode("F25"), None);
        assert_eq!(scancode("Fn"), None);
    }

    // The 15 Swift translator tests, keyed by `code`.

    #[test]
    fn plain_keys_pass_through() {
        let mut t = mac();
        assert_eq!(press(&mut t, "KeyC", "c"), [down(KEY_C)]);
        assert_eq!(release(&mut t, "KeyC", "c"), [up(KEY_C)]);
    }

    #[test]
    fn left_option_types_the_mac_character() {
        let mut t = mac();
        assert!(press(&mut t, "AltLeft", "Alt").is_empty());
        assert_eq!(press(&mut t, "KeyV", "@"), KeyAction::text("@"));
        assert!(release(&mut t, "KeyV", "@").is_empty());
        assert!(release(&mut t, "AltLeft", "Alt").is_empty());
    }

    #[test]
    fn left_option_with_shift_types_and_sends_shift() {
        let mut t = mac();
        assert!(press(&mut t, "AltLeft", "Alt").is_empty());
        assert_eq!(press(&mut t, "ShiftLeft", "Shift"), [down(SC_LSHIFT)]);
        assert_eq!(press(&mut t, "KeyV", "‘"), KeyAction::text("‘"));
    }

    #[test]
    fn surrogate_pairs_are_sent_as_two_units() {
        let mut t = mac();
        press(&mut t, "AltLeft", "Alt");
        assert_eq!(press(&mut t, "KeyV", "😀").len(), 2);
    }

    #[test]
    fn left_option_with_non_character_key_is_alt() {
        let mut t = mac();
        press(&mut t, "AltLeft", "Alt");
        assert_eq!(press(&mut t, "Tab", "Tab"), [down(SC_LMENU), down(SC_TAB)]);
        assert_eq!(release(&mut t, "Tab", "Tab"), [up(SC_TAB)]);
        assert_eq!(release(&mut t, "AltLeft", "Alt"), [up(SC_LMENU)]);
    }

    #[test]
    fn function_key_characters_are_not_typed() {
        let mut t = mac();
        press(&mut t, "AltLeft", "Alt");
        assert_eq!(press(&mut t, "F4", "F4"), [down(SC_LMENU), down(F4)]);
        let mut t = mac();
        press(&mut t, "AltLeft", "Alt");
        assert_eq!(press(&mut t, "F4", "\u{F707}"), [down(SC_LMENU), down(F4)]);
    }

    #[test]
    fn right_option_types_the_mac_character_too() {
        let mut t = mac();
        assert!(press(&mut t, "AltRight", "Alt").is_empty());
        assert_eq!(press(&mut t, "KeyQ", "@"), KeyAction::text("@"));
        assert!(release(&mut t, "KeyQ", "@").is_empty());
        assert!(release(&mut t, "AltRight", "Alt").is_empty());
    }

    #[test]
    fn right_option_with_non_character_key_is_right_alt() {
        let mut t = mac();
        press(&mut t, "AltRight", "Alt");
        assert_eq!(press(&mut t, "Tab", "Tab"), [down(SC_RMENU), down(SC_TAB)]);
        release(&mut t, "Tab", "Tab");
        assert_eq!(release(&mut t, "AltRight", "Alt"), [up(SC_RMENU)]);
    }

    #[test]
    fn command_key_is_control_and_released_at_once() {
        let mut t = mac();
        assert!(press(&mut t, "MetaLeft", "Meta").is_empty());
        assert_eq!(
            press(&mut t, "KeyC", "c"),
            [down(SC_RCONTROL), down(KEY_C), up(KEY_C)]
        );
        // WebKit may still deliver the keyup; it must not release twice.
        assert!(release(&mut t, "KeyC", "c").is_empty());
        assert_eq!(press(&mut t, "KeyV", "v"), [down(KEY_V), up(KEY_V)]);
        assert_eq!(release(&mut t, "MetaLeft", "Meta"), [up(SC_RCONTROL)]);
    }

    #[test]
    fn command_tapped_alone_is_the_windows_key() {
        let mut t = mac();
        press(&mut t, "MetaLeft", "Meta");
        assert_eq!(release(&mut t, "MetaLeft", "Meta"), [down(SC_LWIN), up(SC_LWIN)]);
        // Both ⌘ keys: only the last release counts.
        press(&mut t, "MetaLeft", "Meta");
        press(&mut t, "MetaRight", "Meta");
        assert!(release(&mut t, "MetaRight", "Meta").is_empty());
        assert_eq!(release(&mut t, "MetaLeft", "Meta"), [down(SC_LWIN), up(SC_LWIN)]);
    }

    #[test]
    fn control_stays_control_and_option_with_control_is_alt() {
        let mut t = mac();
        assert_eq!(press(&mut t, "ControlLeft", "Control"), [down(SC_LCONTROL)]);
        press(&mut t, "AltLeft", "Alt");
        assert_eq!(press(&mut t, "KeyE", "e"), [down(SC_LMENU), down(KEY_E)]);
        // Right Control is Control too; it is released with the last Control key.
        assert!(press(&mut t, "ControlRight", "Control").is_empty());
        assert!(release(&mut t, "ControlLeft", "Control").is_empty());
        assert_eq!(release(&mut t, "ControlRight", "Control"), [up(SC_LCONTROL)]);
    }

    #[test]
    fn dead_key_then_letter_types_the_composed_character() {
        let mut t = mac();
        press(&mut t, "AltLeft", "Alt");
        assert!(press(&mut t, "KeyE", "Dead").is_empty());
        assert!(release(&mut t, "KeyE", "Dead").is_empty());
        release(&mut t, "AltLeft", "Alt");
        assert_eq!(press(&mut t, "KeyE", "é"), KeyAction::text("é"));
        assert!(release(&mut t, "KeyE", "é").is_empty());
        assert_eq!(press(&mut t, "KeyE", "e"), [down(KEY_E)]);
    }

    #[test]
    fn caps_lock_toggles_on_each_change() {
        let mut t = mac();
        let toggle = [down(SC_CAPSLOCK), up(SC_CAPSLOCK)];
        assert_eq!(press(&mut t, "CapsLock", "CapsLock"), toggle);
        assert_eq!(release(&mut t, "CapsLock", "CapsLock"), toggle);
    }

    #[test]
    fn shift_sides_are_kept_apart() {
        let mut t = mac();
        assert_eq!(press(&mut t, "ShiftLeft", "Shift"), [down(SC_LSHIFT)]);
        assert_eq!(press(&mut t, "ShiftRight", "Shift"), [down(SC_RSHIFT)]);
        // Auto-repeat of a held modifier sends nothing more.
        assert!(press(&mut t, "ShiftRight", "Shift").is_empty());
        assert_eq!(release(&mut t, "ShiftLeft", "Shift"), [up(SC_LSHIFT)]);
        press(&mut t, "AltLeft", "Alt");
        assert_eq!(press(&mut t, "KeyV", "@"), KeyAction::text("@"));
    }

    #[test]
    fn release_all_lets_go_of_everything_held() {
        let mut t = mac();
        press(&mut t, "ShiftLeft", "Shift");
        press(&mut t, "AltRight", "Alt");
        press(&mut t, "Tab", "Tab");
        assert_eq!(t.release_all(), [up(SC_TAB), up(SC_RMENU), up(SC_LSHIFT)]);
        assert!(t.release_all().is_empty());
        // Nothing is still considered held afterwards.
        assert_eq!(press(&mut t, "KeyC", "c"), [down(KEY_C)]);
    }

    // Windows and Linux.

    #[test]
    fn other_platforms_send_ctrl_alt_and_win_as_they_are() {
        let mut t = other();
        assert_eq!(press(&mut t, "ControlRight", "Control"), [down(SC_RCONTROL)]);
        assert_eq!(press(&mut t, "AltLeft", "Alt"), [down(SC_LMENU)]);
        assert_eq!(press(&mut t, "KeyE", "e"), [down(KEY_E)]);
        assert_eq!(press(&mut t, "MetaLeft", "Meta"), [down(SC_LWIN)]);
        assert_eq!(release(&mut t, "MetaLeft", "Meta"), [up(SC_LWIN)]);
        assert_eq!(press(&mut t, "MetaRight", "Meta"), [down(SC_RWIN)]);
        assert_eq!(press(&mut t, "CapsLock", "CapsLock"), [down(SC_CAPSLOCK)]);
        assert_eq!(release(&mut t, "CapsLock", "CapsLock"), [up(SC_CAPSLOCK)]);
        assert_eq!(release(&mut t, "AltLeft", "Alt"), [up(SC_LMENU)]);
        // Left Alt with a character key is Alt, not Unicode.
        press(&mut t, "AltLeft", "Alt");
        assert_eq!(press(&mut t, "KeyF", "f"), [down(0x21)]);
    }

    #[test]
    fn altgr_types_layout_characters_and_drops_the_fake_control() {
        let mut t = other();
        // Windows: ControlLeft, then AltRight.
        assert_eq!(press(&mut t, "ControlLeft", "Control"), [down(SC_LCONTROL)]);
        assert_eq!(press(&mut t, "AltRight", "AltGraph"), [up(SC_LCONTROL)]);
        assert_eq!(press(&mut t, "KeyV", "@"), KeyAction::text("@"));
        assert!(release(&mut t, "KeyV", "@").is_empty());
        assert!(release(&mut t, "ControlLeft", "Control").is_empty());
        assert!(release(&mut t, "AltRight", "AltGraph").is_empty());
        // Linux: AltRight alone; with a key that types nothing it is right Alt.
        press(&mut t, "AltRight", "AltGraph");
        assert_eq!(press(&mut t, "F4", "F4"), [down(SC_RMENU), down(F4)]);
        release(&mut t, "F4", "F4");
        assert_eq!(release(&mut t, "AltRight", "AltGraph"), [up(SC_RMENU)]);
    }

    #[test]
    fn unknown_physical_keys_type_their_character() {
        let mut t = other();
        assert_eq!(press(&mut t, "", "ß"), KeyAction::text("ß"));
        assert!(release(&mut t, "", "ß").is_empty());
        assert!(press(&mut t, "Fn", "Fn").is_empty());
        assert!(release(&mut t, "Fn", "Fn").is_empty());
    }
}
