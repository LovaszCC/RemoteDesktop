import RemoteDesktopCore
import SwiftUI

struct DetailView: View {
    @Environment(AppState.self) private var state

    var body: some View {
        switch state.item(for: state.selection) {
        case .connection(let connection):
            ConnectionDetailView(connection: connection)
        case .folder(let folder):
            FolderDetailView(folder: folder)
        case .credential(let credential):
            CredentialDetailView(credential: credential)
        case nil:
            ContentUnavailableView("Válassz kapcsolatot", systemImage: "rectangle.connected.to.line.below",
                                   description: Text("Dupla kattintás vagy ⌘↩ a kapcsolódáshoz"))
        }
    }
}

struct ConnectionDetailView: View {
    @Environment(AppState.self) private var state
    let connection: Connection

    private var credential: Credential? { state.library.credential(id: connection.credentialId) }
    private var resolved: Connection { connection.resolved(with: credential) }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                HStack(spacing: 14) {
                    ConnectionIcon(type: connection.type)
                        .font(.system(size: 30))
                        .frame(width: 44)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(connection.name).font(.title2.weight(.semibold))
                        Text(connection.type == .rdp
                             ? "RDP · Windows App" + (connection.rdpAdminSession ? " · /admin" : "")
                             : "SSH · Ghostty")
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button {
                        state.toggleFavorite(connection)
                    } label: {
                        Image(systemName: connection.isFavorite ? "star.fill" : "star")
                            .foregroundStyle(connection.isFavorite ? .yellow : .secondary)
                    }
                    .buttonStyle(.plain)
                    .help("Kedvenc")
                }

                HStack {
                    Button("Kapcsolódás", systemImage: "play.fill") { state.connect(connection) }
                        .buttonStyle(.borderedProminent)
                        .controlSize(.large)
                    Button("Szerkesztés", systemImage: "pencil") { state.edit(connection) }
                        .controlSize(.large)
                    if resolved.passwordEnc != nil {
                        Button("Jelszó másolása", systemImage: "key") { state.copyPassword(connection) }
                            .controlSize(.large)
                    }
                }

                Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 10) {
                    field("Host", connection.host, copyable: true)
                    field("Port", String(connection.effectivePort))
                    if connection.type == .rdp {
                        field("Admin session", connection.rdpAdminSession ? "Igen (/admin)" : "Nem")
                    }
                    if let credential {
                        GridRow {
                            Text("Credential").foregroundStyle(.secondary).gridColumnAlignment(.trailing)
                            Button {
                                state.selection = "k:\(credential.id)"
                            } label: {
                                Label(credential.name, systemImage: "person.badge.key.fill")
                            }
                            .buttonStyle(.link)
                        }
                    }
                    field("Felhasználó", resolved.username, copyable: true)
                    if connection.type == .rdp {
                        field("Domain", resolved.domain)
                    }
                    field("Jelszó", resolved.passwordEnc == nil ? "—" : "••••••••")
                    if connection.type == .ssh {
                        field("SSH kulcs", resolved.sshKeyPath ?? "—")
                        if resolved.sshKeyPassphraseEnc != nil { field("Kulcs jelmondat", "••••••••") }
                    }
                    field("Mappa", LibraryTree.folderPath(connection.folderId, in: state.library))
                }

                if !connection.notes.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Jegyzet").font(.headline)
                        Text(connection.notes).textSelection(.enabled)
                    }
                }
            }
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private func field(_ label: String, _ value: String, copyable: Bool = false) -> some View {
        GridRow {
            Text(label).foregroundStyle(.secondary).gridColumnAlignment(.trailing)
            HStack(spacing: 6) {
                Text(value.isEmpty ? "—" : value).textSelection(.enabled)
                if copyable && !value.isEmpty {
                    Button {
                        Clipboard.copy(value, clearAfter: nil)
                        state.showToast("Másolva")
                    } label: {
                        Image(systemName: "doc.on.doc").font(.caption)
                    }
                    .buttonStyle(.borderless)
                    .help("Másolás")
                }
            }
        }
    }
}

struct FolderDetailView: View {
    @Environment(AppState.self) private var state
    let folder: Folder

    var body: some View {
        let count = state.library.connections.filter { $0.folderId == folder.id }.count
        ContentUnavailableView {
            Label(folder.name, systemImage: "folder")
        } description: {
            Text("\(count) kapcsolat · \(LibraryTree.folderPath(folder.id, in: state.library))")
        } actions: {
            HStack {
                Button("Új RDP") { state.newConnection(.rdp) }
                Button("Új SSH") { state.newConnection(.ssh) }
                Button("Átnevezés") { state.folderEditor = folder }
            }
        }
    }
}

struct CredentialDetailView: View {
    @Environment(AppState.self) private var state
    let credential: Credential

    var body: some View {
        let users = state.library.connections(using: credential.id)
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                HStack(spacing: 14) {
                    Image(systemName: "person.badge.key.fill")
                        .font(.system(size: 30))
                        .foregroundStyle(.orange)
                        .frame(width: 44)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(credential.name).font(.title2.weight(.semibold))
                        Text("Credential").foregroundStyle(.secondary)
                    }
                }

                HStack {
                    Button("Szerkesztés", systemImage: "pencil") { state.edit(credential) }
                        .controlSize(.large)
                    Button("Törlés", systemImage: "trash", role: .destructive) { state.requestDelete(credential) }
                        .controlSize(.large)
                }

                Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 10) {
                    row("Felhasználó", credential.username)
                    row("Domain", credential.domain)
                    row("Jelszó", credential.passwordEnc == nil ? "" : "••••••••")
                    row("SSH kulcs", credential.sshKeyPath ?? "")
                    if credential.sshKeyPassphraseEnc != nil { row("Kulcs jelmondat", "••••••••") }
                }

                if !credential.notes.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Jegyzet").font(.headline)
                        Text(credential.notes).textSelection(.enabled)
                    }
                }

                VStack(alignment: .leading, spacing: 8) {
                    Text("Használja (\(users.count))").font(.headline)
                    if users.isEmpty {
                        Text("Egy kapcsolat sem").foregroundStyle(.secondary)
                    }
                    ForEach(users) { connection in
                        Button {
                            state.selection = "c:\(connection.id)"
                        } label: {
                            HStack {
                                ConnectionIcon(type: connection.type)
                                Text(connection.name)
                                Text(LibraryTree.folderPath(connection.folderId, in: state.library))
                                    .foregroundStyle(.secondary)
                            }
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func row(_ label: String, _ value: String) -> some View {
        GridRow {
            Text(label).foregroundStyle(.secondary).gridColumnAlignment(.trailing)
            Text(value.isEmpty ? "—" : value).textSelection(.enabled)
        }
    }
}
