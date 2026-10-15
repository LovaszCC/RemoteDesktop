import Foundation
import RDPBridge
import Testing

/// Builds CLIPRDR_FILELIST bytes the way a server sends them.
enum FileListBuilder {
    struct Entry {
        var units: [UInt16]
        var directory = false
        var size: UInt64? = 0
        var modified: UInt64? = nil

        init(_ name: String, directory: Bool = false, size: UInt64? = 0, modified: UInt64? = nil) {
            units = Array(name.utf16)
            self.directory = directory
            self.size = size
            self.modified = modified
        }

        init(units: [UInt16]) { self.units = units }
    }

    static func data(_ entries: [Entry], count: UInt32? = nil) -> Data {
        var data = Data()
        data.append(le32(count ?? UInt32(entries.count)))
        for entry in entries {
            var d = [UInt8](repeating: 0, count: 592)
            var flags: UInt32 = 0x04 // FD_ATTRIBUTES
            if entry.size != nil { flags |= 0x40 }
            if entry.modified != nil { flags |= 0x20 }
            put(&d, 0, le32(flags))
            put(&d, 36, le32(entry.directory ? 0x10 : 0x80))
            put(&d, 56, le64(entry.modified ?? 0))
            put(&d, 64, le32(UInt32((entry.size ?? 0) >> 32)))
            put(&d, 68, le32(UInt32((entry.size ?? 0) & 0xFFFF_FFFF)))
            for (i, unit) in entry.units.prefix(260).enumerated() {
                d[72 + i * 2] = UInt8(unit & 0xFF)
                d[73 + i * 2] = UInt8(unit >> 8)
            }
            data.append(contentsOf: d)
        }
        return data
    }

    static func le32(_ v: UInt32) -> Data { withUnsafeBytes(of: v.littleEndian) { Data($0) } }
    static func le64(_ v: UInt64) -> Data { withUnsafeBytes(of: v.littleEndian) { Data($0) } }
    static func put(_ d: inout [UInt8], _ offset: Int, _ bytes: Data) {
        for (i, b) in bytes.enumerated() { d[offset + i] = b }
    }
}

@Suite struct ClipboardFileListTests {
    @Test(arguments: [
        "..\\..\\x", "a\\..\\..\\b", "\\abs", "\\\\srv\\share\\x", "/etc/passwd", "C:\\x", "a:stream", ".\\a",
        "a\\.\\b", "..", ". .", "...", "a\\\\b", "a\\", "tab\there", "nul\u{7F}",
    ])
    func unsafeNamesAreRejected(name: String) {
        #expect(RDPClipboardFileList.sanitizedPath(forRemoteName: name) == nil)
    }

    @Test func namesBecomeRelativePosixPaths() {
        #expect(RDPClipboardFileList.sanitizedPath(forRemoteName: "dir\\sub\\file.txt") == "dir/sub/file.txt")
        #expect(RDPClipboardFileList.sanitizedPath(forRemoteName: "árvíztűrő tükörfúrógép.txt") == "árvíztűrő tükörfúrógép.txt")
        #expect(RDPClipboardFileList.sanitizedPath(forRemoteName: "😀 漢字") == "😀 漢字")
        // Windows ignores trailing dots and spaces.
        #expect(RDPClipboardFileList.sanitizedPath(forRemoteName: "name. \\x.") == "name/x")
    }

    @Test func longComponentsAndDeepPathsAreRejected() {
        #expect(RDPClipboardFileList.sanitizedPath(forRemoteName: String(repeating: "a", count: 256)) == nil)
        #expect(RDPClipboardFileList.sanitizedPath(forRemoteName: String(repeating: "a", count: 255)) != nil)
        let deep = Array(repeating: "d", count: 65).joined(separator: "\\")
        #expect(RDPClipboardFileList.sanitizedPath(forRemoteName: deep) == nil)
    }

    @Test func loneSurrogateIsRejected() {
        let data = FileListBuilder.data([.init(units: [0x61, 0xD800, 0x62])])
        var error: NSString?
        #expect(RDPClipboardFileList.parse(data, maxEntries: 10, error: &error) == nil)
        #expect(error != nil)
    }

    @Test func parsesDirectoriesSizesAndTimes() throws {
        // 2024-01-01T00:00:00Z as FILETIME.
        let filetime: UInt64 = 116_444_736_000_000_000 + 1_704_067_200 * 10_000_000
        let data = FileListBuilder.data([
            .init("dir", directory: true),
            .init("dir\\big.bin", size: 5 << 32 | 7, modified: filetime),
            .init("loose.txt", size: nil),
        ])
        let files = try #require(RDPClipboardFileList.parse(data, maxEntries: 10, error: nil))
        #expect(files.map(\.path) == ["dir", "dir/big.bin", "loose.txt"])
        #expect(files[0].isDirectory)
        #expect(files[1].size == 5 << 32 | 7)
        #expect(files[1].modified == Date(timeIntervalSince1970: 1_704_067_200))
        #expect(!files[2].hasSize)
        #expect(Array(RDPClipboardFileList.topLevelIndices(of: files)) == [0, 2])
    }

    @Test func wholeListFailsOnOneBadEntry() {
        let data = FileListBuilder.data([.init("ok.txt"), .init("..\\evil.txt")])
        var error: NSString?
        #expect(RDPClipboardFileList.parse(data, maxEntries: 10, error: &error) == nil)
        #expect(error?.contains("evil") == true)
    }

    @Test func duplicatesIgnoringCaseAreRejected() {
        let data = FileListBuilder.data([.init("Readme.TXT"), .init("readme.txt")])
        #expect(RDPClipboardFileList.parse(data, maxEntries: 10, error: nil) == nil)
    }

    @Test func entriesBelowAFileAreRejected() {
        let data = FileListBuilder.data([.init("file"), .init("file\\x")])
        #expect(RDPClipboardFileList.parse(data, maxEntries: 10, error: nil) == nil)
    }

    @Test func truncatedOrOversizedListsAreRejected() {
        let full = FileListBuilder.data([.init("a"), .init("b")])
        #expect(RDPClipboardFileList.parse(full.prefix(full.count - 1), maxEntries: 10, error: nil) == nil)
        #expect(RDPClipboardFileList.parse(FileListBuilder.data([.init("a")], count: 3), maxEntries: 10, error: nil) == nil)
        #expect(RDPClipboardFileList.parse(full, maxEntries: 1, error: nil) == nil)
        #expect(RDPClipboardFileList.parse(Data([0, 0, 0, 0]), maxEntries: 10, error: nil) == nil)
        #expect(RDPClipboardFileList.parse(Data([1, 0]), maxEntries: 10, error: nil) == nil)
    }

    @Test func unterminatedNameIsRejected() {
        let data = FileListBuilder.data([.init(String(repeating: "x", count: 260))])
        #expect(RDPClipboardFileList.parse(data, maxEntries: 10, error: nil) == nil)
    }

    @Test func rangesSplitSixtyFourBitOffsets() {
        let size: UInt64 = 5 << 32 + 10
        let range = RDPClipboardFileList.range(forSize: size, offset: 5 << 32, chunk: 4)
        #expect(range.positionHigh == 5 && range.positionLow == 0 && range.length == 4)
        let last = RDPClipboardFileList.range(forSize: size, offset: 5 << 32 + 8, chunk: 4)
        #expect(last.length == 2)
        #expect(RDPClipboardFileList.range(forSize: size, offset: size, chunk: 4).length == 0)
    }
}
