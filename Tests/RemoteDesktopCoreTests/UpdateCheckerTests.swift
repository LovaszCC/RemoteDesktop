import Foundation
import Testing
@testable import RemoteDesktopCore

/// Stubbed HTTP: one status code and body for every request.
func stubFetch(_ status: Int, _ body: String = "") -> UpdateChecker.Fetch {
    { request in
        (Data(body.utf8), HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: nil)!)
    }
}

/// A `releases/latest` response shaped like GitHub's.
func releaseJSON(tag: String, withAssets: Bool = true) -> String {
    let base = "https://github.com/LovaszCC/RemoteDesktop/releases/download/\(tag)"
    let version = tag.hasPrefix("v") ? String(tag.dropFirst()) : tag
    let assets = withAssets ? """
        [{"name": "RemoteDesktop-\(version).dmg", "browser_download_url": "\(base)/RemoteDesktop-\(version).dmg"},
         {"name": "RemoteDesktop-\(version).dmg.sha256", "browser_download_url": "\(base)/RemoteDesktop-\(version).dmg.sha256"}]
        """ : "[]"
    return """
        {"tag_name": "\(tag)", "name": "RemoteDesktop \(version)", "draft": false, "prerelease": false,
         "html_url": "https://github.com/LovaszCC/RemoteDesktop/releases/tag/\(tag)",
         "body": "## What's Changed\\n* Fix launch crash", "assets": \(assets)}
        """
}

@Suite struct UpdateCheckerTests {
    @Test func newerReleaseIsAvailable() async throws {
        let result = try await UpdateChecker(currentVersion: "1.1.1", fetch: stubFetch(200, releaseJSON(tag: "v1.2.0"))).check()
        guard case .available(let release) = result else { Issue.record("expected available, got \(result)"); return }
        #expect(release.version == AppVersion("1.2.0"))
        #expect(release.notes == "## What's Changed\n* Fix launch crash")
        #expect(release.pageURL.absoluteString == "https://github.com/LovaszCC/RemoteDesktop/releases/tag/v1.2.0")
        #expect(release.dmgURL?.lastPathComponent == "RemoteDesktop-1.2.0.dmg")
        #expect(release.checksumURL?.lastPathComponent == "RemoteDesktop-1.2.0.dmg.sha256")
    }

    @Test(arguments: ["1.2.0", "1.2", "1.3.0", "0.0.0-dev.12", ""])
    func notNewerIsUpToDate(current: String) async throws {
        let result = try await UpdateChecker(currentVersion: current, fetch: stubFetch(200, releaseJSON(tag: "v1.2.0"))).check()
        #expect(result == .upToDate(current))
    }

    @Test func releaseWithoutAssetsHasNoDownload() async throws {
        let json = releaseJSON(tag: "v1.2.0", withAssets: false)
        let result = try await UpdateChecker(currentVersion: "1.0.0", fetch: stubFetch(200, json)).check()
        guard case .available(let release) = result else { Issue.record("expected available"); return }
        #expect(release.dmgURL == nil)
        #expect(release.checksumURL == nil)
    }

    @Test func sendsGitHubJSONRequest() async throws {
        let checker = UpdateChecker(currentVersion: "1.0.0") { request in
            #expect(request.url == UpdateChecker.latestReleaseURL)
            #expect(request.value(forHTTPHeaderField: "Accept") == "application/vnd.github+json")
            return try await stubFetch(200, releaseJSON(tag: "v1.0.0"))(request)
        }
        _ = try await checker.check()
    }

    @Test(arguments: [
        (403, UpdateError.rateLimited),
        (429, .rateLimited),
        (404, .badResponse),
        (500, .badResponse),
    ])
    func httpErrors(status: Int, error: UpdateError) async {
        await #expect(throws: error) {
            try await UpdateChecker(currentVersion: "1.0.0", fetch: stubFetch(status)).check()
        }
    }

    @Test(arguments: ["not json", #"{"tag_name": "nightly", "html_url": "https://x", "assets": []}"#, "{}"])
    func unexpectedBodyIsBadResponse(body: String) async {
        await #expect(throws: UpdateError.badResponse) {
            try await UpdateChecker(currentVersion: "1.0.0", fetch: stubFetch(200, body)).check()
        }
    }

    @Test func transportErrorIsOffline() async {
        await #expect(throws: UpdateError.offline) {
            try await UpdateChecker(currentVersion: "1.0.0") { _ in throw URLError(.notConnectedToInternet) }.check()
        }
    }

    @Test func cancellationIsNotReportedAsOffline() async {
        await #expect(throws: URLError.self) {
            try await UpdateChecker(currentVersion: "1.0.0") { _ in throw URLError(.cancelled) }.check()
        }
    }

    @Test(arguments: [UpdateError.offline, .rateLimited, .badResponse, .noDownload, .checksumMismatch])
    func errorsHaveMessages(error: UpdateError) {
        #expect(error.errorDescription?.isEmpty == false)
    }
}

@Suite struct UpdateScheduleTests {
    private let defaults: UserDefaults
    private let suite = "UpdateScheduleTests-\(UUID().uuidString)"

    init() { defaults = UserDefaults(suiteName: suite)! }

    @Test func dueWhenNeverChecked() {
        defer { defaults.removePersistentDomain(forName: suite) }
        #expect(UpdateSchedule.automaticChecksEnabled(in: defaults))
        #expect(UpdateSchedule.lastCheck(in: defaults) == nil)
        #expect(UpdateSchedule.isDue(in: defaults))
    }

    @Test func dueOncePerDay() {
        defer { defaults.removePersistentDomain(forName: suite) }
        let checked = Date(timeIntervalSince1970: 1_000_000)
        UpdateSchedule.markChecked(now: checked, in: defaults)
        #expect(UpdateSchedule.lastCheck(in: defaults) == checked)
        #expect(!UpdateSchedule.isDue(now: checked.addingTimeInterval(23 * 3600), in: defaults))
        #expect(UpdateSchedule.isDue(now: checked.addingTimeInterval(24 * 3600), in: defaults))
    }

    @Test func neverDueWhenTurnedOff() {
        defer { defaults.removePersistentDomain(forName: suite) }
        defaults.set(false, forKey: UpdateSchedule.automaticChecksKey)
        #expect(!UpdateSchedule.automaticChecksEnabled(in: defaults))
        #expect(!UpdateSchedule.isDue(in: defaults))
    }
}
