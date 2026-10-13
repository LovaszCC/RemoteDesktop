import Foundation
import RemoteDesktopCore

/// What a file browser window shows: a connection, optionally starting in a remote directory.
struct FileBrowserTarget: Codable, Hashable {
    let connectionId: String
    var startPath: String?
}

/// A file browser window's remote side: one background `ssh` connection master that every listing and
/// transfer reuses (no further authentication), the current remote directory and the transfer queue.
@MainActor
@Observable
final class FileBrowserSession: Identifiable {
    enum Status: Equatable {
        case connecting
        case ready
        case failed(String)
        case closed
    }

    let id = UUID().uuidString
    let target: FileBrowserTarget
    let connectionName: String
    let command: RemoteCommand
    private(set) var status: Status = .connecting
    private(set) var remotePath = "~"
    private(set) var remoteEntries: [RemoteEntry] = []
    private(set) var isListing = false
    var listError: String?
    let transfers: TransferQueue

    /// Writes the master's launch script (decrypting secrets again) for each (re)connect.
    @ObservationIgnored private let makeScript: (String) throws -> SSHLaunchScript.Files
    @ObservationIgnored private var master: Process?
    @ObservationIgnored private var masterError = ""

    static let connectTimeout: Duration = .seconds(30)

    init(target: FileBrowserTarget, connection: Connection, makeScript: @escaping (String) throws -> SSHLaunchScript.Files) {
        self.target = target
        connectionName = connection.name
        command = RemoteCommand(connection: connection, controlPath: RemoteCommand.makeControlPath())
        self.makeScript = makeScript
        transfers = TransferQueue(command: command)
    }

    var isReady: Bool { status == .ready }

    // MARK: Master

    func connect() {
        stopMaster()
        status = .connecting
        do {
            let files = try makeScript(command.controlPath)
            scheduleRemoval(of: files.directory, after: 90)
            let process = Process()
            process.executableURL = URL(filePath: "/bin/sh")
            process.arguments = [files.launchScript.path]
            process.standardInput = FileHandle.nullDevice
            process.standardOutput = FileHandle.nullDevice
            let errors = Pipe()
            process.standardError = errors
            process.terminationHandler = { [weak self] finished in
                let message = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
                Task { @MainActor in self?.masterExited(finished, errorOutput: message) }
            }
            try process.run()
            master = process
            Task { await waitUntilReady(process) }
        } catch {
            status = .failed(error.localizedDescription)
        }
    }

    /// Ready once the control socket answers `ssh -O check`.
    private func waitUntilReady(_ process: Process) async {
        let deadline = ContinuousClock.now + Self.connectTimeout
        let check = ["/usr/bin/ssh", "-o", "ControlPath=\(command.controlPath)", "-O", "check", "rd"]
        while ContinuousClock.now < deadline {
            try? await Task.sleep(for: .milliseconds(300))
            guard master === process, status == .connecting else { return }
            if FileManager.default.fileExists(atPath: command.controlPath),
               (try? await ProcessRunner.run(check))?.succeeded == true {
                guard master === process else { return }
                status = .ready
                let tar = try? await ProcessRunner.run(command.ssh(TransferPlan.remoteTarCheckScript))
                transfers.remoteHasTar = tar.map { String(decoding: $0.output, as: UTF8.self).contains("yes") } ?? true
                await list(target.startPath ?? "~")
                return
            }
        }
        guard master === process, status == .connecting else { return }
        stopMaster()
        status = .failed(String(localized: "The connection didn't open within 30 seconds."))
    }

    private func masterExited(_ finished: Process, errorOutput: String) {
        guard master === finished else { return }
        master = nil
        guard status != .closed else { return }
        let line = errorOutput.split(whereSeparator: \.isNewline)
            .map { $0.trimmingCharacters(in: .whitespaces) }.last { !$0.isEmpty } ?? ""
        status = .failed(line.isEmpty ? String(localized: "The SSH connection closed.") : line)
        transfers.cancelAll()
    }

    private func stopMaster() {
        guard let process = master else { return }
        master = nil
        process.terminate()
        try? FileManager.default.removeItem(atPath: command.controlPath)
    }

    /// Window closed: stops transfers and the connection.
    func close() {
        status = .closed
        transfers.cancelAll()
        stopMaster()
    }

    // MARK: Listing

    func list(_ path: String) async {
        guard isReady else { return }
        isListing = true
        defer { isListing = false }
        do {
            let result = try await ProcessRunner.run(command.ssh(RemoteListing.script(for: path)))
            guard result.succeeded else {
                listError = result.errorLine.isEmpty ? String(localized: "Can't open \(path).") : result.errorLine
                return
            }
            let listing = try RemoteListing.parse(result.output)
            remotePath = listing.path
            remoteEntries = listing.entries
            listError = nil
        } catch {
            listError = String(localized: "Can't read the folder listing of \(path).")
        }
    }

    func refresh() async { await list(remotePath) }
    func goUp() async { await list(RemotePath.parent(remotePath)) }
    func open(_ entry: RemoteEntry) async {
        guard entry.kind == .directory || entry.kind == .symlink else { return }
        await list(RemotePath.join(remotePath, entry.name))
    }

    func exists(_ path: String) async -> Bool {
        (try? await ProcessRunner.run(command.ssh(TransferPlan.remoteExistsScript(path))))?.succeeded == true
    }
}
