import Foundation
import GRDB

/// One connection parsed from an export, before encryption and folder resolution.
public struct ImportedConnection: Equatable, Sendable {
    public var type: ConnectionType
    public var name: String
    public var groupPath: [String]
    public var host: String
    public var port: Int?
    public var username: String
    public var domain: String
    public var password: String?
    public var sshKeyPath: String?
    public var notes: String
}

public struct ImportSummary: Equatable, Sendable {
    public var imported: Int
    public var foldersCreated: Int
    public var skipped: [String]
    /// Entries whose password was present only in encrypted (`SafePassword`) form.
    public var withoutPassword: Int
}

/// Imports Devolutions Remote Desktop Manager `.rdm` XML exports.
///
/// Each `<Connection>` element is flattened to `path → text` (e.g. `RDP/UserName`) and mapped
/// with fallbacks, since field locations differ between RDM versions and connection types.
public enum RDMImporter {
    public struct ParseResult: Equatable, Sendable {
        public var connections: [ImportedConnection]
        public var emptyGroups: [[String]]
        public var skipped: [String]
        public var encryptedPasswords: Int
    }

    static let rdpTypes: Set<String> = ["RDPConfigured", "RDP", "RDPFilename"]
    static let sshTypes: Set<String> = ["SSHShell", "SSH", "Putty", "SSHTunnel", "Terminal"]

    public static func parse(_ data: Data) throws -> ParseResult {
        let entries = try EntryCollector.collect(data)
        var result = ParseResult(connections: [], emptyGroups: [], skipped: [], encryptedPasswords: 0)

        for fields in entries {
            let kind = fields["ConnectionType"] ?? ""
            let name = fields["Name"] ?? ""
            let group = splitGroup(fields["Group"] ?? fields["GroupName"] ?? "")

            if kind == "Group" {
                // A folder entry's Group already contains its own name.
                result.emptyGroups.append(group.isEmpty ? [name] : group)
                continue
            }

            let type: ConnectionType
            if rdpTypes.contains(kind) { type = .rdp }
            else if sshTypes.contains(kind) { type = .ssh }
            else {
                result.skipped.append(name.isEmpty ? kind : "\(name) (\(kind))")
                continue
            }

            let section = type == .rdp ? "RDP" : "Terminal"
            func value(_ keys: String...) -> String? {
                for key in keys {
                    if let v = fields["\(section)/\(key)"] ?? fields[key], !v.isEmpty { return v }
                }
                return nil
            }

            var host = value("Host", "Url", "HostName", "ComputerName") ?? ""
            var port = value("HostPort", "Port").flatMap(Int.init)
            if let split = splitHostPort(host) {
                host = split.host
                port = port ?? split.port
            }
            guard !host.isEmpty else {
                result.skipped.append("\(name) (no host)")
                continue
            }

            let password = value("Password", "ClearPassword")
            if password == nil, value("SafePassword") != nil { result.encryptedPasswords += 1 }

            result.connections.append(ImportedConnection(
                type: type,
                name: name.isEmpty ? host : name,
                groupPath: group,
                host: host,
                port: port == type.defaultPort ? nil : port,
                username: value("UserName", "Username", "User") ?? "",
                domain: value("Domain") ?? "",
                password: password,
                sshKeyPath: type == .ssh ? value("PrivateKeyPath", "PrivateKeyFileName") : nil,
                notes: value("Description", "Notes") ?? ""
            ))
        }
        return result
    }

    /// Parses and writes everything in one transaction, reusing folders that already exist.
    public static func importFile(
        _ data: Data,
        into repo: ConnectionRepository,
        vault: Vault
    ) throws -> ImportSummary {
        let parsed = try parse(data)
        let encrypted = try parsed.connections.map { ($0, try vault.encryptOptional($0.password)) }
        let foldersBefore = try repo.library().folders.count

        try repo.transaction { db in
            var cache: [String: String] = [:]   // joined path → folder id

            func folderId(for path: [String]) throws -> String? {
                var parentId: String?
                for depth in path.indices {
                    let key = path[...depth].joined(separator: "\u{1F}")
                    if let cached = cache[key] { parentId = cached; continue }
                    let name = path[depth]
                    let existing = try Folder
                        .filter(Column("name") == name)
                        .filter(parentId == nil ? Column("parentId") == nil : Column("parentId") == parentId!)
                        .fetchOne(db)
                    let folder = try existing ?? {
                        let created = Folder(parentId: parentId, name: name)
                        try created.insert(db)
                        return created
                    }()
                    cache[key] = folder.id
                    parentId = folder.id
                }
                return parentId
            }

            for group in parsed.emptyGroups { _ = try folderId(for: group) }
            for (item, passwordEnc) in encrypted {
                try Connection(
                    folderId: try folderId(for: item.groupPath),
                    type: item.type,
                    name: item.name,
                    host: item.host,
                    port: item.port,
                    username: item.username,
                    domain: item.domain,
                    passwordEnc: passwordEnc,
                    sshKeyPath: item.sshKeyPath,
                    notes: item.notes
                ).insert(db)
            }
        }

        return ImportSummary(
            imported: parsed.connections.count,
            foldersCreated: try repo.library().folders.count - foldersBefore,
            skipped: parsed.skipped,
            withoutPassword: parsed.encryptedPasswords
        )
    }

    static func splitGroup(_ group: String) -> [String] {
        group.split(whereSeparator: { $0 == "\\" || $0 == "/" })
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
    }

    /// `host:port` → split; bare IPv6 addresses (multiple colons, no brackets) are left alone.
    static func splitHostPort(_ value: String) -> (host: String, port: Int?)? {
        if value.hasPrefix("["), let close = value.firstIndex(of: "]") {
            let host = String(value[value.index(after: value.startIndex)..<close])
            let rest = value[value.index(after: close)...]
            return (host, rest.hasPrefix(":") ? Int(rest.dropFirst()) : nil)
        }
        let parts = value.split(separator: ":", omittingEmptySubsequences: false)
        guard parts.count == 2, let port = Int(parts[1]) else { return nil }
        return (String(parts[0]), port)
    }
}

/// SAX collector: every `<Connection>` element becomes a flat dictionary of `path → text`.
private final class EntryCollector: NSObject, XMLParserDelegate {
    private var entries: [[String: String]] = []
    private var current: [String: String]?
    private var path: [String] = []
    private var depthInEntry = 0
    private var text = ""
    private var error: Error?

    static func collect(_ data: Data) throws -> [[String: String]] {
        let collector = EntryCollector()
        let parser = XMLParser(data: data)
        parser.delegate = collector
        guard parser.parse() else {
            throw collector.error ?? parser.parserError ?? CocoaError(.fileReadCorruptFile)
        }
        return collector.entries
    }

    func parser(_ parser: XMLParser, didStartElement name: String, namespaceURI: String?, qualifiedName: String?, attributes: [String: String] = [:]) {
        text = ""
        if current == nil {
            if name == "Connection" {
                current = [:]
                path = []
            }
            return
        }
        path.append(name)
    }

    func parser(_ parser: XMLParser, foundCharacters string: String) {
        text += string
    }

    func parser(_ parser: XMLParser, didEndElement name: String, namespaceURI: String?, qualifiedName: String?) {
        guard current != nil else { return }
        if path.isEmpty {
            if name == "Connection" {
                entries.append(current!)
                current = nil
            }
            return
        }
        let key = path.joined(separator: "/")
        let value = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if !value.isEmpty, current![key] == nil {
            current![key] = value
        }
        path.removeLast()
        text = ""
    }

    func parser(_ parser: XMLParser, parseErrorOccurred parseError: Error) {
        error = parseError
    }
}
