import RemoteDesktopCore
import SwiftUI

struct ConnectionEditorView: View {
    @Environment(AppState.self) private var state
    @Environment(ShortcutStore.self) private var shortcuts
    @Environment(\.dismiss) private var dismiss
    @State var draft: ConnectionDraft

    /// Installed browsers, plus the saved one even if it has since been uninstalled.
    private var browsers: [InstalledBrowsers.Browser] {
        var list = InstalledBrowsers.all
        if let saved = draft.browserBundleId, !list.contains(where: { $0.bundleID == saved }) {
            list.append(.init(bundleID: saved, name: InstalledBrowsers.name(for: saved)))
        }
        return list
    }

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section {
                    if draft.isNew {
                        Picker("Type", selection: $draft.type) {
                            ForEach(ConnectionType.allCases, id: \.self) { Text($0.displayName).tag($0) }
                        }
                        .pickerStyle(.segmented)
                    }
                    TextField("Name", text: $draft.name, prompt: Text("e.g. web01 – prod"))
                    FolderPicker(title: "Folder", library: state.library, selection: $draft.folderId)
                    Toggle("Favorite", isOn: $draft.isFavorite)
                }

                Section("Connection") {
                    if draft.type.usesPort {
                        TextField("Host / IP", text: $draft.host, prompt: Text("10.0.0.10 or server.local"))
                        TextField("Port", text: $draft.port, prompt: Text(String(draft.type.defaultPort)))
                    } else {
                        TextField("URL", text: $draft.host, prompt: Text("https://router.local/admin"))
                        Picker("Browser", selection: $draft.browserBundleId) {
                            Text("Default").tag(String?.none)
                            Divider()
                            ForEach(browsers) { Text($0.name).tag(Optional($0.bundleID)) }
                        }
                    }
                    if draft.type == .mikrotik {
                        Picker("Default mode", selection: $draft.mikrotikMode) {
                            ForEach(MikrotikMode.allCases, id: \.self) { Text($0.displayName).tag($0) }
                        }
                        .help("Double-click / \(shortcuts.display(.connect)) connects with this mode; the other one is in the details and the context menu. The port is the SSH port; WinBox uses 8291.")
                    }
                    if draft.type.usesSSH {
                        Picker("SSH terminal", selection: $draft.sshTerminal) {
                            ForEach(SSHTerminal.allCases, id: \.self) { Text($0.displayName).tag($0) }
                        }
                        .help("Embedded: tab inside the window · External: new window of the terminal chosen in Settings")
                    }
                    if draft.type == .rdp {
                        Picker("RDP client", selection: $draft.rdpClient) {
                            ForEach(RDPClient.allCases, id: \.self) { Text($0.displayName).tag($0) }
                        }
                        .help("Embedded: FreeRDP tab inside the window · Windows App: separate app")
                        Toggle("Admin session (/admin)", isOn: $draft.rdpAdminSession)
                            .help("Connect to the administrative (console) session, like mstsc /admin")
                    }
                }

                Section("Authentication") {
                    Picker("Credential", selection: $draft.credentialId) {
                        Text("Own login").tag(String?.none)
                        if !state.library.credentials.isEmpty { Divider() }
                        ForEach(state.library.credentials) { credential in
                            Text(credential.displayUser.isEmpty ? credential.name : "\(credential.name) (\(credential.displayUser))")
                                .tag(Optional(credential.id))
                        }
                    }

                    if let credential = state.library.credential(id: draft.credentialId) {
                        if credential.username.isEmpty {
                            TextField("Username", text: $draft.username)
                            if draft.type == .rdp {
                                TextField("Domain", text: $draft.domain, prompt: Text("optional"))
                            }
                            Text("The credential has no username – this connection uses its own.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        } else {
                            LabeledContent("User", value: credential.displayUser)
                        }
                        LabeledContent("Password", value: credential.passwordEnc == nil ? "—" : "••••••••")
                        if draft.type.usesSSH {
                            LabeledContent("SSH key", value: credential.sshKeyPath ?? "—")
                        }
                    } else {
                        TextField("Username", text: $draft.username)
                        if draft.type == .rdp {
                            TextField("Domain", text: $draft.domain, prompt: Text("optional"))
                        }
                        SecretField(title: "Password", text: $draft.password)
                        if draft.type.usesSSH {
                            SSHKeyField(path: $draft.sshKeyPath)
                            if !draft.sshKeyPath.isEmpty {
                                SecretField(title: "Key passphrase", text: $draft.sshKeyPassphrase)
                            }
                        }
                    }
                }

                Section("Notes") {
                    TextEditor(text: $draft.notes)
                        .frame(minHeight: 60)
                        .font(.body)
                }
            }
            .formStyle(.grouped)

            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(draft.isNew ? "Create" : "Save") {
                    state.save(draft)
                    if draft.isNew { state.select(afterSave: "c:\(draft.id)") }
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(!draft.isValid)
            }
            .padding()
        }
        .frame(width: 480, height: draft.type.usesSSH ? 640 : 600)
    }
}

struct CredentialEditorView: View {
    @Environment(AppState.self) private var state
    @Environment(\.dismiss) private var dismiss
    @State var draft: CredentialDraft
    @State private var saveError: String?

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section {
                    TextField("Name", text: $draft.name, prompt: Text("e.g. Domain Admin"))
                }
                Section("Windows / RDP") {
                    TextField("Username", text: $draft.username)
                    TextField("Domain", text: $draft.domain, prompt: Text("optional, not used for SSH"))
                    SecretField(title: "Password", text: $draft.password)
                }
                Section("SSH key (optional)") {
                    SSHKeyField(path: $draft.sshKeyPath)
                    if !draft.sshKeyPath.isEmpty {
                        SecretField(title: "Key passphrase", text: $draft.sshKeyPassphrase)
                    }
                }
                Section("Notes") {
                    TextEditor(text: $draft.notes)
                        .frame(minHeight: 50)
                        .font(.body)
                }
            }
            .formStyle(.grouped)

            HStack {
                if let saveError {
                    Label(saveError, systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.red)
                        .lineLimit(3)
                }
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(draft.isNew ? "Create" : "Save") {
                    do {
                        try state.save(draft)
                    } catch {
                        saveError = error.localizedDescription
                        return
                    }
                    if draft.isNew { state.select(afterSave: "k:\(draft.id)") }
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(!draft.isValid)
            }
            .padding()
        }
        .frame(width: 460, height: 560)
    }
}

struct FolderEditorView: View {
    @Environment(AppState.self) private var state
    @Environment(\.dismiss) private var dismiss
    @State var folder: Folder

    var body: some View {
        let isNew = !state.library.folders.contains { $0.id == folder.id }
        VStack(alignment: .leading, spacing: 14) {
            Text(isNew ? "New folder" : "Rename folder").font(.headline)
            TextField("Name", text: $folder.name)
                .textFieldStyle(.roundedBorder)
                .onSubmit(save)
            if isNew {
                FolderPicker(title: "Parent", library: state.library, selection: $folder.parentId)
            }
            LabeledContent("Color") {
                LabelColorPicker(selection: $folder.color)
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Button(isNew ? "Create" : "Save", action: save)
                    .keyboardShortcut(.defaultAction)
                    .disabled(folder.name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 340)
    }

    private func save() {
        guard !folder.name.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        folder.name = folder.name.trimmingCharacters(in: .whitespaces)
        state.saveFolder(folder)
        dismiss()
    }
}
