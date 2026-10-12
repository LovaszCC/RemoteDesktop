import AppKit

/// Starts the app again once this instance has quit — used to apply a language change.
@MainActor
enum AppRelaunch {
    /// Set before quitting; acted on only if the quit really happens (not cancelled).
    static var isRequested = false

    /// Relaunching needs the .app bundle; `swift run` has none.
    static var isAvailable: Bool { Bundle.main.bundleURL.pathExtension == "app" }

    /// Called from `applicationWillTerminate`: a detached shell waits for this process to exit, then reopens the app.
    static func launchReplacementIfRequested() {
        guard isRequested, isAvailable else { return }
        let process = Process()
        process.executableURL = URL(filePath: "/bin/sh")
        process.arguments = [
            "-c", "while kill -0 \(ProcessInfo.processInfo.processIdentifier) 2>/dev/null; do sleep 0.2; done; open \"$0\"",
            Bundle.main.bundleURL.path,
        ]
        try? process.run()
    }
}
