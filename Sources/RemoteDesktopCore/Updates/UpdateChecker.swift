import Foundation

public enum UpdateCheckResult: Sendable, Equatable {
    case available(UpdateRelease)
    /// Carries the running version as shown to the user.
    case upToDate(String)
}

public enum UpdateError: Error, Equatable, LocalizedError {
    case offline
    case rateLimited
    case badResponse
    case noDownload
    case checksumMismatch

    public var errorDescription: String? {
        switch self {
        case .offline: String(localized: "Couldn't reach GitHub. Check your internet connection.", bundle: .core)
        case .rateLimited: String(localized: "GitHub is limiting requests right now. Try again later.", bundle: .core)
        case .badResponse: String(localized: "GitHub sent an unexpected response.", bundle: .core)
        case .noDownload: String(localized: "This release has no downloadable disk image.", bundle: .core)
        case .checksumMismatch: String(localized: "The download is corrupted. Try again.", bundle: .core)
        }
    }
}

/// Asks GitHub for the latest published release (drafts and prereleases excluded) and compares it with the
/// running version.
public struct UpdateChecker: Sendable {
    public typealias Fetch = @Sendable (URLRequest) async throws -> (Data, URLResponse)

    public static let latestReleaseURL = URL(string: "https://api.github.com/repos/LovaszCC/RemoteDesktop/releases/latest")!
    public static let defaultFetch: Fetch = { try await URLSession.shared.data(for: $0) }

    let currentVersion: String
    let fetch: Fetch

    public init(
        currentVersion: String = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "",
        fetch: @escaping Fetch = UpdateChecker.defaultFetch
    ) {
        self.currentVersion = currentVersion
        self.fetch = fetch
    }

    public func check() async throws -> UpdateCheckResult {
        var request = URLRequest(url: Self.latestReleaseURL, timeoutInterval: 15)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        let release = try UpdateRelease.decode(gitHubJSON: await Self.data(for: request, using: fetch))
        guard let current = AppVersion(currentVersion), release.version > current else {
            return .upToDate(currentVersion)
        }
        return .available(release)
    }

    /// The response body of a successful request; transport and HTTP failures become `UpdateError`s.
    static func data(for request: URLRequest, using fetch: Fetch) async throws -> Data {
        let (data, response) = try await mappingTransportErrors { try await fetch(request) }
        try checkStatus(of: response)
        return data
    }

    static func mappingTransportErrors<T>(_ operation: () async throws -> T) async throws -> T {
        do {
            return try await operation()
        } catch let error as URLError where error.code != .cancelled {
            throw UpdateError.offline
        }
    }

    static func checkStatus(of response: URLResponse) throws {
        switch (response as? HTTPURLResponse)?.statusCode ?? 0 {
        case 200..<300: return
        case 403, 429: throw UpdateError.rateLimited
        default: throw UpdateError.badResponse
        }
    }
}

/// When the silent launch check runs: enabled in Settings (default on) and at most once per 24 hours.
public enum UpdateSchedule {
    public static let automaticChecksKey = "checkForUpdatesAutomatically"
    public static let lastCheckKey = "lastUpdateCheck"
    static let interval: TimeInterval = 24 * 60 * 60

    public static func automaticChecksEnabled(in defaults: UserDefaults = .standard) -> Bool {
        defaults.object(forKey: automaticChecksKey) as? Bool ?? true
    }

    public static func lastCheck(in defaults: UserDefaults = .standard) -> Date? {
        defaults.object(forKey: lastCheckKey) as? Date
    }

    public static func isDue(now: Date = .now, in defaults: UserDefaults = .standard) -> Bool {
        guard automaticChecksEnabled(in: defaults) else { return false }
        guard let last = lastCheck(in: defaults) else { return true }
        return now.timeIntervalSince(last) >= interval
    }

    public static func markChecked(now: Date = .now, in defaults: UserDefaults = .standard) {
        defaults.set(now, forKey: lastCheckKey)
    }
}
