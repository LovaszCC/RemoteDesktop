import Foundation

/// Runs a command to completion off the main thread and collects its output.
enum ProcessRunner {
    struct Result: Sendable {
        let status: Int32
        let output: Data
        let errorOutput: String

        var succeeded: Bool { status == 0 }

        /// Last non-empty stderr line, e.g. "tar: x: Cannot open: Permission denied".
        var errorLine: String {
            errorOutput.split(whereSeparator: \.isNewline)
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .last { !$0.isEmpty } ?? ""
        }
    }

    static func run(_ arguments: [String]) async throws -> Result {
        try await Task.detached {
            let process = Process()
            process.executableURL = URL(filePath: arguments[0])
            process.arguments = Array(arguments.dropFirst())
            process.standardInput = FileHandle.nullDevice
            let output = Pipe()
            let errors = Pipe()
            process.standardOutput = output
            process.standardError = errors
            try process.run()
            // Read before waiting, so a full pipe can't block the child.
            let data = output.fileHandleForReading.readDataToEndOfFile()
            let errorData = errors.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            return Result(status: process.terminationStatus, output: data,
                          errorOutput: String(decoding: errorData, as: UTF8.self))
        }.value
    }
}
