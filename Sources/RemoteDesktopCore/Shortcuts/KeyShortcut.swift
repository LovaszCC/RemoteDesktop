import Foundation

/// A key plus modifiers, e.g. ⇧⌘N. Stored in the defaults as JSON.
public struct KeyShortcut: Codable, Hashable, Sendable {
    /// A single lowercased character (`"n"`, `","`) or a special key name (see `specialKeys`).
    public var key: String
    public var modifiers: Modifiers

    public struct Modifiers: OptionSet, Codable, Hashable, Sendable {
        public let rawValue: Int
        public init(rawValue: Int) { self.rawValue = rawValue }

        public static let command = Modifiers(rawValue: 1 << 0)
        public static let shift = Modifiers(rawValue: 1 << 1)
        public static let option = Modifiers(rawValue: 1 << 2)
        public static let control = Modifiers(rawValue: 1 << 3)
    }

    /// Special keys and their glyphs.
    public static let specialKeys: [String: String] = [
        "return": "↩", "tab": "⇥", "delete": "⌫", "escape": "⎋", "space": "Space",
        "left": "←", "right": "→", "up": "↑", "down": "↓",
        "f1": "F1", "f2": "F2", "f3": "F3", "f4": "F4", "f5": "F5", "f6": "F6",
        "f7": "F7", "f8": "F8", "f9": "F9", "f10": "F10", "f11": "F11", "f12": "F12",
    ]

    public init(_ key: String, _ modifiers: Modifiers = .command) {
        self.key = Self.specialKeys[key] == nil ? key.lowercased() : key
        self.modifiers = modifiers
    }

    public var isSpecialKey: Bool { Self.specialKeys[key] != nil }

    /// ⌘, ⌃ or ⌥ — without one, the shortcut would take keys away from typing.
    public var hasRequiredModifier: Bool { !modifiers.isDisjoint(with: [.command, .control, .option]) }

    /// macOS menu notation: ⌃⌥⇧⌘ then the key.
    public var displayString: String {
        var text = ""
        if modifiers.contains(.control) { text += "⌃" }
        if modifiers.contains(.option) { text += "⌥" }
        if modifiers.contains(.shift) { text += "⇧" }
        if modifiers.contains(.command) { text += "⌘" }
        return text + (Self.specialKeys[key] ?? key.uppercased())
    }
}
