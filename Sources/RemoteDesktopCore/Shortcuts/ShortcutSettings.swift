import Foundation

public enum ShortcutError: Error, Equatable, Sendable {
    /// No ⌘, ⌃ or ⌥.
    case missingModifier
    /// Taken by macOS or the standard menus (⌘Q, ⌘C, …).
    case reserved
    case inUse(by: ShortcutCommand)

    public var message: String {
        switch self {
        case .missingModifier: String(localized: "Use at least one of ⌘, ⌃ or ⌥.", bundle: .core)
        case .reserved: String(localized: "This shortcut is reserved by macOS.", bundle: .core)
        case .inUse(let command): String(localized: "Already used by: \(command.title)", bundle: .core)
        }
    }
}

/// Shortcuts the user changed; everything else keeps `ShortcutCommand.defaultShortcut`.
public struct ShortcutSettings: Equatable, Sendable {
    public static let defaultsKey = "keyboardShortcuts"

    /// Standard app and system shortcuts that commands may not take.
    public static let reserved: Set<KeyShortcut> = [
        KeyShortcut("q"), KeyShortcut(","), KeyShortcut("c"), KeyShortcut("v"), KeyShortcut("x"),
        KeyShortcut("a"), KeyShortcut("z"), KeyShortcut("z", [.command, .shift]), KeyShortcut("h"),
        KeyShortcut("h", [.command, .option]), KeyShortcut("m"), KeyShortcut("`"), KeyShortcut("f"),
        KeyShortcut("tab"), KeyShortcut("space"), KeyShortcut("f", [.command, .control]),
        KeyShortcut("q", [.command, .control]),
    ]

    /// `nil` value: the user removed the command's shortcut.
    public private(set) var overrides: [ShortcutCommand: KeyShortcut?] = [:]

    public init() {}

    public func shortcut(for command: ShortcutCommand) -> KeyShortcut? {
        if let override = overrides[command] { return override }
        return command.defaultShortcut
    }

    public func isCustomized(_ command: ShortcutCommand) -> Bool { overrides[command] != nil }

    /// Why `shortcut` can't be given to `command`, or nil when it can.
    public func validate(_ shortcut: KeyShortcut, for command: ShortcutCommand) -> ShortcutError? {
        guard shortcut.hasRequiredModifier else { return .missingModifier }
        guard !Self.reserved.contains(shortcut) else { return .reserved }
        if let owner = ShortcutCommand.allCases.first(where: { $0 != command && self.shortcut(for: $0) == shortcut }) {
            return .inUse(by: owner)
        }
        return nil
    }

    /// Sets (or with `nil` removes) a command's shortcut.
    public mutating func set(_ shortcut: KeyShortcut?, for command: ShortcutCommand) throws(ShortcutError) {
        if let shortcut, let error = validate(shortcut, for: command) { throw error }
        if shortcut == command.defaultShortcut {
            overrides.removeValue(forKey: command)
        } else {
            overrides[command] = .some(shortcut)
        }
    }

    /// Back to the default — unless another command took that key meanwhile; then the error says which.
    public mutating func reset(_ command: ShortcutCommand) throws(ShortcutError) {
        try set(command.defaultShortcut, for: command)
    }

    public mutating func resetAll() {
        overrides = [:]
    }

    // MARK: Storage

    public static func load(from defaults: UserDefaults = .standard) -> ShortcutSettings {
        var settings = ShortcutSettings()
        guard let data = defaults.data(forKey: defaultsKey),
              let stored = try? JSONDecoder().decode([String: KeyShortcut?].self, from: data) else { return settings }
        for (name, shortcut) in stored {
            if let command = ShortcutCommand(rawValue: name) { settings.overrides[command] = .some(shortcut) }
        }
        return settings
    }

    public func save(to defaults: UserDefaults = .standard) {
        let stored = Dictionary(uniqueKeysWithValues: overrides.map { ($0.key.rawValue, $0.value) })
        if let data = try? JSONEncoder().encode(stored) { defaults.set(data, forKey: Self.defaultsKey) }
    }
}
