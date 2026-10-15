import Foundation

/// Option types the Mac layout's characters in the embedded terminal (like Ghostty), so it no longer works as
/// Meta. Option+← / → keep jumping words, sent as the readline/zsh Meta sequences.
public enum TerminalOptionKeys {
    public static let leftArrowKeyCode: UInt16 = 123
    public static let rightArrowKeyCode: UInt16 = 124

    /// The text to send for the key, or nil to let the terminal handle it. `otherModifiers`: ⌘, ⌃ or ⇧ held.
    public static func sequence(keyCode: UInt16, option: Bool, otherModifiers: Bool) -> String? {
        guard option, !otherModifiers else { return nil }
        switch keyCode {
        case leftArrowKeyCode: return "\u{1b}b"
        case rightArrowKeyCode: return "\u{1b}f"
        default: return nil
        }
    }
}
