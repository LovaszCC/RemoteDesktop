import AppKit
import RemoteDesktopCore
import SwiftUI

@main
struct RemoteDesktopApp: App {
    @NSApplicationDelegateAdaptor private var appDelegate: AppDelegate
    @State private var state = AppState()

    init() {
        // Needed when launched as a bare executable (`swift run`) instead of from the .app bundle.
        NSApplication.shared.setActivationPolicy(.regular)
    }

    var body: some Scene {
        Window("RemoteDesktop", id: "main") {
            RootView()
                .environment(state)
                .frame(minWidth: 760, minHeight: 460)
                .onAppear { appDelegate.attach(state) }
        }
        .defaultSize(width: 1000, height: 640)
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("Új RDP kapcsolat") { state.newConnection(.rdp) }
                    .keyboardShortcut("n")
                Button("Új SSH kapcsolat") { state.newConnection(.ssh) }
                    .keyboardShortcut("n", modifiers: [.command, .shift])
                Button("Új böngésző kapcsolat") { state.newConnection(.web) }
                    .keyboardShortcut("b", modifiers: [.command, .shift])
                Button("Új MikroTik kapcsolat") { state.newConnection(.mikrotik) }
                    .keyboardShortcut("m", modifiers: [.command, .shift])
                Button("Új mappa") { state.newFolder() }
                    .keyboardShortcut("n", modifiers: [.command, .option])
                Button("Új credential") { state.newCredential() }
                    .keyboardShortcut("n", modifiers: [.command, .control])
                Divider()
                Button("Importálás RDM-ből…") { state.isImporting = true }
                    .keyboardShortcut("i")
                    .disabled(state.phase != .unlocked)
            }
            CommandMenu("Kapcsolat") {
                Button("Kapcsolódás") {
                    if let tag = state.selection { state.activate(tag: tag) }
                }
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(state.phase != .unlocked)
                Divider()
                Button("Session bezárása") { state.closeActiveSession() }
                    .disabled(state.activeSession == nil)
                Button("Következő fül") { state.cycleTabs(forward: true) }
                    .disabled(state.sessions.isEmpty)
                Button("Előző fül") { state.cycleTabs(forward: false) }
                    .disabled(state.sessions.isEmpty)
                Divider()
                Button("Zárolás") { state.lock() }
                    .keyboardShortcut("l", modifiers: [.command, .control])
                    .disabled(state.phase != .unlocked)
            }
        }

        Settings {
            SettingsView()
                .environment(state)
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
        alert.messageText = "Futó SSH sessionök"
        let count = state.sessions.filter(\.isRunning).count
        alert.informativeText = "\(count) session még fut. Kilépéskor mind lezárul."
        alert.addButton(withTitle: "Kilépés")
        alert.addButton(withTitle: "Mégse")
        guard alert.runModal() == .alertFirstButtonReturn else { return .terminateCancel }
        state.terminateAllSessions()
        return .terminateNow
    }
}
