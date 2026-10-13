import Foundation

enum LaunchError: LocalizedError {
    case appNotInstalled(String)
    case openFailed(String)

    var errorDescription: String? {
        switch self {
        case .appNotInstalled(let name): String(localized: "\(name) is not installed.")
        case .openFailed(let detail): String(localized: "Couldn't launch: \(detail)")
        }
    }
}

/// What the UI should tell the user after a launch.
enum LaunchOutcome: Equatable {
    case launched
    case passwordCopied(reason: String)
    /// Browser opened; password then username were put on the clipboard.
    case loginCopied
    /// Opened in an embedded terminal tab; the tab itself is the feedback.
    case embedded
}

/// Removes a temporary launch directory after a delay.
func scheduleRemoval(of url: URL, after seconds: Double) {
    Task.detached {
        try? await Task.sleep(for: .seconds(seconds))
        try? FileManager.default.removeItem(at: url)
    }
}

/// Runs `/usr/bin/open` with the given arguments and waits for it to hand off.
func runOpen(_ arguments: [String]) throws {
    try runProcess("/usr/bin/open", arguments)
}

/// Runs `executable` with the given arguments and waits for it to finish; a non-zero exit becomes `openFailed`.
func runProcess(_ executable: String, _ arguments: [String]) throws {
    let process = Process()
    process.executableURL = URL(filePath: executable)
    process.arguments = arguments
    let errors = Pipe()
    process.standardError = errors
    try process.run()
    process.waitUntilExit()
    guard process.terminationStatus == 0 else {
        let message = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        throw LaunchError.openFailed(message.trimmingCharacters(in: .whitespacesAndNewlines))
    }
}
