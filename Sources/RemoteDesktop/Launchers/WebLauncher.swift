import AppKit
import RemoteDesktopCore

@MainActor
enum WebLauncher {
    /// Pause between clipboard writes so clipboard managers (Raycast) record each value.
    static let clipboardInterval: Duration = .seconds(1)

    /// Opens the URL in the connection's browser, then copies password and username one after
    /// the other — without the concealed marker or auto-clear, so the clipboard history keeps both.
    static func launch(_ connection: Connection, password: String?) throws -> LaunchOutcome {
        guard let url = WebLaunch.url(from: connection.host) else {
            throw LaunchError.openFailed("Érvénytelen URL: \(connection.host)")
        }
        if let bundleID = connection.browserBundleId,
           NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) == nil {
            throw LaunchError.appNotInstalled(InstalledBrowsers.name(for: bundleID))
        }
        try runOpen(ExternalApps.browserOpenArguments(url: url, bundleID: connection.browserBundleId))

        let sequence = WebLaunch.clipboardSequence(username: connection.username, password: password)
        guard !sequence.isEmpty else { return .launched }
        Task { @MainActor in
            for (index, value) in sequence.enumerated() {
                if index > 0 { try? await Task.sleep(for: clipboardInterval) }
                Clipboard.copy(value, clearAfter: nil, concealed: false)
            }
        }
        return .loginCopied
    }
}

/// Browsers registered for `https` URLs, for the per-connection browser picker.
enum InstalledBrowsers {
    struct Browser: Hashable, Identifiable {
        let bundleID: String
        let name: String
        var id: String { bundleID }
    }

    @MainActor
    static var all: [Browser] {
        guard let probe = URL(string: "https://example.com") else { return [] }
        var seen = Set<String>()
        return NSWorkspace.shared.urlsForApplications(toOpen: probe).compactMap { appURL in
            guard let bundleID = Bundle(url: appURL)?.bundleIdentifier, seen.insert(bundleID).inserted else { return nil }
            return Browser(bundleID: bundleID, name: FileManager.default.displayName(atPath: appURL.path).replacingOccurrences(of: ".app", with: ""))
        }
        .sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }

    /// Display name for a bundle ID, falling back to the ID itself when the app is missing.
    @MainActor
    static func name(for bundleID: String) -> String {
        guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) else { return bundleID }
        return FileManager.default.displayName(atPath: url.path).replacingOccurrences(of: ".app", with: "")
    }
}
