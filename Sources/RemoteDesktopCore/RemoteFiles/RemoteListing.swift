import Foundation

public struct RemoteEntry: Identifiable, Hashable, Sendable {
    public enum Kind: Sendable { case file, directory, symlink, other }

    public var id: String { name }
    public var name: String
    public var kind: Kind
    public var size: Int64
    public var modified: Date?
    /// `drwxr-xr-x` style.
    public var permissions: String

    public init(name: String, kind: Kind, size: Int64, modified: Date?, permissions: String) {
        self.name = name
        self.kind = kind
        self.size = size
        self.modified = modified
        self.permissions = permissions
    }

    public var isDirectory: Bool { kind == .directory }
}

public enum RemoteListingError: Error, Equatable {
    case unreadable
}

/// Lists one remote directory: GNU `find -printf` where available (exact, any file name), else `ls -lnA`
/// (BSD, busybox).
public enum RemoteListing {
    static let findMarker = "RD-FIND"
    static let lsMarker = "RD-LS"

    /// Prints the absolute path, a format marker, then the entries.
    public static func script(for path: String) -> String {
        """
        \(RemotePath.cdCommand(path)) || exit 2
        pwd
        if find . -maxdepth 0 -printf '' >/dev/null 2>&1; then
          echo \(findMarker)
          find . -mindepth 1 -maxdepth 1 -printf '%y\\t%s\\t%T@\\t%M\\0%P\\0'
        else
          echo \(lsMarker)
          LC_ALL=C ls -lnA
        fi
        """
    }

    /// Absolute path and entries (directories first, then by name).
    public static func parse(_ data: Data, now: Date = .now) throws -> (path: String, entries: [RemoteEntry]) {
        let text = String(decoding: data, as: UTF8.self)
        let lines = text.split(separator: "\n", maxSplits: 2, omittingEmptySubsequences: false)
        guard lines.count >= 2, lines[0].hasPrefix("/") else { throw RemoteListingError.unreadable }
        let path = String(lines[0])
        let body = lines.count > 2 ? String(lines[2]) : ""
        let entries: [RemoteEntry]
        switch lines[1] {
        case Substring(findMarker): entries = parseFind(body)
        case Substring(lsMarker): entries = parseLS(body, now: now)
        default: throw RemoteListingError.unreadable
        }
        return (path, entries.sorted(by: order))
    }

    static func order(_ a: RemoteEntry, _ b: RemoteEntry) -> Bool {
        if a.isDirectory != b.isDirectory { return a.isDirectory }
        return a.name.localizedStandardCompare(b.name) == .orderedAscending
    }

    /// Records of `type\tsize\tmtime\tmode` NUL `name` NUL.
    static func parseFind(_ body: String) -> [RemoteEntry] {
        let fields = body.split(separator: "\0", omittingEmptySubsequences: false)
        var entries: [RemoteEntry] = []
        var index = 0
        while index + 1 < fields.count {
            let meta = fields[index].split(separator: "\t")
            let name = String(fields[index + 1])
            index += 2
            guard meta.count == 4, !name.isEmpty else { continue }
            entries.append(RemoteEntry(
                name: name,
                kind: kind(findType: meta[0]),
                size: Int64(meta[1]) ?? 0,
                modified: Double(meta[2]).map { Date(timeIntervalSince1970: $0) },
                permissions: String(meta[3])
            ))
        }
        return entries
    }

    private static func kind(findType: Substring) -> RemoteEntry.Kind {
        switch findType {
        case "d": .directory
        case "f": .file
        case "l": .symlink
        default: .other
        }
    }

    private static let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]

    /// `ls -lnA` lines: mode, links, uid, gid, size, month, day, time-or-year, name (`name -> target` for links).
    static func parseLS(_ body: String, now: Date) -> [RemoteEntry] {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .current
        let currentYear = calendar.component(.year, from: now)
        return body.split(separator: "\n").compactMap { line -> RemoteEntry? in
            var rest = Substring(line)
            var fields: [Substring] = []
            while fields.count < 8 {
                rest = rest.drop(while: { $0 == " " })
                guard let end = rest.firstIndex(of: " ") else { return nil }
                fields.append(rest[..<end])
                rest = rest[end...]
            }
            var name = String(rest.dropFirst())
            let mode = fields[0]
            guard let typeChar = mode.first, !name.isEmpty else { return nil }
            let kind: RemoteEntry.Kind = switch typeChar {
            case "d": .directory
            case "-": .file
            case "l": .symlink
            default: .other
            }
            if kind == .symlink, let arrow = name.range(of: " -> ") { name = String(name[..<arrow.lowerBound]) }

            var modified: Date?
            if let month = months.firstIndex(of: String(fields[5])), let day = Int(fields[6]) {
                var parts = DateComponents(month: month + 1, day: day)
                let timeOrYear = fields[7]
                if timeOrYear.contains(":") {
                    let hm = timeOrYear.split(separator: ":")
                    parts.hour = Int(hm[0])
                    parts.minute = Int(hm.count > 1 ? hm[1] : "0")
                    parts.year = currentYear
                    if let date = calendar.date(from: parts), date > now.addingTimeInterval(86_400) { parts.year = currentYear - 1 }
                } else {
                    parts.year = Int(timeOrYear)
                }
                modified = calendar.date(from: parts)
            }
            return RemoteEntry(name: name, kind: kind, size: Int64(fields[4]) ?? 0, modified: modified,
                               permissions: String(mode.prefix(10)))
        }
    }
}
