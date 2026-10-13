import Foundation

/// Bundle identifiers and `open(1)` invocations for the external apps we hand connections to.
public enum ExternalApps {
    public static let windowsAppBundleID = "com.microsoft.rdc.macos"
    public static let winboxBundleID = "com.mikrotik.winbox"

    /// `open -b com.microsoft.rdc.macos <file.rdp>`
    public static func windowsAppOpenArguments(rdpFile: URL) -> [String] {
        ["-b", windowsAppBundleID, rdpFile.path]
    }

    /// Executable and arguments that open a new `terminal` window running the launch script:
    /// - Ghostty: `open -n -b com.mitchellh.ghostty --args -e <script>`
    /// - Terminal.app: `open -b com.apple.Terminal <script>` (runs the executable script)
    /// - iTerm2: `osascript -e <script>` — it has no command-line option for this.
    public static func terminalLaunch(_ terminal: ExternalTerminal, launchScript: URL) -> (executable: String, arguments: [String]) {
        switch terminal {
        case .ghostty: ("/usr/bin/open", ["-n", "-b", terminal.bundleID, "--args", "-e", launchScript.path])
        case .terminal: ("/usr/bin/open", ["-b", terminal.bundleID, launchScript.path])
        case .iterm2: ("/usr/bin/osascript", ["-e", iTermAppleScript(launchScript: launchScript)])
        }
    }

    /// New iTerm2 window with the default profile, running the launch script instead of a shell.
    /// iTerm2 splits `command` like a shell, so the path is POSIX-quoted, then escaped for the AppleScript string.
    static func iTermAppleScript(launchScript: URL) -> String {
        let command = SSHLaunchScript.quote(launchScript.path)
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
        return """
        tell application id "\(ExternalTerminal.iterm2.bundleID)"
            activate
            create window with default profile command "\(command)"
        end tell
        """
    }

    /// `open -n -b com.mikrotik.winbox --args <host> [<user> [<password>]]` — WinBox logs in directly.
    /// WinBox only accepts the password positionally, after the user.
    public static func winboxOpenArguments(host: String, username: String, password: String?) -> [String] {
        var args = ["-n", "-b", winboxBundleID, "--args", host.trimmingCharacters(in: .whitespaces)]
        let user = username.trimmingCharacters(in: .whitespaces)
        guard !user.isEmpty else { return args }
        args.append(user)
        if let password, !password.isEmpty { args.append(password) }
        return args
    }

    /// `open [-b <browser>] <url>` — default browser when no bundle ID is given.
    public static func browserOpenArguments(url: URL, bundleID: String?) -> [String] {
        guard let bundleID, !bundleID.isEmpty else { return [url.absoluteString] }
        return ["-b", bundleID, url.absoluteString]
    }
}
