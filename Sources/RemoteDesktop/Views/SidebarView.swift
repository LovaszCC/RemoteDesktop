import RemoteDesktopCore
import SwiftUI

struct SidebarView: View {
    @Environment(AppState.self) private var state
    @AppStorage("expandedFolders") private var expandedStorage = ""

    var body: some View {
        @Bindable var state = state
        let library = state.library

        List(selection: $state.selectedTags) {
            // Separate builders keep each body small enough for the type checker on CI.
            if !state.searchText.isEmpty {
                searchSections(library)
            } else {
                librarySections(library)
            }
        }
        .listStyle(.sidebar)
        // Search and tree have different section layouts; diffing one into the other sometimes leaves
        // a stale search row behind, so each mode gets its own List instance instead.
        .id(state.searchText.isEmpty)
        .contextMenu(forSelectionType: String.self) { tags in
            if tags.count > 1 {
                let connections = state.sshConnections(forTags: tags)
                Button("Open in split view (\(min(connections.count, SplitLayout.maxPanes)))") { state.openSplit(connections) }
                    .disabled(connections.count < 2)
                // "" matches no folder, so no target is disabled for a mixed selection.
                Button("Save as split…") { state.promptSaveSplit(connections: connections) }
                    .disabled(!(2...SplitLayout.maxPanes).contains(connections.count))
                MoveMenu(currentFolderId: "", excluding: nil) { state.move(tags: Array(tags), to: $0) }
                LinkCredentialMenu(tags: Array(tags))
                Divider()
                Button("Delete \(tags.count) items…", role: .destructive) { state.requestDelete(tags: tags) }
            } else if let tag = tags.first {
                ItemContextMenu(tag: tag)
            } else {
                ForEach(ConnectionType.allCases, id: \.self) { type in
                    Button(type.newTitle) { state.newConnection(type) }
                }
                Button("New folder") { state.newFolder() }
                Button("New credential") { state.newCredential() }
            }
        } primaryAction: { tags in
            guard tags.count == 1, let tag = tags.first else { return }
            if case .credential(let credential) = state.item(for: tag) {
                state.edit(credential)
            } else if tag.hasPrefix("f:") {
                toggle(String(tag.dropFirst(2)))
            } else {
                state.activate(tag: tag)
            }
        }
        .overlay {
            if library.folders.isEmpty && library.connections.isEmpty && library.credentials.isEmpty && state.searchText.isEmpty {
                ContentUnavailableView {
                    Label("No connections", systemImage: "network")
                } description: {
                    Text("⌘N new RDP, ⇧⌘N new SSH connection, ⌘I import")
                }
            }
        }
    }

    // MARK: Expansion (persisted)

    private var expandedBinding: Binding<Set<String>> {
        Binding(
            get: { Set(expandedStorage.split(separator: ",").map(String.init)) },
            set: { expandedStorage = $0.sorted().joined(separator: ",") }
        )
    }

    @ViewBuilder
    private func searchSections(_ library: Library) -> some View {
        Section("Results") {
            ForEach(LibraryTree.search(library, query: state.searchText)) { connection in
                ConnectionRow(connection: connection, subtitle: LibraryTree.folderPath(connection.folderId, in: library))
                    .draggable(state.dragPayload(for: "c:\(connection.id)"))
                    .tag("c:\(connection.id)")
            }
        }
        let credentials = LibraryTree.searchCredentials(library, query: state.searchText)
        if !credentials.isEmpty {
            Section("Credentials") {
                ForEach(credentials) { CredentialRow(credential: $0).tag("k:\($0.id)") }
            }
        }
    }

    @ViewBuilder
    private func librarySections(_ library: Library) -> some View {
        let favorites = LibraryTree.favorites(library)
        if !favorites.isEmpty {
            Section("Favorites") {
                ForEach(favorites) { connection in
                    ConnectionRow(connection: connection, subtitle: nil)
                        .draggable(state.dragPayload(for: "fav:\(connection.id)"))
                        .tag("fav:\(connection.id)")
                }
            }
        }
        if !library.savedSplits.isEmpty {
            Section("Saved splits") {
                ForEach(library.savedSplits) { saved in
                    SavedSplitRow(saved: saved).tag("s:\(saved.id)")
                }
                .onMove { state.moveSavedSplits(from: $0, to: $1) }
            }
        }
        Section {
            TreeRows(nodes: LibraryTree.build(library), expanded: expandedBinding)
        } header: {
            Text("Connections")
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
                .dropDestination(for: String.self) { payloads, _ in
                    state.move(tags: SidebarDrag.tags(from: payloads), to: nil)
                    return true
                }
        }
        // Always present: inserting the section together with its first row confuses the List.
        Section("Credentials") {
            ForEach(library.credentials) { CredentialRow(credential: $0).tag("k:\($0.id)") }
        }
    }

    private func toggle(_ folderId: String) {
        var set = expandedBinding.wrappedValue
        if !set.insert(folderId).inserted { set.remove(folderId) }
        expandedBinding.wrappedValue = set
    }

}

/// Recursive folder tree with persisted disclosure state.
private struct TreeRows: View {
    @Environment(AppState.self) private var state
    let nodes: [TreeNode]
    @Binding var expanded: Set<String>

    var body: some View {
        ForEach(nodes) { node in
            switch node.kind {
            case .folder(let folder):
                DisclosureGroup(isExpanded: Binding(
                    get: { expanded.contains(folder.id) },
                    set: { isOn in
                        if isOn { expanded.insert(folder.id) } else { expanded.remove(folder.id) }
                    }
                )) {
                    TreeRows(nodes: node.children ?? [], expanded: $expanded)
                } label: {
                    Label {
                        Text(folder.name)
                    } icon: {
                        Image(systemName: folder.color == nil ? "folder" : "folder.fill")
                            .foregroundStyle(folder.color?.color ?? .accentColor)
                    }
                        .draggable(state.dragPayload(for: node.id))
                        .dropDestination(for: String.self) { payloads, _ in
                            state.move(tags: SidebarDrag.tags(from: payloads), to: folder.id)
                            expanded.insert(folder.id)
                            return true
                        }
                }
                .tag(node.id)
            case .connection(let connection):
                ConnectionRow(connection: connection, subtitle: nil)
                    .draggable(state.dragPayload(for: node.id))
                    .tag(node.id)
            }
        }
    }
}

struct ConnectionRow: View {
    @Environment(AppState.self) private var state
    let connection: Connection
    let subtitle: String?

    var body: some View {
        HStack(spacing: 8) {
            ConnectionIcon(type: connection.type)
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 5) {
                    Text(connection.name).lineLimit(1)
                    if let color = connection.color {
                        RoundedRectangle(cornerRadius: 2)
                            .fill(color.color)
                            .frame(width: 8, height: 8)
                            .help(Text(verbatim: color.title))
                    }
                }
                if let subtitle, !subtitle.isEmpty {
                    Text(subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer(minLength: 4)
            if state.isConnected(connection.id) {
                Circle().fill(.green).frame(width: 6, height: 6).help("Open session")
            }
            if state.hasActivePortForward(connection.id) {
                Image(systemName: "arrow.left.arrow.right").font(.caption2).foregroundStyle(.green)
                    .help("Port forward active")
            }
            if connection.isFavorite {
                Image(systemName: "star.fill").font(.caption2).foregroundStyle(.yellow)
            }
        }
        .help("\(connection.type.displayName) · \(connection.username.isEmpty ? "" : connection.username + "@")\(connection.host)")
    }
}

struct CredentialRow: View {
    @Environment(AppState.self) private var state
    let credential: Credential

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "person.badge.key.fill")
                .foregroundStyle(.orange)
                .frame(width: 18)
            Text(credential.name).lineLimit(1)
            Spacer(minLength: 4)
            let uses = state.library.connections(using: credential.id).count
            if uses > 0 {
                Text("\(uses)").font(.caption).foregroundStyle(.secondary).monospacedDigit()
            }
        }
        .help(credential.displayUser)
    }
}

struct ConnectionIcon: View {
    let type: ConnectionType

    var body: some View {
        Image(systemName: type.systemImage)
            .foregroundStyle(type.tint)
            .frame(width: 18)
    }
}

/// Context menu for a folder or connection tag.
struct ItemContextMenu: View {
    @Environment(AppState.self) private var state
    let tag: String

    var body: some View {
        switch state.item(for: tag) {
        case .connection(let connection):
            if connection.type == .mikrotik {
                ForEach(MikrotikMode.allCases, id: \.self) { mode in
                    Button("Connect – \(mode.displayName)") { state.connect(connection, mode: mode) }
                }
            } else {
                Button("Connect") { state.connect(connection) }
            }
            if connection.type == .rdp {
                Button("New embedded RDP session") { state.connect(connection, rdpClient: .embedded) }
                Button("Open in Windows App") { state.connect(connection, rdpClient: .windowsApp) }
            }
            if connection.type.usesSSH {
                Button("New embedded SSH session") { state.connect(connection, mode: .ssh, terminal: .embedded) }
                Button("Open in split view (2×)") { state.openSplit([connection, connection]) }
                Button("Files (SCP)…") { state.openFiles(connection) }
                Button("Open in \(SSHLauncher.currentTerminal.displayName)") {
                    state.connect(connection, mode: .ssh, terminal: .external)
                }
            }
            Divider()
            Button("Edit…") { state.edit(connection) }
            Button("Duplicate") { state.duplicate(connection) }
            Button(connection.isFavorite ? "Remove from favorites" : "Add to favorites") {
                state.toggleFavorite(connection)
            }
            MoveMenu(currentFolderId: connection.folderId, excluding: nil) { state.move(connectionId: connection.id, to: $0) }
            Divider()
            Button("Copy password") { state.copyPassword(connection) }
            Button("Copy host") { Clipboard.copy(connection.host, clearAfter: nil) }
            Divider()
            Button("Delete", role: .destructive) { state.delete(connection) }
        case .folder(let folder):
            ForEach(ConnectionType.allCases, id: \.self) { type in
                Button("\(type.newTitle) here") { state.selection = tag; state.newConnection(type) }
            }
            Button("New subfolder") { state.selection = tag; state.newFolder() }
            Divider()
            Button("Rename…") { state.folderEditor = folder }
            Menu("Color") {
                LabelColorMenuItems(selection: folder.color) { state.setColor($0, folderId: folder.id) }
            }
            MoveMenu(currentFolderId: folder.parentId, excluding: folder.id) { state.move(folderId: folder.id, to: $0) }
            Divider()
            Button("Delete folder", role: .destructive) { state.deleteFolder(folder) }
        case .credential(let credential):
            Button("Edit…") { state.edit(credential) }
            Divider()
            Button("Delete credential", role: .destructive) { state.requestDelete(credential) }
        case .savedSplit(let saved):
            Button("Open") { state.openSavedSplit(saved) }
            Button("Rename…") { state.promptRename(saved) }
            Divider()
            Button("Delete saved split", role: .destructive) { state.deleteSavedSplit(saved) }
        case nil:
            EmptyView()
        }
    }
}

struct MoveMenu: View {
    @Environment(AppState.self) private var state
    let currentFolderId: String?
    let excluding: String?
    let move: (String?) -> Void

    var body: some View {
        Menu("Move") {
            Button("(Root)") { move(nil) }.disabled(currentFolderId == nil)
            ForEach(LibraryTree.flattenedFolders(state.library, excluding: excluding), id: \.folder.id) { entry in
                Button(String(repeating: "   ", count: entry.depth) + entry.folder.name) { move(entry.folder.id) }
                    .disabled(entry.folder.id == currentFolderId)
            }
        }
    }
}

extension ConnectionType {
    var tint: Color {
        switch self {
        case .rdp: .blue
        case .ssh: .green
        case .web: .purple
        case .mikrotik: .orange
        }
    }
}

/// A saved split in the sidebar: name and the panes' connection names.
struct SavedSplitRow: View {
    @Environment(AppState.self) private var state
    let saved: SavedSplit

    var body: some View {
        let connections = saved.connections(in: state.library)
        HStack(spacing: 8) {
            Image(systemName: "rectangle.split.2x2")
                .foregroundStyle(connections.isEmpty ? .secondary : SwiftUI.Color.green)
                .frame(width: 18)
            VStack(alignment: .leading, spacing: 1) {
                Text(saved.name).lineLimit(1)
                Text(connections.isEmpty ? String(localized: "no connections") : connections.map(\.name).joined(separator: " + "))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
    }
}

/// Sets one linked credential (or the own login) on every connection of a multi-selection.
struct LinkCredentialMenu: View {
    @Environment(AppState.self) private var state
    let tags: [String]

    var body: some View {
        Menu("Linked credential") {
            Button("Own login") { state.setCredential(nil, forTags: tags) }
            if !state.library.credentials.isEmpty { Divider() }
            ForEach(state.library.credentials) { credential in
                Button(credential.name) { state.setCredential(credential.id, forTags: tags) }
            }
        }
        .disabled(state.library.connectionIds(forTags: tags).isEmpty)
    }
}
