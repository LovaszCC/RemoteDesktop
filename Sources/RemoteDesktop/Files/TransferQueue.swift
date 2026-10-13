import Foundation
import RemoteDesktopCore

/// Downloads and uploads of one file browser window, run one at a time.
@MainActor
@Observable
final class TransferQueue {
    @MainActor
    @Observable
    final class Item: Identifiable {
        enum Status: Equatable {
            case waiting, running, done, skipped, cancelled
            case failed(String)
        }

        let id = UUID()
        let name: String
        let direction: TransferDirection
        let sourceDirectory: String
        let destinationDirectory: String
        var status: Status = .waiting
        var bytesDone: Int64 = 0
        /// nil while unknown (or for an scp copy, which reports no progress).
        var bytesTotal: Int64?

        init(name: String, direction: TransferDirection, sourceDirectory: String, destinationDirectory: String) {
            self.name = name
            self.direction = direction
            self.sourceDirectory = sourceDirectory
            self.destinationDirectory = destinationDirectory
        }

        var isFinished: Bool { ![.waiting, .running].contains(status) }

        var fraction: Double? {
            guard let bytesTotal, bytesTotal > 0 else { return nil }
            return min(Double(bytesDone) / Double(bytesTotal), status == .done ? 1 : 0.99)
        }
    }

    /// A destination that already exists, waiting for the user's choice.
    struct Conflict: Identifiable {
        let id = UUID()
        let name: String
        let isFolder: Bool
        fileprivate let answer: (ConflictChoice, Bool) -> Void
    }

    private(set) var items: [Item] = []
    /// Compress the tar stream with gzip (good for text, can be slower for already compressed data or fast LANs).
    var compress = true
    var pendingConflict: Conflict?
    /// Set after connecting; without tar (e.g. RouterOS) only single files can be copied, with scp.
    var remoteHasTar = true
    /// Called after each finished item, so the destination pane can refresh.
    @ObservationIgnored var onItemFinished: ((TransferDirection) -> Void)?

    @ObservationIgnored private let command: RemoteCommand
    @ObservationIgnored private var policy: ConflictPolicy = .ask
    @ObservationIgnored private var isRunning = false
    @ObservationIgnored private var current: (item: Item, transfer: RunningTransfer)?

    init(command: RemoteCommand) {
        self.command = command
        // A sink that exits early must make our write fail (EPIPE), not kill the app.
        signal(SIGPIPE, SIG_IGN)
    }

    var activeCount: Int { items.filter { !$0.isFinished }.count }

    // MARK: Queue

    func enqueue(names: [String], direction: TransferDirection, from sourceDirectory: String, to destinationDirectory: String) {
        if activeCount == 0 { policy = .ask }
        for name in names {
            items.append(Item(name: name, direction: direction, sourceDirectory: sourceDirectory,
                              destinationDirectory: destinationDirectory))
        }
        runNext()
    }

    func cancel(_ item: Item) {
        switch item.status {
        case .waiting: item.status = .cancelled
        case .running: if current?.item === item { current?.transfer.cancel() }
        default: break
        }
    }

    func cancelAll() {
        for item in items where !item.isFinished { cancel(item) }
        if let conflict = pendingConflict {
            pendingConflict = nil
            conflict.answer(.skip, true)
        }
    }

    func clearFinished() {
        items.removeAll(where: \.isFinished)
    }

    private func runNext() {
        guard !isRunning, let item = items.first(where: { $0.status == .waiting }) else { return }
        isRunning = true
        item.status = .running
        Task {
            await run(item)
            isRunning = false
            onItemFinished?(item.direction)
            runNext()
        }
    }

    // MARK: One item

    private func destinationPath(_ item: Item) -> String {
        switch item.direction {
        case .download: (item.destinationDirectory as NSString).appendingPathComponent(item.name)
        case .upload: RemotePath.join(item.destinationDirectory, item.name)
        }
    }

    private func sourcePath(_ item: Item) -> String {
        switch item.direction {
        case .download: RemotePath.join(item.sourceDirectory, item.name)
        case .upload: (item.sourceDirectory as NSString).appendingPathComponent(item.name)
        }
    }

    private func destinationExists(_ item: Item) async -> Bool {
        switch item.direction {
        case .download:
            return FileManager.default.fileExists(atPath: destinationPath(item))
        case .upload:
            let result = try? await ProcessRunner.run(command.ssh(TransferPlan.remoteExistsScript(destinationPath(item))))
            return result?.succeeded == true
        }
    }

    private func isDirectory(_ item: Item) async -> Bool {
        switch item.direction {
        case .upload:
            var isDirectory: ObjCBool = false
            FileManager.default.fileExists(atPath: sourcePath(item), isDirectory: &isDirectory)
            return isDirectory.boolValue
        case .download:
            let result = try? await ProcessRunner.run(command.ssh("test -d \(SSHLaunchScript.quote(sourcePath(item)))"))
            return result?.succeeded == true
        }
    }

    private func run(_ item: Item) async {
        let existed = await destinationExists(item)
        let folder = await isDirectory(item)
        if folder, !remoteHasTar {
            item.status = .failed(String(localized: "Folders need tar on the server."))
            return
        }
        switch await resolveConflict(item, exists: existed, isFolder: folder) {
        case .skip:
            item.status = .skipped
            return
        case .replace:
            break
        }
        guard item.status == .running else { return } // cancelled while asking

        item.bytesTotal = await size(of: item)
        let plan = TransferPlan.make(name: item.name, direction: item.direction, sourceDirectory: item.sourceDirectory,
                                     destinationDirectory: item.destinationDirectory, compress: compress,
                                     remoteHasTar: remoteHasTar, command: command)
        if case .copy = plan { item.bytesTotal = nil }
        let transfer = RunningTransfer(plan: plan)
        current = (item, transfer)
        let progress = Task { @MainActor in
            while !Task.isCancelled {
                item.bytesDone = transfer.bytesCopied
                try? await Task.sleep(for: .milliseconds(250))
            }
        }
        let outcome = await transfer.run()
        progress.cancel()
        current = nil
        item.bytesDone = transfer.bytesCopied

        switch outcome {
        case .succeeded:
            item.status = .done
        case .cancelled:
            item.status = .cancelled
            if !existed { await removePartial(item) }
        case .failed(let message):
            item.status = .failed(message.isEmpty ? String(localized: "The transfer failed.") : message)
            if !existed { await removePartial(item) }
        }
    }

    private func resolveConflict(_ item: Item, exists: Bool, isFolder: Bool) async -> ConflictChoice {
        if let choice = policy.resolve(exists: exists) { return choice }
        return await withCheckedContinuation { continuation in
            pendingConflict = Conflict(name: item.name, isFolder: isFolder) { [weak self] choice, applyToAll in
                if applyToAll { self?.policy = .remembering(choice) }
                continuation.resume(returning: choice)
            }
        }
    }

    func answerConflict(_ choice: ConflictChoice, applyToAll: Bool) {
        guard let conflict = pendingConflict else { return }
        pendingConflict = nil
        conflict.answer(choice, applyToAll)
    }

    /// Bytes to copy: `du` on the server, the files' sizes here.
    private func size(of item: Item) async -> Int64? {
        switch item.direction {
        case .download:
            let result = try? await ProcessRunner.run(command.ssh(TransferPlan.remoteSizeScript(sourcePath(item))))
            return result.flatMap { TransferPlan.parseDuOutput(String(decoding: $0.output, as: UTF8.self)) }
        case .upload:
            let path = sourcePath(item)
            return await Task.detached { Self.localSize(path) }.value
        }
    }

    nonisolated static func localSize(_ path: String) -> Int64 {
        let url = URL(filePath: path)
        guard let enumerator = FileManager.default.enumerator(at: url, includingPropertiesForKeys: [.fileSizeKey]) else {
            return (try? FileManager.default.attributesOfItem(atPath: path)[.size] as? Int64) ?? 0
        }
        var total = (try? FileManager.default.attributesOfItem(atPath: path)[.size] as? Int64) ?? 0
        for case let file as URL in enumerator {
            total += Int64((try? file.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0)
        }
        return total
    }

    /// A cancelled or failed copy into a destination that didn't exist before leaves nothing behind.
    private func removePartial(_ item: Item) async {
        let path = destinationPath(item)
        switch item.direction {
        case .download:
            try? FileManager.default.removeItem(atPath: path)
        case .upload:
            _ = try? await ProcessRunner.run(command.ssh("rm -rf -- \(SSHLaunchScript.quote(path))"))
        }
    }
}

/// The processes of one `TransferPlan`; for a stream, the app copies the source's output into the sink and
/// counts the bytes.
final class RunningTransfer: @unchecked Sendable {
    enum Outcome: Sendable {
        case succeeded, cancelled
        case failed(String)
    }

    private let plan: TransferPlan
    private let lock = NSLock()
    private var processes: [Process] = []
    /// Tail of each process's standard error, drained while it runs (a full pipe would block it).
    private var errorTails: [Data] = []
    private var copied: Int64 = 0
    private var cancelled = false

    init(plan: TransferPlan) {
        self.plan = plan
    }

    var bytesCopied: Int64 { lock.withLock { copied } }

    func cancel() {
        let running = lock.withLock { () -> [Process] in
            cancelled = true
            return processes
        }
        for process in running where process.isRunning { process.terminate() }
    }

    func run() async -> Outcome {
        await Task.detached { self.runBlocking() }.value
    }

    private func makeProcess(_ arguments: [String]) -> Process {
        let process = Process()
        process.executableURL = URL(filePath: arguments[0])
        process.arguments = Array(arguments.dropFirst())
        let errors = Pipe()
        process.standardError = errors
        let index = lock.withLock { () -> Int in
            processes.append(process)
            errorTails.append(Data())
            return errorTails.count - 1
        }
        errors.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let chunk = handle.availableData
            guard let self else { return }
            if chunk.isEmpty {
                handle.readabilityHandler = nil
                return
            }
            self.lock.withLock {
                self.errorTails[index].append(chunk)
                if self.errorTails[index].count > 8192 { self.errorTails[index] = self.errorTails[index].suffix(4096) }
            }
        }
        return process
    }

    /// Chains processes with pipes; returns them with the chain's input and output pipe.
    private func chain(_ commands: [[String]], input: Any, output: Any) -> [Process] {
        let chained = commands.map(makeProcess)
        for (index, process) in chained.enumerated() {
            process.standardInput = index == 0 ? input : chained[index - 1].standardOutput
            process.standardOutput = index == chained.count - 1 ? output : Pipe()
        }
        return chained
    }

    private func runBlocking() -> Outcome {
        let all: [Process]
        switch plan {
        case .copy(let arguments):
            let process = makeProcess(arguments)
            process.standardInput = FileHandle.nullDevice
            process.standardOutput = FileHandle.nullDevice
            all = [process]
            guard start(all) else { return failure() }
        case .stream(let source, let sink):
            let fromSource = Pipe()
            let toSink = Pipe()
            let sources = chain(source, input: FileHandle.nullDevice, output: fromSource)
            let sinks = chain(sink, input: toSink, output: FileHandle.nullDevice)
            all = sources + sinks
            guard start(sinks + sources) else { return failure() }
            // Our copies of the children's pipe ends must be closed, or the readers never see EOF.
            for process in all {
                if let pipe = process.standardOutput as? Pipe, pipe !== fromSource { try? pipe.fileHandleForWriting.close() }
            }
            try? fromSource.fileHandleForWriting.close()
            try? toSink.fileHandleForReading.close()
            pump(from: fromSource.fileHandleForReading, to: toSink.fileHandleForWriting)
        }
        for process in all { process.waitUntilExit() }
        if lock.withLock({ cancelled }) { return .cancelled }
        return all.allSatisfy { $0.terminationStatus == 0 } ? .succeeded : failure()
    }

    private func start(_ processes: [Process]) -> Bool {
        do {
            for process in processes { try process.run() }
            return true
        } catch {
            cancel()
            return false
        }
    }

    private func pump(from reader: FileHandle, to writer: FileHandle) {
        while true {
            let chunk = reader.availableData
            if chunk.isEmpty { break }
            do {
                try writer.write(contentsOf: chunk)
            } catch {
                break // the sink exited; its status says why
            }
            lock.withLock { copied += Int64(chunk.count) }
        }
        try? writer.close()
    }

    /// The last error line of the processes that failed.
    private func failure() -> Outcome {
        Thread.sleep(forTimeInterval: 0.1) // let the stderr handlers take the last lines
        let (all, tails) = lock.withLock { (processes, errorTails) }
        let lines = zip(all, tails).compactMap { process, data -> String? in
            // Never launched (pid 0): no status to read.
            guard process.processIdentifier != 0, process.isRunning || process.terminationStatus != 0 else { return nil }
            return String(decoding: data, as: UTF8.self).split(whereSeparator: \.isNewline)
                .map { $0.trimmingCharacters(in: .whitespaces) }.last { !$0.isEmpty }
        }
        return .failed(lines.first ?? "")
    }
}
