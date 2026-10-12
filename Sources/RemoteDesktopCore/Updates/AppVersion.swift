import Foundation

/// A release version such as `1.2.3` or the tag `v1.2.3`, compared number by number (`1.10.0` > `1.9.0`).
/// Anything else (`0.0.0-dev.12`, empty) doesn't parse, so dev builds never look outdated.
public struct AppVersion: Comparable, Sendable, CustomStringConvertible {
    public let components: [Int]

    public init?(_ text: String) {
        var text = Substring(text.trimmingCharacters(in: .whitespaces))
        if text.first == "v" || text.first == "V" { text = text.dropFirst() }
        let parts = text.split(separator: ".", omittingEmptySubsequences: false)
        let numbers = parts.compactMap { part in part.allSatisfy(\.isASCII) ? Int(part) : nil }
        guard !numbers.isEmpty, numbers.count == parts.count, numbers.allSatisfy({ $0 >= 0 }) else { return nil }
        components = numbers
    }

    public var description: String { components.map(String.init).joined(separator: ".") }

    /// Both component lists padded with zeros to the same length, so `1.2` equals `1.2.0`.
    private static func padded(_ lhs: AppVersion, _ rhs: AppVersion) -> ([Int], [Int]) {
        let count = max(lhs.components.count, rhs.components.count)
        let pad = { (version: AppVersion) in version.components + Array(repeating: 0, count: count - version.components.count) }
        return (pad(lhs), pad(rhs))
    }

    public static func == (lhs: AppVersion, rhs: AppVersion) -> Bool {
        let (left, right) = padded(lhs, rhs)
        return left == right
    }

    public static func < (lhs: AppVersion, rhs: AppVersion) -> Bool {
        let (left, right) = padded(lhs, rhs)
        return left.lexicographicallyPrecedes(right)
    }
}
