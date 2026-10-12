import Foundation
import GRDB

public enum ConnectionType: String, Codable, CaseIterable, Sendable {
    case rdp
    case ssh
    case web

    public var defaultPort: Int {
        switch self {
        case .rdp: 3389
        case .ssh: 22
        case .web: 443
        }
    }

    public var displayName: String {
        switch self {
        case .rdp: "RDP"
        case .ssh: "SSH"
        case .web: "Böngésző"
        }
    }

    /// Menu title for creating a connection of this type.
    public var newTitle: String {
        switch self {
        case .rdp: "Új RDP kapcsolat"
        case .ssh: "Új SSH kapcsolat"
        case .web: "Új böngésző kapcsolat"
        }
    }

    /// SF Symbol shown in the sidebar and menus.
    public var systemImage: String {
        switch self {
        case .rdp: "display"
        case .ssh: "apple.terminal"
        case .web: "globe"
        }
    }

    /// Whether the connection has a host + port (as opposed to a URL).
    public var usesPort: Bool { self != .web }
}

/// A saved connection. Secret fields (`passwordEnc`, `sshKeyPassphraseEnc`) hold AES-GCM
/// sealed boxes produced by `Vault`; everything else is stored in clear text so listing
/// and searching never needs the master key.
public struct Connection: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var folderId: String?
    public var credentialId: String?
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
    /// RDP only: connect to the administrative session (`mstsc /admin`).
    public var rdpAdminSession: Bool
    /// Web only: browser to open the URL with; `nil` means the system default browser.
    public var browserBundleId: String?
    public var notes: String
    public var createdAt: Date
    public var updatedAt: Date

    public init(
        id: String = UUID().uuidString,
        folderId: String? = nil,
        credentialId: String? = nil,
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
        rdpAdminSession: Bool = false,
        browserBundleId: String? = nil,
        notes: String = "",
        createdAt: Date = .now,
        updatedAt: Date = .now
    ) {
        self.id = id
        self.folderId = folderId
        self.credentialId = credentialId
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
        self.rdpAdminSession = rdpAdminSession
        self.browserBundleId = browserBundleId
        self.notes = notes
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    public var effectivePort: Int { port ?? type.defaultPort }
}

extension Connection: FetchableRecord, PersistableRecord {
    public static let databaseTableName = "connection"
}
