import Foundation
import GRDB

/// A saved local port forward through an SSH connection: `127.0.0.1:localPort` → `remoteHost:remotePort`
/// as seen from the SSH server (`ssh -L`).
public struct PortForward: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var connectionId: String
    public var name: String
    public var localPort: Int
    public var remoteHost: String
    public var remotePort: Int
    public var createdAt: Date

    public init(
        id: String = UUID().uuidString,
        connectionId: String,
        name: String = "",
        localPort: Int,
        remoteHost: String,
        remotePort: Int,
        createdAt: Date = .now
    ) {
        self.id = id
        self.connectionId = connectionId
        self.name = name
        self.localPort = localPort
        self.remoteHost = remoteHost
        self.remotePort = remotePort
        self.createdAt = createdAt
    }

    /// The name, or the destination when unnamed.
    public var displayName: String {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        return trimmed.isEmpty ? destination : trimmed
    }

    /// `host:port`, IPv6 hosts bracketed.
    public var destination: String {
        let host = remoteHost.trimmingCharacters(in: .whitespaces)
        return host.contains(":") ? "[\(host)]:\(remotePort)" : "\(host):\(remotePort)"
    }

    static let validPorts = 1...65535

    /// Why this forward can't be saved next to `others` (the connection's saved forwards), or `nil`.
    public func validationError(among others: [PortForward]) -> PortForwardError? {
        guard Self.validPorts.contains(localPort) else { return .invalidLocalPort }
        guard !remoteHost.trimmingCharacters(in: .whitespaces).isEmpty else { return .missingRemoteHost }
        guard Self.validPorts.contains(remotePort) else { return .invalidRemotePort }
        let clash = others.contains { $0.id != id && $0.connectionId == connectionId && $0.localPort == localPort }
        return clash ? .duplicateLocalPort : nil
    }
}

extension PortForward: FetchableRecord, PersistableRecord {
    public static let databaseTableName = "portForward"
}

public enum PortForwardError: Error, Equatable, LocalizedError {
    case invalidLocalPort
    case missingRemoteHost
    case invalidRemotePort
    case duplicateLocalPort

    public var errorDescription: String? {
        switch self {
        case .invalidLocalPort: String(localized: "The local port must be between 1 and 65535.", bundle: .core)
        case .missingRemoteHost: String(localized: "Enter the remote host.", bundle: .core)
        case .invalidRemotePort: String(localized: "The remote port must be between 1 and 65535.", bundle: .core)
        case .duplicateLocalPort: String(localized: "Another forward of this connection uses this local port.", bundle: .core)
        }
    }
}
