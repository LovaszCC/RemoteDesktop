import AppKit
import RemoteDesktopCore
import SwiftUI

/// Settings tab listing every customizable command with a shortcut recorder.
struct ShortcutsSettingsView: View {
    @Environment(ShortcutStore.self) private var shortcuts
    /// Command whose recorder is listening for keys.
    @State private var recording: ShortcutCommand?
    @State private var errors: [ShortcutCommand: String] = [:]

    var body: some View {
        Form {
            Section("File") {
                ForEach(ShortcutCommand.fileMenu, id: \.self, content: row)
            }
            Section("Connection") {
                ForEach(ShortcutCommand.connectionMenu, id: \.self, content: row)
            }
            Section {
                HStack {
                    Text("Click a shortcut, then press the new keys. ⎋ cancels, ⌫ removes the shortcut.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button("Restore defaults") {
                        recording = nil
                        errors = [:]
                        shortcuts.resetAll()
                    }
                    .disabled(ShortcutCommand.allCases.allSatisfy { !shortcuts.settings.isCustomized($0) })
                }
            }
        }
        .formStyle(.grouped)
        .onDisappear { recording = nil }
    }

    private func row(_ command: ShortcutCommand) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            LabeledContent(command.title) {
                HStack(spacing: 6) {
                    if shortcuts.settings.isCustomized(command) {
                        Button {
                            perform(command) { () throws(ShortcutError) in try shortcuts.reset(command) }
                        } label: {
                            Image(systemName: "arrow.uturn.backward")
                        }
                        .buttonStyle(.borderless)
                        .help("Restore default (\(command.defaultShortcut.displayString))")
                    }
                    ShortcutRecorder(
                        shortcut: shortcuts.shortcut(for: command),
                        isRecording: Binding(
                            get: { recording == command },
                            set: { recording = $0 ? command : nil; errors[command] = nil }
                        )
                    ) { newShortcut in
                        perform(command) { () throws(ShortcutError) in try shortcuts.set(newShortcut, for: command) }
                    }
                }
            }
            if let error = errors[command] {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            }
        }
    }

    /// Applies a change; on success recording ends, on a refused shortcut the error shows and recording goes on.
    private func perform(_ command: ShortcutCommand, _ change: () throws(ShortcutError) -> Void) {
        do {
            try change()
            errors[command] = nil
            recording = nil
        } catch {
            errors[command] = error.message
        }
    }
}

/// Button showing a shortcut; while recording, the next key press (with ⌘, ⌃ or ⌥) becomes the new shortcut.
private struct ShortcutRecorder: View {
    let shortcut: KeyShortcut?
    @Binding var isRecording: Bool
    let onChange: (KeyShortcut?) -> Void

    @State private var monitor: Any?

    var body: some View {
        Button {
            isRecording.toggle()
        } label: {
            Group {
                if isRecording {
                    Text("Press keys…")
                } else if let shortcut {
                    Text(verbatim: shortcut.displayString)
                } else {
                    Text("None").foregroundStyle(.secondary)
                }
            }
            .frame(minWidth: 90)
        }
        .tint(isRecording ? .accentColor : nil)
        .buttonStyle(.bordered)
        .onChange(of: isRecording, initial: true) { updateMonitor() }
        .onDisappear { stopMonitor() }
    }

    private func updateMonitor() {
        stopMonitor()
        guard isRecording else { return }
        // Consumes key presses while recording, so menu shortcuts don't fire.
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            let pressed = ShortcutStore.keyShortcut(from: event)
            MainActor.assumeIsolated { handle(pressed) }
            return nil
        }
    }

    private func handle(_ pressed: KeyShortcut?) {
        guard let pressed else { return }
        if pressed.modifiers.isEmpty, pressed.key == "escape" {
            isRecording = false
        } else if pressed.modifiers.isEmpty, pressed.key == "delete" {
            onChange(nil)
        } else {
            onChange(pressed)
        }
    }

    private func stopMonitor() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
    }
}
