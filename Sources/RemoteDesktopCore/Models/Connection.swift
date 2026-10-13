import Foundation
import GRDB

public enum ConnectionType: String, Codable, CaseIterable, Sendable {
    case rdp
    case ssh
    case web
    case mikrotik

    public var defaultPort: Int {
        switch self {
        case .rdp: 3389
        case .ssh: 22
        case .web: 443
        case .mikrotik: 22
        }
    }

    public var displayName: String {
        switch self {
        case .rdp: "RDP"
        case .ssh: "SSH"
        case .web: String(localized: "Web", bundle: .core)
        case .mikrotik: "MikroTik"
        }
    }

    /// Menu title for creating a connection of this type.
    public var newTitle: String {
        switch self {
        case .rdp: String(localized: "New RDP connection", bundle: .core)
        case .ssh: String(localized: "New SSH connection", bundle: .core)
        case .web: String(localized: "New web connection", bundle: .core)
        case .mikrotik: String(localized: "New MikroTik connection", bundle: .core)
        }
    }

    /// SF Symbol shown in the sidebar and menus.
    public var systemImage: String {
        switch self {
        case .rdp: "display"
        case .ssh: "apple.terminal"
        case .web: "globe"
        case .mikrotik: "network"
        }
    }

    /// Whether the connection has a host + port (as opposed to a URL).
    public var usesPort: Bool { self != .web }

    /// Whether the connection can be opened over SSH (and so has SSH key settings).
    public var usesSSH: Bool { self == .ssh || self == .mikrotik }
}

/// Where an SSH session runs.
public enum SSHTerminal: String, Codable, CaseIterable, Sendable {
    /// Tab inside the main window.
    case embedded
    /// Separate window in the external terminal chosen in Settings (`ExternalTerminal`).
    case external

    public var displayName: String {
        switch self {
        case .embedded: String(localized: "Embedded", bundle: .core)
        case .external: String(localized: "External terminal", bundle: .core)
        }
    }
}

/// Which client opens an RDP connection.
public enum RDPClient: String, Codable, CaseIterable, Sendable {
    /// FreeRDP tab inside the main window.
    case embedded
    /// Microsoft Windows App with a generated `.rdp` file.
    case windowsApp

    public var displayName: String {
        switch self {
        case .embedded: String(localized: "Embedded (FreeRDP)", bundle: .core)
        case .windowsApp: "Windows App"
        }
    }
}

/// How a MikroTik connection is opened.
public enum MikrotikMode: String, Codable, CaseIterable, Sendable {
    case winbox
    case ssh

    public var displayName: String {
        switch self {
        case .winbox: "WinBox"
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
    /// MikroTik only: mode used by double-click / ⌘↩.
    public var mikrotikMode: MikrotikMode
    /// SSH / MikroTik SSH: embedded tab or external terminal window.
    public var sshTerminal: SSHTerminal
    /// RDP: embedded FreeRDP tab or Windows App.
    public var rdpClient: RDPClient
    /// Tag color, also the default color of the connection's session tabs.
    public var color: LabelColor?
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
        mikrotikMode: MikrotikMode = .winbox,
        sshTerminal: SSHTerminal = .embedded,
        rdpClient: RDPClient = .embedded,
        color: LabelColor? = nil,
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
        self.mikrotikMode = mikrotikMode
        self.sshTerminal = sshTerminal
        self.rdpClient = rdpClient
        self.color = color
        self.notes = notes
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    public var effectivePort: Int { port ?? type.defaultPort }
}

extension Connection: FetchableRecord, PersistableRecord {
    public static let databaseTableName = "connection"
}
