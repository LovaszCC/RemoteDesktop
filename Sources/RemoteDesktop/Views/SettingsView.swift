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
    @State private var language = AppLanguage.current()
    @State private var showsRestartPrompt = false

    /// Language the running app actually uses (a new choice applies after a restart).
    private let runningLanguage = Bundle.main.preferredLocalizations.first

    var body: some View {
        @Bindable var state = state
        Form {
            Section("Language") {
                Picker("Language", selection: $language) {
                    ForEach(AppLanguage.allCases, id: \.self) { Text(verbatim: $0.displayName).tag($0) }
                }
                .onChange(of: language) {
                    language.apply()
                    showsRestartPrompt = true
                }
                if language.rawValue != runningLanguage {
                    Text("Takes effect after RemoteDesktop restarts.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

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
        .alert("Restart RemoteDesktop to change the language?", isPresented: $showsRestartPrompt) {
            if AppRelaunch.isAvailable {
                Button("Restart now") {
                    AppRelaunch.isRequested = true
                    NSApp.terminate(nil)
                }
                Button("Later", role: .cancel) {}
            } else {
                Button("OK", role: .cancel) {}
            }
        } message: {
            Text("Running sessions are closed when the app quits.")
        }
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
