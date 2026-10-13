import Foundation

/// Fixed palette for folder icons, connections and session tabs (Finder-tag style).
public enum LabelColor: String, Codable, CaseIterable, Sendable {
    case red, orange, yellow, green, blue, purple, pink, gray

    public var title: String {
        switch self {
        case .red: String(localized: "Red", bundle: .core)
        case .orange: String(localized: "Orange", bundle: .core)
        case .yellow: String(localized: "Yellow", bundle: .core)
        case .green: String(localized: "Green", bundle: .core)
        case .blue: String(localized: "Blue", bundle: .core)
        case .purple: String(localized: "Purple", bundle: .core)
        case .pink: String(localized: "Pink", bundle: .core)
        case .gray: String(localized: "Gray", bundle: .core)
        }
    }

    /// The color all entries share, or nil when they differ or there are none.
    public static func common(_ colors: [LabelColor?]) -> LabelColor? {
        guard let first = colors.first, colors.allSatisfy({ $0 == first }) else { return nil }
        return first
    }
}
