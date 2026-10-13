import Foundation
import GRDB
import Testing
@testable import RemoteDesktopCore

@Suite struct ConnectionColorTests {
    @Test func migrationLeavesExistingConnectionsUncoloredAndRoundTrips() throws {
        let queue = try DatabaseQueue()
        try AppDatabase.migrator.migrate(queue, upTo: "v11-folder-color")
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO connection (id, type, name, host, username, domain, isFavorite, notes, createdAt, updatedAt)
                VALUES ('old', 'ssh', 'legacy', 'h', '', '', 0, '', '2025-01-01 00:00:00', '2025-01-01 00:00:00')
                """)
        }
        let repo = ConnectionRepository(database: try AppDatabase(queue))
        #expect(try repo.connection(id: "old")?.color == nil)

        let saved = try repo.save(Connection(type: .ssh, name: "prod", host: "h", color: .red))
        #expect(try repo.connection(id: saved.id)?.color == .red)
    }
}

@Suite struct TabColorOverrideTests {
    @Test func automaticUnlessOverridden() {
        var tabs = SessionTabs()
        tabs.open("a")
        #expect(tabs.color(for: "a", automatic: .red) == .red)
        tabs.setColor(.blue, for: "a")
        #expect(tabs.color(for: "a", automatic: .red) == .blue)
        tabs.setColor(nil, for: "a")
        #expect(tabs.color(for: "a", automatic: .red) == nil)
        tabs.resetColor(for: "a")
        #expect(tabs.color(for: "a", automatic: .red) == .red)
    }

    @Test func unknownTabIsIgnored() {
        var tabs = SessionTabs()
        tabs.setColor(.blue, for: "x")
        #expect(tabs.colorOverrides.isEmpty)
    }

    @Test func closeDropsAndReplaceMovesOverride() {
        var tabs = SessionTabs()
        ["a", "b"].forEach { tabs.open($0) }
        tabs.setColor(.green, for: "a")
        tabs.setColor(nil, for: "b")
        tabs.replace("a", with: "s")
        #expect(tabs.color(for: "s", automatic: nil) == .green)
        #expect(tabs.colorOverrides["a"] == nil)
        tabs.close("b")
        #expect(tabs.colorOverrides["b"] == nil)
        #expect(tabs.color(for: "b", automatic: .red) == .red)
    }
}
