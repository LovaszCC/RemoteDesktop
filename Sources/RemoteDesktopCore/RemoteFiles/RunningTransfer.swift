import Foundation

/// The processes of one `TransferPlan`; for a stream, the app copies the source's output into the sink and
/// counts the bytes.
public final class RunningTransfer: @unchecked Sendable {
    public enum Outcome: Sendable, Equatable {
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

    public init(plan: TransferPlan) {
        self.plan = plan
        // A sink that exits early must make our write fail (EPIPE), not kill the process.
        signal(SIGPIPE, SIG_IGN)
    }

    public var bytesCopied: Int64 { lock.withLock { copied } }

    public func cancel() {
        let running = lock.withLock { () -> [Process] in
            cancelled = true
            return processes
        }
        for process in running where process.isRunning { process.terminate() }
    }

    public func run() async -> Outcome {
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
