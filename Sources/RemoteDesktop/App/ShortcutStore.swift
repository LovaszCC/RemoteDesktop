import AppKit
import RemoteDesktopCore
import SwiftUI

/// The user's keyboard shortcuts, shared by the menus, the session key monitor and Settings.
@MainActor
@Observable
final class ShortcutStore {
    private(set) var settings: ShortcutSettings
    @ObservationIgnored private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        settings = .load(from: defaults)
    }

    func shortcut(for command: ShortcutCommand) -> KeyShortcut? { settings.shortcut(for: command) }

    func set(_ shortcut: KeyShortcut?, for command: ShortcutCommand) throws(ShortcutError) {
        try settings.set(shortcut, for: command)
        settings.save(to: defaults)
    }

    func reset(_ command: ShortcutCommand) throws(ShortcutError) {
        try settings.reset(command)
        settings.save(to: defaults)
    }

    func resetAll() {
        settings.resetAll()
        settings.save(to: defaults)
    }

    /// Menu notation for help texts ("⌘D"), "—" when the command has no shortcut.
    func display(_ command: ShortcutCommand) -> String { shortcut(for: command)?.displayString ?? "—" }

    /// For `.keyboardShortcut(_:)`; nil when the command has no shortcut.
    func keyboardShortcut(for command: ShortcutCommand) -> KeyboardShortcut? {
        guard let shortcut = shortcut(for: command) else { return nil }
        return KeyboardShortcut(Self.keyEquivalent(shortcut.key), modifiers: Self.eventModifiers(shortcut.modifiers))
    }

    // MARK: Conversion

    private static let specialKeyEquivalents: [String: KeyEquivalent] = [
        "return": .return, "tab": .tab, "delete": .delete, "escape": .escape, "space": .space,
        "left": .leftArrow, "right": .rightArrow, "up": .upArrow, "down": .downArrow,
    ]

    private static func keyEquivalent(_ key: String) -> KeyEquivalent {
        if let special = specialKeyEquivalents[key] { return special }
        if key.hasPrefix("f"), let number = Int(key.dropFirst()), let scalar = UnicodeScalar(NSF1FunctionKey + number - 1) {
            return KeyEquivalent(Character(scalar))
        }
        return KeyEquivalent(key.first ?? " ")
    }

    private static func eventModifiers(_ modifiers: KeyShortcut.Modifiers) -> EventModifiers {
        var result: EventModifiers = []
        if modifiers.contains(.command) { result.insert(.command) }
        if modifiers.contains(.shift) { result.insert(.shift) }
        if modifiers.contains(.option) { result.insert(.option) }
        if modifiers.contains(.control) { result.insert(.control) }
        return result
    }

    /// Key codes of the special keys (Enter on the keypad counts as Return).
    private static let specialKeyCodes: [UInt16: String] = [
        36: "return", 76: "return", 48: "tab", 51: "delete", 53: "escape", 49: "space",
        123: "left", 124: "right", 125: "down", 126: "up",
        122: "f1", 120: "f2", 99: "f3", 118: "f4", 96: "f5", 97: "f6",
        98: "f7", 100: "f8", 101: "f9", 109: "f10", 103: "f11", 111: "f12",
    ]

    /// The shortcut a key-down event represents (unshifted key, so ⇧⌘N is `n` + ⇧⌘).
    static func keyShortcut(from event: NSEvent) -> KeyShortcut? {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        var modifiers: KeyShortcut.Modifiers = []
        if flags.contains(.command) { modifiers.insert(.command) }
        if flags.contains(.shift) { modifiers.insert(.shift) }
        if flags.contains(.option) { modifiers.insert(.option) }
        if flags.contains(.control) { modifiers.insert(.control) }
        if let special = specialKeyCodes[event.keyCode] { return KeyShortcut(special, modifiers) }
        guard let character = event.characters(byApplyingModifiers: [])?.lowercased().first,
              !character.isWhitespace else { return nil }
        return KeyShortcut(String(character), modifiers)
    }
}
