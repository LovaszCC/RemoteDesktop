import AppKit
import RemoteDesktopCore

@MainActor
enum WinboxLauncher {
    /// Opens a new WinBox instance that connects and logs in to the device directly.
    static func launch(_ connection: Connection, password: String?) throws -> LaunchOutcome {
        guard NSWorkspace.shared.urlForApplication(withBundleIdentifier: ExternalApps.winboxBundleID) != nil else {
            throw LaunchError.appNotInstalled("WinBox")
        }
        try runOpen(ExternalApps.winboxOpenArguments(host: connection.host, username: connection.username, password: password))
        return .launched
    }
}
