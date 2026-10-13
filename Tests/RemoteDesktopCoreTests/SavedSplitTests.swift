import Foundation
import GRDB
import Testing
@testable import RemoteDesktopCore

@Suite struct SavedSplitTests {
    @Test func migrationAndRoundTripKeepOrderAndDuplicates() throws {
        let queue = try DatabaseQueue()
        try AppDatabase.migrator.migrate(queue, upTo: "v12-connection-color")
        let repo = ConnectionRepository(database: try AppDatabase(queue))
        #expect(try repo.library().savedSplits.isEmpty)

        let a = try repo.save(Connection(type: .ssh, name: "a", host: "a"))
        let b = try repo.save(Connection(type: .mikrotik, name: "b", host: "b"))
        let saved = try repo.save(SavedSplit(name: "switches", connectionIds: [b.id, a.id, a.id]))
        let loaded = try #require(try repo.library().savedSplits.first)
        #expect(loaded.id == saved.id)
        #expect(loaded.name == "switches")
        #expect(loaded.connectionIds == [b.id, a.id, a.id])
    }

    @Test func connectionsSkipMissingKeepOrderAndCap() {
        let a = Connection(id: "a", type: .ssh, name: "a", host: "a")
        let b = Connection(id: "b", type: .ssh, name: "b", host: "b")
        let library = Library(connections: [a, b])
        #expect(SavedSplit(name: "x", connectionIds: ["b", "gone", "a"]).connections(in: library).map(\.id) == ["b", "a"])
        #expect(SavedSplit(name: "x", connectionIds: ["a", "a", "b", "b", "a"]).connections(in: library).count == 4)
        #expect(SavedSplit(name: "x", connectionIds: ["gone"]).connections(in: library).isEmpty)
    }

    @Test func newSplitsGoLastAndReorder() throws {
        let repo = ConnectionRepository(database: try .inMemory())
        let first = try repo.save(SavedSplit(name: "1", connectionIds: []))
        let second = try repo.save(SavedSplit(name: "2", connectionIds: []))
        let third = try repo.save(SavedSplit(name: "3", connectionIds: []))
        #expect(try repo.library().savedSplits.map(\.name) == ["1", "2", "3"])

        try repo.reorderSavedSplits([third.id, first.id, second.id])
        #expect(try repo.library().savedSplits.map(\.name) == ["3", "1", "2"])
    }

    @Test func renameKeepsPlaceAndDeleteRemoves() throws {
        let repo = ConnectionRepository(database: try .inMemory())
        _ = try repo.save(SavedSplit(name: "1", connectionIds: []))
        var second = try repo.save(SavedSplit(name: "2", connectionIds: ["x"]))
        _ = try repo.save(SavedSplit(name: "3", connectionIds: []))

        second.name = "renamed"
        try repo.save(second)
        #expect(try repo.library().savedSplits.map(\.name) == ["1", "renamed", "3"])

        try repo.deleteSavedSplit(id: second.id)
        #expect(try repo.library().savedSplits.map(\.name) == ["1", "3"])
    }
}
