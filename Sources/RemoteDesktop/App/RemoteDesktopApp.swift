import AppKit
import RemoteDesktopCore
import SwiftUI

@main
struct RemoteDesktopApp: App {
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
