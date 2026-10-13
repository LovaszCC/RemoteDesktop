import Foundation

/// `ssh` / `scp` command lines that reuse a file browser's connection master (see `SSHLaunchScript.controlPath`),
/// so they neither authenticate nor prompt.
public struct RemoteCommand: Sendable {
    /// The resolved connection (credential applied).
    public let connection: Connection
    public let controlPath: String

    public init(connection: Connection, controlPath: String) {
        self.connection = connection
        self.controlPath = controlPath
    }

    /// A fresh control socket path. Short and under /tmp: socket paths are limited to 104 bytes, and the
    /// per-user `$TMPDIR` alone takes about 50.
    public static func makeControlPath() -> String {
        "/tmp/rd-" + UUID().uuidString.prefix(8).lowercased()
    }

    private var options: [String] {
        ["-o", "ControlPath=\(controlPath)", "-o", "ControlMaster=no", "-o", "BatchMode=yes"]
    }

    private var user: String { connection.username.trimmingCharacters(in: .whitespaces) }
    private var host: String { connection.host.trimmingCharacters(in: .whitespaces) }
    private var hasCustomPort: Bool { connection.effectivePort != ConnectionType.ssh.defaultPort }

    private var destination: String { user.isEmpty ? host : "\(user)@\(host)" }

    /// `ssh … host -- <script>`; the script runs in the remote user's shell.
    public func ssh(_ script: String) -> [String] {
        var args = ["/usr/bin/ssh"] + options
        if hasCustomPort { args += ["-p", String(connection.effectivePort)] }
        return args + [destination, "--", script]
    }

    /// `scp` (SFTP protocol, so paths are not interpreted by a remote shell) keeping times and modes.
    private func scp(_ from: String, _ to: String) -> [String] {
        var args = ["/usr/bin/scp", "-p", "-q"] + options
        if hasCustomPort { args += ["-P", String(connection.effectivePort)] }
        return args + [from, to]
    }

    /// `host:path`, with an IPv6 address in brackets.
    private func remote(_ path: String) -> String {
        let hostPart = host.contains(":") ? "[\(host)]" : host
        return (user.isEmpty ? hostPart : "\(user)@\(hostPart)") + ":" + path
    }

    public func download(file remotePath: String, to localPath: String) -> [String] {
        scp(remote(remotePath), localPath)
    }

    public func upload(file localPath: String, to remotePath: String) -> [String] {
        scp(localPath, remote(remotePath))
    }
}
