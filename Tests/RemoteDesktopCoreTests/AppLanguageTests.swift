import Foundation
import Testing
@testable import RemoteDesktopCore

@Suite struct AppLanguageTests {
    /// Fresh, isolated defaults domain per test.
    private func makeDefaults() -> (UserDefaults, String) {
        let domain = "RemoteDesktopTests.AppLanguage.\(UUID().uuidString)"
        return (UserDefaults(suiteName: domain)!, domain)
    }

    @Test func defaultsToEnglishWhenUnset() {
        let (defaults, domain) = makeDefaults()
        defer { defaults.removePersistentDomain(forName: domain) }
        #expect(AppLanguage.current(in: defaults, domain: domain) == .english)
    }

    @Test func applyRoundTripsBothLanguages() {
        let (defaults, domain) = makeDefaults()
        defer { defaults.removePersistentDomain(forName: domain) }
        for language in AppLanguage.allCases {
            language.apply(to: defaults)
            #expect(defaults.stringArray(forKey: "AppleLanguages") == [language.rawValue])
            #expect(AppLanguage.current(in: defaults, domain: domain) == language)
        }
    }

    @Test func regionVariantsAndUnsupportedLanguages() {
        let (defaults, domain) = makeDefaults()
        defer { defaults.removePersistentDomain(forName: domain) }
        defaults.set(["hu-HU"], forKey: "AppleLanguages")
        #expect(AppLanguage.current(in: defaults, domain: domain) == .hungarian)
        defaults.set(["de"], forKey: "AppleLanguages")
        #expect(AppLanguage.current(in: defaults, domain: domain) == .english)
        #expect(AppLanguage.language(for: "en-GB") == .english)
        #expect(AppLanguage.language(for: "hungarian") == nil)
    }

    @Test func firstLaunchPinsEnglishButKeepsAChoice() {
        let (defaults, domain) = makeDefaults()
        defer { defaults.removePersistentDomain(forName: domain) }
        AppLanguage.applyDefaultIfUnset(in: defaults, domain: domain)
        #expect(defaults.stringArray(forKey: "AppleLanguages") == ["en"])

        AppLanguage.hungarian.apply(to: defaults)
        AppLanguage.applyDefaultIfUnset(in: defaults, domain: domain)
        #expect(defaults.stringArray(forKey: "AppleLanguages") == ["hu"])
    }

    @Test func pickerNamesAreNative() {
        #expect(AppLanguage.english.displayName == "English")
        #expect(AppLanguage.hungarian.displayName == "Magyar")
    }
}
