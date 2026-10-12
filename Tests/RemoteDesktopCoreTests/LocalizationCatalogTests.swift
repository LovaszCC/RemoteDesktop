import Foundation
import Testing

/// Every UI string must have a Hungarian translation with the same format placeholders.
@Suite struct LocalizationCatalogTests {
    static let catalogs = ["RemoteDesktop", "RemoteDesktopCore"]

    private static func catalog(_ module: String) throws -> [String: Any] {
        let url = URL(filePath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appending(path: "Sources/\(module)/Resources/Localizable.xcstrings")
        let json = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any]
        return try #require(json)
    }

    /// `%@`, `%lld`, `%d`, `%u`, … in order of appearance.
    private static func placeholders(_ text: String) -> [String] {
        let regex = /%(?:\d+\$)?(?:ll|l|h)?[@dDuUxXfeEgGcs]/
        return text.matches(of: regex).map { String($0.output) }
    }

    /// All string values of one localization (plain or plural variants).
    private static func values(_ localization: [String: Any]) -> [String] {
        if let unit = localization["stringUnit"] as? [String: Any] {
            return [unit["value"] as? String ?? ""]
        }
        let plural = (localization["variations"] as? [String: Any])?["plural"] as? [String: Any] ?? [:]
        return plural.values.compactMap { (($0 as? [String: Any])?["stringUnit"] as? [String: Any])?["value"] as? String }
    }

    @Test(arguments: catalogs)
    func sourceLanguageIsEnglish(module: String) throws {
        #expect(try Self.catalog(module)["sourceLanguage"] as? String == "en")
    }

    @Test(arguments: catalogs)
    func everyKeyHasACompleteHungarianTranslation(module: String) throws {
        let strings = try #require(try Self.catalog(module)["strings"] as? [String: [String: Any]])
        #expect(!strings.isEmpty)
        for (key, entry) in strings {
            let localizations = entry["localizations"] as? [String: [String: Any]] ?? [:]
            let hungarian = try #require(localizations["hu"], "no Hungarian for \(key)")
            if let unit = hungarian["stringUnit"] as? [String: Any] {
                #expect(unit["state"] as? String == "translated", "\(key)")
            }
            let expected = Self.placeholders(key).sorted()
            for value in Self.values(hungarian) {
                #expect(!value.isEmpty, "\(key)")
                #expect(Self.placeholders(value).sorted() == expected, "hu placeholders differ: \(key)")
            }
            if let english = localizations["en"] {
                for value in Self.values(english) {
                    #expect(Self.placeholders(value).sorted() == expected, "en placeholders differ: \(key)")
                }
            }
        }
    }
}
