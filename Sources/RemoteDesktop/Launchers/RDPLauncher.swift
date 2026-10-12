import AppKit
import RemoteDesktopCore

@MainActor
enum RDPLauncher {
    /// Opens a generated `.rdp` file in Windows App, then types the password into its
    /// credential prompt via the Accessibility API (clipboard fallback).
    static func launch(_ connection: Connection, password: String?, onFallback: @escaping @MainActor (String) -> Void) throws -> LaunchOutcome {
        guard NSWorkspace.shared.urlForApplication(withBundleIdentifier: ExternalApps.windowsAppBundleID) != nil else {
            throw LaunchError.appNotInstalled("Windows App")
        }

        let directory = FileManager.default.temporaryDirectory
            .appending(path: "RemoteDesktop")
            .appending(path: UUID().uuidString, directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let file = directory.appending(path: RDPFileBuilder.fileName(for: connection))
        try RDPFileBuilder.contents(for: connection).write(to: file, atomically: true, encoding: .utf8)
        scheduleRemoval(of: directory, after: 60)

        try runOpen(ExternalApps.windowsAppOpenArguments(rdpFile: file))

        guard let password, !password.isEmpty else { return .launched }

        guard AccessibilityAutofill.isTrusted(prompt: true) else {
            Clipboard.copy(password)
            return .passwordCopied(reason: "Accessibility engedély hiányzik")
        }

        Task { @MainActor in
            let filled = await AccessibilityAutofill.fillPassword(
                password,
                bundleIdentifier: ExternalApps.windowsAppBundleID
            )
            if !filled {
                Clipboard.copy(password)
                onFallback("Nem találtam a jelszómezőt — jelszó a vágólapon (30 mp)")
            }
        }
        return .launched
    }
}
