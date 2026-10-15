import Foundation
import Testing
@testable import RemoteDesktopCore

@Suite struct CredentialLinkTests {
    let repo: ConnectionRepository

    init() throws {
        repo = ConnectionRepository(database: try .inMemory())
    }

    @Test func linksAndUnlinksConnections() throws {
        let credential = try repo.save(Credential(name: "admin"))
        let a = try repo.save(Connection(type: .ssh, name: "a", host: "h", username: "own"))
        let b = try repo.save(Connection(type: .rdp, name: "b", host: "h"))

        #expect(try repo.setCredential(credential.id, connectionIds: [a.id, b.id, "missing"]) == 2)
        #expect(Set(try repo.library().connections(using: credential.id).map(\.id)) == [a.id, b.id])

        #expect(try repo.setCredential(credential.id, connectionIds: [a.id]) == 0)

        #expect(try repo.setCredential(nil, connectionIds: [a.id]) == 1)
        let unlinked = try #require(try repo.connection(id: a.id))
        #expect(unlinked.credentialId == nil)
        #expect(unlinked.username == "own")
        #expect(try repo.library().connections(using: credential.id).map(\.id) == [b.id])
    }

    @Test func relinkingMovesConnectionToOtherCredential() throws {
        let first = try repo.save(Credential(name: "first"))
        let second = try repo.save(Credential(name: "second"))
        let connection = try repo.save(Connection(credentialId: first.id, type: .ssh, name: "a", host: "h"))

        try repo.setCredential(second.id, connectionIds: [connection.id])

        let library = try repo.library()
        #expect(library.connections(using: first.id).isEmpty)
        #expect(library.connections(using: second.id).map(\.id) == [connection.id])
    }

    @Test func tagsResolveToConnectionsIncludingFolderTrees() throws {
        let parent = try repo.save(Folder(name: "Parent"))
        let child = try repo.save(Folder(parentId: parent.id, name: "Child"))
        let other = try repo.save(Folder(name: "Other"))
        let direct = try repo.save(Connection(type: .ssh, name: "direct", host: "h"))
        let inParent = try repo.save(Connection(folderId: parent.id, type: .ssh, name: "p", host: "h"))
        let inChild = try repo.save(Connection(folderId: child.id, type: .ssh, name: "c", host: "h"))
        _ = try repo.save(Connection(folderId: other.id, type: .ssh, name: "o", host: "h"))
        let library = try repo.library()

        let ids = library.connectionIds(forTags: ["c:\(direct.id)", "fav:\(direct.id)", "f:\(parent.id)", "c:\(inChild.id)", "k:x"])

        #expect(ids.prefix(2) == [direct.id, inChild.id])
        #expect(Set(ids) == [direct.id, inParent.id, inChild.id])
        #expect(ids.count == 3)
        #expect(library.connectionIds(forTags: ["k:x"]).isEmpty)
    }
}
