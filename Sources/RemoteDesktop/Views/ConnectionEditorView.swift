import RemoteDesktopCore
import SwiftUI

struct ConnectionEditorView: View {
    @Environment(AppState.self) private var state
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
                        Picker("Típus", selection: $draft.type) {
                            ForEach(ConnectionType.allCases, id: \.self) { Text($0.displayName).tag($0) }
                        }
                        .pickerStyle(.segmented)
                    }
                    TextField("Név", text: $draft.name, prompt: Text("pl. web01 – prod"))
                    FolderPicker(title: "Mappa", library: state.library, selection: $draft.folderId)
                    Toggle("Kedvenc", isOn: $draft.isFavorite)
                }

                Section("Kapcsolat") {
                    if draft.type.usesPort {
                        TextField("Host / IP", text: $draft.host, prompt: Text("10.0.0.10 vagy server.local"))
                        TextField("Port", text: $draft.port, prompt: Text(String(draft.type.defaultPort)))
                    } else {
                        TextField("URL", text: $draft.host, prompt: Text("https://router.local/admin"))
                        Picker("Böngésző", selection: $draft.browserBundleId) {
                            Text("Alapértelmezett").tag(String?.none)
                            Divider()
                            ForEach(browsers) { Text($0.name).tag(Optional($0.bundleID)) }
                        }
                    }
                    if draft.type == .mikrotik {
                        Picker("Alapértelmezett mód", selection: $draft.mikrotikMode) {
                            ForEach(MikrotikMode.allCases, id: \.self) { Text($0.displayName).tag($0) }
                        }
                        .help("Dupla kattintás / ⌘↩ ezzel kapcsolódik; a másik mód a részleteknél és a helyi menüben érhető el. A port az SSH port, a WinBox a 8291-et használja.")
                    }
                    if draft.type.usesSSH {
                        Picker("SSH terminál", selection: $draft.sshTerminal) {
                            ForEach(SSHTerminal.allCases, id: \.self) { Text($0.displayName).tag($0) }
                        }
                        .help("Beágyazott: fül az ablakon belül · Ghostty: külön terminálablak")
                    }
                    if draft.type == .rdp {
                        Toggle("Admin session (/admin)", isOn: $draft.rdpAdminSession)
                            .help("Kapcsolódás az adminisztratív (konzol) sessionhöz, mint mstsc /admin")
                    }
                }

                Section("Hitelesítés") {
                    Picker("Credential", selection: $draft.credentialId) {
                        Text("Saját adatok").tag(String?.none)
                        if !state.library.credentials.isEmpty { Divider() }
                        ForEach(state.library.credentials) { credential in
                            Text(credential.displayUser.isEmpty ? credential.name : "\(credential.name) (\(credential.displayUser))")
                                .tag(Optional(credential.id))
                        }
                    }

                    if let credential = state.library.credential(id: draft.credentialId) {
                        LabeledContent("Felhasználó", value: credential.displayUser.isEmpty ? "—" : credential.displayUser)
                        LabeledContent("Jelszó", value: credential.passwordEnc == nil ? "—" : "••••••••")
                        if draft.type.usesSSH {
                            LabeledContent("SSH kulcs", value: credential.sshKeyPath ?? "—")
                        }
                    } else {
                        TextField("Felhasználónév", text: $draft.username)
                        if draft.type == .rdp {
                            TextField("Domain", text: $draft.domain, prompt: Text("opcionális"))
                        }
                        SecretField(title: "Jelszó", text: $draft.password)
                        if draft.type.usesSSH {
                            SSHKeyField(path: $draft.sshKeyPath)
                            if !draft.sshKeyPath.isEmpty {
                                SecretField(title: "Kulcs jelmondat", text: $draft.sshKeyPassphrase)
                            }
                        }
                    }
                }

                Section("Jegyzet") {
                    TextEditor(text: $draft.notes)
                        .frame(minHeight: 60)
                        .font(.body)
                }
            }
            .formStyle(.grouped)

            HStack {
                Spacer()
                Button("Mégse", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(draft.isNew ? "Létrehozás" : "Mentés") {
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
                    TextField("Név", text: $draft.name, prompt: Text("pl. Domain Admin"))
                }
                Section("Windows / RDP") {
                    TextField("Felhasználónév", text: $draft.username)
                    TextField("Domain", text: $draft.domain, prompt: Text("opcionális, SSH-nál nem használt"))
                    SecretField(title: "Jelszó", text: $draft.password)
                }
                Section("SSH kulcs (opcionális)") {
                    SSHKeyField(path: $draft.sshKeyPath)
                    if !draft.sshKeyPath.isEmpty {
                        SecretField(title: "Kulcs jelmondat", text: $draft.sshKeyPassphrase)
                    }
                }
                Section("Jegyzet") {
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
                Button("Mégse", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(draft.isNew ? "Létrehozás" : "Mentés") {
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
            Text(isNew ? "Új mappa" : "Mappa átnevezése").font(.headline)
            TextField("Név", text: $folder.name)
                .textFieldStyle(.roundedBorder)
                .onSubmit(save)
            if isNew {
                FolderPicker(title: "Szülő", library: state.library, selection: $folder.parentId)
            }
            HStack {
                Spacer()
                Button("Mégse", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Button(isNew ? "Létrehozás" : "Mentés", action: save)
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
