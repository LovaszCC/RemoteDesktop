import AppKit
import RemoteDesktopCore

@MainActor
enum SSHLauncher {
    /// Opens a new `terminal` window running `ssh`; secrets are answered via the askpass helper,
    /// or a stored unlocked key is used directly.
    static func launch(_ script: SSHLaunchScript, in terminal: ExternalTerminal) throws -> LaunchOutcome {
        guard isInstalled(terminal.bundleID) else {
            throw LaunchError.appNotInstalled(terminal.displayName)
        }
        let files = try script.write()
        do {
            let command = ExternalApps.terminalLaunch(terminal, launchScript: files.launchScript)
            try runProcess(command.executable, command.arguments)
        } catch {
            try? FileManager.default.removeItem(at: files.directory)
            throw error
        }
        // Long enough for the user to confirm an unknown host key before the password prompt.
        scheduleRemoval(of: files.directory, after: 90)
        return .launched
    }

    static func isInstalled(_ bundleID: String) -> Bool {
        NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) != nil
    }

    /// The external terminal chosen in Settings (Ghostty if installed, else Terminal.app, when unset).
    static var currentTerminal: ExternalTerminal {
        ExternalTerminal.current(isInstalled: isInstalled)
    }
}
