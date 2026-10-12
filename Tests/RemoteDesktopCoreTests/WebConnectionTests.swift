import Foundation
import GRDB
import Testing
@testable import RemoteDesktopCore

@Suite struct WebConnectionTests {
    @Test func urlNormalisation() {
        #expect(WebLaunch.url(from: "router.local/admin")?.absoluteString == "https://router.local/admin")
        #expect(WebLaunch.url(from: "  http://10.0.0.1:8080/x ")?.absoluteString == "http://10.0.0.1:8080/x")
        #expect(WebLaunch.url(from: "https://example.com")?.absoluteString == "https://example.com")
        #expect(WebLaunch.url(from: "") == nil)
        #expect(WebLaunch.url(from: "not a url") == nil)
        #expect(WebLaunch.url(from: "https://") == nil)
    }

    @Test func clipboardSequencePasswordThenUsername() {
        #expect(WebLaunch.clipboardSequence(username: "bob", password: "pw") == ["pw", "bob"])
        #expect(WebLaunch.clipboardSequence(username: "bob", password: nil) == ["bob"])
        #expect(WebLaunch.clipboardSequence(username: "", password: "pw") == ["pw"])
        #expect(WebLaunch.clipboardSequence(username: "", password: "").isEmpty)
    }

    @Test func browserOpenArguments() throws {
        let url = try #require(URL(string: "https://example.com/login"))
        #expect(ExternalApps.browserOpenArguments(url: url, bundleID: nil) == ["https://example.com/login"])
        #expect(ExternalApps.browserOpenArguments(url: url, bundleID: "") == ["https://example.com/login"])
        #expect(ExternalApps.browserOpenArguments(url: url, bundleID: "com.brave.Browser") == ["-b", "com.brave.Browser", "https://example.com/login"])
    }

    @Test func typeMetadata() {
        // Localized: English or Hungarian depending on the test process language.
        #expect(["Web", "Böngésző"].contains(ConnectionType.web.displayName))
        #expect(!ConnectionType.web.usesPort)
        #expect(ConnectionType.ssh.usesPort)
        #expect(Set(ConnectionType.allCases.map(\.systemImage)).count == ConnectionType.allCases.count)
    }

    @Test func browserRoundtripsAndDefaultsNilAfterMigration() throws {
        let queue = try DatabaseQueue()
        try AppDatabase.migrator.migrate(queue, upTo: "v4-unlocked-key")
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO connection (id, type, name, host, username, domain, isFavorite, notes, createdAt, updatedAt)
                VALUES ('old', 'ssh', 'legacy', 'h', '', '', 0, '', '2025-01-01 00:00:00', '2025-01-01 00:00:00')
                """)
        }
        let repo = ConnectionRepository(database: try AppDatabase(queue))
        #expect(try repo.connection(id: "old")?.browserBundleId == nil)

        let saved = try repo.save(Connection(type: .web, name: "w", host: "example.com", browserBundleId: "com.apple.Safari"))
        let loaded = try #require(try repo.connection(id: saved.id))
        #expect(loaded.type == .web)
        #expect(loaded.browserBundleId == "com.apple.Safari")
    }
}
