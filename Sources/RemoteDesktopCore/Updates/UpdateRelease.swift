import Foundation

/// The parts of a GitHub release the update check needs.
public struct UpdateRelease: Sendable, Equatable {
    public let version: AppVersion
    /// Release notes (Markdown).
    public let notes: String
    public let pageURL: URL
    public let dmgURL: URL?
    public let checksumURL: URL?

    public init(version: AppVersion, notes: String, pageURL: URL, dmgURL: URL?, checksumURL: URL?) {
        self.version = version
        self.notes = notes
        self.pageURL = pageURL
        self.dmgURL = dmgURL
        self.checksumURL = checksumURL
    }

    /// Decodes a `GET /repos/{owner}/{repo}/releases/latest` response.
    static func decode(gitHubJSON data: Data) throws -> UpdateRelease {
        guard let release = try? JSONDecoder().decode(GitHubRelease.self, from: data),
              let version = AppVersion(release.tagName) else { throw UpdateError.badResponse }
        let asset = { (suffix: String) in release.assets.first { $0.name.hasSuffix(suffix) }?.url }
        return UpdateRelease(
            version: version,
            notes: release.body ?? "",
            pageURL: release.pageURL,
            dmgURL: asset(".dmg"),
            checksumURL: asset(".dmg.sha256")
        )
    }
}

private struct GitHubRelease: Decodable {
    let tagName: String
    let body: String?
    let pageURL: URL
    let assets: [Asset]

    struct Asset: Decodable {
        let name: String
        let url: URL

        enum CodingKeys: String, CodingKey {
            case name
            case url = "browser_download_url"
        }
    }

    enum CodingKeys: String, CodingKey {
        case tagName = "tag_name"
        case body
        case pageURL = "html_url"
        case assets
    }
}
