import Foundation
import GRDB
import Testing
@testable import RemoteDesktopCore

@Suite struct LabelColorTests {
    @Test func rawValuesAreStable() {
        #expect(LabelColor.allCases.map(\.rawValue) == ["red", "orange", "yellow", "green", "blue", "purple", "pink", "gray"])
        #expect(LabelColor.allCases.allSatisfy { !$0.title.isEmpty })
    }

    @Test func commonColor() {
        #expect(LabelColor.common([.red, .red]) == .red)
        #expect(LabelColor.common([.red, .blue]) == nil)
        #expect(LabelColor.common([.red, nil]) == nil)
        #expect(LabelColor.common([nil, nil]) == nil)
        #expect(LabelColor.common([]) == nil)
    }
}

@Suite struct FolderColorTests {
    @Test func migrationLeavesExistingFoldersUncoloredAndRoundTrips() throws {
        let queue = try DatabaseQueue()
        try AppDatabase.migrator.migrate(queue, upTo: "v10-external-terminal")
        try queue.write { db in
            try db.execute(sql: "INSERT INTO folder (id, parentId, name, sortOrder) VALUES ('old', NULL, 'Old', 0)")
        }
        let repo = ConnectionRepository(database: try AppDatabase(queue))
        #expect(try repo.library().folders.first { $0.id == "old" }?.color == nil)

        try repo.save(Folder(id: "red", name: "Prod", color: .red))
        #expect(try repo.library().folders.first { $0.id == "red" }?.color == .red)
    }
}
