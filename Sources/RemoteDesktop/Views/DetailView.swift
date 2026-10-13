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
            ContentUnavailableView("Select a connection", systemImage: "rectangle.connected.to.line.below",
                                   description: Text("Double-click or ⌘↩ to connect"))
        }
    }
}

struct ConnectionDetailView: View {
    @Environment(AppState.self) private var state
    /// Observed so the subtitle follows the external terminal chosen in Settings.
    @AppStorage(ExternalTerminal.defaultsKey) private var externalTerminalChoice: String?
    let connection: Connection

    private var credential: Credential? { state.library.credential(id: connection.credentialId) }
    private var resolved: Connection { connection.resolved(with: credential) }

    private var subtitle: String {
        switch connection.type {
        case .rdp: "RDP · " + (connection.rdpClient == .embedded ? String(localized: "embedded (FreeRDP)") : "Windows App")
            + (connection.rdpAdminSession ? " · /admin" : "")
        case .ssh: "SSH · " + (connection.sshTerminal == .embedded ? String(localized: "embedded terminal") : SSHLauncher.currentTerminal.displayName)
        case .mikrotik: String(localized: "MikroTik · default \(connection.mikrotikMode.displayName)")
        case .web: connection.type.displayName + " · "
            + (connection.browserBundleId.map(InstalledBrowsers.name(for:)) ?? String(localized: "default browser"))
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                HStack(spacing: 14) {
                    ConnectionIcon(type: connection.type)
                        .font(.system(size: 30))
                        .frame(width: 44)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(connection.name).font(.title2.weight(.semibold))
                        Text(subtitle)
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
                    .help("Favorite")
                }

                HStack {
                    if connection.type == .mikrotik {
                        ForEach(MikrotikMode.allCases, id: \.self) { mode in
                            mikrotikButton(mode)
                        }
                    } else {
                        Button("Connect", systemImage: "play.fill") { state.connect(connection) }
                            .buttonStyle(.borderedProminent)
                            .controlSize(.large)
                    }
                    Button("Edit", systemImage: "pencil") { state.edit(connection) }
                        .controlSize(.large)
                    if resolved.passwordEnc != nil {
                        Button("Copy password", systemImage: "key") { state.copyPassword(connection) }
                            .controlSize(.large)
                    }
                }

                Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 10) {
                    if connection.type.usesPort {
                        field("Host", connection.host, copyable: true)
                        field("Port", String(connection.effectivePort))
                    } else {
                        field("URL", WebLaunch.url(from: connection.host)?.absoluteString ?? connection.host, copyable: true)
                    }
                    if connection.type == .rdp {
                        field("Admin session", connection.rdpAdminSession ? String(localized: "Yes (/admin)") : String(localized: "No"))
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
                    field("User", resolved.username, copyable: true)
                    if connection.type == .rdp {
                        field("Domain", resolved.domain)
                    }
                    field("Password", resolved.passwordEnc == nil ? "—" : "••••••••")
                    if connection.type.usesSSH {
                        field("SSH key", resolved.sshKeyPath ?? "—")
                        if resolved.sshKeyPassphraseEnc != nil { field("Key passphrase", "••••••••") }
                    }
                    field("Folder", LibraryTree.folderPath(connection.folderId, in: state.library))
                }

                if connection.type == .ssh {
                    PortForwardSection(connection: connection)
                }

                if !connection.notes.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Notes").font(.headline)
                        Text(connection.notes).textSelection(.enabled)
                    }
                }
            }
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    /// The default mode is the prominent button.
    @ViewBuilder
    private func mikrotikButton(_ mode: MikrotikMode) -> some View {
        let button = Button(mode.displayName, systemImage: mode == .ssh ? "apple.terminal" : "network") {
            state.connect(connection, mode: mode)
        }
        .controlSize(.large)
        if mode == connection.mikrotikMode {
            button.buttonStyle(.borderedProminent)
        } else {
            button
        }
    }

    @ViewBuilder
    private func field(_ label: LocalizedStringKey, _ value: String, copyable: Bool = false) -> some View {
        GridRow {
            Text(label).foregroundStyle(.secondary).gridColumnAlignment(.trailing)
            HStack(spacing: 6) {
                Text(value.isEmpty ? "—" : value).textSelection(.enabled)
                if copyable && !value.isEmpty {
                    Button {
                        Clipboard.copy(value, clearAfter: nil)
                        state.showToast(String(localized: "Copied"))
                    } label: {
                        Image(systemName: "doc.on.doc").font(.caption)
                    }
                    .buttonStyle(.borderless)
                    .help("Copy")
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
            Text("\(count) connections · \(LibraryTree.folderPath(folder.id, in: state.library))")
        } actions: {
            HStack {
                ForEach(ConnectionType.allCases, id: \.self) { type in
                    Button("New \(type.displayName)") { state.newConnection(type) }
                }
                Button("Rename") { state.folderEditor = folder }
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
                    Button("Edit", systemImage: "pencil") { state.edit(credential) }
                        .controlSize(.large)
                    Button("Delete", systemImage: "trash", role: .destructive) { state.requestDelete(credential) }
                        .controlSize(.large)
                }

                Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 10) {
                    row("User", credential.username)
                    row("Domain", credential.domain)
                    row("Password", credential.passwordEnc == nil ? "" : "••••••••")
                    row("SSH key", credential.sshKeyPath ?? "")
                    if credential.sshKeyPassphraseEnc != nil { row("Key passphrase", "••••••••") }
                    if credential.sshKeyUnlockedEnc != nil { row("Unlocked key", String(localized: "stored")) }
                }

                if !credential.notes.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Notes").font(.headline)
                        Text(credential.notes).textSelection(.enabled)
                    }
                }

                VStack(alignment: .leading, spacing: 8) {
                    Text("Used by (\(users.count))").font(.headline)
                    if users.isEmpty {
                        Text("No connections").foregroundStyle(.secondary)
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

    private func row(_ label: LocalizedStringKey, _ value: String) -> some View {
        GridRow {
            Text(label).foregroundStyle(.secondary).gridColumnAlignment(.trailing)
            Text(value.isEmpty ? "—" : value).textSelection(.enabled)
        }
    }
}
