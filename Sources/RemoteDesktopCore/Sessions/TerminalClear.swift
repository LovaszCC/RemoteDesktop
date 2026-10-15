import Foundation

/// "Clear terminal" (⌘K, like Ghostty): wipe the screen and scrollback locally, then let the remote shell
/// repaint its prompt and the half-typed line at the top.
public enum TerminalClear {
    public enum Action: Equatable, Sendable {
        /// A full-screen program (vim, htop, mc) owns the alternate screen; clearing would only garble it.
        case none
        /// The process has exited: nothing would answer Ctrl+L.
        case localOnly
        case localAndRedraw
    }

    /// Cursor home, erase screen (ED 2), erase scrollback (ED 3).
    public static let localSequence = "\u{1b}[H\u{1b}[2J\u{1b}[3J"
    /// Ctrl+L: bash, zsh, fish and RouterOS repaint the prompt with the current input line.
    public static let redrawPrompt: [UInt8] = [0x0C]

    public static func action(isAlternateScreen: Bool, isRunning: Bool) -> Action {
        if isAlternateScreen { return .none }
        return isRunning ? .localAndRedraw : .localOnly
    }
}
