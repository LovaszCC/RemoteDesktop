import AppKit
import RemoteDesktopCore

@MainActor
enum SSHLauncher {
    /// Opens a new Ghostty window running `ssh`; secrets are answered via the askpass helper,
    /// or a stored unlocked key is used directly.
    static func launch(_ script: SSHLaunchScript) throws -> LaunchOutcome {
        guard NSWorkspace.shared.urlForApplication(withBundleIdentifier: ExternalApps.ghosttyBundleID) != nil else {
            throw LaunchError.appNotInstalled("Ghostty")
        }
        let files = try script.write()
        do {
            try runOpen(ExternalApps.ghosttyOpenArguments(launchScript: files.launchScript))
        } catch {
            try? FileManager.default.removeItem(at: files.directory)
            throw error
        }
        // Long enough for the user to confirm an unknown host key before the password prompt.
        scheduleRemoval(of: files.directory, after: 90)
        return .launched
    }
}
