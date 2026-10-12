import Foundation
import GRDB
import Testing
@testable import RemoteDesktopCore

@Suite struct EmbeddedRDPTests {
    @Test func defaultsToEmbeddedAfterMigrationAndRoundtrips() throws {
        let queue = try DatabaseQueue()
        try AppDatabase.migrator.migrate(queue, upTo: "v7-ssh-terminal")
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO connection (id, type, name, host, username, domain, isFavorite, notes, createdAt, updatedAt)
                VALUES ('old', 'rdp', 'legacy', 'h', '', '', 0, '', '2025-01-01 00:00:00', '2025-01-01 00:00:00')
                """)
        }
        let repo = ConnectionRepository(database: try AppDatabase(queue))
        #expect(try repo.connection(id: "old")?.rdpClient == .embedded)

        let saved = try repo.save(Connection(type: .rdp, name: "w", host: "h", rdpClient: .windowsApp))
        #expect(try repo.connection(id: saved.id)?.rdpClient == .windowsApp)
    }

    @Test func sessionSettingsFromConnection() {
        let plain = RDPSessionSettings(connection: Connection(
            type: .rdp, name: "n", host: " srv01 ", username: " bob ", domain: "CORP", rdpAdminSession: true
        ))
        #expect(plain == RDPSessionSettings(connection: Connection(type: .rdp, name: "x", host: "srv01", username: "bob", domain: "CORP", rdpAdminSession: true)))
        #expect(plain.host == "srv01")
        #expect(plain.port == 3389)
        #expect(plain.username == "bob")
        #expect(plain.domain == "CORP")
        #expect(plain.adminSession)

        let custom = RDPSessionSettings(connection: Connection(type: .rdp, name: "n", host: "h", port: 3390))
        #expect(custom.port == 3390)
        #expect(!custom.adminSession)
    }

    @Test func qualifiedUsernamesOverrideDomainField() {
        let backslash = RDPSessionSettings(connection: Connection(type: .rdp, name: "n", host: "h", username: "AD\\alice", domain: "OTHER"))
        #expect(backslash.domain == "AD")
        #expect(backslash.username == "alice")

        let upn = RDPSessionSettings(connection: Connection(type: .rdp, name: "n", host: "h", username: "alice@ad.local", domain: "OTHER"))
        #expect(upn.domain == "")
        #expect(upn.username == "alice@ad.local")
    }

    @Test func disconnectClassification() {
        #expect(RDPDisconnectReason(lastError: 0, errorInfo: 0) == .clean)
        #expect(RDPDisconnectReason(lastError: 0, errorInfo: 0x0C) == .clean)
        #expect(RDPDisconnectReason(lastError: 0, errorInfo: 0x0B) == .clean)
        #expect(RDPDisconnectReason(lastError: 0, errorInfo: 0x01) == .clean)
        #expect(RDPDisconnectReason(lastError: 0, errorInfo: 0x02) == .clean)
        #expect(RDPDisconnectReason(lastError: 0, errorInfo: 0x03) == .failure) // idle timeout
        #expect(RDPDisconnectReason(lastError: 0, errorInfo: 0x05) == .failure) // taken over
        #expect(RDPDisconnectReason(lastError: 0x0002_0009, errorInfo: 0) == .failure)
    }

    @Test func typingKeystrokes() {
        #expect(RDPTyping.keystrokes(for: "aB") == [0x61, 0x42])
        #expect(RDPTyping.keystrokes(for: "a\r\nb\nc\rd") == [0x61, -1, 0x62, -1, 0x63, -1, 0x64])
        #expect(RDPTyping.keystrokes(for: "\r\n\r\n") == [-1, -1])
        #expect(RDPTyping.keystrokes(for: "x\ty") == [0x78, -2, 0x79])
        #expect(RDPTyping.keystrokes(for: "őű") == [0x0151, 0x0171])
        #expect(RDPTyping.keystrokes(for: "😀") == [0xD83D, 0xDE00])
        #expect(RDPTyping.keystrokes(for: "").isEmpty)
    }
}
