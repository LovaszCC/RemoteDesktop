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
            Section("Adatbázis") {
                LabeledContent("Fájl") {
                    Text(state.databaseURL.path(percentEncoded: false))
                        .textSelection(.enabled)
                        .lineLimit(2)
                        .truncationMode(.middle)
                }
                HStack {
                    Button("Meglévő megnyitása…", action: openExisting)
                    Button("Új létrehozása…", action: createNew)
                    Button("Megjelenítés Finderben") {
                        NSWorkspace.shared.activateFileViewerSelecting([state.databaseURL])
                    }
                }
                Text("iCloud Drive / Dropbox mappába is tehető — a jelszavak a mesterjelszóval titkosítva vannak.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Biztonság") {
                Toggle("Mesterkulcs megjegyzése ezen a Macen (Keychain)", isOn: Binding(
                    get: { state.rememberKey },
                    set: { state.rememberKey = $0 }
                ))
                Button("Zárolás most") { state.lock() }
                    .disabled(state.phase != .unlocked)
            }

            if state.phase == .unlocked {
                Section("Mesterjelszó módosítása") {
                    SecureField("Jelenlegi", text: $currentPassword)
                    SecureField("Új", text: $newPassword)
                    SecureField("Új újra", text: $confirmPassword)
                    Button("Módosítás") {
                        if state.changeMasterPassword(current: currentPassword, new: newPassword) {
                            currentPassword = ""; newPassword = ""; confirmPassword = ""
                        }
                    }
                    .disabled(currentPassword.isEmpty || newPassword.isEmpty || newPassword != confirmPassword)
                }
            }

            Section("Beágyazott RDP") {
                LabeledContent("FreeRDP", value: RDPSessionBridge.freeRDPVersion())
            }
            Section("RDP jelszó automatikus beírása") {
                LabeledContent("Accessibility engedély") {
                    Label(accessibilityTrusted ? "Megadva" : "Hiányzik",
                          systemImage: accessibilityTrusted ? "checkmark.circle.fill" : "xmark.circle.fill")
                        .foregroundStyle(accessibilityTrusted ? .green : .red)
                }
                HStack {
                    Button("Rendszerbeállítások megnyitása") {
                        _ = AccessibilityAutofill.isTrusted(prompt: true)
                        NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!)
                    }
                    Button("Ellenőrzés") { accessibilityTrusted = AccessibilityAutofill.isTrusted(prompt: false) }
                }
                Text("Enélkül a jelszó a vágólapra kerül 30 másodpercre.")
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
        panel.prompt = "Megnyitás"
        if panel.runModal() == .OK, let url = panel.url {
            state.useDatabase(at: url)
        }
    }

    private func createNew() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "connections.sqlite"
        panel.prompt = "Létrehozás"
        if panel.runModal() == .OK, let url = panel.url {
            state.useDatabase(at: url)
        }
    }
}
