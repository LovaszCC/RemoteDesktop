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

/// Turns text into keystrokes for "type clipboard": UTF-16 units as Unicode key events, with line
/// breaks and tabs sent as real Enter / Tab keys so terminals and forms react to them.
public enum RDPTyping {
    /// Bridge encoding: values >= 0 are UTF-16 code units, the negative ones are special keys.
    public static let enter: Int32 = -1
    public static let tab: Int32 = -2
    /// Above this length the UI asks before typing (~2 ms per keystroke).
    public static let confirmationThreshold = 2000

    public static func keystrokes(for text: String) -> [Int32] {
        var result: [Int32] = []
        var previousWasCR = false
        for unit in text.utf16 {
            switch unit {
            case 0x0D:
                result.append(enter)
                previousWasCR = true
                continue
            case 0x0A:
                if !previousWasCR { result.append(enter) }
            case 0x09:
                result.append(tab)
            default:
                result.append(Int32(unit))
            }
            previousWasCR = false
        }
        return result
    }
}
