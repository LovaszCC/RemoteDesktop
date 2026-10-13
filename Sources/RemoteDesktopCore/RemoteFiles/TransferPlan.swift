import Foundation

public enum TransferDirection: Sendable {
    case download, upload
}

/// How one file or folder is copied. Normally a tar stream: `source` processes are chained by pipes, the app
/// reads the last one's output (uncompressed tar, so progress can be counted against the size) and writes it
/// into the first `sink` process. Compression is a separate `gzip` stage on the wire side. Without tar on the
/// remote side (e.g. RouterOS) a file is copied with `scp`.
public enum TransferPlan: Equatable, Sendable {
    case stream(source: [[String]], sink: [[String]])
    case copy([String])

    /// - Parameters:
    ///   - sourceDirectory: folder holding the item on the source side.
    ///   - destinationDirectory: folder to copy into on the other side.
    public static func make(
        name: String,
        direction: TransferDirection,
        sourceDirectory: String,
        destinationDirectory: String,
        compress: Bool,
        remoteHasTar: Bool = true,
        command: RemoteCommand
    ) -> TransferPlan {
        let quote = SSHLaunchScript.quote
        guard remoteHasTar else {
            switch direction {
            case .download:
                return .copy(command.download(file: RemotePath.join(sourceDirectory, name),
                                              to: (destinationDirectory as NSString).appendingPathComponent(name)))
            case .upload:
                return .copy(command.upload(file: (sourceDirectory as NSString).appendingPathComponent(name),
                                            to: RemotePath.join(destinationDirectory, name)))
            }
        }
        switch direction {
        case .download:
            let remote = command.ssh("tar -C \(quote(sourceDirectory)) -c\(compress ? "z" : "")f - \(quote("./" + name))")
            return .stream(
                source: compress ? [remote, ["/usr/bin/gzip", "-dc"]] : [remote],
                sink: [["/usr/bin/tar", "-xf", "-", "-C", destinationDirectory]]
            )
        case .upload:
            // No AppleDouble (._*) files, xattrs or ACLs for the Linux side; the remote side doesn't take over
            // this Mac's user and group ids (-o).
            let local = ["/usr/bin/tar", "--no-mac-metadata", "--no-xattrs", "--no-acls", "-cf", "-",
                         "-C", sourceDirectory, "./" + name]
            let remote = command.ssh("tar -C \(quote(destinationDirectory)) -x\(compress ? "z" : "")of -")
            return .stream(source: [local], sink: compress ? [["/usr/bin/gzip", "-c"], remote] : [remote])
        }
    }

    /// Prints `yes` when the remote side has tar.
    public static let remoteTarCheckScript = "command -v tar >/dev/null 2>&1 && echo yes"

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
