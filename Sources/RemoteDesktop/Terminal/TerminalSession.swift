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

    /// How a "copy next command output" recording ended.
    enum RecordingResult: Equatable {
        case copied(String)
        case empty
        case alternateScreen
        case tooLarge
    }

    let id = UUID().uuidString
    let connectionId: String
    private(set) var title: String
    private(set) var type: ConnectionType
    private(set) var status: Status = .running
    /// Last `user@host…` title the remote shell set, e.g. `root@web: ~`. Program titles (`htop`) don't replace
    /// it, so a root shell stays recognized while a program runs in it.
    private(set) var terminalTitle: String?
    /// "Copy next command output" state (see `OutputRecorder`).
    private(set) var recording: OutputRecorder.Phase = .idle

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
    @ObservationIgnored var onUserInput: ((TerminalSession, ArraySlice<UInt8>) -> Void)?
    /// Called when a recording ends with a result (not when it is cancelled).
    @ObservationIgnored var onRecordingResult: ((RecordingResult) -> Void)?
    @ObservationIgnored private var bridge: DelegateBridge?
    @ObservationIgnored private var recorder = OutputRecorder()
    /// Pending "the prompt is back" check; replaced by every new output chunk.
    @ObservationIgnored private var promptCheck: Task<Void, Never>?

    /// Quiet time after the prompt reappears before the capture counts as finished.
    static let promptSettleTime: Duration = .milliseconds(150)

    init(connection: Connection, makeScript: @escaping () throws -> SSHLaunchScript.Files) {
        connectionId = connection.id
        title = connection.name
        type = connection.type
        self.makeScript = makeScript
        view = BroadcastingTerminalView(frame: NSRect(x: 0, y: 0, width: 800, height: 500))
        view.font = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)
        // Option types the Mac layout's characters (@, |, [ …), like Ghostty; see `TerminalOptionKeys`.
        view.optionAsMetaKey = false
        let bridge = DelegateBridge(session: self)
        self.bridge = bridge
        view.processDelegate = bridge
        view.onUserInput = { [weak self] data in
            guard let self else { return }
            self.recordInput(data)
            self.onUserInput?(self, data)
        }
        view.onOutput = { [weak self] data in self?.recordOutput(data) }
        view.onEscape = { [weak self] in
            guard let self, self.recording != .idle else { return false }
            self.cancelRecording()
            return true
        }
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

    /// Wipes screen and scrollback and has the shell repaint its prompt (see `TerminalClear`). Goes only to
    /// this pane's process, never to broadcast peers.
    func clear() {
        let action = TerminalClear.action(isAlternateScreen: view.getTerminal().isCurrentBufferAlternate, isRunning: isRunning)
        guard action != .none else { return }
        view.feed(text: TerminalClear.localSequence)
        if action == .localAndRedraw { write(TerminalClear.redrawPrompt[...]) }
    }

    func terminate() {
        if isRunning { view.terminate() }
    }

    // MARK: Copy next command output

    /// Arms the recorder: the output of the next command the user enters is copied when its prompt comes back.
    func startRecording() {
        guard isRunning, recording == .idle else { return }
        recorder.arm()
        recording = recorder.phase
        view.window?.makeFirstResponder(view)
    }

    /// The record button: arms when idle, otherwise stops (and copies when already capturing).
    func toggleRecording() {
        if recording == .idle { startRecording() } else { stopRecording() }
    }

    /// Manual stop: copies what was captured so far (nothing yet when still waiting for Enter).
    func stopRecording() {
        switch recording {
        case .idle: return
        case .armed: cancelRecording()
        case .capturing: finishRecording()
        }
    }

    func cancelRecording() {
        promptCheck?.cancel()
        recorder.cancel()
        recording = recorder.phase
    }

    private func recordInput(_ data: ArraySlice<UInt8>) {
        if recorder.userInput(data) { recording = recorder.phase }
    }

    private func recordOutput(_ data: ArraySlice<UInt8>) {
        let event = recorder.output(data)
        guard recording == .capturing else { return }
        promptCheck?.cancel()
        if view.getTerminal().isCurrentBufferAlternate {
            cancelRecording()
            onRecordingResult?(.alternateScreen)
            return
        }
        switch event {
        case .none:
            break
        case .tooLarge:
            cancelRecording()
            onRecordingResult?(.tooLarge)
        case .atPrompt:
            promptCheck = Task { [weak self] in
                try? await Task.sleep(for: Self.promptSettleTime)
                guard !Task.isCancelled else { return }
                self?.finishRecording()
            }
        }
    }

    private func finishRecording() {
        promptCheck?.cancel()
        let text = recorder.finish()
        recording = recorder.phase
        onRecordingResult?(text.isEmpty ? .empty : .copied(text))
    }

    fileprivate func processTerminated(exitCode reported: Int32?) {
        cancelRecording()
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
    /// Everything the process printed, after the terminal has shown it.
    var onOutput: ((ArraySlice<UInt8>) -> Void)?
    /// Esc pressed while this view has the focus; returning true swallows the key.
    var onEscape: (() -> Bool)?
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
    private var optionKeyMonitor: Any?

    /// Option+← / → word jumps (`TerminalOptionKeys`); SwiftTerm's `keyDown` isn't open, so a local monitor
    /// catches them while this view has the focus. Sent like typing, so a broadcasting split mirrors them.
    /// Esc goes to `onEscape` first (cancels a recording).
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if let optionKeyMonitor { NSEvent.removeMonitor(optionKeyMonitor) }
        optionKeyMonitor = nil
        guard window != nil else { return }
        optionKeyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self, event.window === self.window, self.window?.firstResponder === self else { return event }
            let flags = event.modifierFlags.intersection([.option, .command, .control, .shift])
            if event.keyCode == 53, flags.isEmpty, self.onEscape?() == true { return nil }
            guard let text = TerminalOptionKeys.sequence(keyCode: event.keyCode, option: flags.contains(.option),
                                                         otherModifiers: flags != .option) else { return event }
            self.send(txt: text)
            return nil
        }
    }

    override func dataReceived(slice: ArraySlice<UInt8>) {
        super.dataReceived(slice: slice)
        onOutput?(slice)
    }

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
