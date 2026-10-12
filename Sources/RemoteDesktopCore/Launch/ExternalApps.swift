import Foundation

/// Bundle identifiers and `open(1)` invocations for the external apps we hand connections to.
public enum ExternalApps {
    public static let windowsAppBundleID = "com.microsoft.rdc.macos"
    public static let ghosttyBundleID = "com.mitchellh.ghostty"
    public static let winboxBundleID = "com.mikrotik.winbox"

    /// `open -b com.microsoft.rdc.macos <file.rdp>`
    public static func windowsAppOpenArguments(rdpFile: URL) -> [String] {
        ["-b", windowsAppBundleID, rdpFile.path]
    }

    /// `open -na Ghostty --args -e <script>` — new Ghostty window running the launch script.
    public static func ghosttyOpenArguments(launchScript: URL) -> [String] {
        ["-n", "-b", ghosttyBundleID, "--args", "-e", launchScript.path]
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
