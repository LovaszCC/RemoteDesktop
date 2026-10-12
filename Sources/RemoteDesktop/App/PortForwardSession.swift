import Foundation
import RemoteDesktopCore

/// One running `ssh -N -L` tunnel for a saved port forward. Runs without a terminal: the password or key
/// passphrase comes from the askpass helper, and the host key must already be trusted.
@MainActor
@Observable
final class PortForwardSession {
    enum Status: Equatable {
        case starting
        case active
        case failed(String)
        case stopped
    }

    let forward: PortForward
    private(set) var status: Status = .stopped

    /// Writes a fresh tunnel script (decrypting secrets again) for each start.
    @ObservationIgnored private let makeScript: () throws -> SSHLaunchScript.Files
    @ObservationIgnored private var process: Process?

    /// How long ssh gets to authenticate and open the local port.
    static let startTimeout: Duration = .seconds(30)

    init(forward: PortForward, makeScript: @escaping () throws -> SSHLaunchScript.Files) {
        self.forward = forward
        self.makeScript = makeScript
    }

    var isRunning: Bool { status == .starting || status == .active }

    func start() throws {
        stop()
        let port = forward.localPort
        guard !LocalPort.isListening(port) else {
            status = .failed(String(localized: "Local port \(String(port)) is already in use."))
            return
        }
        let files = try makeScript()
        scheduleRemoval(of: files.directory, after: 90)

        let process = Process()
        process.executableURL = URL(filePath: "/bin/sh")
        process.arguments = [files.launchScript.path]
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        let errors = Pipe()
        process.standardError = errors
        let output = ErrorOutput()
        errors.fileHandleForReading.readabilityHandler = { handle in output.append(handle.availableData) }
        process.terminationHandler = { [weak self] finished in
            errors.fileHandleForReading.readabilityHandler = nil
            let code = finished.terminationStatus
            let message = output.lastLine
            Task { @MainActor in self?.didExit(finished, code: code, message: message) }
        }
        status = .starting
        try process.run()
        self.process = process
        Task { await waitUntilListening(process) }
    }

    func stop() {
        guard let process else { return }
        self.process = nil
        status = .stopped
        process.terminate()
    }

    /// Active once ssh accepts connections on the local port.
    private func waitUntilListening(_ process: Process) async {
        let port = forward.localPort
        let deadline = ContinuousClock.now + Self.startTimeout
        while ContinuousClock.now < deadline {
            try? await Task.sleep(for: .milliseconds(250))
            guard self.process === process, status == .starting else { return }
            if await Task.detached(operation: { LocalPort.isListening(port) }).value {
                if self.process === process, status == .starting { status = .active }
                return
            }
        }
        guard self.process === process, status == .starting else { return }
        stop()
        status = .failed(String(localized: "The tunnel didn't open within 30 seconds."))
    }

    private func didExit(_ finished: Process, code: Int32, message: String) {
        guard process === finished else { return } // stopped or restarted meanwhile
        process = nil
        status = .failed(message.isEmpty ? String(localized: "SSH exited (status \(String(code))).") : message)
    }
}

/// Collects ssh's standard error from the pipe's reader thread; keeps only the tail.
private final class ErrorOutput: @unchecked Sendable {
    private let lock = NSLock()
    private var data = Data()

    func append(_ chunk: Data) {
        lock.withLock {
            data.append(chunk)
            if data.count > 8192 { data = data.suffix(4096) }
        }
    }

    /// The last non-empty line, e.g. "Permission denied (publickey,password)."
    var lastLine: String {
        lock.withLock {
            String(decoding: data, as: UTF8.self)
                .split(whereSeparator: \.isNewline)
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .last { !$0.isEmpty } ?? ""
        }
    }
}
