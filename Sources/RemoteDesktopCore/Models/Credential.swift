import Foundation
import GRDB

/// Reusable login that connections can link to instead of storing their own.
/// RDP uses username/domain/password; SSH uses username/password/key/passphrase.
public struct Credential: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var name: String
    public var username: String
    public var domain: String
    public var passwordEnc: Data?
    public var sshKeyPath: String?
    public var sshKeyPassphraseEnc: Data?
    public var notes: String
    public var createdAt: Date
    public var updatedAt: Date

    public init(
        id: String = UUID().uuidString,
        name: String,
        username: String = "",
        domain: String = "",
        passwordEnc: Data? = nil,
        sshKeyPath: String? = nil,
        sshKeyPassphraseEnc: Data? = nil,
        notes: String = "",
        createdAt: Date = .now,
        updatedAt: Date = .now
    ) {
        self.id = id
        self.name = name
        self.username = username
        self.domain = domain
        self.passwordEnc = passwordEnc
        self.sshKeyPath = sshKeyPath
        self.sshKeyPassphraseEnc = sshKeyPassphraseEnc
        self.notes = notes
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    /// "DOMAIN\user" style label for lists.
    public var displayUser: String {
        if username.isEmpty { return "" }
        return domain.isEmpty ? username : "\(domain)\\\(username)"
    }
}

extension Credential: FetchableRecord, PersistableRecord {
    public static let databaseTableName = "credential"
}

extension Connection {
    /// The connection as it should be launched: a linked credential replaces every auth field.
    public func resolved(with credential: Credential?) -> Connection {
        guard let credential else { return self }
        var copy = self
        copy.username = credential.username
        copy.domain = credential.domain
        copy.passwordEnc = credential.passwordEnc
        copy.sshKeyPath = credential.sshKeyPath
        copy.sshKeyPassphraseEnc = credential.sshKeyPassphraseEnc
        return copy
    }
}
