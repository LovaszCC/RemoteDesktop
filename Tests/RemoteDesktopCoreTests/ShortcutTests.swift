import Foundation
import Testing
@testable import RemoteDesktopCore

@Suite struct KeyShortcutTests {
    @Test func displayUsesMenuOrderAndGlyphs() {
        #expect(KeyShortcut("n", [.command, .shift]).displayString == "⇧⌘N")
        #expect(KeyShortcut("l", [.command, .control]).displayString == "⌃⌘L")
        #expect(KeyShortcut("i", [.command, .option, .shift, .control]).displayString == "⌃⌥⇧⌘I")
        #expect(KeyShortcut("return").displayString == "⌘↩")
        #expect(KeyShortcut("tab", [.control, .shift]).displayString == "⌃⇧⇥")
        #expect(KeyShortcut("f5", .option).displayString == "⌥F5")
    }

    @Test func charactersAreLowercasedSpecialsKept() {
        #expect(KeyShortcut("N").key == "n")
        #expect(KeyShortcut("return").isSpecialKey)
        #expect(!KeyShortcut("n").isSpecialKey)
    }

    @Test func requiredModifier() {
        #expect(!KeyShortcut("n", []).hasRequiredModifier)
        #expect(!KeyShortcut("n", .shift).hasRequiredModifier)
        #expect(KeyShortcut("n", .option).hasRequiredModifier)
        #expect(KeyShortcut("n", .control).hasRequiredModifier)
    }
}

@Suite struct ShortcutDefaultsTests {
    @Test func defaultsAreUniqueValidAndNotReserved() {
        let defaults = ShortcutCommand.allCases.map(\.defaultShortcut)
        #expect(Set(defaults).count == defaults.count)
        for command in ShortcutCommand.allCases {
            #expect(command.defaultShortcut.hasRequiredModifier)
            #expect(!ShortcutSettings.reserved.contains(command.defaultShortcut))
            #expect(ShortcutSettings().validate(command.defaultShortcut, for: command) == nil)
            #expect(!command.title.isEmpty)
        }
    }

    @Test func defaultsMatchPreviousKeys() {
        #expect(ShortcutCommand.newSSH.defaultShortcut.displayString == "⇧⌘N")
        #expect(ShortcutCommand.connect.defaultShortcut.displayString == "⌘↩")
        #expect(ShortcutCommand.splitTerminal.defaultShortcut.displayString == "⌘D")
        #expect(ShortcutCommand.broadcastAll.defaultShortcut.displayString == "⌥⌘I")
        #expect(ShortcutCommand.closeTab.defaultShortcut.displayString == "⌘W")
        #expect(ShortcutCommand.nextTab.defaultShortcut.displayString == "⌃⇥")
        #expect(ShortcutCommand.previousTab.defaultShortcut.displayString == "⌃⇧⇥")
        #expect(ShortcutCommand.lock.defaultShortcut.displayString == "⌃⌘L")
    }

    @Test func menuGroupsCoverEveryCommandOnce() {
        let grouped = ShortcutCommand.fileMenu + ShortcutCommand.connectionMenu
        #expect(grouped.count == ShortcutCommand.allCases.count)
        #expect(Set(grouped) == Set(ShortcutCommand.allCases))
    }
}

@Suite struct ShortcutSettingsTests {
    @Test func validateRefusesWithReason() {
        let settings = ShortcutSettings()
        #expect(settings.validate(KeyShortcut("k", []), for: .lock) == .missingModifier)
        #expect(settings.validate(KeyShortcut("k", .shift), for: .lock) == .missingModifier)
        #expect(settings.validate(KeyShortcut("q"), for: .lock) == .reserved)
        #expect(settings.validate(KeyShortcut("n", [.command, .shift]), for: .lock) == .inUse(by: .newSSH))
        #expect(settings.validate(KeyShortcut("l", [.command, .control]), for: .lock) == nil)
        #expect(settings.validate(KeyShortcut("k", [.command, .option]), for: .lock) == nil)
        #expect(!ShortcutError.inUse(by: .newSSH).message.isEmpty)
    }

    @Test func setClearAndReset() throws {
        var settings = ShortcutSettings()
        try settings.set(KeyShortcut("s", [.command, .control]), for: .newSSH)
        #expect(settings.shortcut(for: .newSSH) == KeyShortcut("s", [.command, .control]))
        #expect(settings.isCustomized(.newSSH))
        // The freed default can go to another command.
        try settings.set(KeyShortcut("n", [.command, .shift]), for: .newWeb)
        #expect(settings.validate(KeyShortcut("s", [.command, .control]), for: .lock) == .inUse(by: .newSSH))

        try settings.set(nil, for: .lock)
        #expect(settings.shortcut(for: .lock) == nil)
        #expect(settings.isCustomized(.lock))

        // newSSH's default is now taken by newWeb.
        #expect(throws: ShortcutError.inUse(by: .newWeb)) { try settings.reset(.newSSH) }
        try settings.reset(.newWeb)
        try settings.reset(.newSSH)
        #expect(!settings.isCustomized(.newSSH))
        #expect(settings.shortcut(for: .newSSH) == ShortcutCommand.newSSH.defaultShortcut)

        settings.resetAll()
        #expect(settings.overrides.isEmpty)
        #expect(settings.shortcut(for: .lock) == ShortcutCommand.lock.defaultShortcut)
    }

    @Test func settingTheDefaultIsNotACustomization() throws {
        var settings = ShortcutSettings()
        try settings.set(ShortcutCommand.lock.defaultShortcut, for: .lock)
        #expect(settings.overrides.isEmpty)
    }

    @Test func refusedSetLeavesSettingsUnchanged() {
        var settings = ShortcutSettings()
        #expect(throws: ShortcutError.reserved) { try settings.set(KeyShortcut("c"), for: .lock) }
        #expect(settings == ShortcutSettings())
    }
}

@Suite struct ShortcutStorageTests {
    private func makeDefaults() -> (UserDefaults, String) {
        let domain = "RemoteDesktopTests.Shortcuts.\(UUID().uuidString)"
        return (UserDefaults(suiteName: domain)!, domain)
    }

    @Test func roundTripsOverridesIncludingCleared() throws {
        let (defaults, domain) = makeDefaults()
        defer { defaults.removePersistentDomain(forName: domain) }
        var settings = ShortcutSettings()
        try settings.set(KeyShortcut("s", [.command, .control]), for: .newSSH)
        try settings.set(nil, for: .lock)
        settings.save(to: defaults)

        let loaded = ShortcutSettings.load(from: defaults)
        #expect(loaded == settings)
        #expect(loaded.shortcut(for: .lock) == nil)
        #expect(loaded.shortcut(for: .connect) == ShortcutCommand.connect.defaultShortcut)

        let stored = try #require(defaults.data(forKey: ShortcutSettings.defaultsKey))
        let json = try #require(try JSONSerialization.jsonObject(with: stored) as? [String: Any])
        #expect(Set(json.keys) == ["newSSH", "lock"])
    }

    @Test func missingCorruptOrUnknownFallsBackToDefaults() {
        let (defaults, domain) = makeDefaults()
        defer { defaults.removePersistentDomain(forName: domain) }
        #expect(ShortcutSettings.load(from: defaults) == ShortcutSettings())

        defaults.set(Data("not json".utf8), forKey: ShortcutSettings.defaultsKey)
        #expect(ShortcutSettings.load(from: defaults) == ShortcutSettings())

        defaults.set(Data(#"{"gone":{"key":"k","modifiers":1}}"#.utf8), forKey: ShortcutSettings.defaultsKey)
        #expect(ShortcutSettings.load(from: defaults) == ShortcutSettings())
    }
}
