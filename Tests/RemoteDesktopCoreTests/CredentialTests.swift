import Foundation
import GRDB
import Testing
@testable import RemoteDesktopCore

@Suite struct CredentialTests {
    let repo: ConnectionRepository

    init() throws {
        repo = ConnectionRepository(database: try .inMemory())
    }

    @Test func crudAndLibrary() throws {
        var credential = try repo.save(Credential(name: "Domain Admin", username: "admin", domain: "CORP", passwordEnc: Data([1])))
        credential.name = "Domain Admin 2"
        try repo.save(credential)

        #expect(try repo.credential(id: credential.id)?.name == "Domain Admin 2")
        let library = try repo.library()
        #expect(library.credentials.map(\.name) == ["Domain Admin 2"])
        #expect(library.credential(id: credential.id)?.username == "admin")
        #expect(library.credential(id: nil) == nil)
    }

    @Test func deletingCredentialUnlinksConnections() throws {
        let credential = try repo.save(Credential(name: "c"))
        let connection = try repo.save(Connection(credentialId: credential.id, type: .rdp, name: "x", host: "h", username: "own"))
        #expect(try repo.library().connections(using: credential.id).map(\.id) == [connection.id])

        try repo.deleteCredential(id: credential.id)

        let loaded = try #require(try repo.connection(id: connection.id))
        #expect(loaded.credentialId == nil)
        #expect(loaded.username == "own")
        #expect(try repo.library().credentials.isEmpty)
    }

    @Test func resolvedReplacesAllAuthFields() {
        let connection = Connection(
            type: .ssh, name: "s", host: "h", username: "own", domain: "OWN",
            passwordEnc: Data([1]), sshKeyPath: "~/own", sshKeyPassphraseEnc: Data([2])
        )
        let credential = Credential(name: "c", username: "shared", domain: "", passwordEnc: Data([9]), sshKeyPath: nil)

        let resolved = connection.resolved(with: credential)
        #expect(resolved.username == "shared")
        #expect(resolved.domain == "")
        #expect(resolved.passwordEnc == Data([9]))
        #expect(resolved.sshKeyPath == nil)
        #expect(resolved.sshKeyPassphraseEnc == nil)
        #expect(resolved.host == "h")
        #expect(connection.resolved(with: nil) == connection)
    }

    @Test func libraryResolvesLinkedCredential() throws {
        let credential = try repo.save(Credential(name: "c", username: "shared"))
        let linked = try repo.save(Connection(credentialId: credential.id, type: .rdp, name: "a", host: "h", username: "own"))
        let dangling = Connection(credentialId: "missing", type: .rdp, name: "b", host: "h", username: "own")

        let library = try repo.library()
        #expect(library.resolved(linked).username == "shared")
        #expect(library.resolved(dangling).username == "own")
    }

    @Test func displayUser() {
        #expect(Credential(name: "c", username: "bob", domain: "CORP").displayUser == "CORP\\bob")
        #expect(Credential(name: "c", username: "bob").displayUser == "bob")
        #expect(Credential(name: "c").displayUser == "")
    }

    @Test func migratesExistingV1DatabaseKeepingRows() throws {
        let queue = try DatabaseQueue()
        try AppDatabase.migrator.migrate(queue, upTo: "v1")
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO connection (id, type, name, host, username, domain, isFavorite, notes, createdAt, updatedAt)
                VALUES ('old', 'rdp', 'legacy', '10.0.0.1', 'u', 'D', 0, '', '2025-01-01 00:00:00', '2025-01-01 00:00:00')
                """)
        }

        let repo = ConnectionRepository(database: try AppDatabase(queue))
        let legacy = try #require(try repo.connection(id: "old"))
        #expect(legacy.name == "legacy")
        #expect(legacy.credentialId == nil)
    }
}
