import Foundation

/// Terminal app that opens SSH connections set to `SSHTerminal.external`. A global setting, stored in the
/// app's defaults.
public enum ExternalTerminal: String, CaseIterable, Sendable {
    case ghostty
    case iterm2
    case terminal

    public static let defaultsKey = "externalTerminal"

    public var bundleID: String {
        switch self {
        case .ghostty: "com.mitchellh.ghostty"
        case .iterm2: "com.googlecode.iterm2"
        case .terminal: "com.apple.Terminal"
        }
    }

    /// App name, shown untranslated.
    public var displayName: String {
        switch self {
        case .ghostty: "Ghostty"
        case .iterm2: "iTerm2"
        case .terminal: "Terminal"
        }
    }

    /// The stored choice; when unset, Ghostty if installed (the earlier fixed choice), else Terminal.app.
    public static func current(in defaults: UserDefaults = .standard, isInstalled: (String) -> Bool) -> ExternalTerminal {
        if let stored = defaults.string(forKey: defaultsKey).flatMap(ExternalTerminal.init(rawValue:)) {
            return stored
        }
        return isInstalled(ghostty.bundleID) ? .ghostty : .terminal
    }

    public func apply(to defaults: UserDefaults = .standard) {
        defaults.set(rawValue, forKey: Self.defaultsKey)
    }
}
