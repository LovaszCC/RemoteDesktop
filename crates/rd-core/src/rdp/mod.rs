//! Rules of the embedded RDP session (Swift `RDPSessionRules` and the parts of `RDPBridge` that decide
//! things): the `DOMAIN\user` / `user@domain` split, how a disconnect is classified, the desktop size
//! rules for Display Control resizes, and "type clipboard" sequencing. The keyboard mapping lives in
//! [`keyboard`].

pub mod keyboard;

use std::time::{Duration, Instant};

/// "DOMAIN\user" and "user@domain" typed into the user field win over the domain field (Swift
/// `RDPSessionSettings.splitUser`): `DOMAIN\user` splits into user and domain, `user@domain` stays whole
/// with an empty domain, and anything else keeps the domain field. Both are trimmed of spaces.
#[must_use]
pub fn split_user(user: &str, domain: &str) -> (String, String) {
    let user = user.trim_matches(' ');
    let domain = domain.trim_matches(' ');
    if let Some((left, right)) = user.split_once('\\') {
        return (right.to_owned(), left.to_owned());
    }
    let domain = if user.contains('@') { "" } else { domain };
    (user.to_owned(), domain.to_owned())
}

/// The error info codes ([MS-RDPBCGR] 2.2.5.1.1) of user-initiated endings.
pub const CLEAN_ERROR_INFO: [u32; 4] = [
    0x0000_0001, // RPC initiated disconnect
    0x0000_0002, // RPC initiated logoff
    0x0000_000B, // disconnect by user
    0x0000_000C, // logoff by user
];

/// The "last error" of a session whose credential prompt was cancelled (`FreeRDP`'s
/// `FREERDP_ERROR_CONNECT_CANCELLED`, kept so the codes match the Swift app).
pub const CANCELLED_LAST_ERROR: u32 = 0x0002_000B;

/// How an RDP session ended (Swift `RDPDisconnectReason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectReason {
    /// The user logged off or disconnected inside the session, or cancelled the credential prompt: the
    /// tab can close.
    Clean,
    /// The connection failed or dropped: the tab stays with a reconnect option.
    Failure,
}

impl DisconnectReason {
    /// Only an explicit logoff / disconnect reason from the server, or a cancelled credential prompt,
    /// counts as clean; a session that ends without any code (a dropped transport, a client-side failure)
    /// keeps its tab.
    #[must_use]
    pub fn classify(last_error: u32, error_info: u32) -> Self {
        if CLEAN_ERROR_INFO.contains(&error_info) || last_error == CANCELLED_LAST_ERROR {
            Self::Clean
        } else {
            Self::Failure
        }
    }

    /// Whether the tab may close by itself.
    #[must_use]
    pub fn is_clean(self) -> bool {
        self == Self::Clean
    }
}

/// The smallest and largest desktop edge Display Control accepts.
pub const MIN_EDGE: u32 = 200;
/// See [`MIN_EDGE`].
pub const MAX_EDGE: u32 = 8192;
/// The smallest initial desktop (before the first resize adapts it to the tab).
pub const INITIAL_MIN: (u32, u32) = (800, 600);
/// How long the view must keep its size before a resize goes to the server.
pub const RESIZE_DEBOUNCE: Duration = Duration::from_millis(300);

/// The desktop size to connect with for a view of `width`×`height`: at least 800×600, at most
/// 8192×8192, the width even (Swift `RDPSession.initialSize`).
#[must_use]
pub fn initial_size(width: u32, height: u32) -> (u32, u32) {
    let w = width.clamp(INITIAL_MIN.0, MAX_EDGE) & !1;
    let h = height.clamp(INITIAL_MIN.1, MAX_EDGE);
    (w, h)
}

/// The monitor layout sent for a view of `width`×`height`: both edges within 200…8192, the width even
/// (Swift `sendMonitorLayout`).
#[must_use]
pub fn layout_size(width: u32, height: u32) -> (u32, u32) {
    (
        width.clamp(MIN_EDGE, MAX_EDGE) & !1,
        height.clamp(MIN_EDGE, MAX_EDGE),
    )
}

/// Smart sizing: while the desktop differs from the view (the server hasn't caught up with a resize
/// yet), the picture is scaled to fill the view. Returns the horizontal and vertical scale, or `None`
/// when the sizes match and the picture is drawn 1:1.
#[must_use]
pub fn smart_sizing(view: (u32, u32), desktop: (u32, u32)) -> Option<(f64, f64)> {
    if view == desktop || desktop.0 == 0 || desktop.1 == 0 || view.0 == 0 || view.1 == 0 {
        return None;
    }
    Some((
        f64::from(view.0) / f64::from(desktop.0),
        f64::from(view.1) / f64::from(desktop.1),
    ))
}

/// Maps a point in the view to the desktop while smart sizing is on (identity when the sizes match),
/// clamped to the desktop.
#[must_use]
pub fn view_to_desktop(point: (f64, f64), view: (u32, u32), desktop: (u32, u32)) -> (u16, u16) {
    let (sx, sy) = smart_sizing(view, desktop).unwrap_or((1.0, 1.0));
    let clamp = |v: f64, max: u32| -> u16 {
        let max = f64::from(max.saturating_sub(1).min(u32::from(u16::MAX)));
        // In range after the clamp, so the cast neither truncates nor wraps.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let value = v.clamp(0.0, max).floor() as u16;
        value
    };
    (clamp(point.0 / sx, desktop.0), clamp(point.1 / sy, desktop.1))
}

/// The resize debounce (Swift `viewDidResize` + `performSelector:afterDelay:0.3`): every view size change
/// restarts a 300 ms timer; when it fires, the clamped layout goes out unless the desktop already has
/// that size.
#[derive(Debug, Clone, Default)]
pub struct ResizeDebouncer {
    pending: Option<((u32, u32), Instant)>,
}

impl ResizeDebouncer {
    /// A debouncer with nothing pending.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The view now measures `width`×`height` (at `now`).
    pub fn request(&mut self, width: u32, height: u32, now: Instant) {
        self.pending = Some(((width, height), now + RESIZE_DEBOUNCE));
    }

    /// When the pending resize is due, if any.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.pending.map(|(_, at)| at)
    }

    /// Whether a resize waits.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The layout to send at `now` for a desktop of `current` size: `None` before the deadline, or when
    /// the desktop already has the clamped size (the request is then dropped).
    pub fn due(&mut self, now: Instant, current: (u32, u32)) -> Option<(u32, u32)> {
        let ((width, height), at) = self.pending?;
        if now < at {
            return None;
        }
        self.pending = None;
        let size = layout_size(width, height);
        (size != current).then_some(size)
    }

    /// Puts a layout that couldn't be sent yet (Display Control not open) back, due at once.
    pub fn retry(&mut self, size: (u32, u32), now: Instant) {
        if self.pending.is_none() {
            self.pending = Some((size, now));
        }
    }
}

/// One keystroke of "type clipboard".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedKey {
    /// A UTF-16 code unit sent as a Unicode key event (press and release).
    Unit(u16),
    /// The real Enter key (scancode 0x1C).
    Enter,
    /// The real Tab key (scancode 0x0F).
    Tab,
}

/// Above this many characters the UI asks before typing (~20 ms per keystroke).
pub const TYPE_CONFIRMATION_THRESHOLD: usize = 2000;
/// The pause after each typed keystroke. The Swift app's 2 ms is far too fast for Windows 11 Notepad: it
/// drops, repeats and reorders characters from about the 20th on. Measured live with
/// `windows_notepad_typing` (crates/rd-rdp/tests/live.rs): 2–5 ms always garbled, 10–15 ms
/// sometimes, 20 ms exact in 7 runs out of 7. The same with `FreeRDP` as with `IronRDP`: the pace is the
/// remote side's limit, not the client's.
pub const TYPE_INTERVAL: Duration = Duration::from_millis(20);

/// Turns text into keystrokes (Swift `RDPTyping.keystrokes`): UTF-16 units as Unicode events, CR, LF
/// and CRLF as one Enter, tabs as the Tab key.
#[must_use]
pub fn typing_keystrokes(text: &str) -> Vec<TypedKey> {
    let mut keys = Vec::with_capacity(text.len());
    let mut previous_was_cr = false;
    for unit in text.encode_utf16() {
        match unit {
            0x0D => {
                keys.push(TypedKey::Enter);
                previous_was_cr = true;
                continue;
            }
            0x0A => {
                if !previous_was_cr {
                    keys.push(TypedKey::Enter);
                }
            }
            0x09 => keys.push(TypedKey::Tab),
            unit => keys.push(TypedKey::Unit(unit)),
        }
        previous_was_cr = false;
    }
    keys
}

/// Whether typing `text` needs a confirmation first (more than 2000 characters).
#[must_use]
pub fn typing_needs_confirmation(text: &str) -> bool {
    text.chars().count() > TYPE_CONFIRMATION_THRESHOLD
}

/// The "typing takes about N s" estimate for `characters` characters, at [`TYPE_INTERVAL`].
#[must_use]
pub fn typing_seconds(characters: usize) -> usize {
    characters * 20 / 1000 + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_user_handles_qualified_names() {
        let pair = |a: &str, b: &str| (a.to_owned(), b.to_owned());
        assert_eq!(split_user(" bob ", " CORP "), pair("bob", "CORP"));
        assert_eq!(split_user("AD\\alice", "OTHER"), pair("alice", "AD"));
        assert_eq!(split_user("alice@ad.local", "OTHER"), pair("alice@ad.local", ""));
        assert_eq!(split_user("", ""), pair("", ""));
    }

    #[test]
    fn disconnect_classification() {
        use DisconnectReason::{Clean, Failure};
        // A client-side failure ends with no codes at all: not a logoff.
        assert_eq!(DisconnectReason::classify(0, 0), Failure);
        assert_eq!(DisconnectReason::classify(0, 0x0C), Clean);
        assert_eq!(DisconnectReason::classify(0, 0x0B), Clean);
        assert_eq!(DisconnectReason::classify(0, 0x01), Clean);
        assert_eq!(DisconnectReason::classify(0, 0x02), Clean);
        assert_eq!(DisconnectReason::classify(0, 0x03), Failure); // idle timeout
        assert_eq!(DisconnectReason::classify(0, 0x05), Failure); // taken over
        assert_eq!(DisconnectReason::classify(0x0002_0009, 0), Failure);
        assert_eq!(DisconnectReason::classify(0x0002_000B, 0), Clean); // prompt cancelled
        assert!(Clean.is_clean() && !Failure.is_clean());
    }

    #[test]
    fn initial_size_is_at_least_800_by_600_with_an_even_width() {
        assert_eq!(initial_size(1281, 801), (1280, 801));
        assert_eq!(initial_size(300, 200), (800, 600));
        assert_eq!(initial_size(10_000, 9000), (8192, 8192));
        assert_eq!(initial_size(1001, 700), (1000, 700));
    }

    #[test]
    fn layout_size_clamps_and_evens() {
        assert_eq!(layout_size(1601, 901), (1600, 901));
        assert_eq!(layout_size(10, 10), (200, 200));
        assert_eq!(layout_size(9001, 9001), (8192, 8192));
        assert_eq!(layout_size(201, 200), (200, 200));
    }

    #[test]
    fn smart_sizing_until_the_server_catches_up() {
        assert_eq!(smart_sizing((1280, 800), (1280, 800)), None);
        assert_eq!(smart_sizing((1600, 900), (800, 450)), Some((2.0, 2.0)));
        assert_eq!(smart_sizing((0, 0), (800, 600)), None);
        assert_eq!(view_to_desktop((100.0, 50.0), (1600, 900), (800, 450)), (50, 25));
        assert_eq!(view_to_desktop((100.7, 50.2), (800, 600), (800, 600)), (100, 50));
        assert_eq!(view_to_desktop((-5.0, 9999.0), (800, 600), (800, 600)), (0, 599));
    }

    #[test]
    fn resize_waits_300_ms_after_the_last_change() {
        let t0 = Instant::now();
        let mut debouncer = ResizeDebouncer::new();
        assert!(!debouncer.is_pending());
        debouncer.request(1000, 700, t0);
        debouncer.request(1201, 801, t0 + Duration::from_millis(200));
        assert_eq!(debouncer.deadline(), Some(t0 + Duration::from_millis(500)));
        assert_eq!(debouncer.due(t0 + Duration::from_millis(400), (800, 600)), None);
        assert_eq!(
            debouncer.due(t0 + Duration::from_millis(500), (800, 600)),
            Some((1200, 801))
        );
        assert!(!debouncer.is_pending());
        assert_eq!(debouncer.due(t0 + Duration::from_secs(1), (800, 600)), None);
    }

    #[test]
    fn resize_to_the_current_size_is_dropped_and_retry_is_due_at_once() {
        let t0 = Instant::now();
        let mut debouncer = ResizeDebouncer::new();
        debouncer.request(1281, 800, t0);
        assert_eq!(debouncer.due(t0 + RESIZE_DEBOUNCE, (1280, 800)), None);
        assert!(!debouncer.is_pending());
        debouncer.retry((1600, 900), t0);
        assert_eq!(debouncer.due(t0, (1280, 800)), Some((1600, 900)));
        // A newer request wins over a retry.
        debouncer.request(1000, 700, t0);
        debouncer.retry((1600, 900), t0);
        assert_eq!(
            debouncer.due(t0 + RESIZE_DEBOUNCE, (1280, 800)),
            Some((1000, 700))
        );
    }

    #[test]
    fn typing_keystrokes() {
        use TypedKey::{Enter, Tab, Unit};
        assert_eq!(super::typing_keystrokes("aB"), [Unit(0x61), Unit(0x42)]);
        assert_eq!(
            super::typing_keystrokes("a\r\nb\nc\rd"),
            [
                Unit(0x61),
                Enter,
                Unit(0x62),
                Enter,
                Unit(0x63),
                Enter,
                Unit(0x64)
            ]
        );
        assert_eq!(super::typing_keystrokes("\r\n\r\n"), [Enter, Enter]);
        assert_eq!(super::typing_keystrokes("x\ty"), [Unit(0x78), Tab, Unit(0x79)]);
        assert_eq!(super::typing_keystrokes("őű"), [Unit(0x0151), Unit(0x0171)]);
        assert_eq!(super::typing_keystrokes("😀"), [Unit(0xD83D), Unit(0xDE00)]);
        assert!(super::typing_keystrokes("").is_empty());
    }

    #[test]
    fn long_texts_need_confirmation() {
        assert!(!typing_needs_confirmation(&"a".repeat(2000)));
        assert!(typing_needs_confirmation(&"a".repeat(2001)));
        // Characters, not bytes.
        assert!(!typing_needs_confirmation(&"ő".repeat(2000)));
        assert_eq!(typing_seconds(2500), 51);
        assert_eq!(typing_seconds(10), 1);
    }
}
