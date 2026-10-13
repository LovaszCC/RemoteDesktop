import AppKit
import RemoteDesktopCore
import SwiftUI

@main
struct RemoteDesktopApp: App {
    @NSApplicationDelegateAdaptor private var appDelegate: AppDelegate
    @State private var state = AppState()
    @State private var updates = UpdateModel()
    @State private var shortcuts = ShortcutStore()

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
                .environment(shortcuts)
                .frame(minWidth: 760, minHeight: 460)
                .onAppear { appDelegate.attach(state, shortcuts: shortcuts) }
        }
        .defaultSize(width: 1000, height: 640)
        .commands {
            CommandGroup(after: .appInfo) {
                CheckForUpdatesButton().environment(updates)
            }
            CommandGroup(replacing: .newItem) {
                Button(ConnectionType.rdp.newTitle) { state.newConnection(.rdp) }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .newRDP))
                Button(ConnectionType.ssh.newTitle) { state.newConnection(.ssh) }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .newSSH))
                Button(ConnectionType.web.newTitle) { state.newConnection(.web) }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .newWeb))
                Button(ConnectionType.mikrotik.newTitle) { state.newConnection(.mikrotik) }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .newMikrotik))
                Button("New folder") { state.newFolder() }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .newFolder))
                Button("New credential") { state.newCredential() }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .newCredential))
                Divider()
                Button("Import from RDM…") { state.isImporting = true }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .importRDM))
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
                .keyboardShortcut(shortcuts.keyboardShortcut(for: .connect))
                .disabled(state.phase != .unlocked)
                Divider()
                Button("Type clipboard into RDP session") { state.typeClipboardIntoActiveSession() }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .typeClipboard))
                    .disabled(state.activeRDPSession?.status != .connected)
                Button("Split terminal") { state.splitActiveTab() }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .splitTerminal))
                    .disabled(!state.canSplitActiveTab)
                Button("Broadcast input to all panes") { state.toggleBroadcastInActiveSplit() }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .broadcastAll))
                    .disabled(state.activeSplit == nil)
                // Also handled by AppDelegate's key monitor, so the terminal and the window's Close don't take them.
                Button("Close session") { state.closeActiveSession() }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .closeTab))
                    .disabled(state.activeSession == nil)
                Button("Next tab") { state.cycleTabs(forward: true) }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .nextTab))
                    .disabled(state.sessions.isEmpty)
                Button("Previous tab") { state.cycleTabs(forward: false) }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .previousTab))
                    .disabled(state.sessions.isEmpty)
                Divider()
                Button("Lock") { state.lock() }
                    .keyboardShortcut(shortcuts.keyboardShortcut(for: .lock))
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
                .environment(shortcuts)
        }
    }
}

/// Quit confirmation for running sessions, and close / next / previous tab shortcuts while a session tab is
/// active (the window's own Close item and the terminal view would otherwise take them).
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private weak var state: AppState?
    private weak var shortcuts: ShortcutStore?
    private var keyMonitor: Any?

    func attach(_ state: AppState, shortcuts: ShortcutStore) {
        self.state = state
        self.shortcuts = shortcuts
        guard keyMonitor == nil else { return }
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            // Local monitors run on the main thread; pass only Sendable values across.
            let key = KeyPress(windowTitle: event.window?.title, shortcut: ShortcutStore.keyShortcut(from: event))
            let handled = MainActor.assumeIsolated { self?.handle(key) == true }
            return handled ? nil : event
        }
    }

    private struct KeyPress: Sendable {
        let windowTitle: String?
        let shortcut: KeyShortcut?
    }

    private func handle(_ key: KeyPress) -> Bool {
        // Only the main window; sheets and the Settings window keep their own shortcuts.
        guard let state, let shortcuts, let pressed = key.shortcut,
              !state.sessions.isEmpty, key.windowTitle == "RemoteDesktop" else { return false }
        switch pressed {
        case shortcuts.shortcut(for: .closeTab) where state.activeSession != nil:
            state.closeActiveSession()
        case shortcuts.shortcut(for: .nextTab):
            state.cycleTabs(forward: true)
        case shortcuts.shortcut(for: .previousTab):
            state.cycleTabs(forward: false)
        default:
            return false
        }
        return true
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
