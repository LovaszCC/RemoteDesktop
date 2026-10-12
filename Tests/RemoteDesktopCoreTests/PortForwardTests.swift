import Darwin
import Foundation
import GRDB
import Testing
@testable import RemoteDesktopCore

@Suite struct PortForwardModelTests {
    private func forward(_ local: Int = 13389, host: String = "10.0.0.20", remote: Int = 3389,
                         connection: String = "c1", id: String = UUID().uuidString) -> PortForward {
        PortForward(id: id, connectionId: connection, localPort: local, remoteHost: host, remotePort: remote)
    }

    @Test func validForwardHasNoError() {
        #expect(forward().validationError(among: []) == nil)
        #expect(forward(1, remote: 65535).validationError(among: []) == nil)
    }

    @Test(arguments: [
        (0, "h", 22, PortForwardError.invalidLocalPort),
        (65536, "h", 22, .invalidLocalPort),
        (-1, "h", 22, .invalidLocalPort),
        (8080, "  ", 22, .missingRemoteHost),
        (8080, "h", 0, .invalidRemotePort),
        (8080, "h", 70000, .invalidRemotePort),
    ])
    func rejectsInvalidValues(local: Int, host: String, remote: Int, error: PortForwardError) {
        #expect(forward(local, host: host, remote: remote).validationError(among: []) == error)
        #expect(error.errorDescription?.isEmpty == false)
    }

    @Test func localPortIsUniquePerConnection() {
        let existing = forward(8080, id: "a")
        #expect(forward(8080, id: "b").validationError(among: [existing]) == .duplicateLocalPort)
        #expect(PortForwardError.duplicateLocalPort.errorDescription?.isEmpty == false)
        // The forward itself (editing) and other connections don't clash.
        #expect(forward(8080, id: "a").validationError(among: [existing]) == nil)
        #expect(forward(8080, connection: "c2", id: "b").validationError(among: [existing]) == nil)
    }

    @Test func displayNameAndDestination() {
        var named = forward(host: "fs.local", remote: 445)
        #expect(named.displayName == "fs.local:445")
        named.name = "  File server "
        #expect(named.displayName == "File server")
        #expect(forward(host: "fe80::1", remote: 22).destination == "[fe80::1]:22")
    }
}

@Suite struct PortForwardStorageTests {
    @Test func migratesExistingDatabaseAndRoundTrips() throws {
        let queue = try DatabaseQueue()
        try AppDatabase.migrator.migrate(queue, upTo: "v8-rdp-client")
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO connection (id, type, name, host, username, domain, isFavorite, notes, createdAt, updatedAt)
                VALUES ('gw', 'ssh', 'gateway', 'gw.example', '', '', 0, '', '2025-01-01 00:00:00', '2025-01-01 00:00:00')
                """)
        }
        let repo = ConnectionRepository(database: try AppDatabase(queue))
        #expect(try repo.library().portForwards.isEmpty)

        let saved = try repo.save(PortForward(connectionId: "gw", name: "RDP", localPort: 13389,
                                              remoteHost: "10.0.0.20", remotePort: 3389))
        let library = try repo.library()
        // Compared by id: SQLite keeps dates to the millisecond.
        #expect(library.portForwards.map(\.id) == [saved.id])
        #expect(library.portForwards.first?.remoteHost == "10.0.0.20")
        #expect(library.portForwards(for: "gw").map(\.id) == [saved.id])
        #expect(library.portForwards(for: "other").isEmpty)

        var edited = saved
        edited.localPort = 23389
        try repo.save(edited)
        #expect(try repo.library().portForwards.map(\.localPort) == [23389])

        try repo.deletePortForward(id: saved.id)
        #expect(try repo.library().portForwards.isEmpty)
    }

    @Test func deletingTheConnectionDeletesItsForwards() throws {
        let repo = ConnectionRepository(database: try AppDatabase.inMemory())
        let gateway = try repo.save(Connection(type: .ssh, name: "gw", host: "gw.example"))
        let other = try repo.save(Connection(type: .ssh, name: "other", host: "other.example"))
        try repo.save(PortForward(connectionId: gateway.id, localPort: 1, remoteHost: "a", remotePort: 1))
        let kept = try repo.save(PortForward(connectionId: other.id, localPort: 2, remoteHost: "b", remotePort: 2))

        try repo.deleteConnection(id: gateway.id)
        #expect(try repo.library().portForwards.map(\.id) == [kept.id])
    }

    @Test func forwardNeedsAnExistingConnection() throws {
        let repo = ConnectionRepository(database: try AppDatabase.inMemory())
        #expect(throws: (any Error).self) {
            try repo.save(PortForward(connectionId: "missing", localPort: 1, remoteHost: "a", remotePort: 1))
        }
    }
}

@Suite struct TunnelCommandTests {
    private let forward = PortForward(connectionId: "c", localPort: 13389, remoteHost: "10.0.0.20", remotePort: 3389)

    @Test func tunnelArguments() {
        let connection = Connection(type: .ssh, name: "gw", host: " gw.example ", port: 2222, username: "admin",
                                    sshKeyPath: "~/.ssh/id_ed25519")
        let args = SSHLaunchScript(connection: connection, password: nil, keyPassphrase: nil, forward: forward).sshArguments
        #expect(args == [
            "ssh", "-p", "2222", "-i", NSHomeDirectory() + "/.ssh/id_ed25519",
            "-N", "-o", "ExitOnForwardFailure=yes", "-o", "ServerAliveInterval=30", "-o", "ServerAliveCountMax=3",
            "-o", "StrictHostKeyChecking=yes", "-L", "127.0.0.1:13389:10.0.0.20:3389",
            "admin@gw.example",
        ])
    }

    @Test func interactiveSessionHasNoTunnelOptions() {
        let args = SSHLaunchScript(connection: Connection(type: .ssh, name: "gw", host: "h"), password: nil, keyPassphrase: nil).sshArguments
        #expect(args == ["ssh", "h"])
    }

    @Test func ipv6DestinationIsBracketed() {
        let v6 = PortForward(connectionId: "c", localPort: 8443, remoteHost: "fd00::5", remotePort: 443)
        #expect(SSHLaunchScript.tunnelOptions(v6).last == "127.0.0.1:8443:[fd00::5]:443")
    }

    @Test func tunnelScriptUsesAskpassWithoutTerminalTitle() throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "rd-tunnel-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let script = SSHLaunchScript(connection: Connection(type: .ssh, name: "gw", host: "h", username: "u"),
                                     password: "secret", keyPassphrase: nil, forward: forward)
        let files = try script.write(under: root)
        let text = try String(contentsOf: files.launchScript, encoding: .utf8)
        #expect(!text.contains("\\033]0;"))
        #expect(text.contains("SSH_ASKPASS_REQUIRE=force"))
        #expect(text.contains("'-L' '127.0.0.1:13389:10.0.0.20:3389'"))
        #expect(FileManager.default.fileExists(atPath: files.directory.appending(path: "password").path))

        let check = Process()
        check.executableURL = URL(filePath: "/bin/sh")
        check.arguments = ["-n", files.launchScript.path]
        try check.run()
        check.waitUntilExit()
        #expect(check.terminationStatus == 0)
    }
}

@Suite struct SSHHostKeysTests {
    private static let config = """
        user admin
        hostname 10.1.2.3
        port 2222
        userknownhostsfile ~/.ssh/known_hosts ~/.ssh/known_hosts2
        globalknownhostsfile /etc/ssh/ssh_known_hosts /etc/ssh/ssh_known_hosts2
        stricthostkeychecking ask
        """

    private func temporaryDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appending(path: "rd-hostkeys-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    private func target(port: Int = 22, alias: String? = nil, files: [String]) -> SSHHostKeys.Target {
        SSHHostKeys.Target(hostname: "gw.example", port: port, hostKeyAlias: alias,
                           userKnownHostsFiles: files, globalKnownHostsFiles: [])
    }

    @Test func parsesSSHConfigOutput() throws {
        let target = try SSHHostKeys.parseConfig(Self.config, home: "/Users/me")
        #expect(target.hostname == "10.1.2.3")
        #expect(target.port == 2222)
        #expect(target.hostKeyAlias == nil)
        #expect(target.userKnownHostsFiles == ["/Users/me/.ssh/known_hosts", "/Users/me/.ssh/known_hosts2"])
        #expect(target.globalKnownHostsFiles == ["/etc/ssh/ssh_known_hosts", "/etc/ssh/ssh_known_hosts2"])
        #expect(target.lookupName == "[10.1.2.3]:2222")

        let aliased = try SSHHostKeys.parseConfig(Self.config + "\nhostkeyalias office-gw", home: "/Users/me")
        #expect(aliased.lookupName == "office-gw")
    }

    @Test func unreadableConfigThrows() {
        #expect(throws: SSHHostKeyError.configUnreadable) { try SSHHostKeys.parseConfig("port 22") }
        #expect(SSHHostKeyError.configUnreadable.errorDescription?.isEmpty == false)
    }

    @Test func targetRunsSSHConfigQuery() throws {
        let keys = SSHHostKeys { executable, arguments, _ in
            #expect(executable == "/usr/bin/ssh")
            #expect(arguments == ["-G", "-p", "2222", "-l", "admin", "gw.example"])
            return (0, Data(Self.config.utf8))
        }
        let connection = Connection(type: .ssh, name: "gw", host: " gw.example", port: 2222, username: "admin")
        #expect(try keys.target(for: connection).hostname == "10.1.2.3")
        let failing = SSHHostKeys { _, _, _ in (255, Data()) }
        #expect(throws: SSHHostKeyError.configUnreadable) { try failing.target(for: connection) }
    }

    @Test func checksKnownHostsWithSSHKeygen() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appending(path: "known_hosts")
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl"
        try "gw.example \(key)\n[gw.example]:2222 \(key)\n".write(to: file, atomically: true, encoding: .utf8)
        let keys = SSHHostKeys()

        #expect(try keys.isKnown(target(files: [file.path])))
        #expect(try keys.isKnown(target(port: 2222, files: [file.path])))
        #expect(try !keys.isKnown(target(port: 2200, files: [file.path])))
        #expect(try !keys.isKnown(target(alias: "other", files: [file.path])))
        #expect(try !keys.isKnown(target(files: [directory.appending(path: "missing").path])))
    }

    @Test func scanAndFingerprints() throws {
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl"
        let scanner = SSHHostKeys { executable, arguments, input in
            if executable.hasSuffix("ssh-keyscan") {
                #expect(arguments == ["-T", "10", "-p", "2222", "gw.example"])
                return (0, Data("# gw.example:2222 SSH-2.0-OpenSSH\n[gw.example]:2222 \(key)\n".utf8))
            }
            return try SSHHostKeys.runProcess(executable, arguments, input)
        }
        let lines = try scanner.scan(target(port: 2222, files: []))
        #expect(lines == ["[gw.example]:2222 \(key)"])
        let fingerprints = try scanner.fingerprints(of: lines)
        #expect(fingerprints.count == 1)
        #expect(fingerprints[0].hasPrefix("ED25519 SHA256:"))

        let silent = SSHHostKeys { _, _, _ in (1, Data()) }
        #expect(throws: SSHHostKeyError.unreachable) { try silent.scan(target(files: [])) }
        #expect(SSHHostKeyError.unreachable.errorDescription?.isEmpty == false)
    }

    @Test func trustAppendsUnderTheLookupName() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appending(path: "ssh/known_hosts")
        let keys = SSHHostKeys()

        try keys.trust(["10.1.2.3 ssh-ed25519 AAAA1"], for: target(port: 2222, files: [file.path]))
        #expect(try String(contentsOf: file, encoding: .utf8) == "[gw.example]:2222 ssh-ed25519 AAAA1\n")
        let mode = try FileManager.default.attributesOfItem(atPath: file.path)[.posixPermissions] as? Int
        #expect(mode == 0o600)

        // Existing file without a trailing newline keeps its entries.
        try "old ssh-rsa BBBB".write(to: file, atomically: true, encoding: .utf8)
        try keys.trust(["x ssh-ed25519 AAAA2"], for: target(alias: "office-gw", files: [file.path]))
        #expect(try String(contentsOf: file, encoding: .utf8) == "old ssh-rsa BBBB\noffice-gw ssh-ed25519 AAAA2\n")

        #expect(throws: SSHHostKeyError.configUnreadable) { try keys.trust(["x"], for: target(files: [])) }
    }
}

@Suite struct LocalPortTests {
    @Test func detectsAListener() throws {
        let server = socket(AF_INET, SOCK_STREAM, 0)
        defer { close(server) }
        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = 0 // any free port
        address.sin_addr.s_addr = inet_addr("127.0.0.1")
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        try withUnsafeMutablePointer(to: &address) { pointer in
            try pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { socketAddress in
                try #require(bind(server, socketAddress, length) == 0)
                try #require(listen(server, 1) == 0)
                try #require(getsockname(server, socketAddress, &length) == 0)
            }
        }
        let port = Int(UInt16(bigEndian: address.sin_port))
        #expect(LocalPort.isListening(port))
        close(server)
        #expect(!LocalPort.isListening(port))
    }

    @Test(arguments: [0, -5, 70000])
    func invalidPortIsNotListening(port: Int) {
        #expect(!LocalPort.isListening(port))
    }
}
