import Foundation

/// Bundle identifiers and `open(1)` invocations for the external apps we hand connections to.
public enum ExternalApps {
    public static let windowsAppBundleID = "com.microsoft.rdc.macos"
    public static let ghosttyBundleID = "com.mitchellh.ghostty"

    /// `open -b com.microsoft.rdc.macos <file.rdp>`
    public static func windowsAppOpenArguments(rdpFile: URL) -> [String] {
        ["-b", windowsAppBundleID, rdpFile.path]
    }

    /// `open -na Ghostty --args -e <script>` — new Ghostty window running the launch script.
    public static func ghosttyOpenArguments(launchScript: URL) -> [String] {
        ["-n", "-b", ghosttyBundleID, "--args", "-e", launchScript.path]
    }

    /// `open [-b <browser>] <url>` — default browser when no bundle ID is given.
    public static func browserOpenArguments(url: URL, bundleID: String?) -> [String] {
        guard let bundleID, !bundleID.isEmpty else { return [url.absoluteString] }
        return ["-b", bundleID, url.absoluteString]
    }
}
