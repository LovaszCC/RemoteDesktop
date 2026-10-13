import Foundation
import GRDB
import Testing
@testable import RemoteDesktopCore

@Suite struct RDPFileBuilderTests {
    @Test func writesAddressAndDomainUser() {
        let text = RDPFileBuilder.contents(for: Connection(type: .rdp, name: "dc", host: "10.0.0.5", username: "admin", domain: "CORP"))
        #expect(text.contains("full address:s:10.0.0.5\r\n"))
        #expect(text.contains("username:s:CORP\\admin\r\n"))
        #expect(text.contains("prompt for credentials:i:0"))
        #expect(!text.contains("password"))
    }

    @Test func customPortAndIPv6() {
        #expect(RDPFileBuilder.address(host: "srv", port: 3390) == "srv:3390")
        #expect(RDPFileBuilder.address(host: "fe80::1", port: 3389) == "[fe80::1]")
        #expect(RDPFileBuilder.address(host: "[fe80::1]", port: 4000) == "[fe80::1]:4000")
        #expect(RDPFileBuilder.address(host: " srv ", port: 3389) == "srv")
    }

    @Test func qualifiedUsernameVariants() {
        #expect(RDPFileBuilder.qualifiedUsername(username: "bob", domain: "") == "bob")
        #expect(RDPFileBuilder.qualifiedUsername(username: "bob", domain: "CORP") == "CORP\\bob")
        #expect(RDPFileBuilder.qualifiedUsername(username: "bob@corp.local", domain: "CORP") == "bob@corp.local")
        #expect(RDPFileBuilder.qualifiedUsername(username: "X\\bob", domain: "CORP") == "X\\bob")
        #expect(RDPFileBuilder.qualifiedUsername(username: " ", domain: "CORP") == "")
    }

    @Test func omitsUsernameWhenEmpty() {
        let text = RDPFileBuilder.contents(for: Connection(type: .rdp, name: "x", host: "h"))
        #expect(!text.contains("username:s:"))
    }

    @Test func fileNameIsSanitised() {
        #expect(RDPFileBuilder.fileName(for: Connection(type: .rdp, name: "web/01: prod", host: "h")) == "web_01_ prod.rdp")
        #expect(RDPFileBuilder.fileName(for: Connection(type: .rdp, name: "", host: "h")) == "connection.rdp")
    }
}

@Suite struct SSHLaunchScriptTests {
    let root = FileManager.default.temporaryDirectory.appending(path: "rd-tests-\(UUID().uuidString)")

    @Test func sshArgumentsDefaultPort() {
        let script = SSHLaunchScript(connection: Connection(type: .ssh, name: "w", host: "web01", username: "root"), password: nil, keyPassphrase: nil)
        #expect(script.sshArguments == ["ssh", "root@web01"])
    }

    @Test func sshArgumentsPortKeyNoUser() {
        let connection = Connection(type: .ssh, name: "w", host: "web01", port: 2222, sshKeyPath: "~/.ssh/id_ed25519")
        let args = SSHLaunchScript(connection: connection, password: nil, keyPassphrase: nil).sshArguments
        #expect(args == ["ssh", "-p", "2222", "-i", NSHomeDirectory() + "/.ssh/id_ed25519", "web01"])
    }

    @Test func quoting() {
        #expect(SSHLaunchScript.quote("it's") == "'it'\\''s'")
        #expect(SSHLaunchScript.quote("a b") == "'a b'")
    }

    @Test func emptySecretsAreIgnored() {
        let script = SSHLaunchScript(connection: Connection(type: .ssh, name: "w", host: "h"), password: "", keyPassphrase: "")
        #expect(!script.usesAskpass)
    }

    @Test func plainLaunchHasNoAskpass() throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let files = try SSHLaunchScript(connection: Connection(type: .ssh, name: "w", host: "h"), password: nil, keyPassphrase: nil).write(under: root)
        let text = try String(contentsOf: files.launchScript, encoding: .utf8)
        #expect(!text.contains("SSH_ASKPASS"))
        #expect(text.hasSuffix("exec 'ssh' 'h'\n"))
        #expect(!FileManager.default.fileExists(atPath: files.directory.appending(path: "askpass.sh").path))
    }

    @Test func writesPrivateFilesWithAskpass() throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let connection = Connection(type: .ssh, name: "web 01", host: "h", username: "u")
        let files = try SSHLaunchScript(connection: connection, password: "pw'1", keyPassphrase: "pp").write(under: root)

        let fm = FileManager.default
        func mode(_ url: URL) throws -> Int { try fm.attributesOfItem(atPath: url.path)[.posixPermissions] as! Int }
        #expect(try mode(files.directory) == 0o700)
        #expect(try mode(files.launchScript) == 0o700)
        #expect(try mode(files.directory.appending(path: "password")) == 0o600)
        #expect(try String(contentsOf: files.directory.appending(path: "password"), encoding: .utf8) == "pw'1")

        let text = try String(contentsOf: files.launchScript, encoding: .utf8)
        #expect(text.contains("export SSH_ASKPASS_REQUIRE=force"))
        #expect(text.contains("'web 01'"))
        #expect(text.contains("exec 'ssh' 'u@h'"))
    }

    @Test func askpassAnswersOnceFromSecretFile() throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let files = try SSHLaunchScript(connection: Connection(type: .ssh, name: "w", host: "h"), password: "hunter2", keyPassphrase: "keypp").write(under: root)

        func runAskpass(_ prompt: String) throws -> String {
            let process = Process()
            process.executableURL = files.directory.appending(path: "askpass.sh")
            process.arguments = [prompt]
            process.environment = [
                "RD_PASSWORD_FILE": files.directory.appending(path: "password").path,
                "RD_PASSPHRASE_FILE": files.directory.appending(path: "passphrase").path,
            ]
            let pipe = Pipe()
            process.standardOutput = pipe
            try process.run()
            process.waitUntilExit()
            return String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        }

        #expect(try runAskpass("u@h's password: ") == "hunter2")
        #expect(try runAskpass("Enter passphrase for key '/k': ") == "keypp")
        #expect(!FileManager.default.fileExists(atPath: files.directory.appending(path: "password").path))
        #expect(!FileManager.default.fileExists(atPath: files.directory.appending(path: "passphrase").path))
    }
}

@Suite struct ExternalAppsTests {
    @Test func openArguments() {
        #expect(ExternalApps.windowsAppOpenArguments(rdpFile: URL(filePath: "/tmp/a.rdp")) == ["-b", "com.microsoft.rdc.macos", "/tmp/a.rdp"])
        #expect(ExternalApps.terminalLaunch(.ghostty, launchScript: URL(filePath: "/tmp/l.sh")).arguments == ["-n", "-b", "com.mitchellh.ghostty", "--args", "-e", "/tmp/l.sh"])
    }
}

@Suite struct RDPAdminSessionTests {
    @Test func adminSessionLineOnlyWhenEnabled() {
        let normal = RDPFileBuilder.contents(for: Connection(type: .rdp, name: "a", host: "h"))
        let admin = RDPFileBuilder.contents(for: Connection(type: .rdp, name: "a", host: "h", rdpAdminSession: true))
        #expect(!normal.contains("administrative session"))
        #expect(admin.contains("administrative session:i:1\r\n"))
    }

    @Test func flagRoundtripsAndDefaultsFalseAfterMigration() throws {
        let queue = try GRDB.DatabaseQueue()
        try AppDatabase.migrator.migrate(queue, upTo: "v2-credentials")
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO connection (id, type, name, host, username, domain, isFavorite, notes, createdAt, updatedAt)
                VALUES ('old', 'rdp', 'legacy', 'h', '', '', 0, '', '2025-01-01 00:00:00', '2025-01-01 00:00:00')
                """)
        }
        let repo = ConnectionRepository(database: try AppDatabase(queue))
        #expect(try repo.connection(id: "old")?.rdpAdminSession == false)

        let saved = try repo.save(Connection(type: .rdp, name: "n", host: "h", rdpAdminSession: true))
        #expect(try repo.connection(id: saved.id)?.rdpAdminSession == true)
    }

    @Test func importerMapsAdminMode() throws {
        let xml = """
        <ArrayOfConnection>
          <Connection><ConnectionType>RDPConfigured</ConnectionType><Name>a</Name><Url>a</Url><RDP><AdminMode>true</AdminMode></RDP></Connection>
          <Connection><ConnectionType>RDPConfigured</ConnectionType><Name>b</Name><Url>b</Url></Connection>
        </ArrayOfConnection>
        """
        let result = try RDMImporter.parse(Data(xml.utf8))
        #expect(result.connections.map(\.rdpAdminSession) == [true, false])
    }
}
