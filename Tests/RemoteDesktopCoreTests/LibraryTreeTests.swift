import Foundation
import Testing
@testable import RemoteDesktopCore

@Suite struct LibraryTreeTests {
    let prod = Folder(id: "prod", name: "Prod")
    let web = Folder(id: "web", parentId: "prod", name: "Web")
    let archive = Folder(id: "arch", name: "Archive", sortOrder: 1)

    var library: Library {
        Library(
            folders: [archive, web, prod],
            connections: [
                Connection(id: "1", folderId: "web", type: .ssh, name: "web02", host: "10.0.0.2", isFavorite: true),
                Connection(id: "2", folderId: "web", type: .ssh, name: "web01", host: "10.0.0.1"),
                Connection(id: "3", type: .rdp, name: "laptop", host: "laptop.local", username: "bob"),
                Connection(id: "4", folderId: "gone", type: .rdp, name: "orphan", host: "x", isFavorite: true),
                Connection(id: "5", folderId: "prod", type: .rdp, name: "dc01", host: "dc", notes: "domain controller"),
            ]
        )
    }

    @Test func buildsNestedTreeFoldersFirst() {
        let tree = LibraryTree.build(library)
        #expect(tree.map(\.id) == ["f:prod", "f:arch", "c:3", "c:4"])

        let prodChildren = tree[0].children ?? []
        #expect(prodChildren.map(\.id) == ["f:web", "c:5"])
        #expect(prodChildren[0].children?.map(\.id) == ["c:2", "c:1"])
        #expect(tree[1].children == [])
        #expect(tree[2].children == nil)
    }

    @Test func favoritesSortedByName() {
        #expect(LibraryTree.favorites(library).map(\.id) == ["4", "1"])
    }

    @Test func searchAllTermsAcrossFields() {
        #expect(LibraryTree.search(library, query: "").isEmpty)
        #expect(LibraryTree.search(library, query: "WEB").map(\.id) == ["2", "1"])
        #expect(LibraryTree.search(library, query: "prod web01").map(\.id) == ["2"])
        #expect(LibraryTree.search(library, query: "bob").map(\.id) == ["3"])
        #expect(LibraryTree.search(library, query: "controller").map(\.id) == ["5"])
        #expect(LibraryTree.search(library, query: "nomatch").isEmpty)
    }

    @Test func folderPathAndCycleSafety() {
        #expect(LibraryTree.folderPath("web", in: library) == "Prod / Web")
        #expect(LibraryTree.folderPath(nil, in: library) == "")
        let cyclic = Library(folders: [Folder(id: "a", parentId: "b", name: "A"), Folder(id: "b", parentId: "a", name: "B")])
        #expect(LibraryTree.folderPath("a", in: cyclic) == "B / A")
    }

    @Test func flattenedFoldersWithExclusion() {
        let all = LibraryTree.flattenedFolders(library)
        #expect(all.map(\.folder.id) == ["arch", "prod", "web"])
        #expect(all.map(\.depth) == [0, 0, 1])
        #expect(LibraryTree.flattenedFolders(library, excluding: "prod").map(\.folder.id) == ["arch"])
    }
}
