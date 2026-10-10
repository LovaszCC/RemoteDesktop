import Foundation
import Testing
@testable import RemoteDesktopCore

@Suite struct RDMImporterTests {
    static let sample = """
    <?xml version="1.0" encoding="utf-8"?>
    <ArrayOfConnection>
      <Connection>
        <ConnectionType>Group</ConnectionType>
        <Group>Customers\\Empty</Group>
        <Name>Empty</Name>
      </Connection>
      <Connection>
        <ConnectionType>RDPConfigured</ConnectionType>
        <Group>Customers\\Acme</Group>
        <Name>acme-dc01</Name>
        <Url>10.0.0.10:3390</Url>
        <Description>Domain controller</Description>
        <RDP>
          <UserName>administrator</UserName>
          <Domain>ACME</Domain>
          <Password>Secret1</Password>
        </RDP>
      </Connection>
      <Connection>
        <ConnectionType>SSHShell</ConnectionType>
        <Group>Customers/Acme</Group>
        <Name>acme-web</Name>
        <Terminal>
          <Host>web.acme.local</Host>
          <HostPort>22</HostPort>
          <Username>deploy</Username>
          <PrivateKeyPath>~/.ssh/acme</PrivateKeyPath>
          <SafePassword>ENCRYPTED==</SafePassword>
        </Terminal>
      </Connection>
      <Connection>
        <ConnectionType>RDPConfigured</ConnectionType>
        <Name>rootbox</Name>
        <Url>rootbox</Url>
        <UserName>me</UserName>
      </Connection>
      <Connection>
        <ConnectionType>WebBrowser</ConnectionType>
        <Name>portal</Name>
        <Url>https://portal</Url>
      </Connection>
      <Connection>
        <ConnectionType>RDPConfigured</ConnectionType>
        <Name>nohost</Name>
      </Connection>
    </ArrayOfConnection>
    """

    @Test func parsesEntries() throws {
        let result = try RDMImporter.parse(Data(Self.sample.utf8))

        #expect(result.emptyGroups == [["Customers", "Empty"]])
        #expect(result.skipped == ["portal (WebBrowser)", "nohost (no host)"])
        #expect(result.encryptedPasswords == 1)
        #expect(result.connections.count == 3)

        let dc = result.connections[0]
        #expect(dc.type == .rdp)
        #expect(dc.groupPath == ["Customers", "Acme"])
        #expect(dc.host == "10.0.0.10")
        #expect(dc.port == 3390)
        #expect(dc.username == "administrator")
        #expect(dc.domain == "ACME")
        #expect(dc.password == "Secret1")
        #expect(dc.notes == "Domain controller")

        let web = result.connections[1]
        #expect(web.type == .ssh)
        #expect(web.host == "web.acme.local")
        #expect(web.port == nil)
        #expect(web.username == "deploy")
        #expect(web.sshKeyPath == "~/.ssh/acme")
        #expect(web.password == nil)

        let root = result.connections[2]
        #expect(root.groupPath.isEmpty)
        #expect(root.username == "me")
    }

    @Test func importsIntoDatabaseReusingFolders() throws {
        let repo = ConnectionRepository(database: try .inMemory())
        let vault = try Vault.create(password: "pw", in: repo, iterations: 1_000)
        let existing = try repo.save(Folder(name: "Customers"))

        let summary = try RDMImporter.importFile(Data(Self.sample.utf8), into: repo, vault: vault)

        #expect(summary.imported == 3)
        #expect(summary.foldersCreated == 2)   // Empty + Acme; Customers reused
        #expect(summary.withoutPassword == 1)
        #expect(summary.skipped.count == 2)

        let library = try repo.library()
        let acme = try #require(library.folders.first { $0.name == "Acme" })
        #expect(acme.parentId == existing.id)
        let dc = try #require(library.connections.first { $0.name == "acme-dc01" })
        #expect(dc.folderId == acme.id)
        #expect(try vault.decryptOptional(dc.passwordEnc) == "Secret1")
        #expect(library.connections.first { $0.name == "rootbox" }?.folderId == nil)
    }

    @Test func invalidXMLThrows() {
        #expect(throws: (any Error).self) { try RDMImporter.parse(Data("<ArrayOfConnection><Connection>".utf8)) }
    }

    @Test func helpers() {
        #expect(RDMImporter.splitGroup("A\\ B /C\\\\") == ["A", "B", "C"])
        #expect(RDMImporter.splitHostPort("h:22")! == ("h", 22))
        #expect(RDMImporter.splitHostPort("[fe80::1]:3390")! == ("fe80::1", 3390))
        #expect(RDMImporter.splitHostPort("[fe80::1]")! == ("fe80::1", nil))
        #expect(RDMImporter.splitHostPort("fe80::1") == nil)
        #expect(RDMImporter.splitHostPort("host") == nil)
    }
}
