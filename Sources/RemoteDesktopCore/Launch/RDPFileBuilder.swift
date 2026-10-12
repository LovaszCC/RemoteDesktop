import Foundation

/// Builds `.rdp` files understood by Microsoft's Windows App.
///
/// The password is deliberately not written: on macOS Windows App ignores `password 51:b:`
/// (DPAPI blobs only work on Windows), so it is typed in by `RDPLauncher` instead.
public enum RDPFileBuilder {
    public static func contents(for connection: Connection) -> String {
        var lines = [
            "full address:s:\(address(host: connection.host, port: connection.effectivePort))",
            "screen mode id:i:2",
            "use multimon:i:0",
            "dynamic resolution:i:1",
            "session bpp:i:32",
            "prompt for credentials:i:0",
            "authentication level:i:2",
        ]
        let user = qualifiedUsername(username: connection.username, domain: connection.domain)
        if !user.isEmpty {
            lines.append("username:s:\(user)")
        }
        if connection.rdpAdminSession {
            lines.append("administrative session:i:1")
        }
        return lines.joined(separator: "\r\n") + "\r\n"
    }

    /// `DOMAIN\user`, unless the user name already carries a domain (`user@domain`, `DOMAIN\user`).
    public static func qualifiedUsername(username: String, domain: String) -> String {
        let username = username.trimmingCharacters(in: .whitespaces)
        let domain = domain.trimmingCharacters(in: .whitespaces)
        guard !username.isEmpty else { return "" }
        if domain.isEmpty || username.contains("\\") || username.contains("@") { return username }
        return "\(domain)\\\(username)"
    }

    static func address(host: String, port: Int) -> String {
        let host = host.trimmingCharacters(in: .whitespaces)
        let bracketed = host.contains(":") && !host.hasPrefix("[") ? "[\(host)]" : host
        return port == ConnectionType.rdp.defaultPort ? bracketed : "\(bracketed):\(port)"
    }

    /// Safe file name derived from the connection name.
    public static func fileName(for connection: Connection) -> String {
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_. "))
        let cleaned = String(connection.name.unicodeScalars.map { allowed.contains($0) ? Character($0) : "_" })
            .trimmingCharacters(in: .whitespaces)
        return (cleaned.isEmpty ? "connection" : cleaned) + ".rdp"
    }
}
