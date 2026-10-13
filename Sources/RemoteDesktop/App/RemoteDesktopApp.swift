import AppKit
import RemoteDesktopCore
import SwiftUI

@main
struct RemoteDesktopApp: App {
    @NSApplicationDelegateAdaptor private var appDelegate: AppDelegate
    @State private var state = AppState()
    @State private var updates = UpdateModel()

    init() {
        // Before any string is looked up, so a Hungarian system doesn't pick Hungarian on its own.
        AppLanguage.applyDefaultIfUnset()
        // Needed when launched as a bare executable (`swift run`) instead of from the .app bundle.
        NSApplication.shared.setActivationPolicy(.regular)
    }

    var body: some Scene {
        Window("RemoteDesktop", id: "main") {
            RootView()
                // Inside .environment(updates): a modifier only sees the environment set outside it.
                .modifier(UpdateWindowOpener())
                .environment(state)
                .environment(updates)
                .frame(minWidth: 760, minHeight: 460)
                .onAppear { appDelegate.attach(state) }
        }
        .defaultSize(width: 1000, height: 640)
        .commands {
            CommandGroup(after: .appInfo) {
                CheckForUpdatesButton().environment(updates)
            }
            CommandGroup(replacing: .newItem) {
                Button(ConnectionType.rdp.newTitle) { state.newConnection(.rdp) }
                    .keyboardShortcut("n")
                Button(ConnectionType.ssh.newTitle) { state.newConnection(.ssh) }
                    .keyboardShortcut("n", modifiers: [.command, .shift])
                Button(ConnectionType.web.newTitle) { state.newConnection(.web) }
                    .keyboardShortcut("b", modifiers: [.command, .shift])
                Button(ConnectionType.mikrotik.newTitle) { state.newConnection(.mikrotik) }
                    .keyboardShortcut("m", modifiers: [.command, .shift])
                Button("New folder") { state.newFolder() }
                    .keyboardShortcut("n", modifiers: [.command, .option])
                Button("New credential") { state.newCredential() }
                    .keyboardShortcut("n", modifiers: [.command, .control])
                Divider()
                Button("Import from RDM…") { state.isImporting = true }
                    .keyboardShortcut("i")
                    .disabled(state.phase != .unlocked)
            }
            CommandMenu("Connection") {
                Button("Connect") {
                    if let tag = state.selection {
                        state.activate(tag: tag)
                    } else if state.selectedSSHConnections.count > 1 {
                        state.openSplit(state.selectedSSHConnections)
                    }
                }
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(state.phase != .unlocked)
                Divider()
                Button("Type clipboard into RDP session") { state.typeClipboardIntoActiveSession() }
                    .keyboardShortcut("v", modifiers: [.command, .shift])
                    .disabled(state.activeRDPSession?.status != .connected)
                Button("Split terminal") { state.splitActiveTab() }
                    .keyboardShortcut("d")
                    .disabled(!state.canSplitActiveTab)
                Button("Broadcast input to all panes") { state.toggleBroadcastInActiveSplit() }
                    .keyboardShortcut("i", modifiers: [.command, .option])
                    .disabled(state.activeSplit == nil)
                Button("Close session") { state.closeActiveSession() }
                    .disabled(state.activeSession == nil)
                Button("Next tab") { state.cycleTabs(forward: true) }
                    .disabled(state.sessions.isEmpty)
                Button("Previous tab") { state.cycleTabs(forward: false) }
                    .disabled(state.sessions.isEmpty)
                Divider()
                Button("Lock") { state.lock() }
                    .keyboardShortcut("l", modifiers: [.command, .control])
                    .disabled(state.phase != .unlocked)
            }
        }

        Window("Software Update", id: "update") {
            UpdateView()
                .environment(updates)
        }
        .windowResizability(.contentSize)
        .defaultPosition(.center)
        .restorationBehavior(.disabled)

        Settings {
            SettingsView()
                .environment(state)
                .environment(updates)
        }
    }
}

/// Quit confirmation for running sessions, and ⌘W / ⌃Tab handling while a session tab is active
/// (the window's own Close item and the terminal view would otherwise take them).
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private weak var state: AppState?
    private var keyMonitor: Any?

    func attach(_ state: AppState) {
        self.state = state
        guard keyMonitor == nil else { return }
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            // Local monitors run on the main thread; pass only Sendable values across.
            let key = KeyPress(
                windowTitle: event.window?.title,
                flags: event.modifierFlags.intersection(.deviceIndependentFlagsMask),
                characters: event.charactersIgnoringModifiers,
                keyCode: event.keyCode
            )
            let handled = MainActor.assumeIsolated { self?.handle(key) == true }
            return handled ? nil : event
        }
    }

    private struct KeyPress: Sendable {
        let windowTitle: String?
        let flags: NSEvent.ModifierFlags
        let characters: String?
        let keyCode: UInt16
    }

    private func handle(_ key: KeyPress) -> Bool {
        // Only the main window; sheets and the Settings window keep their own shortcuts.
        guard let state, !state.sessions.isEmpty, key.windowTitle == "RemoteDesktop" else { return false }
        if key.flags == .command, key.characters == "w", state.activeSession != nil {
            state.closeActiveSession()
            return true
        }
        if key.keyCode == 48, key.flags.contains(.control) { // Tab
            state.cycleTabs(forward: !key.flags.contains(.shift))
            return true
        }
        return false
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let state, state.hasRunningSessions else { return .terminateNow }
        let alert = NSAlert()
        alert.messageText = String(localized: "Sessions are running")
        let count = state.runningSessionCount
        alert.informativeText = String(localized: "\(count) sessions are still running. Quitting closes all of them.")
        alert.addButton(withTitle: String(localized: "Quit"))
        alert.addButton(withTitle: String(localized: "Cancel"))
        guard alert.runModal() == .alertFirstButtonReturn else {
            AppRelaunch.isRequested = false // a later, ordinary quit must not relaunch
            return .terminateCancel
        }
        state.terminateAllSessions()
        return .terminateNow
    }

    func applicationWillTerminate(_ notification: Notification) {
        AppRelaunch.launchReplacementIfRequested()
    }
}
