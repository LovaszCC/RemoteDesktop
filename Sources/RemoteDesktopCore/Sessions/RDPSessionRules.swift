import Foundation

/// Plain values for one embedded RDP session, derived from a (credential-resolved) connection.
public struct RDPSessionSettings: Equatable, Sendable {
    public var host: String
    public var port: Int
    public var username: String
    public var domain: String
    public var adminSession: Bool

    public init(connection: Connection) {
        host = connection.host.trimmingCharacters(in: .whitespaces)
        port = connection.effectivePort
        adminSession = connection.rdpAdminSession
        let user = connection.username.trimmingCharacters(in: .whitespaces)
        let domain = connection.domain.trimmingCharacters(in: .whitespaces)
        // "DOMAIN\user" and "user@domain" typed into the user field win over the domain field.
        if let backslash = user.firstIndex(of: "\\") {
            self.domain = String(user[..<backslash])
            username = String(user[user.index(after: backslash)...])
        } else {
            self.domain = user.contains("@") ? "" : domain
            username = user
        }
    }
}

/// How an RDP session ended, from FreeRDP's last error and the server's error info.
public enum RDPDisconnectReason: Equatable, Sendable {
    /// User logged off or disconnected from inside the session; the tab can close.
    case clean
    /// Connection failed or dropped; the tab stays with a reconnect option.
    case failure

    // [MS-RDPBCGR] 2.2.5.1.1 error info codes for user-initiated endings.
    static let cleanErrorInfo: Set<UInt32> = [
        0x0000_0001, // RPC initiated disconnect
        0x0000_0002, // RPC initiated logoff
        0x0000_000B, // disconnect by user
        0x0000_000C, // logoff by user
    ]

    public init(lastError: UInt32, errorInfo: UInt32) {
        if Self.cleanErrorInfo.contains(errorInfo) || (errorInfo == 0 && lastError == 0) {
            self = .clean
        } else {
            self = .failure
        }
    }
}
