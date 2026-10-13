import Foundation
import GRDB
import Testing
@testable import RemoteDesktopCore

/// Builds WinBox `.cdb` files like a real export (synthetic data only).
private enum CDB {
    static func string(_ id: UInt32, _ value: String) -> [UInt8] {
        let bytes = Array(value.utf8)
        return [UInt8(id & 0xFF), UInt8(id >> 8 & 0xFF), UInt8(id >> 16 & 0xFF), 0x21, UInt8(bytes.count)] + bytes
    }

    static func flag(_ id: UInt32, _ on: Bool) -> [UInt8] {
        [UInt8(id & 0xFF), UInt8(id >> 8 & 0xFF), UInt8(id >> 16 & 0xFF), on ? 0x01 : 0x00]
    }

    static func byte(_ id: UInt32, _ value: UInt8) -> [UInt8] {
        [UInt8(id & 0xFF), UInt8(id >> 8 & 0xFF), UInt8(id >> 16 & 0xFF), 0x09, value]
    }

    static func record(_ fields: [[UInt8]]) -> [UInt8] {
        let body = Array("M2".utf8) + fields.flatMap { $0 }
        let length = UInt32(body.count)
        return [UInt8(length & 0xFF), UInt8(length >> 8 & 0xFF), UInt8(length >> 16 & 0xFF), UInt8(length >> 24)] + body
    }

    static func file(_ records: [[UInt8]]) -> Data {
        Data([0x0D, 0xF0, 0x1D, 0xC0] + records.flatMap { $0 })
    }

    /// Field order as WinBox writes it.
    static func entry(index: UInt8, name: String, group: String, password: String, user: String, address: String) -> [UInt8] {
        record([flag(7, true), byte(0xFE0001, index), string(0x0B, ""), string(0x0C, "<own>"), string(8, group),
                string(0xFE0009, name), string(4, name), string(3, password), string(2, user), string(1, address)])
    }

    static let sample = file([
        entry(index: 1, name: "core-rtr", group: "Szolnok", password: "s3cret", user: "admin", address: "10.9.0.1"),
        entry(index: 2, name: "Bábolna", group: "Szolnok", password: "", user: "admin", address: "10.9.1.1:8292"),
    ])
}

@Suite struct WinboxImporterTests {
    @Test func parsesEntriesLikeARealExport() throws {
        let items = try WinboxImporter.parse(CDB.sample)
        #expect(items.count == 2)
        let first = items[0]
        #expect(first.type == .mikrotik)
        #expect(first.name == "core-rtr")
        #expect(first.host == "10.9.0.1")
        #expect(first.username == "admin")
        #expect(first.password == "s3cret")
        #expect(first.groupPath == ["Szolnok"])
        #expect(first.notes.isEmpty)
        let second = items[1]
        #expect(second.name == "Bábolna")
        #expect(second.host == "10.9.1.1")
        #expect(second.password == nil)
        #expect(second.notes.contains("8292"))
    }

    @Test func nameFallsBackToNoteThenHostAndNoteGoesToNotes() throws {
        let data = CDB.file([
            CDB.record([CDB.string(4, "rack 3"), CDB.string(1, "192.0.2.5")]),
            CDB.record([CDB.string(0xFE0009, "edge"), CDB.string(4, "rack 4"), CDB.string(1, "192.0.2.6")]),
            CDB.record([CDB.string(1, "192.0.2.7")]),
            CDB.record([CDB.string(2, "no address")]),
        ])
        let items = try WinboxImporter.parse(data)
        #expect(items.map(\.name) == ["rack 3", "edge", "192.0.2.7"])
        #expect(items[1].notes == "rack 4")
        #expect(items[2].groupPath.isEmpty)
    }

    @Test func skipsUnknownIdsAndOtherValueTypes() throws {
        let uint32: [UInt8] = [0x0D, 0x00, 0x00, 0x08, 1, 2, 3, 4]
        let bytes: [UInt8] = [0x0E, 0x00, 0x00, 0x31, 2, 0xAA, 0xBB]
        let data = CDB.file([CDB.record([uint32, bytes, CDB.string(0x77, "x"), CDB.string(1, "192.0.2.9")])])
        #expect(try WinboxImporter.parse(data).map(\.host) == ["192.0.2.9"])
    }

    @Test func rejectsDamagedFiles() throws {
        #expect(throws: WinboxImportError.unreadable) { try WinboxImporter.parse(Data("hello".utf8)) }
        var truncated = [UInt8](CDB.sample)
        truncated.removeLast(5)
        #expect(throws: WinboxImportError.unreadable) { try WinboxImporter.parse(Data(truncated)) }
        let unknownType: [UInt8] = [0x01, 0x00, 0x00, 0x7F]
        #expect(throws: WinboxImportError.unreadable) { try WinboxImporter.parse(CDB.file([CDB.record([unknownType])])) }
        #expect(try WinboxImporter.parse(CDB.file([])).isEmpty)
    }

    @Test func splitsAddresses() {
        #expect(WinboxImporter.splitAddress("10.0.0.1") == ("10.0.0.1", nil))
        #expect(WinboxImporter.splitAddress("10.0.0.1:8292") == ("10.0.0.1", "8292"))
        #expect(WinboxImporter.splitAddress("[fe80::1]:8292") == ("fe80::1", "8292"))
        #expect(WinboxImporter.splitAddress("fe80::1") == ("fe80::1", nil))
        #expect(WinboxImporter.splitAddress("router.lan") == ("router.lan", nil))
    }

    @Test func writesUnderTargetWithGroupFoldersAndDetectsDuplicates() throws {
        let repo = ConnectionRepository(database: try .inMemory())
        let vault = try Vault.create(password: "pw", in: repo, iterations: 1_000)
        let target = Folder(name: "Mikrotik")
        try repo.save(target)
        let items = try WinboxImporter.parse(CDB.sample)

        let first = try ImportWriter.write(connections: items, under: target.id, into: repo, vault: vault)
        #expect(first.imported == 2)
        #expect(first.foldersCreated == 1)
        let library = try repo.library()
        let group = try #require(library.folders.first { $0.name == "Szolnok" })
        #expect(group.parentId == target.id)
        let imported = library.connections.filter { $0.folderId == group.id }
        #expect(imported.count == 2)
        #expect(try vault.decryptOptional(imported.first { $0.name == "core-rtr" }?.passwordEnc) == "s3cret")

        #expect(WinboxImporter.isDuplicate(items[0], among: library.connections, folderId: group.id))
        #expect(!WinboxImporter.isDuplicate(items[0], among: library.connections, folderId: target.id))
        var other = items[0]
        other.username = "someone"
        #expect(!WinboxImporter.isDuplicate(other, among: library.connections, folderId: group.id))

        // Importing again reuses the group folder.
        let second = try ImportWriter.write(connections: [items[0]], under: target.id, into: repo, vault: vault)
        #expect(second.foldersCreated == 0)
    }

    /// Checks a real export without printing anything from it: `RD_WINBOX_CDB=/path/to/export.cdb swift test`.
    @Test(.enabled(if: ProcessInfo.processInfo.environment["RD_WINBOX_CDB"] != nil))
    func parsesRealExport() throws {
        let path = try #require(ProcessInfo.processInfo.environment["RD_WINBOX_CDB"])
        let items = try WinboxImporter.parse(Data(contentsOf: URL(filePath: path)))
        #expect(!items.isEmpty)
        #expect(items.allSatisfy { !$0.host.isEmpty && !$0.name.isEmpty && $0.type == .mikrotik })
    }
}
