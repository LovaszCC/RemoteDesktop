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
    /// Last `user@host…` title the remote shell set, e.g. `root@web: ~`. Program titles (`htop`) don't replace
    /// it, so a root shell stays recognized while a program runs in it.
    private(set) var terminalTitle: String?

    @ObservationIgnored let view: BroadcastingTerminalView
    /// Writes a fresh launch script (decrypting secrets again) for each (re)start.
    @ObservationIgnored private let makeScript: () throws -> SSHLaunchScript.Files
    /// Container currently showing `view` (see `TerminalHostView`).
    @ObservationIgnored weak var host: NSView?
    @ObservationIgnored var onExit: ((TerminalSession, Int32?) -> Void)?
    /// What the user typed or pasted into this terminal (after it went to its own process); set by a split.
    /// Called when the user clicks into this terminal (used by splits to remember the focused pane).
    @ObservationIgnored var onFocus: (() -> Void)? {
        didSet { view.onClick = onFocus }
    }
    @ObservationIgnored var onUserInput: ((TerminalSession, ArraySlice<UInt8>) -> Void)? {
        didSet {
            view.onUserInput = onUserInput.map { handler in { [weak self] data in if let self { handler(self, data) } } }
        }
    }
    @ObservationIgnored private var bridge: DelegateBridge?

    init(connection: Connection, makeScript: @escaping () throws -> SSHLaunchScript.Files) {
        connectionId = connection.id
        title = connection.name
        type = connection.type
        self.makeScript = makeScript
        view = BroadcastingTerminalView(frame: NSRect(x: 0, y: 0, width: 800, height: 500))
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
        terminalTitle = nil
        view.startProcess(
            executable: "/bin/sh",
            args: [files.launchScript.path],
            environment: environment.map { "\($0.key)=\($0.value)" },
            execName: "ssh"
        )
    }

    func reconnect() throws {
        view.feed(text: "\r\n\u{1b}[2m— " + String(localized: "Reconnecting") + " —\u{1b}[0m\r\n")
        try start()
    }

    /// Writes input mirrored from another pane straight to the process (not through the view, so it is
    /// not reported as this pane's own input). Bracketed-paste markers are dropped unless this terminal
    /// asked for them, so shells without the mode (e.g. RouterOS) don't see stray escape sequences.
    func write(_ data: ArraySlice<UInt8>) {
        guard isRunning else { return }
        if BroadcastingTerminalView.isBracketedPasteMarker(data), !view.getTerminal().bracketedPasteMode { return }
        view.process.send(data: data)
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
        if !trimmed.isEmpty {
            title = trimmed
            if RootDetection.user(fromTitle: trimmed) != nil { terminalTitle = trimmed }
        }
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

/// Terminal view that reports keyboard and paste input, but not the replies the emulator itself sends to the
/// program (cursor position reports, device attributes, mouse events).
final class BroadcastingTerminalView: LocalProcessTerminalView {
    var onUserInput: ((ArraySlice<UInt8>) -> Void)?
    private var isEmulatorReply = false

    /// Mouse-down inside the view. (`mouseDown` / `becomeFirstResponder` aren't open in SwiftTerm, so a local
    /// event monitor watches clicks while a handler is set.)
    var onClick: (() -> Void)? {
        didSet {
            if let clickMonitor { NSEvent.removeMonitor(clickMonitor) }
            clickMonitor = onClick == nil ? nil : NSEvent.addLocalMonitorForEvents(matching: .leftMouseDown) { [weak self] event in
                if let self, event.window === self.window,
                   self.bounds.contains(self.convert(event.locationInWindow, from: nil)) {
                    self.onClick?()
                }
                return event
            }
        }
    }
    private var clickMonitor: Any?

    override func send(source: Terminal, data: ArraySlice<UInt8>) {
        isEmulatorReply = true
        defer { isEmulatorReply = false }
        super.send(source: source, data: data)
    }

    override func send(source: TerminalView, data: ArraySlice<UInt8>) {
        super.send(source: source, data: data)
        if !isEmulatorReply { onUserInput?(data) }
    }

    /// `ESC [200~` / `ESC [201~`, sent separately around pasted text.
    private static let bracketedPasteMarkers: [[UInt8]] = [Array("\u{1b}[200~".utf8), Array("\u{1b}[201~".utf8)]

    static func isBracketedPasteMarker(_ data: ArraySlice<UInt8>) -> Bool {
        bracketedPasteMarkers.contains { data.elementsEqual($0) }
    }
}
