import Foundation

/// Sidebar node: a folder (with children) or a connection (leaf).
public struct TreeNode: Identifiable, Hashable, Sendable {
    public enum Kind: Hashable, Sendable {
        case folder(Folder)
        case connection(Connection)
    }

    public let kind: Kind
    public var children: [TreeNode]?

    public var id: String {
        switch kind {
        case .folder(let folder): "f:\(folder.id)"
        case .connection(let connection): "c:\(connection.id)"
        }
    }
}

public enum LibraryTree {
    /// Folders first (by sort order, then name), then connections by name, recursively.
    public static func build(_ library: Library) -> [TreeNode] {
        let foldersByParent = Dictionary(grouping: library.folders, by: \.parentId)
        let connectionsByFolder = Dictionary(grouping: library.connections, by: \.folderId)
        let knownFolders = Set(library.folders.map(\.id))

        func nodes(parent: String?) -> [TreeNode] {
            let folders = (foldersByParent[parent] ?? [])
                .sorted { ($0.sortOrder, $0.name.localizedLowercase) < ($1.sortOrder, $1.name.localizedLowercase) }
                .map { TreeNode(kind: .folder($0), children: nodes(parent: $0.id)) }
            var connections = connectionsByFolder[parent] ?? []
            if parent == nil {
                // Connections pointing at a missing folder are shown at the root.
                connections += library.connections.filter { $0.folderId.map { !knownFolders.contains($0) } ?? false }
            }
            return folders + connections
                .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
                .map { TreeNode(kind: .connection($0), children: nil) }
        }
        return nodes(parent: nil)
    }

    public static func favorites(_ library: Library) -> [Connection] {
        library.connections
            .filter(\.isFavorite)
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    /// Every whitespace-separated term must match name, host, user, notes or folder path.
    public static func search(_ library: Library, query: String) -> [Connection] {
        let terms = query.lowercased().split(whereSeparator: \.isWhitespace).map(String.init)
        guard !terms.isEmpty else { return [] }
        return library.connections.filter { connection in
            let haystack = [
                connection.name, connection.host, connection.username, connection.notes,
                folderPath(connection.folderId, in: library),
            ].joined(separator: " ").lowercased()
            return terms.allSatisfy(haystack.contains)
        }
        .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    /// Connections that can be linked to the credential (not linked to it yet), sorted by name; a non-empty
    /// query must match name, host, user or folder path term by term.
    public static func linkCandidates(_ library: Library, for credentialId: String, query: String) -> [Connection] {
        let terms = query.lowercased().split(whereSeparator: \.isWhitespace).map(String.init)
        return library.connections.filter { connection in
            guard connection.credentialId != credentialId else { return false }
            let haystack = [connection.name, connection.host, connection.username, folderPath(connection.folderId, in: library)]
                .joined(separator: " ").lowercased()
            return terms.allSatisfy(haystack.contains)
        }
        .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    /// Credentials whose name or user matches every term.
    public static func searchCredentials(_ library: Library, query: String) -> [Credential] {
        let terms = query.lowercased().split(whereSeparator: \.isWhitespace).map(String.init)
        guard !terms.isEmpty else { return [] }
        return library.credentials.filter { credential in
            let haystack = "\(credential.name) \(credential.displayUser) \(credential.notes)".lowercased()
            return terms.allSatisfy(haystack.contains)
        }
    }

    /// "Parent / Child" for a folder id, empty for the root.
    public static func folderPath(_ folderId: String?, in library: Library) -> String {
        let byId = Dictionary(uniqueKeysWithValues: library.folders.map { ($0.id, $0) })
        var names: [String] = []
        var cursor = folderId
        var seen = Set<String>()
        while let id = cursor, let folder = byId[id], seen.insert(id).inserted {
            names.insert(folder.name, at: 0)
            cursor = folder.parentId
        }
        return names.joined(separator: " / ")
    }

    /// Depth-first folder list for pickers, optionally excluding a subtree (for "move folder").
    public static func flattenedFolders(_ library: Library, excluding excluded: String? = nil) -> [(folder: Folder, depth: Int)] {
        let byParent = Dictionary(grouping: library.folders, by: \.parentId)
        var result: [(Folder, Int)] = []
        func walk(_ parent: String?, _ depth: Int) {
            for folder in (byParent[parent] ?? []).sorted(by: { $0.name.localizedLowercase < $1.name.localizedLowercase })
            where folder.id != excluded {
                result.append((folder, depth))
                walk(folder.id, depth + 1)
            }
        }
        walk(nil, 0)
        return result
    }
}
