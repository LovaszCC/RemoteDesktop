import Foundation
import GRDB

/// Owns the SQLite connection and schema migrations.
public final class AppDatabase: Sendable {
    public let writer: any DatabaseWriter

    public init(_ writer: any DatabaseWriter) throws {
        self.writer = writer
        try Self.migrator.migrate(writer)
    }

    /// Opens (or creates) the database file at `url`.
    ///
    /// Uses rollback journal (`DELETE`) instead of WAL so the database stays a single file,
    /// which is safe to keep in iCloud Drive / Dropbox folders.
    public static func open(at url: URL) throws -> AppDatabase {
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true
        )
        var config = Configuration()
        config.foreignKeysEnabled = true
        config.prepareDatabase { db in
            try db.execute(sql: "PRAGMA journal_mode = DELETE")
        }
        let queue = try DatabaseQueue(path: url.path, configuration: config)
        return try AppDatabase(queue)
    }

    /// In-memory database for tests and previews.
    public static func inMemory() throws -> AppDatabase {
        var config = Configuration()
        config.foreignKeysEnabled = true
        return try AppDatabase(DatabaseQueue(configuration: config))
    }

    public static var defaultURL: URL {
        URL.applicationSupportDirectory
            .appending(path: "RemoteDesktop", directoryHint: .isDirectory)
            .appending(path: "connections.sqlite")
    }

    static var migrator: DatabaseMigrator {
        var migrator = DatabaseMigrator()

        migrator.registerMigration("v1") { db in
            try db.create(table: "meta") { t in
                t.primaryKey("key", .text)
                t.column("value", .blob).notNull()
            }

            try db.create(table: "folder") { t in
                t.primaryKey("id", .text)
                t.column("parentId", .text)
                    .references("folder", onDelete: .cascade)
                    .indexed()
                t.column("name", .text).notNull()
                t.column("sortOrder", .integer).notNull().defaults(to: 0)
            }

            try db.create(table: "connection") { t in
                t.primaryKey("id", .text)
                t.column("folderId", .text)
                    .references("folder", onDelete: .setNull)
                    .indexed()
                t.column("type", .text).notNull()
                t.column("name", .text).notNull()
                t.column("host", .text).notNull()
                t.column("port", .integer)
                t.column("username", .text).notNull().defaults(to: "")
                t.column("domain", .text).notNull().defaults(to: "")
                t.column("passwordEnc", .blob)
                t.column("sshKeyPath", .text)
                t.column("sshKeyPassphraseEnc", .blob)
                t.column("isFavorite", .boolean).notNull().defaults(to: false)
                t.column("notes", .text).notNull().defaults(to: "")
                t.column("createdAt", .datetime).notNull()
                t.column("updatedAt", .datetime).notNull()
            }
        }

        migrator.registerMigration("v2-credentials") { db in
            try db.create(table: "credential") { t in
                t.primaryKey("id", .text)
                t.column("name", .text).notNull()
                t.column("username", .text).notNull().defaults(to: "")
                t.column("domain", .text).notNull().defaults(to: "")
                t.column("passwordEnc", .blob)
                t.column("sshKeyPath", .text)
                t.column("sshKeyPassphraseEnc", .blob)
                t.column("notes", .text).notNull().defaults(to: "")
                t.column("createdAt", .datetime).notNull()
                t.column("updatedAt", .datetime).notNull()
            }
            try db.alter(table: "connection") { t in
                t.add(column: "credentialId", .text)
                    .references("credential", onDelete: .setNull)
            }
            try db.create(index: "connection_on_credentialId", on: "connection", columns: ["credentialId"])
        }

        return migrator
    }
}
