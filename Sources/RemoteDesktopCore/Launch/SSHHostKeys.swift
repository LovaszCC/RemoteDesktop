import Foundation

/// Host key trust for background tunnels, which have no terminal to confirm an unknown key: checks the
/// known-hosts files `ssh` itself would use, scans an unknown host's keys so the user can confirm their
/// fingerprints, and records accepted keys.
public struct SSHHostKeys: Sendable {
    /// Runs a tool and returns its exit status and standard output.
    public typealias Run = @Sendable (_ executable: String, _ arguments: [String], _ input: Data?) throws -> (status: Int32, output: Data)

    /// Where `ssh` will connect and which known-hosts entries it checks, after `~/.ssh/config`.
    public struct Target: Equatable, Sendable {
        public var hostname: String
        public var port: Int
        public var hostKeyAlias: String?
        public var userKnownHostsFiles: [String]
        public var globalKnownHostsFiles: [String]

        /// The name `ssh` looks up: the alias as is, otherwise the host with a non-default port as `[host]:port`.
        public var lookupName: String {
            if let hostKeyAlias { return hostKeyAlias }
            return port == 22 ? hostname : "[\(hostname)]:\(port)"
        }
    }

    let run: Run

    public init(run: @escaping Run = SSHHostKeys.runProcess) {
        self.run = run
    }

    /// Resolves `connection` through `ssh -G`.
    public func target(for connection: Connection) throws -> Target {
        var arguments = ["-G", "-p", String(connection.effectivePort)]
        let user = connection.username.trimmingCharacters(in: .whitespaces)
        if !user.isEmpty { arguments += ["-l", user] }
        arguments.append(connection.host.trimmingCharacters(in: .whitespaces))
        let (status, output) = try run("/usr/bin/ssh", arguments, nil)
        guard status == 0 else { throw SSHHostKeyError.configUnreadable }
        return try Self.parseConfig(String(decoding: output, as: UTF8.self))
    }

    static func parseConfig(_ text: String, home: String = NSHomeDirectory()) throws -> Target {
        var values: [String: String] = [:]
        for line in text.split(separator: "\n") {
            let parts = line.split(separator: " ", maxSplits: 1)
            if parts.count == 2 { values[parts[0].lowercased()] = String(parts[1]) }
        }
        guard let hostname = values["hostname"], let port = values["port"].flatMap(Int.init) else {
            throw SSHHostKeyError.configUnreadable
        }
        let files = { (key: String) in
            (values[key] ?? "").split(separator: " ").map { path in
                path.hasPrefix("~/") ? home + path.dropFirst() : String(path)
            }
        }
        return Target(
            hostname: hostname,
            port: port,
            hostKeyAlias: values["hostkeyalias"],
            userKnownHostsFiles: files("userknownhostsfile"),
            globalKnownHostsFiles: files("globalknownhostsfile")
        )
    }

    /// Whether any known-hosts file already has a key for the target (`ssh-keygen -F`).
    public func isKnown(_ target: Target) throws -> Bool {
        for file in target.userKnownHostsFiles + target.globalKnownHostsFiles
        where FileManager.default.fileExists(atPath: file) {
            if try run("/usr/bin/ssh-keygen", ["-F", target.lookupName, "-f", file], nil).status == 0 { return true }
        }
        return false
    }

    /// The host's public keys as known-hosts lines (`ssh-keyscan`).
    public func scan(_ target: Target) throws -> [String] {
        let (_, output) = try run("/usr/bin/ssh-keyscan", ["-T", "10", "-p", String(target.port), target.hostname], nil)
        let lines = String(decoding: output, as: UTF8.self)
            .split(separator: "\n")
            .map(String.init)
            .filter { !$0.hasPrefix("#") && $0.split(separator: " ").count >= 3 }
        guard !lines.isEmpty else { throw SSHHostKeyError.unreachable }
        return lines
    }

    /// `SHA256:…` fingerprints with key type, one per key (`ssh-keygen -lf -`).
    public func fingerprints(of lines: [String]) throws -> [String] {
        let input = Data(lines.joined(separator: "\n").appending("\n").utf8)
        let (_, output) = try run("/usr/bin/ssh-keygen", ["-lf", "-"], input)
        return String(decoding: output, as: UTF8.self).split(separator: "\n").compactMap { line in
            // "256 SHA256:abc host (ED25519)" → "ED25519 SHA256:abc"
            let parts = line.split(separator: " ")
            guard parts.count >= 2 else { return nil }
            let type = parts.last.map { $0.trimmingCharacters(in: CharacterSet(charactersIn: "()")) } ?? ""
            return "\(type) \(parts[1])"
        }
    }

    /// Appends the scanned keys under the name `ssh` looks up to the user's first known-hosts file.
    public func trust(_ lines: [String], for target: Target) throws {
        guard let path = target.userKnownHostsFiles.first else { throw SSHHostKeyError.configUnreadable }
        let file = URL(filePath: path)
        let fm = FileManager.default
        try fm.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true,
                               attributes: [.posixPermissions: 0o700])
        let entries = lines.map { line in
            ([target.lookupName] + line.split(separator: " ").dropFirst().map(String.init)).joined(separator: " ")
        }
        var contents = (try? Data(contentsOf: file)) ?? Data()
        if let last = contents.last, last != UInt8(ascii: "\n") { contents.append(UInt8(ascii: "\n")) }
        contents.append(Data((entries.joined(separator: "\n") + "\n").utf8))
        if fm.fileExists(atPath: path) {
            try contents.write(to: file)
        } else {
            guard fm.createFile(atPath: path, contents: contents, attributes: [.posixPermissions: 0o600]) else {
                throw CocoaError(.fileWriteUnknown)
            }
        }
    }

    public static let runProcess: Run = { executable, arguments, input in
        let process = Process()
        process.executableURL = URL(filePath: executable)
        process.arguments = arguments
        let output = Pipe()
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        let inputPipe = Pipe()
        process.standardInput = input == nil ? FileHandle.nullDevice : inputPipe
        try process.run()
        if let input {
            inputPipe.fileHandleForWriting.write(input)
            try inputPipe.fileHandleForWriting.close()
        }
        let data = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return (process.terminationStatus, data)
    }
}

public enum SSHHostKeyError: Error, Equatable, LocalizedError {
    case configUnreadable
    case unreachable

    public var errorDescription: String? {
        switch self {
        case .configUnreadable: String(localized: "Couldn't read the SSH settings for this host.", bundle: .core)
        case .unreachable: String(localized: "Couldn't fetch the host's SSH key. Check the host and port.", bundle: .core)
        }
    }
}
