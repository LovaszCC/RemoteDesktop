import Foundation
import RDPBridge
import Testing

/// Serves File Contents Requests from in-memory files, optionally misbehaving.
final class FakeFileServer: @unchecked Sendable {
    enum Fault { case none, refuse, oversize, wrongStream, silent }

    let contents: [Data]
    var fault: Fault
    private let lock = NSLock()
    private(set) var requests: [(stream: UInt32, index: UInt32, kind: RDPFileContentsKind, range: RDPFileRange)] = []
    weak var downloader: RDPClipboardDownloader?

    init(_ contents: [Data], fault: Fault = .none) {
        self.contents = contents
        self.fault = fault
    }

    var sender: RDPFileContentsSender {
        { [self] stream, index, kind, range in
            lock.withLock { requests.append((stream, index, kind, range)) }
            DispatchQueue.global().async { self.answer(stream, index, kind, range) }
        }
    }

    private func answer(_ stream: UInt32, _ index: UInt32, _ kind: RDPFileContentsKind, _ range: RDPFileRange) {
        guard let downloader else { return }
        let file = contents[Int(index)]
        switch fault {
        case .silent: return
        case .refuse: downloader.receiveStreamId(stream, ok: false, data: Data()); return
        case .wrongStream: downloader.receiveStreamId(stream + 100, ok: true, data: Data([1])); return
        default: break
        }
        if kind == .size {
            downloader.receiveStreamId(stream, ok: true, data: withUnsafeBytes(of: UInt64(file.count).littleEndian) { Data($0) })
            return
        }
        let offset = Int(UInt64(range.positionHigh) << 32 | UInt64(range.positionLow))
        var chunk = file.subdata(in: offset..<min(file.count, offset + Int(range.length)))
        if fault == .oversize { chunk.append(contentsOf: [0, 0]) }
        downloader.receiveStreamId(stream, ok: true, data: chunk)
    }
}

@Suite struct ClipboardDownloaderTests {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("rdp-downloader-tests-\(UUID().uuidString)")

    func file(_ path: String, size: UInt64?, directory: Bool = false, modified: Date? = nil) -> RDPRemoteFile {
        RDPRemoteFile(path: path, isDirectory: directory, hasSize: size != nil, size: size ?? 0, modified: modified)
    }

    func run(_ files: [RDPRemoteFile], server: FakeFileServer, stall: TimeInterval = 5) async -> (URL?, String?) {
        let downloader = RDPClipboardDownloader(root: root, sender: server.sender, chunkSize: 3, allowHugeFiles: false,
                                                stallTimeout: stall)
        server.downloader = downloader
        return await withCheckedContinuation { continuation in
            downloader.downloadFiles(files) { folder, error in continuation.resume(returning: (folder, error)) }
        }
    }

    @Test func downloadsFoldersAndFilesInChunks() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let modified = Date(timeIntervalSince1970: 1_700_000_000)
        let server = FakeFileServer([Data(), Data("hello world".utf8), Data("x".utf8)])
        let (folder, error) = await run([
            file("dir", size: 0, directory: true),
            file("dir/hello.txt", size: 11, modified: modified),
            file("sized-by-request.txt", size: nil),
        ], server: server)
        #expect(error == nil)
        let base = try #require(folder)
        #expect(try String(contentsOf: base.appendingPathComponent("dir/hello.txt"), encoding: .utf8) == "hello world")
        #expect(try String(contentsOf: base.appendingPathComponent("sized-by-request.txt"), encoding: .utf8) == "x")
        let attributes = try FileManager.default.attributesOfItem(atPath: base.appendingPathComponent("dir/hello.txt").path)
        #expect(attributes[.modificationDate] as? Date == modified)
        #expect(getxattr(base.appendingPathComponent("dir/hello.txt").path, "com.apple.quarantine", nil, 0, 0, 0) > 0)
        // 11 bytes in chunks of 3 → 4 range requests; the unsized file asks its size first.
        #expect(server.requests.filter { $0.index == 1 }.count == 4)
        #expect(server.requests.first { $0.index == 2 }?.kind == .size)
    }

    @Test func refusedOrBrokenResponsesFailAndLeaveNothing() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        for fault in [FakeFileServer.Fault.refuse, .oversize] {
            let server = FakeFileServer([Data("data".utf8)], fault: fault)
            let (folder, error) = await run([file("a.txt", size: 4)], server: server)
            #expect(folder == nil)
            #expect(error != nil)
        }
        let leftovers = try FileManager.default.contentsOfDirectory(atPath: root.path)
        #expect(leftovers.isEmpty)
    }

    @Test func staleStreamsAreIgnoredAndTheWatchdogFires() async {
        defer { try? FileManager.default.removeItem(at: root) }
        let server = FakeFileServer([Data("data".utf8)], fault: .wrongStream)
        let (folder, error) = await run([file("a.txt", size: 4)], server: server, stall: 0.4)
        #expect(folder == nil)
        #expect(error?.contains("stopped") == true)
    }

    @Test func cancelStopsTheDownload() async {
        defer { try? FileManager.default.removeItem(at: root) }
        let server = FakeFileServer([Data("data".utf8)], fault: .silent)
        let downloader = RDPClipboardDownloader(root: root, sender: server.sender, chunkSize: 3, allowHugeFiles: false,
                                                stallTimeout: 30)
        server.downloader = downloader
        let result: String? = await withCheckedContinuation { continuation in
            downloader.downloadFiles([file("a.txt", size: 4)]) { _, error in continuation.resume(returning: error) }
            downloader.cancel()
        }
        #expect(result == "cancelled")
    }

    @Test func hugeFilesNeedServerSupport() async {
        defer { try? FileManager.default.removeItem(at: root) }
        let server = FakeFileServer([Data()])
        let (folder, error) = await run([file("big.iso", size: 5 << 30)], server: server)
        #expect(folder == nil)
        #expect(error?.contains("4 GB") == true)
        #expect(server.requests.isEmpty)
    }

    @Test func writesOnlyIntoAFreshFolderBelowTheRoot() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let server = FakeFileServer([Data(), Data("x".utf8)])
        let first = await run([file("dir", size: 0, directory: true), file("dir/x", size: 1)], server: server)
        let second = await run([file("dir", size: 0, directory: true), file("dir/x", size: 1)], server: server)
        let a = try #require(first.0), b = try #require(second.0)
        #expect(a != b)
        for folder in [a, b] {
            #expect(folder.deletingLastPathComponent().standardizedFileURL.path == root.standardizedFileURL.path)
        }
    }
}
