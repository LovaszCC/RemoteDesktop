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
