import Foundation
import Testing
@testable import RemoteDesktopCore

@Suite struct SidebarDragTests {
    @Test func payloadCarriesSelectionOnlyForSelectedRow() {
        let selection: Set<String> = ["c:2", "f:9", "c:1"]
        #expect(SidebarDrag.payload(for: "c:1", selection: selection) == "c:1\nc:2\nf:9")
        #expect(SidebarDrag.payload(for: "c:7", selection: selection) == "c:7")
        #expect(SidebarDrag.payload(for: "c:1", selection: ["c:1"]) == "c:1")
    }

    @Test func tagsFromPayloads() {
        #expect(SidebarDrag.tags(from: ["c:1\nf:2", "c:3"]) == ["c:1", "f:2", "c:3"])
        #expect(SidebarDrag.tags(from: []).isEmpty)
    }

    @Test func movableMapsFavoritesAndIgnoresCredentials() {
        let items = SidebarDrag.movable(["c:1", "fav:1", "fav:2", "f:9", "k:5", "f:9", "junk"])
        #expect(items.connectionIds == ["1", "2"])
        #expect(items.folderIds == ["9"])
    }
}

@Suite struct SidebarSelectionItemsTests {
    @Test func itemsIncludeCredentials() {
        let items = SidebarDrag.items(["k:5", "c:1", "fav:1", "f:9", "k:5", "x:3"])
        #expect(items.connectionIds == ["1"])
        #expect(items.folderIds == ["9"])
        #expect(items.credentialIds == ["5"])
    }

    @Test func deletingAFolderKeepsItsConnectionsAtTheRoot() throws {
        let repo = ConnectionRepository(database: try .inMemory())
        let parent = try repo.save(Folder(name: "p"))
        let child = try repo.save(Folder(parentId: parent.id, name: "c"))
        let kept = try repo.save(Connection(folderId: child.id, type: .ssh, name: "s", host: "h"))
        try repo.deleteFolder(id: parent.id)
        let library = try repo.library()
        #expect(library.folders.isEmpty)
        #expect(library.connections.first { $0.id == kept.id }?.folderId == nil)
    }
}
