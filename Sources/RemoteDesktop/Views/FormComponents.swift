import AppKit
import RemoteDesktopCore
import SwiftUI

/// Secure field with a reveal toggle.
struct SecretField: View {
    let title: LocalizedStringKey
    @Binding var text: String
    @State private var revealed = false

    var body: some View {
        LabeledContent(title) {
            HStack {
                Group {
                    if revealed { TextField(title, text: $text) } else { SecureField(title, text: $text) }
                }
                .labelsHidden()
                Button {
                    revealed.toggle()
                } label: {
                    Image(systemName: revealed ? "eye.slash" : "eye")
                }
                .buttonStyle(.borderless)
                .help(revealed ? "Hide" : "Show")
            }
        }
    }
}

/// SSH private key path field with a file browser (stores `~`-relative paths).
struct SSHKeyField: View {
    @Binding var path: String

    var body: some View {
        LabeledContent("SSH key") {
            HStack {
                TextField("SSH key", text: $path, prompt: Text("~/.ssh/id_ed25519"))
                    .labelsHidden()
                Button("Browse…", action: choose)
            }
        }
    }

    private func choose() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.showsHiddenFiles = true
        panel.directoryURL = URL(filePath: NSHomeDirectory()).appending(path: ".ssh")
        panel.prompt = String(localized: "Choose")
        if panel.runModal() == .OK, let url = panel.url {
            let home = NSHomeDirectory()
            path = url.path.hasPrefix(home) ? "~" + url.path.dropFirst(home.count) : url.path
        }
    }
}

/// Folder picker with indented hierarchy.
struct FolderPicker: View {
    let title: LocalizedStringKey
    let library: Library
    @Binding var selection: String?

    var body: some View {
        Picker(title, selection: $selection) {
            Text("(Root)").tag(String?.none)
            ForEach(LibraryTree.flattenedFolders(library), id: \.folder.id) { entry in
                Text(String(repeating: "   ", count: entry.depth) + entry.folder.name).tag(Optional(entry.folder.id))
            }
        }
    }
}
