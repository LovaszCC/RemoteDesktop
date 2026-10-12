import CommonCrypto
import CryptoKit
import Foundation
import GRDB

public enum VaultError: Error, Equatable {
    case notInitialized
    case alreadyInitialized
    case wrongPassword
    case emptyPassword
    case keyDerivationFailed
    case corruptData
}

/// Encrypts connection secrets with a key derived from the master password.
///
/// The salt, iteration count and a sealed verifier constant live in the database `meta`
/// table, so a database file copied to another Mac can be unlocked with the same password.
public struct Vault: Sendable {
    static let saltKey = "kdf_salt"
    static let iterationsKey = "kdf_iterations"
    static let verifierKey = "verifier"
    static let verifierPlaintext = Data("remotedesktop-vault-v1".utf8)
    public static let defaultIterations = 600_000

    let key: SymmetricKey

    init(key: SymmetricKey) {
        self.key = key
    }

    /// Raw key bytes, for caching in the Keychain ("remember on this Mac").
    public var keyData: Data {
        key.withUnsafeBytes { Data($0) }
    }

    // MARK: Lifecycle

    public static func isInitialized(_ repo: ConnectionRepository) throws -> Bool {
        try repo.meta(verifierKey) != nil
    }

    /// Creates a new vault for an empty database.
    public static func create(
        password: String,
        in repo: ConnectionRepository,
        iterations: Int = defaultIterations
    ) throws -> Vault {
        guard !password.isEmpty else { throw VaultError.emptyPassword }
        guard try !isInitialized(repo) else { throw VaultError.alreadyInitialized }

        let salt = randomBytes(16)
        let vault = Vault(key: try deriveKey(password: password, salt: salt, iterations: iterations))
        try repo.setMeta(saltKey, salt)
        try repo.setMeta(iterationsKey, withUnsafeBytes(of: UInt32(iterations).bigEndian) { Data($0) })
        try repo.setMeta(verifierKey, try vault.seal(verifierPlaintext))
        return vault
    }

    public static func unlock(password: String, in repo: ConnectionRepository) throws -> Vault {
        guard let salt = try repo.meta(saltKey),
              let iterationsData = try repo.meta(iterationsKey), iterationsData.count == 4
        else { throw VaultError.notInitialized }

        let iterations = iterationsData.reduce(0) { ($0 << 8) | Int($1) }
        let vault = Vault(key: try deriveKey(password: password, salt: salt, iterations: iterations))
        try vault.verify(in: repo)
        return vault
    }

    /// Unlocks with a previously cached raw key (from the Keychain).
    public static func unlock(keyData: Data, in repo: ConnectionRepository) throws -> Vault {
        let vault = Vault(key: SymmetricKey(data: keyData))
        try vault.verify(in: repo)
        return vault
    }

    /// Re-derives the key from `newPassword` and re-encrypts every stored secret atomically.
    public func changePassword(
        to newPassword: String,
        in repo: ConnectionRepository,
        iterations: Int = defaultIterations
    ) throws -> Vault {
        guard !newPassword.isEmpty else { throw VaultError.emptyPassword }
        let salt = Self.randomBytes(16)
        let newVault = Vault(key: try Self.deriveKey(password: newPassword, salt: salt, iterations: iterations))
        let old = self

        try repo.transaction { db in
            for var connection in try Connection.fetchAll(db) {
                connection.passwordEnc = try connection.passwordEnc.map { try newVault.seal(old.open($0)) }
                connection.sshKeyPassphraseEnc = try connection.sshKeyPassphraseEnc.map { try newVault.seal(old.open($0)) }
                try connection.update(db)
            }
            let upsert = "INSERT INTO meta(key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value"
            try db.execute(sql: upsert, arguments: [Self.saltKey, salt])
            try db.execute(sql: upsert, arguments: [Self.iterationsKey, withUnsafeBytes(of: UInt32(iterations).bigEndian) { Data($0) }])
            try db.execute(sql: upsert, arguments: [Self.verifierKey, try newVault.seal(Self.verifierPlaintext)])
        }
        return newVault
    }

    // MARK: Encryption

    public func encrypt(_ secret: String) throws -> Data {
        try seal(Data(secret.utf8))
    }

    /// `nil` / empty strings are stored as `nil` so "no password" stays distinguishable.
    public func encryptOptional(_ secret: String?) throws -> Data? {
        guard let secret, !secret.isEmpty else { return nil }
        return try encrypt(secret)
    }

    public func decrypt(_ data: Data) throws -> String {
        guard let string = String(data: try open(data), encoding: .utf8) else { throw VaultError.corruptData }
        return string
    }

    public func decryptOptional(_ data: Data?) throws -> String? {
        try data.map(decrypt)
    }

    func seal(_ plaintext: Data) throws -> Data {
        guard let combined = try AES.GCM.seal(plaintext, using: key).combined else { throw VaultError.corruptData }
        return combined
    }

    func open(_ data: Data) throws -> Data {
        do {
            return try AES.GCM.open(AES.GCM.SealedBox(combined: data), using: key)
        } catch {
            throw VaultError.corruptData
        }
    }

    private func verify(in repo: ConnectionRepository) throws {
        guard let verifier = try repo.meta(Self.verifierKey) else { throw VaultError.notInitialized }
        guard (try? open(verifier)) == Self.verifierPlaintext else { throw VaultError.wrongPassword }
    }

    // MARK: Helpers

    static func deriveKey(password: String, salt: Data, iterations: Int) throws -> SymmetricKey {
        let passwordData = Data(password.utf8)
        var derived = Data(count: 32)
        let status = derived.withUnsafeMutableBytes { derivedBytes in
            salt.withUnsafeBytes { saltBytes in
                passwordData.withUnsafeBytes { passwordBytes in
                    CCKeyDerivationPBKDF(
                        CCPBKDFAlgorithm(kCCPBKDF2),
                        passwordBytes.baseAddress?.assumingMemoryBound(to: CChar.self), passwordData.count,
                        saltBytes.baseAddress?.assumingMemoryBound(to: UInt8.self), salt.count,
                        CCPseudoRandomAlgorithm(kCCPRFHmacAlgSHA256), UInt32(iterations),
                        derivedBytes.baseAddress?.assumingMemoryBound(to: UInt8.self), 32
                    )
                }
            }
        }
        guard status == kCCSuccess else { throw VaultError.keyDerivationFailed }
        return SymmetricKey(data: derived)
    }

    static func randomBytes(_ count: Int) -> Data {
        Data(SymmetricKey(size: .init(bitCount: count * 8)).withUnsafeBytes { Array($0) })
    }
}
