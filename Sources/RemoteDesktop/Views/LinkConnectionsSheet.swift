import RemoteDesktopCore
import SwiftUI

/// Picks connections to link to a credential; ones linked to another credential move over.
struct LinkConnectionsSheet: View {
    @Environment(AppState.self) private var state
    @Environment(\.dismiss) private var dismiss
    let credential: Credential
    @State private var query = ""
    @State private var picked: Set<String> = []

    var body: some View {
        let candidates = LibraryTree.linkCandidates(state.library, for: credential.id, query: query)
        VStack(alignment: .leading, spacing: 12) {
            Text("Link connections to “\(credential.name)”").font(.headline)
            TextField("Search", text: $query, prompt: Text("Name, host, user or folder"))
                .textFieldStyle(.roundedBorder)

            List(candidates) { connection in
                Toggle(isOn: binding(for: connection.id)) {
                    HStack(spacing: 6) {
                        ConnectionIcon(type: connection.type)
                        Text(connection.name)
                        Text(LibraryTree.folderPath(connection.folderId, in: state.library))
                            .foregroundStyle(.secondary)
                        if let current = state.library.credential(id: connection.credentialId) {
                            Text("currently: \(current.name) — will move")
                                .font(.caption2)
                                .padding(.horizontal, 5)
                                .background(.quaternary, in: Capsule())
                        }
                    }
                }
            }
            .overlay {
                if candidates.isEmpty { ContentUnavailableView("No connections", systemImage: "magnifyingglass") }
            }

            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Add \(picked.count)") {
                    state.setCredential(credential.id, connectionIds: Array(picked))
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(picked.isEmpty)
            }
        }
        .padding(20)
        .frame(width: 560, height: 480)
    }

    private func binding(for id: String) -> Binding<Bool> {
        Binding {
            picked.contains(id)
        } set: { isOn in
            if isOn { picked.insert(id) } else { picked.remove(id) }
        }
    }
}
