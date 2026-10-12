import AppKit
import RemoteDesktopCore
import SwiftTerm

/// One embedded SSH session: owns its terminal view so output and scrollback survive tab switches.
@MainActor
@Observable
final class TerminalSession: Identifiable {
    enum Status: Equatable {
        case running
        case exited(Int32?)
    }

    let id = UUID().uuidString
    let connectionId: String
    private(set) var title: String
    private(set) var type: ConnectionType
    private(set) var status: Status = .running

    @ObservationIgnored let view: LocalProcessTerminalView
    /// Writes a fresh launch script (decrypting secrets again) for each (re)start.
    @ObservationIgnored private let makeScript: () throws -> SSHLaunchScript.Files
    @ObservationIgnored var onExit: ((TerminalSession, Int32?) -> Void)?
    @ObservationIgnored private var bridge: DelegateBridge?

    init(connection: Connection, makeScript: @escaping () throws -> SSHLaunchScript.Files) {
        connectionId = connection.id
        title = connection.name
        type = connection.type
        self.makeScript = makeScript
        view = LocalProcessTerminalView(frame: NSRect(x: 0, y: 0, width: 800, height: 500))
        view.font = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)
        view.optionAsMetaKey = true
        let bridge = DelegateBridge(session: self)
        self.bridge = bridge
        view.processDelegate = bridge
    }

    var isRunning: Bool { status == .running }

    func start() throws {
        let files = try makeScript()
        scheduleRemoval(of: files.directory, after: 90)
        var environment = ProcessInfo.processInfo.environment
        for pair in Terminal.getEnvironmentVariables(termName: "xterm-256color") {
            let parts = pair.split(separator: "=", maxSplits: 1).map(String.init)
            if parts.count == 2 { environment[parts[0]] = parts[1] }
        }
        status = .running
        view.startProcess(
            executable: "/bin/sh",
            args: [files.launchScript.path],
            environment: environment.map { "\($0.key)=\($0.value)" },
            execName: "ssh"
        )
    }

    func reconnect() throws {
        view.feed(text: "\r\n\u{1b}[2m— Újrakapcsolódás —\u{1b}[0m\r\n")
        try start()
    }

    func terminate() {
        if isRunning { view.terminate() }
    }

    fileprivate func processTerminated(exitCode reported: Int32?) {
        let exitCode = SessionTabs.exitCode(fromReported: reported)
        status = .exited(exitCode)
        onExit?(self, exitCode)
    }

    fileprivate func titleChanged(_ newTitle: String) {
        let trimmed = newTitle.trimmingCharacters(in: .whitespaces)
        if !trimmed.isEmpty { title = trimmed }
    }

    /// SwiftTerm's delegate protocol is not actor-isolated; its callbacks arrive on the main queue,
    /// which the `@preconcurrency` conformance checks at runtime.
    @MainActor
    private final class DelegateBridge: NSObject, @preconcurrency LocalProcessTerminalViewDelegate {
        weak var session: TerminalSession?

        init(session: TerminalSession) { self.session = session }

        func sizeChanged(source: LocalProcessTerminalView, newCols: Int, newRows: Int) {}
        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}

        func setTerminalTitle(source: LocalProcessTerminalView, title: String) {
            session?.titleChanged(title)
        }

        func processTerminated(source: TerminalView, exitCode: Int32?) {
            session?.processTerminated(exitCode: exitCode)
        }
    }
}
