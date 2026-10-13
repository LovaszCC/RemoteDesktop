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
