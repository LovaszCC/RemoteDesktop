import Foundation
import GRDB

public enum ConnectionType: String, Codable, CaseIterable, Sendable {
    case rdp
    case ssh

    public var defaultPort: Int {
        switch self {
        case .rdp: 3389
        case .ssh: 22
        }
    }

    public var displayName: String {
        switch self {
        case .rdp: "RDP"
        case .ssh: "SSH"
        }
    }
}

/// A saved connection. Secret fields (`passwordEnc`, `sshKeyPassphraseEnc`) hold AES-GCM
/// sealed boxes produced by `Vault`; everything else is stored in clear text so listing
/// and searching never needs the master key.
public struct Connection: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var folderId: String?
    public var type: ConnectionType
    public var name: String
    public var host: String
    public var port: Int?
    public var username: String
    public var domain: String
    public var passwordEnc: Data?
    public var sshKeyPath: String?
    public var sshKeyPassphraseEnc: Data?
    public var isFavorite: Bool
    public var notes: String
    public var createdAt: Date
    public var updatedAt: Date

    public init(
        id: String = UUID().uuidString,
        folderId: String? = nil,
        type: ConnectionType,
        name: String,
        host: String,
        port: Int? = nil,
        username: String = "",
        domain: String = "",
        passwordEnc: Data? = nil,
        sshKeyPath: String? = nil,
        sshKeyPassphraseEnc: Data? = nil,
        isFavorite: Bool = false,
        notes: String = "",
        createdAt: Date = .now,
        updatedAt: Date = .now
    ) {
        self.id = id
        self.folderId = folderId
        self.type = type
        self.name = name
        self.host = host
        self.port = port
        self.username = username
        self.domain = domain
        self.passwordEnc = passwordEnc
        self.sshKeyPath = sshKeyPath
        self.sshKeyPassphraseEnc = sshKeyPassphraseEnc
        self.isFavorite = isFavorite
        self.notes = notes
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    public var effectivePort: Int { port ?? type.defaultPort }
}

extension Connection: FetchableRecord, PersistableRecord {
    public static let databaseTableName = "connection"
}
