import Foundation

/// Runs a command to completion off the main thread and collects its output.
public enum ProcessRunner {
    public struct Result: Sendable {
        public let status: Int32
        public let output: Data
        public let errorOutput: String

        public var succeeded: Bool { status == 0 }

        /// Last non-empty stderr line, e.g. "tar: x: Cannot open: Permission denied".
        public var errorLine: String {
            errorOutput.split(whereSeparator: \.isNewline)
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .last { !$0.isEmpty } ?? ""
        }
    }

    public static func run(_ arguments: [String]) async throws -> Result {
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
