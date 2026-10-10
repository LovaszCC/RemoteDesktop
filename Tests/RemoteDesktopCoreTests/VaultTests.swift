import Foundation
import Testing
@testable import RemoteDesktopCore

@Suite struct VaultTests {
    let repo: ConnectionRepository
    let iterations = 1_000

    init() throws {
        repo = ConnectionRepository(database: try .inMemory())
    }

    @Test func createThenUnlockWithSamePassword() throws {
        #expect(try !Vault.isInitialized(repo))
        let created = try Vault.create(password: "s3cret", in: repo, iterations: iterations)
        #expect(try Vault.isInitialized(repo))

        let unlocked = try Vault.unlock(password: "s3cret", in: repo)
        #expect(unlocked.keyData == created.keyData)
    }

    @Test func wrongPasswordIsRejected() throws {
        _ = try Vault.create(password: "right", in: repo, iterations: iterations)
        #expect(throws: VaultError.wrongPassword) { try Vault.unlock(password: "wrong", in: repo) }
    }

    @Test func unlockBeforeCreateFails() {
        #expect(throws: VaultError.notInitialized) { try Vault.unlock(password: "x", in: repo) }
        #expect(throws: VaultError.notInitialized) { try Vault.unlock(keyData: Data(count: 32), in: repo) }
    }

    @Test func createTwiceAndEmptyPasswordFail() throws {
        #expect(throws: VaultError.emptyPassword) { try Vault.create(password: "", in: repo, iterations: iterations) }
        _ = try Vault.create(password: "a", in: repo, iterations: iterations)
        #expect(throws: VaultError.alreadyInitialized) { try Vault.create(password: "b", in: repo, iterations: iterations) }
    }

    @Test func unlockWithCachedKey() throws {
        let vault = try Vault.create(password: "pw", in: repo, iterations: iterations)
        let fromKey = try Vault.unlock(keyData: vault.keyData, in: repo)
        #expect(try fromKey.decrypt(vault.encrypt("hello")) == "hello")
        #expect(throws: VaultError.wrongPassword) { try Vault.unlock(keyData: Data(count: 32), in: repo) }
    }

    @Test func encryptDecryptRoundtripUsesFreshNonces() throws {
        let vault = try Vault.create(password: "pw", in: repo, iterations: iterations)
        let a = try vault.encrypt("P@ssw0rd áé")
        let b = try vault.encrypt("P@ssw0rd áé")
        #expect(a != b)
        #expect(try vault.decrypt(a) == "P@ssw0rd áé")
    }

    @Test func optionalHelpersTreatEmptyAsNil() throws {
        let vault = try Vault.create(password: "pw", in: repo, iterations: iterations)
        #expect(try vault.encryptOptional(nil) == nil)
        #expect(try vault.encryptOptional("") == nil)
        #expect(try vault.decryptOptional(nil) == nil)
        #expect(try vault.decryptOptional(vault.encryptOptional("x")) == "x")
    }

    @Test func tamperedDataThrows() throws {
        let vault = try Vault.create(password: "pw", in: repo, iterations: iterations)
        var data = try vault.encrypt("x")
        data[data.count - 1] ^= 0xFF
        #expect(throws: VaultError.corruptData) { try vault.decrypt(data) }
        #expect(throws: VaultError.corruptData) { try vault.decrypt(Data([1, 2, 3])) }
    }

    @Test func keyDerivationIsDeterministic() throws {
        let salt = Data(repeating: 7, count: 16)
        let a = try Vault.deriveKey(password: "pw", salt: salt, iterations: 10)
        let b = try Vault.deriveKey(password: "pw", salt: salt, iterations: 10)
        let c = try Vault.deriveKey(password: "pw", salt: salt, iterations: 11)
        #expect(a == b)
        #expect(a != c)
    }

    @Test func changePasswordReencryptsSecrets() throws {
        let vault = try Vault.create(password: "old", in: repo, iterations: iterations)
        let connection = try repo.save(Connection(
            type: .ssh, name: "s", host: "h",
            passwordEnc: vault.encrypt("pw1"),
            sshKeyPassphraseEnc: vault.encrypt("pp1")
        ))
        try repo.save(Connection(type: .rdp, name: "nopw", host: "h"))

        let newVault = try vault.changePassword(to: "new", in: repo, iterations: iterations)

        #expect(throws: VaultError.wrongPassword) { try Vault.unlock(password: "old", in: repo) }
        let unlocked = try Vault.unlock(password: "new", in: repo)
        #expect(unlocked.keyData == newVault.keyData)

        let loaded = try #require(try repo.connection(id: connection.id))
        #expect(try unlocked.decryptOptional(loaded.passwordEnc) == "pw1")
        #expect(try unlocked.decryptOptional(loaded.sshKeyPassphraseEnc) == "pp1")
        #expect(throws: VaultError.emptyPassword) { try newVault.changePassword(to: "", in: repo) }
    }
}

@Suite struct KeychainStoreTests {
    @Test func saveLoadDelete() throws {
        let store = KeychainStore(service: "hu.softworks.RemoteDesktop.tests.\(UUID().uuidString)")
        let account = "/tmp/test.sqlite"
        defer { store.delete(account: account) }

        #expect(store.load(account: account) == nil)
        try store.save(Data([1, 2, 3]), account: account)
        try store.save(Data([4, 5]), account: account)
        #expect(store.load(account: account) == Data([4, 5]))
        #expect(store.delete(account: account))
        #expect(store.load(account: account) == nil)
    }
}
