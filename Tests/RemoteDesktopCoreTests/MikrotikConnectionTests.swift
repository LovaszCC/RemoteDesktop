import Foundation
import GRDB
import Testing
@testable import RemoteDesktopCore

@Suite struct MikrotikConnectionTests {
    @Test func winboxArguments() {
        let prefix = ["-n", "-b", "com.mikrotik.winbox", "--args"]
        #expect(ExternalApps.winboxOpenArguments(host: " 10.0.0.1 ", username: "admin", password: "pw") == prefix + ["10.0.0.1", "admin", "pw"])
        #expect(ExternalApps.winboxOpenArguments(host: "10.0.0.1", username: "admin", password: nil) == prefix + ["10.0.0.1", "admin"])
        #expect(ExternalApps.winboxOpenArguments(host: "10.0.0.1", username: "admin", password: "") == prefix + ["10.0.0.1", "admin"])
        #expect(ExternalApps.winboxOpenArguments(host: "10.0.0.1", username: "", password: "pw") == prefix + ["10.0.0.1"])
    }

    @Test func typeMetadata() {
        #expect(ConnectionType.mikrotik.usesSSH)
        #expect(ConnectionType.ssh.usesSSH)
        #expect(!ConnectionType.rdp.usesSSH)
        #expect(!ConnectionType.web.usesSSH)
        #expect(ConnectionType.mikrotik.usesPort)
        #expect(ConnectionType.mikrotik.defaultPort == 22)
        #expect(MikrotikMode.winbox.displayName == "WinBox")
    }

    @Test func modeRoundtripsAndDefaultsToWinboxAfterMigration() throws {
        let queue = try DatabaseQueue()
        try AppDatabase.migrator.migrate(queue, upTo: "v5-web")
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO connection (id, type, name, host, username, domain, isFavorite, notes, createdAt, updatedAt)
                VALUES ('old', 'ssh', 'legacy', 'h', '', '', 0, '', '2025-01-01 00:00:00', '2025-01-01 00:00:00')
                """)
        }
        let repo = ConnectionRepository(database: try AppDatabase(queue))
        #expect(try repo.connection(id: "old")?.mikrotikMode == .winbox)

        let saved = try repo.save(Connection(type: .mikrotik, name: "rb", host: "10.0.0.1", mikrotikMode: .ssh))
        let loaded = try #require(try repo.connection(id: saved.id))
        #expect(loaded.type == .mikrotik)
        #expect(loaded.mikrotikMode == .ssh)
    }
}
