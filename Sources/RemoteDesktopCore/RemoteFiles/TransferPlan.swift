import Foundation

public enum TransferDirection: Sendable {
    case download, upload
}

/// How one item is copied: folders as a single tar stream piped through the app (so progress can be counted),
/// files with `scp`.
public enum TransferPlan: Equatable, Sendable {
    /// `source`'s stdout is fed to `sink`'s stdin.
    case stream(source: [String], sink: [String])
    case copy([String])

    /// - Parameters:
    ///   - sourceDirectory: folder holding the item on the source side.
    ///   - destinationDirectory: folder to copy into on the other side.
    public static func make(
        name: String,
        isDirectory: Bool,
        direction: TransferDirection,
        sourceDirectory: String,
        destinationDirectory: String,
        compress: Bool,
        command: RemoteCommand
    ) -> TransferPlan {
        let z = compress ? "z" : ""
        let quote = SSHLaunchScript.quote
        guard isDirectory else {
            switch direction {
            case .download:
                return .copy(command.download(file: RemotePath.join(sourceDirectory, name), to: (destinationDirectory as NSString).appendingPathComponent(name)))
            case .upload:
                return .copy(command.upload(file: (sourceDirectory as NSString).appendingPathComponent(name), to: RemotePath.join(destinationDirectory, name)))
            }
        }
        switch direction {
        case .download:
            return .stream(
                source: command.ssh("tar -C \(quote(sourceDirectory)) -c\(z)f - \(quote("./" + name))"),
                sink: ["/usr/bin/tar", "-x\(z)f", "-", "-C", destinationDirectory]
            )
        case .upload:
            // No AppleDouble (._*) files, xattrs or ACLs for the Linux side; the remote side doesn't take over
            // this Mac's user and group ids (-o).
            return .stream(
                source: ["/usr/bin/tar", "--no-mac-metadata", "--no-xattrs", "--no-acls", "-c\(z)f", "-",
                         "-C", sourceDirectory, "./" + name],
                sink: command.ssh("tar -C \(quote(destinationDirectory)) -x\(z)of -")
            )
        }
    }

    /// Remote size of a file or folder in bytes (`du -sk`, so rounded up to KiB).
    public static func remoteSizeScript(_ path: String) -> String {
        "du -sk -- \(SSHLaunchScript.quote(path)) 2>/dev/null"
    }

    public static func parseDuOutput(_ output: String) -> Int64? {
        output.split(whereSeparator: { $0 == "\t" || $0 == " " || $0 == "\n" }).first.flatMap { Int64($0) }.map { $0 * 1024 }
    }

    public static func remoteExistsScript(_ path: String) -> String {
        "test -e \(SSHLaunchScript.quote(path)) || test -L \(SSHLaunchScript.quote(path))"
    }
}

public enum ConflictChoice: Sendable {
    case replace, skip
}

/// What to do when the destination already exists; "apply to all" turns a choice into a policy.
public enum ConflictPolicy: Equatable, Sendable {
    case ask, replaceAll, skipAll

    /// nil: ask the user.
    public func resolve(exists: Bool) -> ConflictChoice? {
        guard exists else { return .replace }
        switch self {
        case .ask: return nil
        case .replaceAll: return .replace
        case .skipAll: return .skip
        }
    }

    public static func remembering(_ choice: ConflictChoice) -> ConflictPolicy {
        choice == .replace ? .replaceAll : .skipAll
    }
}
