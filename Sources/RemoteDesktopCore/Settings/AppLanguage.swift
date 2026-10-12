import Foundation

/// UI language of the app. Applied through the app's own `AppleLanguages` default (the same per-app
/// setting as *System Settings → Language & Region → Applications*), so it takes effect on the next launch.
public enum AppLanguage: String, CaseIterable, Sendable {
    case english = "en"
    case hungarian = "hu"

    static let defaultsKey = "AppleLanguages"

    /// Native name, shown untranslated in the picker.
    public var displayName: String {
        switch self {
        case .english: "English"
        case .hungarian: "Magyar"
        }
    }

    /// The language stored in `defaults` (the app's own domain); English when unset or unsupported.
    public static func current(in defaults: UserDefaults = .standard, domain: String? = Bundle.main.bundleIdentifier) -> AppLanguage {
        let stored = domain.flatMap { defaults.persistentDomain(forName: $0)?[defaultsKey] as? [String] }
        return stored?.first.flatMap(language(for:)) ?? .english
    }

    /// Writes this language for the next launch.
    public func apply(to defaults: UserDefaults = .standard) {
        defaults.set([rawValue], forKey: Self.defaultsKey)
    }

    /// First launch: pin English (the default) unless a language was already chosen, so a Hungarian
    /// system doesn't switch the app to Hungarian on its own.
    public static func applyDefaultIfUnset(in defaults: UserDefaults = .standard, domain: String? = Bundle.main.bundleIdentifier) {
        guard let domain, defaults.persistentDomain(forName: domain)?[defaultsKey] == nil else { return }
        AppLanguage.english.apply(to: defaults)
    }

    /// Maps `en`, `en-US`, `hu-HU`, … to a supported language.
    static func language(for identifier: String) -> AppLanguage? {
        allCases.first { identifier == $0.rawValue || identifier.hasPrefix($0.rawValue + "-") }
    }
}
