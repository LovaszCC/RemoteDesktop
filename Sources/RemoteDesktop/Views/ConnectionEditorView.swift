import AppKit
import RemoteDesktopCore
import SwiftUI

struct ConnectionEditorView: View {
    @Environment(AppState.self) private var state
    @Environment(\.dismiss) private var dismiss
    @State var draft: ConnectionDraft
    @State private var showPassword = false

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
                    Picker("Mappa", selection: $draft.folderId) {
                        Text("(Gyökér)").tag(String?.none)
                        ForEach(LibraryTree.flattenedFolders(state.library), id: \.folder.id) { entry in
                            Text(String(repeating: "   ", count: entry.depth) + entry.folder.name).tag(Optional(entry.folder.id))
                        }
                    }
                    Toggle("Kedvenc", isOn: $draft.isFavorite)
                }

                Section("Kapcsolat") {
                    TextField("Host / IP", text: $draft.host, prompt: Text("10.0.0.10 vagy server.local"))
                    TextField("Port", text: $draft.port, prompt: Text(String(draft.type.defaultPort)))
                }

                Section("Hitelesítés") {
                    TextField("Felhasználónév", text: $draft.username)
                    if draft.type == .rdp {
                        TextField("Domain", text: $draft.domain, prompt: Text("opcionális"))
                    }
                    passwordField("Jelszó", text: $draft.password)
                    if draft.type == .ssh {
                        LabeledContent("SSH kulcs") {
                            HStack {
                                TextField("SSH kulcs", text: $draft.sshKeyPath, prompt: Text("~/.ssh/id_ed25519"))
                                    .labelsHidden()
                                Button("Tallózás…", action: chooseKey)
                            }
                        }
                        if !draft.sshKeyPath.isEmpty {
                            passwordField("Kulcs jelmondat", text: $draft.sshKeyPassphrase)
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
                    if draft.isNew { state.selection = "c:\(draft.id)" }
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(!draft.isValid)
            }
            .padding()
        }
        .frame(width: 480, height: draft.type == .ssh ? 620 : 580)
    }

    @ViewBuilder
    private func passwordField(_ title: String, text: Binding<String>) -> some View {
        LabeledContent(title) {
            HStack {
                Group {
                    if showPassword { TextField(title, text: text) } else { SecureField(title, text: text) }
                }
                .labelsHidden()
                Button {
                    showPassword.toggle()
                } label: {
                    Image(systemName: showPassword ? "eye.slash" : "eye")
                }
                .buttonStyle(.borderless)
                .help(showPassword ? "Elrejtés" : "Megjelenítés")
            }
        }
    }

    private func chooseKey() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.showsHiddenFiles = true
        panel.directoryURL = URL(filePath: NSHomeDirectory()).appending(path: ".ssh")
        panel.prompt = "Kiválasztás"
        if panel.runModal() == .OK, let url = panel.url {
            let home = NSHomeDirectory()
            draft.sshKeyPath = url.path.hasPrefix(home) ? "~" + url.path.dropFirst(home.count) : url.path
        }
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
                Picker("Szülő", selection: $folder.parentId) {
                    Text("(Gyökér)").tag(String?.none)
                    ForEach(LibraryTree.flattenedFolders(state.library), id: \.folder.id) { entry in
                        Text(String(repeating: "   ", count: entry.depth) + entry.folder.name).tag(Optional(entry.folder.id))
                    }
                }
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
