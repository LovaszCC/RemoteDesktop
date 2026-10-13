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

    @Test func credentialWithoutUsernameKeepsOwnUserAndDomain() {
        let connection = Connection(type: .rdp, name: "s", host: "h", username: "own", domain: "OWN", passwordEnc: Data([1]))
        let keyOnly = Credential(name: "key", username: "", domain: "", passwordEnc: Data([9]), sshKeyPath: "~/.ssh/id_rsa")

        let resolved = connection.resolved(with: keyOnly)
        #expect(resolved.username == "own")
        #expect(resolved.domain == "OWN")
        #expect(resolved.passwordEnc == Data([9]))
        #expect(resolved.sshKeyPath == "~/.ssh/id_rsa")
    }

    @Test func credentialUsernameBringsItsDomainEvenWhenEmpty() {
        let connection = Connection(type: .rdp, name: "s", host: "h", username: "own", domain: "OWN")
        let local = Credential(name: "local", username: "admin", domain: "")
        let resolved = connection.resolved(with: local)
        #expect(resolved.username == "admin")
        #expect(resolved.domain == "")
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

@Suite struct CredentialVaultAndImportTests {
    @Test func changePasswordReencryptsCredentials() throws {
        let repo = ConnectionRepository(database: try .inMemory())
        let vault = try Vault.create(password: "old", in: repo, iterations: 1_000)
        let credential = try repo.save(Credential(name: "c", passwordEnc: vault.encrypt("pw"), sshKeyPassphraseEnc: vault.encrypt("pp")))

        let newVault = try vault.changePassword(to: "new", in: repo, iterations: 1_000)

        let loaded = try #require(try repo.credential(id: credential.id))
        #expect(try newVault.decryptOptional(loaded.passwordEnc) == "pw")
        #expect(try newVault.decryptOptional(loaded.sshKeyPassphraseEnc) == "pp")
    }

    static let sample = """
    <?xml version="1.0" encoding="utf-8"?>
    <ArrayOfConnection>
      <Connection>
        <ConnectionType>Credential</ConnectionType>
        <ID>cred-1</ID>
        <Name>Domain Admin</Name>
        <Credentials>
          <UserName>admin</UserName>
          <Domain>CORP</Domain>
          <Password>S3cret</Password>
        </Credentials>
      </Connection>
      <Connection>
        <ConnectionType>Credential</ConnectionType>
        <ID>cred-2</ID>
        <Name>Encrypted</Name>
        <Credentials>
          <UserName>x</UserName>
          <SafePassword>ENC==</SafePassword>
        </Credentials>
      </Connection>
      <Connection>
        <ConnectionType>RDPConfigured</ConnectionType>
        <Name>dc01</Name>
        <Url>dc01</Url>
        <CredentialConnectionID>cred-1</CredentialConnectionID>
      </Connection>
      <Connection>
        <ConnectionType>RDPConfigured</ConnectionType>
        <Name>other</Name>
        <Url>other</Url>
        <CredentialConnectionID>unknown-ref</CredentialConnectionID>
      </Connection>
    </ArrayOfConnection>
    """

    @Test func parsesCredentialEntries() throws {
        let result = try RDMImporter.parse(Data(Self.sample.utf8))
        #expect(result.credentials.map(\.ref) == ["cred-1", "cred-2"])
        #expect(result.credentials[0].username == "admin")
        #expect(result.credentials[0].domain == "CORP")
        #expect(result.credentials[0].password == "S3cret")
        #expect(result.credentials[1].password == nil)
        #expect(result.encryptedPasswords == 1)
        #expect(result.connections.map(\.credentialRef) == ["cred-1", "unknown-ref"])
    }

    @Test func importLinksConnectionsToCredentials() throws {
        let repo = ConnectionRepository(database: try .inMemory())
        let vault = try Vault.create(password: "pw", in: repo, iterations: 1_000)

        let summary = try RDMImporter.importFile(Data(Self.sample.utf8), into: repo, vault: vault)
        #expect(summary.credentialsImported == 2)
        #expect(summary.imported == 2)

        let library = try repo.library()
        let admin = try #require(library.credentials.first { $0.name == "Domain Admin" })
        #expect(try vault.decryptOptional(admin.passwordEnc) == "S3cret")
        #expect(library.connections.first { $0.name == "dc01" }?.credentialId == admin.id)
        #expect(library.connections.first { $0.name == "other" }?.credentialId == nil)
    }
}
