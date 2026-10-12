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

    public init(connection: Connection, password: String?, keyPassphrase: String?) {
        self.connection = connection
        self.password = password?.isEmpty == true ? nil : password
        self.keyPassphrase = keyPassphrase?.isEmpty == true ? nil : keyPassphrase
    }

    public struct Files: Sendable {
        public let directory: URL
        public let launchScript: URL
    }

    // MARK: Command

    public var sshArguments: [String] {
        var args = ["ssh"]
        if connection.effectivePort != ConnectionType.ssh.defaultPort {
            args += ["-p", String(connection.effectivePort)]
        }
        if let key = connection.sshKeyPath?.trimmingCharacters(in: .whitespaces), !key.isEmpty {
            args += ["-i", (key as NSString).expandingTildeInPath]
        }
        let host = connection.host.trimmingCharacters(in: .whitespaces)
        let user = connection.username.trimmingCharacters(in: .whitespaces)
        args.append(user.isEmpty ? host : "\(user)@\(host)")
        return args
    }

    var usesAskpass: Bool { password != nil || keyPassphrase != nil }

    // MARK: Scripts

    func launchScript(directory: URL) -> String {
        var lines = ["#!/bin/sh"]
        lines.append("printf '\\033]0;%s\\007' \(Self.quote(connection.name))")
        if usesAskpass {
            lines.append("export SSH_ASKPASS=\(Self.quote(directory.appending(path: "askpass.sh").path))")
            lines.append("export SSH_ASKPASS_REQUIRE=force")
            lines.append("export RD_PASSWORD_FILE=\(Self.quote(directory.appending(path: "password").path))")
            lines.append("export RD_PASSPHRASE_FILE=\(Self.quote(directory.appending(path: "passphrase").path))")
        }
        lines.append("exec " + sshArguments.map(Self.quote).joined(separator: " "))
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
