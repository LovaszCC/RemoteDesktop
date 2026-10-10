import Foundation
import Testing
@testable import RemoteDesktopCore

@Suite struct ConnectionRepositoryTests {
    let repo: ConnectionRepository

    init() throws {
        repo = ConnectionRepository(database: try .inMemory())
    }

    @Test func savesAndLoadsLibrary() throws {
        let folder = try repo.save(Folder(name: "Prod"))
        try repo.save(Connection(folderId: folder.id, type: .rdp, name: "dc01", host: "10.0.0.1", username: "admin", domain: "CORP"))
        try repo.save(Connection(type: .ssh, name: "web01", host: "web01.local", username: "root"))

        let library = try repo.library()
        #expect(library.folders.map(\.name) == ["Prod"])
        #expect(library.connections.map(\.name) == ["dc01", "web01"])
        #expect(library.connections[0].folderId == folder.id)
        #expect(library.connections[0].domain == "CORP")
    }

    @Test func effectivePortFallsBackToTypeDefault() {
        #expect(Connection(type: .rdp, name: "a", host: "h").effectivePort == 3389)
        #expect(Connection(type: .ssh, name: "a", host: "h").effectivePort == 22)
        #expect(Connection(type: .ssh, name: "a", host: "h", port: 2222).effectivePort == 2222)
    }

    @Test func updatesExistingConnection() throws {
        var connection = try repo.save(Connection(type: .ssh, name: "old", host: "h"))
        connection.name = "new"
        try repo.save(connection)
        #expect(try repo.connection(id: connection.id)?.name == "new")
        #expect(try repo.library().connections.count == 1)
    }

    @Test func deletingFolderCascadesSubfoldersAndOrphansConnections() throws {
        let parent = try repo.save(Folder(name: "Parent"))
        let child = try repo.save(Folder(parentId: parent.id, name: "Child"))
        let connection = try repo.save(Connection(folderId: child.id, type: .rdp, name: "x", host: "h"))

        try repo.deleteFolder(id: parent.id)

        let library = try repo.library()
        #expect(library.folders.isEmpty)
        #expect(try repo.connection(id: connection.id)?.folderId == nil)
    }

    @Test func moveFolderRejectsCycles() throws {
        let a = try repo.save(Folder(name: "A"))
        let b = try repo.save(Folder(parentId: a.id, name: "B"))

        #expect(throws: RepositoryError.folderCycle) { try repo.moveFolder(id: a.id, to: b.id) }
        #expect(throws: RepositoryError.folderCycle) { try repo.moveFolder(id: a.id, to: a.id) }

        let c = try repo.save(Folder(name: "C"))
        try repo.moveFolder(id: c.id, to: b.id)
        #expect(try repo.library().folders.first { $0.id == c.id }?.parentId == b.id)
    }

    @Test func moveMissingFolderThrows() throws {
        #expect(throws: RepositoryError.notFound) { try repo.moveFolder(id: "nope", to: nil) }
    }

    @Test func movesConnectionAndTogglesFavorite() throws {
        let folder = try repo.save(Folder(name: "F"))
        let connection = try repo.save(Connection(type: .ssh, name: "s", host: "h"))

        try repo.moveConnection(id: connection.id, to: folder.id)
        try repo.setFavorite(id: connection.id, true)

        let loaded = try #require(try repo.connection(id: connection.id))
        #expect(loaded.folderId == folder.id)
        #expect(loaded.isFavorite)

        #expect(throws: RepositoryError.notFound) { try repo.moveConnection(id: "nope", to: nil) }
        #expect(throws: RepositoryError.notFound) { try repo.setFavorite(id: "nope", true) }
    }

    @Test func deletesConnection() throws {
        let connection = try repo.save(Connection(type: .ssh, name: "s", host: "h"))
        try repo.deleteConnection(id: connection.id)
        #expect(try repo.connection(id: connection.id) == nil)
    }

    @Test func searchMatchesNameHostUserAndNotes() throws {
        try repo.save(Connection(type: .rdp, name: "Domain Controller", host: "10.1.1.1"))
        try repo.save(Connection(type: .ssh, name: "web", host: "web01.example.com", username: "deploy"))
        try repo.save(Connection(type: .ssh, name: "db", host: "db", notes: "postgres primary"))

        #expect(try repo.search("controller").map(\.name) == ["Domain Controller"])
        #expect(try repo.search("EXAMPLE").map(\.name) == ["web"])
        #expect(try repo.search("deploy").map(\.name) == ["web"])
        #expect(try repo.search("postgres").map(\.name) == ["db"])
        #expect(try repo.search("  ").count == 3)
    }

    @Test func metaUpserts() throws {
        #expect(try repo.meta("k") == nil)
        try repo.setMeta("k", Data([1]))
        try repo.setMeta("k", Data([2]))
        #expect(try repo.meta("k") == Data([2]))
    }

    @Test func opensFileDatabaseWithRollbackJournal() throws {
        let url = FileManager.default.temporaryDirectory
            .appending(path: UUID().uuidString)
            .appending(path: "test.sqlite")
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }

        let database = try AppDatabase.open(at: url)
        try ConnectionRepository(database: database).save(Folder(name: "x"))
        let mode = try database.writer.read { db in try String.fetchOne(db, sql: "PRAGMA journal_mode") }
        #expect(mode == "delete")
        #expect(FileManager.default.fileExists(atPath: url.path))
    }

    @Test func defaultURLLivesInApplicationSupport() {
        #expect(AppDatabase.defaultURL.path.hasSuffix("RemoteDesktop/connections.sqlite"))
    }
}
