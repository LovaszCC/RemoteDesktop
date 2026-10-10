import Foundation
import GRDB

/// Everything the sidebar needs, loaded in one read.
public struct Library: Equatable, Sendable {
    public var folders: [Folder]
    public var connections: [Connection]

    public init(folders: [Folder] = [], connections: [Connection] = []) {
        self.folders = folders
        self.connections = connections
    }
}

public struct ConnectionRepository: Sendable {
    public let database: AppDatabase

    public init(database: AppDatabase) {
        self.database = database
    }

    // MARK: Library

    public func library() throws -> Library {
        try database.writer.read(Self.fetchLibrary)
    }

    /// Emits a fresh `Library` every time folders or connections change.
    public func observeLibrary() -> ValueObservation<ValueReducers.Fetch<Library>> {
        ValueObservation.tracking(Self.fetchLibrary)
    }

    @Sendable static func fetchLibrary(_ db: Database) throws -> Library {
        Library(
            folders: try Folder.order(Column("sortOrder"), Column("name").collating(.nocase)).fetchAll(db),
            connections: try Connection.order(Column("name").collating(.nocase)).fetchAll(db)
        )
    }

    // MARK: Folders

    @discardableResult
    public func save(_ folder: Folder) throws -> Folder {
        try database.writer.write { db in try folder.save(db) }
        return folder
    }

    /// Deletes the folder and its subfolders; connections inside fall back to the root.
    public func deleteFolder(id: String) throws {
        _ = try database.writer.write { db in try Folder.deleteOne(db, key: id) }
    }

    /// Moves a folder under `parentId`, refusing to create a cycle.
    public func moveFolder(id: String, to parentId: String?) throws {
        try database.writer.write { db in
            var cursor = parentId
            while let current = cursor {
                if current == id { throw RepositoryError.folderCycle }
                cursor = try Folder.fetchOne(db, key: current)?.parentId
            }
            guard var folder = try Folder.fetchOne(db, key: id) else { throw RepositoryError.notFound }
            folder.parentId = parentId
            try folder.update(db)
        }
    }

    // MARK: Connections

    @discardableResult
    public func save(_ connection: Connection) throws -> Connection {
        var connection = connection
        connection.updatedAt = .now
        try database.writer.write { [connection] db in try connection.save(db) }
        return connection
    }

    public func connection(id: String) throws -> Connection? {
        try database.writer.read { db in try Connection.fetchOne(db, key: id) }
    }

    public func deleteConnection(id: String) throws {
        _ = try database.writer.write { db in try Connection.deleteOne(db, key: id) }
    }

    public func moveConnection(id: String, to folderId: String?) throws {
        try database.writer.write { db in
            guard var connection = try Connection.fetchOne(db, key: id) else { throw RepositoryError.notFound }
            connection.folderId = folderId
            connection.updatedAt = .now
            try connection.update(db)
        }
    }

    public func setFavorite(id: String, _ isFavorite: Bool) throws {
        try database.writer.write { db in
            guard var connection = try Connection.fetchOne(db, key: id) else { throw RepositoryError.notFound }
            connection.isFavorite = isFavorite
            try connection.update(db)
        }
    }

    /// Case-insensitive match on name, host, username and notes.
    public func search(_ text: String) throws -> [Connection] {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return try library().connections }
        let pattern = "%\(trimmed)%"
        return try database.writer.read { db in
            try Connection
                .filter(sql: "name LIKE ? OR host LIKE ? OR username LIKE ? OR notes LIKE ?",
                        arguments: [pattern, pattern, pattern, pattern])
                .order(Column("name"))
                .fetchAll(db)
        }
    }

    // MARK: Meta

    public func meta(_ key: String) throws -> Data? {
        try database.writer.read { db in
            try Data.fetchOne(db, sql: "SELECT value FROM meta WHERE key = ?", arguments: [key])
        }
    }

    public func setMeta(_ key: String, _ value: Data) throws {
        try database.writer.write { db in
            try db.execute(
                sql: "INSERT INTO meta(key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                arguments: [key, value]
            )
        }
    }

    /// Runs several writes atomically (used by importer and re-encryption).
    public func transaction(_ work: @escaping @Sendable (Database) throws -> Void) throws {
        try database.writer.write(work)
    }
}

public enum RepositoryError: Error, Equatable {
    case notFound
    case folderCycle
}
