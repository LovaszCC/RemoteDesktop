import CryptoKit
import Foundation
import Testing
@testable import RemoteDesktopCore

@Suite struct UpdateDownloaderTests {
    private let directory = FileManager.default.temporaryDirectory.appending(path: "UpdateDownloaderTests-\(UUID().uuidString)")
    private let release = UpdateRelease(
        version: AppVersion("1.2.0")!,
        notes: "",
        pageURL: URL(string: "https://github.com/LovaszCC/RemoteDesktop/releases/tag/v1.2.0")!,
        dmgURL: URL(string: "https://example.com/RemoteDesktop-1.2.0.dmg")!,
        checksumURL: URL(string: "https://example.com/RemoteDesktop-1.2.0.dmg.sha256")!
    )
    private let image = Data("disk image bytes".utf8)

    private var destination: URL { directory.appending(path: "RemoteDesktop-1.2.0.dmg") }

    private static func sha256(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    /// `shasum -a 256` output for `data`.
    private func checksumFetch(for data: Data) -> UpdateChecker.Fetch {
        stubFetch(200, "\(Self.sha256(data))  RemoteDesktop-1.2.0.dmg\n")
    }

    /// Serves `data` as a fresh temporary file, like `URLSession.download(from:)`.
    private static func serving(_ data: Data, status: Int = 200) -> UpdateDownloader.Download {
        { url in
            let file = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
            try data.write(to: file)
            return (file, HTTPURLResponse(url: url, statusCode: status, httpVersion: nil, headerFields: nil)!)
        }
    }

    private func cleanUp() { try? FileManager.default.removeItem(at: directory) }

    @Test func savesVerifiedImage() async throws {
        defer { cleanUp() }
        let downloader = UpdateDownloader(fetch: checksumFetch(for: image), download: Self.serving(image))
        let saved = try await downloader.download(release, to: directory)
        #expect(saved == destination)
        #expect(try Data(contentsOf: saved) == image)
    }

    @Test func rejectsCorruptedImage() async throws {
        defer { cleanUp() }
        let downloader = UpdateDownloader(fetch: checksumFetch(for: image), download: Self.serving(Data("tampered".utf8)))
        await #expect(throws: UpdateError.checksumMismatch) {
            try await downloader.download(release, to: directory)
        }
        #expect(!FileManager.default.fileExists(atPath: destination.path))
    }

    @Test func reusesVerifiedCopy() async throws {
        defer { cleanUp() }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try image.write(to: destination)
        let downloader = UpdateDownloader(fetch: checksumFetch(for: image)) { _ in
            Issue.record("a verified copy must not be downloaded again")
            throw URLError(.badURL)
        }
        #expect(try await downloader.download(release, to: directory) == destination)
    }

    @Test func replacesStaleCopy() async throws {
        defer { cleanUp() }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try Data("older download".utf8).write(to: destination)
        let downloader = UpdateDownloader(fetch: checksumFetch(for: image), download: Self.serving(image))
        _ = try await downloader.download(release, to: directory)
        #expect(try Data(contentsOf: destination) == image)
    }

    @Test func releaseWithoutImageHasNoDownload() async {
        let bare = UpdateRelease(version: release.version, notes: "", pageURL: release.pageURL, dmgURL: nil, checksumURL: nil)
        await #expect(throws: UpdateError.noDownload) {
            try await UpdateDownloader(fetch: stubFetch(200), download: Self.serving(image)).download(bare, to: directory)
        }
    }

    @Test func downloadHTTPErrorIsReported() async {
        defer { cleanUp() }
        let downloader = UpdateDownloader(fetch: checksumFetch(for: image), download: Self.serving(image, status: 404))
        await #expect(throws: UpdateError.badResponse) {
            try await downloader.download(release, to: directory)
        }
    }

    @Test func downloadTransportErrorIsOffline() async {
        let downloader = UpdateDownloader(fetch: checksumFetch(for: image)) { _ in throw URLError(.timedOut) }
        await #expect(throws: UpdateError.offline) {
            try await downloader.download(release, to: directory)
        }
    }

    @Test(arguments: [
        ("\(String(repeating: "a", count: 64))  RemoteDesktop-1.2.0.dmg\n", String(repeating: "a", count: 64)),
        (String(repeating: "AB", count: 32), String(repeating: "ab", count: 32)),
    ])
    func parsesChecksumFile(text: String, hash: String) throws {
        #expect(try UpdateDownloader.checksum(fromFile: Data(text.utf8)) == hash)
    }

    @Test(arguments: ["", "abc  file.dmg", String(repeating: "z", count: 64)])
    func rejectsBadChecksumFile(text: String) {
        #expect(throws: UpdateError.badResponse) { try UpdateDownloader.checksum(fromFile: Data(text.utf8)) }
    }
}
