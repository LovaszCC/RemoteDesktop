import Foundation

public enum SSHKeyUnlockError: Error, Equatable, LocalizedError {
    case fileUnreadable(String)
    case wrongPassphrase
    case toolFailed(String)

    public var errorDescription: String? {
        switch self {
        case .fileUnreadable(let path): "Az SSH kulcs nem olvasható: \(path)"
        case .wrongPassphrase: "Hibás kulcs jelmondat."
        case .toolFailed(let output): "Az SSH kulcs feloldása nem sikerült: \(output)"
        }
    }
}

/// Removes the passphrase from a private key with `ssh-keygen -p` and returns the unlocked key.
///
/// Works on a private copy in a `0700` temp directory, so the original file is never touched.
/// The passphrase reaches `ssh-keygen` through an askpass helper reading an environment variable,
/// keeping it out of the process argument list.
public enum SSHKeyUnlocker {
    public static func unlock(
        keyPath: String,
        passphrase: String,
        sshKeygen: URL = URL(filePath: "/usr/bin/ssh-keygen")
    ) throws -> String {
        let fm = FileManager.default
        let source = URL(filePath: (keyPath as NSString).expandingTildeInPath)
        guard let original = fm.contents(atPath: source.path) else {
            throw SSHKeyUnlockError.fileUnreadable(keyPath)
        }

        let directory = fm.temporaryDirectory.appending(path: "RemoteDesktop-unlock-\(UUID().uuidString)", directoryHint: .isDirectory)
        try fm.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        defer { try? fm.removeItem(at: directory) }

        let key = directory.appending(path: "key")
        let askpass = directory.appending(path: "askpass.sh")
        guard fm.createFile(atPath: key.path, contents: original, attributes: [.posixPermissions: 0o600]),
              fm.createFile(atPath: askpass.path, contents: Data("#!/bin/sh\nprintf '%s\\n' \"$RD_UNLOCK_PASSPHRASE\"\n".utf8), attributes: [.posixPermissions: 0o700])
        else { throw CocoaError(.fileWriteUnknown) }

        let process = Process()
        process.executableURL = sshKeygen
        process.arguments = ["-p", "-N", "", "-f", key.path]
        process.environment = [
            "PATH": "/usr/bin:/bin",
            "SSH_ASKPASS": askpass.path,
            "SSH_ASKPASS_REQUIRE": "force",
            "DISPLAY": ":0",
            "RD_UNLOCK_PASSPHRASE": passphrase,
        ]
        process.standardInput = FileHandle.nullDevice
        let output = Pipe()
        process.standardOutput = output
        process.standardError = output
        try process.run()
        let log = String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        process.waitUntilExit()

        guard process.terminationStatus == 0 else {
            if log.localizedCaseInsensitiveContains("incorrect passphrase") || log.localizedCaseInsensitiveContains("bad passphrase") {
                throw SSHKeyUnlockError.wrongPassphrase
            }
            throw SSHKeyUnlockError.toolFailed(log.trimmingCharacters(in: .whitespacesAndNewlines))
        }
        guard let unlocked = fm.contents(atPath: key.path) else { throw SSHKeyUnlockError.fileUnreadable(key.path) }
        return String(decoding: unlocked, as: UTF8.self)
    }
}
