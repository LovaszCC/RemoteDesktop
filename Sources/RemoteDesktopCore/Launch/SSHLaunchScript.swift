import Foundation

/// Generates the shell scripts that run `ssh` inside a terminal and answer its password /
/// passphrase prompts through `SSH_ASKPASS`, so no external tools (sshpass, expect) are needed.
///
/// Each secret lives in its own `0600` file inside a `0700` per-launch directory and is
/// deleted by the askpass helper on first read. Any other prompt — host key confirmation,
/// a retry after a wrong password — is asked interactively on the terminal.
public struct SSHLaunchScript: Sendable {
    public var connection: Connection
    public var password: String?
    public var keyPassphrase: String?
    /// Passphrase-free private key; replaces `sshKeyPath` and the passphrase prompt when set.
    public var unlockedKey: String?
    /// Set for a background tunnel (`ssh -N -L`) instead of an interactive session.
    public var forward: PortForward?

    public init(connection: Connection, password: String?, keyPassphrase: String?, unlockedKey: String? = nil,
                forward: PortForward? = nil) {
        self.connection = connection
        self.forward = forward
        self.password = password?.isEmpty == true ? nil : password
        self.unlockedKey = unlockedKey?.isEmpty == true ? nil : unlockedKey
        self.keyPassphrase = (keyPassphrase?.isEmpty == true || self.unlockedKey != nil) ? nil : keyPassphrase
    }

    public struct Files: Sendable {
        public let directory: URL
        public let launchScript: URL
    }

    // MARK: Command

    public var sshArguments: [String] { sshArguments(directory: nil) }

    func sshArguments(directory: URL?) -> [String] {
        var args = ["ssh"]
        if connection.effectivePort != ConnectionType.ssh.defaultPort {
            args += ["-p", String(connection.effectivePort)]
        }
        if unlockedKey != nil, let directory {
            args += ["-i", directory.appending(path: "key").path, "-o", "IdentitiesOnly=yes"]
        } else if let key = connection.sshKeyPath?.trimmingCharacters(in: .whitespaces), !key.isEmpty {
            args += ["-i", (key as NSString).expandingTildeInPath]
        }
        if let forward { args += Self.tunnelOptions(forward) }
        let host = connection.host.trimmingCharacters(in: .whitespaces)
        let user = connection.username.trimmingCharacters(in: .whitespaces)
        args.append(user.isEmpty ? host : "\(user)@\(host)")
        return args
    }

    /// No shell, keep-alives, exit when the local port can't be bound, and only already trusted host keys
    /// (there is no terminal to confirm one). Listens on loopback only.
    static func tunnelOptions(_ forward: PortForward) -> [String] {
        let host = forward.remoteHost.trimmingCharacters(in: .whitespaces)
        let destination = host.contains(":") ? "[\(host)]" : host
        return [
            "-N",
            "-o", "ExitOnForwardFailure=yes",
            "-o", "ServerAliveInterval=30",
            "-o", "ServerAliveCountMax=3",
            "-o", "StrictHostKeyChecking=yes",
            "-L", "127.0.0.1:\(forward.localPort):\(destination):\(forward.remotePort)",
        ]
    }

    var usesAskpass: Bool { password != nil || keyPassphrase != nil }

    // MARK: Scripts

    func launchScript(directory: URL) -> String {
        var lines = ["#!/bin/sh"]
        if forward == nil { lines.append("printf '\\033]0;%s\\007' \(Self.quote(connection.name))") }
        if usesAskpass {
            lines.append("export SSH_ASKPASS=\(Self.quote(directory.appending(path: "askpass.sh").path))")
            lines.append("export SSH_ASKPASS_REQUIRE=force")
            lines.append("export RD_PASSWORD_FILE=\(Self.quote(directory.appending(path: "password").path))")
            lines.append("export RD_PASSPHRASE_FILE=\(Self.quote(directory.appending(path: "passphrase").path))")
        }
        lines.append("exec " + sshArguments(directory: directory).map(Self.quote).joined(separator: " "))
        return lines.joined(separator: "\n") + "\n"
    }

    static let askpassScript = """
    #!/bin/sh
    # Called by ssh with the prompt as $1. Answers once from the secret file, then asks on the tty.
    prompt="$1"
    case "$prompt" in
      *[Pp]assphrase*) secret="$RD_PASSPHRASE_FILE" ;;
      *[Pp]assword*) secret="$RD_PASSWORD_FILE" ;;
      *) secret="" ;;
    esac
    if [ -n "$secret" ] && [ -f "$secret" ]; then
      cat "$secret"
      rm -f "$secret"
      exit 0
    fi
    printf '%s' "$prompt" > /dev/tty
    if [ -n "$secret" ]; then
      stty -echo < /dev/tty
      IFS= read -r answer < /dev/tty
      stty echo < /dev/tty
      printf '\\n' > /dev/tty
    else
      IFS= read -r answer < /dev/tty
    fi
    printf '%s\\n' "$answer"

    """

    /// Writes launch script, askpass helper and secrets into a fresh private directory under `root`.
    public func write(under root: URL = FileManager.default.temporaryDirectory.appending(path: "RemoteDesktop")) throws -> Files {
        let fm = FileManager.default
        try fm.createDirectory(at: root, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let directory = root.appending(path: UUID().uuidString, directoryHint: .isDirectory)
        try fm.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])

        func write(_ name: String, _ contents: String, mode: Int) throws -> URL {
            let url = directory.appending(path: name)
            guard fm.createFile(atPath: url.path, contents: Data(contents.utf8), attributes: [.posixPermissions: mode]) else {
                throw CocoaError(.fileWriteUnknown)
            }
            return url
        }

        let launch = try write("launch.sh", launchScript(directory: directory), mode: 0o700)
        if let unlockedKey {
            // ssh rejects keys without a trailing newline.
            _ = try write("key", unlockedKey.hasSuffix("\n") ? unlockedKey : unlockedKey + "\n", mode: 0o600)
        }
        if usesAskpass {
            _ = try write("askpass.sh", Self.askpassScript, mode: 0o700)
            if let password { _ = try write("password", password, mode: 0o600) }
            if let keyPassphrase { _ = try write("passphrase", keyPassphrase, mode: 0o600) }
        }
        return Files(directory: directory, launchScript: launch)
    }

    /// POSIX single-quote escaping.
    public static func quote(_ value: String) -> String {
        "'" + value.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }
}
