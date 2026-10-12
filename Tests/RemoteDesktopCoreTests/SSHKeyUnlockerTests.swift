import Foundation
import GRDB
import Testing
@testable import RemoteDesktopCore

@Suite struct SSHKeyUnlockerTests {
    let directory = FileManager.default.temporaryDirectory.appending(path: "rd-unlock-tests-\(UUID().uuidString)")

    /// Generates an ed25519 key protected by `passphrase` and returns its path.
    func makeKey(passphrase: String) throws -> String {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let path = directory.appending(path: "id_test").path
        let process = Process()
        process.executableURL = URL(filePath: "/usr/bin/ssh-keygen")
        process.arguments = ["-q", "-t", "ed25519", "-N", passphrase, "-C", "test", "-f", path]
        process.standardOutput = FileHandle.nullDevice
        try process.run()
        process.waitUntilExit()
        #expect(process.terminationStatus == 0)
        return path
    }

    @Test func unlocksWithCorrectPassphraseWithoutTouchingOriginal() throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        let path = try makeKey(passphrase: "s3cr'et pass")
        let original = try String(contentsOfFile: path, encoding: .utf8)

        let unlocked = try SSHKeyUnlocker.unlock(keyPath: path, passphrase: "s3cr'et pass")

        #expect(unlocked.contains("BEGIN OPENSSH PRIVATE KEY"))
        #expect(unlocked != original)
        #expect(try String(contentsOfFile: path, encoding: .utf8) == original)

        // The unlocked key loads with an empty passphrase.
        let check = directory.appending(path: "unlocked")
        FileManager.default.createFile(atPath: check.path, contents: Data(unlocked.utf8), attributes: [.posixPermissions: 0o600])
        let process = Process()
        process.executableURL = URL(filePath: "/usr/bin/ssh-keygen")
        process.arguments = ["-y", "-P", "", "-f", check.path]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        process.waitUntilExit()
        #expect(process.terminationStatus == 0)
    }

    @Test func wrongPassphrase() throws {
        defer { try? FileManager.default.removeItem(at: directory) }
        let path = try makeKey(passphrase: "right")
        #expect(throws: SSHKeyUnlockError.wrongPassphrase) {
            try SSHKeyUnlocker.unlock(keyPath: path, passphrase: "wrong")
        }
    }

    @Test func missingFile() {
        #expect(throws: SSHKeyUnlockError.fileUnreadable("/nonexistent/id_x")) {
            try SSHKeyUnlocker.unlock(keyPath: "/nonexistent/id_x", passphrase: "pp")
        }
    }
}

@Suite struct UnlockedKeyStorageTests {
    let root = FileManager.default.temporaryDirectory.appending(path: "rd-unlocked-launch-\(UUID().uuidString)")

    @Test func roundtripsAndDefaultsNilAfterMigration() throws {
        let queue = try DatabaseQueue()
        try AppDatabase.migrator.migrate(queue, upTo: "v3-rdp-admin")
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO credential (id, name, username, domain, notes, createdAt, updatedAt)
                VALUES ('old', 'legacy', '', '', '', '2025-01-01 00:00:00', '2025-01-01 00:00:00')
                """)
        }
        let repo = ConnectionRepository(database: try AppDatabase(queue))
        #expect(try repo.credential(id: "old")?.sshKeyUnlockedEnc == nil)

        let saved = try repo.save(Credential(name: "k", sshKeyUnlockedEnc: Data([1, 2, 3])))
        #expect(try repo.credential(id: saved.id)?.sshKeyUnlockedEnc == Data([1, 2, 3]))
    }

    @Test func changePasswordReencryptsUnlockedKey() throws {
        let repo = ConnectionRepository(database: try .inMemory())
        let vault = try Vault.create(password: "old", in: repo, iterations: 1_000)
        let credential = try repo.save(Credential(name: "c", sshKeyUnlockedEnc: vault.encrypt("KEY")))

        let newVault = try vault.changePassword(to: "new", in: repo, iterations: 1_000)

        let loaded = try #require(try repo.credential(id: credential.id))
        #expect(try newVault.decryptOptional(loaded.sshKeyUnlockedEnc) == "KEY")
    }

    @Test func launchUsesPrivateUnlockedKeyFile() throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let connection = Connection(type: .ssh, name: "w", host: "h", username: "u", sshKeyPath: "~/.ssh/id_original")
        let script = SSHLaunchScript(connection: connection, password: nil, keyPassphrase: "pp", unlockedKey: "KEY")
        #expect(script.keyPassphrase == nil)
        #expect(!script.usesAskpass)

        let files = try script.write(under: root)
        let key = files.directory.appending(path: "key")
        let fm = FileManager.default
        #expect(try fm.attributesOfItem(atPath: key.path)[.posixPermissions] as? Int == 0o600)
        #expect(try String(contentsOf: key, encoding: .utf8) == "KEY\n")
        #expect(!fm.fileExists(atPath: files.directory.appending(path: "passphrase").path))

        let text = try String(contentsOf: files.launchScript, encoding: .utf8)
        #expect(text.contains("'-i' \(SSHLaunchScript.quote(key.path)) '-o' 'IdentitiesOnly=yes'"))
        #expect(!text.contains("id_original"))
    }

    @Test func withoutUnlockedKeyUsesOriginalPath() {
        let connection = Connection(type: .ssh, name: "w", host: "h", sshKeyPath: "/k/id")
        let script = SSHLaunchScript(connection: connection, password: nil, keyPassphrase: "pp", unlockedKey: "")
        #expect(script.keyPassphrase == "pp")
        #expect(script.sshArguments == ["ssh", "-i", "/k/id", "h"])
    }
}
