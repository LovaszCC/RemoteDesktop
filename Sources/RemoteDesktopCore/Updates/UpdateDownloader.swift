import CryptoKit
import Foundation

/// Downloads a release's disk image and checks it against the release's `.sha256` file.
public struct UpdateDownloader: Sendable {
    /// Downloads to a temporary file the caller then owns.
    public typealias Download = @Sendable (URL) async throws -> (URL, URLResponse)

    public static let defaultDownload: Download = { try await URLSession.shared.download(from: $0) }

    let fetch: UpdateChecker.Fetch
    let download: Download

    public init(fetch: @escaping UpdateChecker.Fetch = UpdateChecker.defaultFetch,
                download: @escaping Download = UpdateDownloader.defaultDownload) {
        self.fetch = fetch
        self.download = download
    }

    /// Saves the release's DMG into `directory` (replacing a stale copy) and returns its location.
    /// A copy already there with the right checksum is reused without downloading.
    public func download(_ release: UpdateRelease, to directory: URL) async throws -> URL {
        guard let dmgURL = release.dmgURL, let checksumURL = release.checksumURL else { throw UpdateError.noDownload }
        let expected = try Self.checksum(
            fromFile: await UpdateChecker.data(for: URLRequest(url: checksumURL, timeoutInterval: 15), using: fetch))
        let destination = directory.appending(path: dmgURL.lastPathComponent)
        if (try? Self.sha256(of: destination)) == expected { return destination }

        let (temporary, response) = try await UpdateChecker.mappingTransportErrors { try await download(dmgURL) }
        defer { try? FileManager.default.removeItem(at: temporary) }
        try UpdateChecker.checkStatus(of: response)
        guard try Self.sha256(of: temporary) == expected else { throw UpdateError.checksumMismatch }

        let fileManager = FileManager.default
        try fileManager.createDirectory(at: directory, withIntermediateDirectories: true)
        try? fileManager.removeItem(at: destination)
        try fileManager.moveItem(at: temporary, to: destination)
        return destination
    }

    /// The hash from a `shasum -a 256` line: `<hex>  <file name>`.
    static func checksum(fromFile data: Data) throws -> String {
        let hash = String(decoding: data, as: UTF8.self).split(whereSeparator: \.isWhitespace).first.map { $0.lowercased() }
        guard let hash, hash.count == 64, hash.allSatisfy(\.isHexDigit) else { throw UpdateError.badResponse }
        return hash
    }

    /// Lowercase hex SHA-256 of a file, read in chunks.
    static func sha256(of url: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }
}
