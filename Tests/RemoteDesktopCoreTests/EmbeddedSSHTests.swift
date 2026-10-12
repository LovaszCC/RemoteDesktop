import Foundation
import GRDB
import Testing
@testable import RemoteDesktopCore

@Suite struct SessionTabsTests {
    @Test func openActivatesNewTab() {
        var tabs = SessionTabs()
        #expect(tabs.active == nil)
        tabs.open("a")
        tabs.open("b")
        #expect(tabs.ids == ["a", "b"])
        #expect(tabs.active == "b")
        tabs.open("a")
        #expect(tabs.ids == ["a", "b"])
        #expect(tabs.active == "a")
    }

    @Test func closingActiveSelectsNeighbour() {
        var tabs = SessionTabs()
        ["a", "b", "c"].forEach { tabs.open($0) }
        tabs.select("b")
        tabs.close("b")
        #expect(tabs.active == "c")
        tabs.close("c")
        #expect(tabs.active == "a")
        tabs.close("a")
        #expect(tabs.active == nil)
        #expect(tabs.ids.isEmpty)
    }

    @Test func closingInactiveKeepsActive() {
        var tabs = SessionTabs()
        ["a", "b"].forEach { tabs.open($0) }
        tabs.close("a")
        #expect(tabs.active == "b")
        tabs.select(nil)
        tabs.close("b")
        #expect(tabs.active == nil)
        tabs.close("missing")
    }

    @Test func selectIgnoresUnknown() {
        var tabs = SessionTabs()
        tabs.open("a")
        tabs.select("zzz")
        #expect(tabs.active == "a")
        tabs.select(nil)
        #expect(tabs.active == nil)
    }

    @Test func cycleWrapsThroughDetails() {
        var tabs = SessionTabs()
        ["a", "b"].forEach { tabs.open($0) }
        tabs.select(nil)
        tabs.cycle(forward: true)
        #expect(tabs.active == "a")
        tabs.cycle(forward: true)
        #expect(tabs.active == "b")
        tabs.cycle(forward: true)
        #expect(tabs.active == nil)
        tabs.cycle(forward: false)
        #expect(tabs.active == "b")
    }

    @Test func exitPolicy() {
        #expect(SessionTabs.closesTab(exitCode: 0))
        #expect(!SessionTabs.closesTab(exitCode: 255))
        #expect(!SessionTabs.closesTab(exitCode: 1))
        #expect(!SessionTabs.closesTab(exitCode: nil))
    }
}

@Suite struct SSHTerminalSettingTests {
    @Test func defaultsToEmbeddedAfterMigrationAndRoundtrips() throws {
        let queue = try DatabaseQueue()
        try AppDatabase.migrator.migrate(queue, upTo: "v6-mikrotik")
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO connection (id, type, name, host, username, domain, isFavorite, notes, createdAt, updatedAt)
                VALUES ('old', 'ssh', 'legacy', 'h', '', '', 0, '', '2025-01-01 00:00:00', '2025-01-01 00:00:00')
                """)
        }
        let repo = ConnectionRepository(database: try AppDatabase(queue))
        #expect(try repo.connection(id: "old")?.sshTerminal == .embedded)

        let saved = try repo.save(Connection(type: .ssh, name: "g", host: "h", sshTerminal: .ghostty))
        #expect(try repo.connection(id: saved.id)?.sshTerminal == .ghostty)
    }

    @Test func launchScriptRunsViaShWithoutExecBit() throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "rd-embedded-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let files = try SSHLaunchScript(connection: Connection(type: .ssh, name: "w", host: "h"), password: nil, keyPassphrase: nil).write(under: root)
        let text = try String(contentsOf: files.launchScript, encoding: .utf8)
        #expect(text.hasPrefix("#!/bin/sh\n"))

        let process = Process()
        process.executableURL = URL(filePath: "/bin/sh")
        process.arguments = ["-n", files.launchScript.path]
        try process.run()
        process.waitUntilExit()
        #expect(process.terminationStatus == 0)
    }
}
