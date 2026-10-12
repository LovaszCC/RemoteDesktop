import AppKit
import RDPBridge
import RemoteDesktopCore
import SwiftUI
import UniformTypeIdentifiers

struct SettingsView: View {
    @Environment(AppState.self) private var state
    @State private var currentPassword = ""
    @State private var newPassword = ""
    @State private var confirmPassword = ""
    @State private var accessibilityTrusted = AccessibilityAutofill.isTrusted(prompt: false)

    var body: some View {
        @Bindable var state = state
        Form {
            Section("Database") {
                LabeledContent("File") {
                    Text(state.databaseURL.path(percentEncoded: false))
                        .textSelection(.enabled)
                        .lineLimit(2)
                        .truncationMode(.middle)
                }
                HStack {
                    Button("Open existing…", action: openExisting)
                    Button("Create new…", action: createNew)
                    Button("Show in Finder") {
                        NSWorkspace.shared.activateFileViewerSelecting([state.databaseURL])
                    }
                }
                Text("Can live in iCloud Drive or Dropbox — passwords are encrypted with the master password.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Security") {
                Toggle("Remember the master key on this Mac (Keychain)", isOn: Binding(
                    get: { state.rememberKey },
                    set: { state.rememberKey = $0 }
                ))
                Button("Lock now") { state.lock() }
                    .disabled(state.phase != .unlocked)
            }

            if state.phase == .unlocked {
                Section("Change master password") {
                    SecureField("Current", text: $currentPassword)
                    SecureField("New", text: $newPassword)
                    SecureField("New again", text: $confirmPassword)
                    Button("Change") {
                        if state.changeMasterPassword(current: currentPassword, new: newPassword) {
                            currentPassword = ""; newPassword = ""; confirmPassword = ""
                        }
                    }
                    .disabled(currentPassword.isEmpty || newPassword.isEmpty || newPassword != confirmPassword)
                }
            }

            Section("Embedded RDP") {
                LabeledContent("FreeRDP", value: RDPSessionBridge.freeRDPVersion())
            }
            Section("RDP password autofill") {
                LabeledContent("Accessibility permission") {
                    Label(accessibilityTrusted ? "Granted" : "Missing",
                          systemImage: accessibilityTrusted ? "checkmark.circle.fill" : "xmark.circle.fill")
                        .foregroundStyle(accessibilityTrusted ? .green : .red)
                }
                HStack {
                    Button("Open System Settings") {
                        _ = AccessibilityAutofill.isTrusted(prompt: true)
                        NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!)
                    }
                    Button("Check") { accessibilityTrusted = AccessibilityAutofill.isTrusted(prompt: false) }
                }
                Text("Without it, the password is copied to the clipboard for 30 seconds.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .frame(width: 520)
        .fixedSize(horizontal: false, vertical: true)
    }

    private func openExisting() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "sqlite") ?? .data, .data]
        panel.prompt = String(localized: "Open")
        if panel.runModal() == .OK, let url = panel.url {
            state.useDatabase(at: url)
        }
    }

    private func createNew() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "connections.sqlite"
        panel.prompt = String(localized: "Create")
        if panel.runModal() == .OK, let url = panel.url {
            state.useDatabase(at: url)
        }
    }
}
