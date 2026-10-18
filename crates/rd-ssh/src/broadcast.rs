//! Input mirrored from one split pane to another (Swift `TerminalSession.write`): bracketed-paste
//! markers (`ESC [200~` / `ESC [201~`) only reach a target whose remote side asked for bracketed paste,
//! so shells without the mode (e.g. RouterOS) don't see stray escape sequences.

use std::borrow::Cow;

/// Starts a bracketed paste.
pub const PASTE_START: &[u8] = b"\x1b[200~";
/// Ends a bracketed paste.
pub const PASTE_END: &[u8] = b"\x1b[201~";

/// `data` as a target with `bracketed_paste` on or off should receive it: unchanged when the mode is on,
/// else without the markers.
#[must_use]
pub fn filter_for_target(data: &[u8], bracketed_paste: bool) -> Cow<'_, [u8]> {
    if bracketed_paste || !contains_marker(data) {
        return Cow::Borrowed(data);
    }
    let mut out = Vec::with_capacity(data.len());
    let mut rest = data;
    while !rest.is_empty() {
        if rest.starts_with(PASTE_START) || rest.starts_with(PASTE_END) {
            rest = &rest[PASTE_START.len()..];
        } else {
            out.push(rest[0]);
            rest = &rest[1..];
        }
    }
    Cow::Owned(out)
}

fn contains_marker(data: &[u8]) -> bool {
    data.windows(PASTE_START.len())
        .any(|w| w == PASTE_START || w == PASTE_END)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_markers_for_bracketed_targets() {
        let data = b"\x1b[200~ls\rpwd\x1b[201~";
        assert!(matches!(filter_for_target(data, true), Cow::Borrowed(d) if d == data));
    }

    #[test]
    fn strips_markers_for_other_targets() {
        assert_eq!(
            &*filter_for_target(b"\x1b[200~ls\rpwd\x1b[201~", false),
            b"ls\rpwd"
        );
        // Markers arriving on their own (SwiftTerm sends them separately) vanish entirely.
        assert!(filter_for_target(PASTE_START, false).is_empty());
        assert!(filter_for_target(PASTE_END, false).is_empty());
    }

    #[test]
    fn leaves_other_escape_sequences_alone() {
        let arrows = b"\x1b[A\x1b[200\x1b[2~x";
        assert!(matches!(filter_for_target(arrows, false), Cow::Borrowed(_)));
        assert_eq!(&*filter_for_target(b"\x1b[A\x1b[200~x", false), b"\x1b[Ax");
        assert_eq!(&*filter_for_target(b"", false), b"");
    }
}
