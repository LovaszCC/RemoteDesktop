import RemoteDesktopCore
import SwiftUI

struct SidebarView: View {
    @Environment(AppState.self) private var state
    @AppStorage("expandedFolders") private var expandedStorage = ""

    var body: some View {
        @Bindable var state = state
        let library = state.library

        List(selection: $state.selection) {
            if !state.searchText.isEmpty {
                Section("Találatok") {
                    ForEach(LibraryTree.search(library, query: state.searchText)) { connection in
                        ConnectionRow(connection: connection, subtitle: LibraryTree.folderPath(connection.folderId, in: library))
                            .tag("c:\(connection.id)")
                    }
                }
                let credentials = LibraryTree.searchCredentials(library, query: state.searchText)
                if !credentials.isEmpty {
                    Section("Credentialok") {
                        ForEach(credentials) { CredentialRow(credential: $0).tag("k:\($0.id)") }
                    }
                }
            } else {
                let favorites = LibraryTree.favorites(library)
                if !favorites.isEmpty {
                    Section("Kedvencek") {
                        ForEach(favorites) { connection in
                            ConnectionRow(connection: connection, subtitle: nil)
                                .tag("fav:\(connection.id)")
                        }
                    }
                }
                Section {
                    TreeRows(nodes: LibraryTree.build(library), expanded: expandedBinding)
                } header: {
                    Text("Kapcsolatok")
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                        .dropDestination(for: String.self) { tags, _ in
                            move(tags, to: nil)
                        }
                }
                if !library.credentials.isEmpty {
                    Section("Credentialok") {
                        ForEach(library.credentials) { CredentialRow(credential: $0).tag("k:\($0.id)") }
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .contextMenu(forSelectionType: String.self) { tags in
            if let tag = tags.first {
                ItemContextMenu(tag: tag)
            } else {
                Button("Új RDP kapcsolat") { state.newConnection(.rdp) }
                Button("Új SSH kapcsolat") { state.newConnection(.ssh) }
                Button("Új mappa") { state.newFolder() }
                Button("Új credential") { state.newCredential() }
            }
        } primaryAction: { tags in
            guard let tag = tags.first else { return }
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
                    Label("Nincs kapcsolat", systemImage: "network")
                } description: {
                    Text("⌘N új RDP, ⇧⌘N új SSH kapcsolat, ⌘I import")
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

    private func toggle(_ folderId: String) {
        var set = expandedBinding.wrappedValue
        if !set.insert(folderId).inserted { set.remove(folderId) }
        expandedBinding.wrappedValue = set
    }

    private func move(_ tags: [String], to folderId: String?) -> Bool {
        for tag in tags {
            switch state.item(for: tag) {
            case .connection(let connection): state.move(connectionId: connection.id, to: folderId)
            case .folder(let folder) where folder.id != folderId: state.move(folderId: folder.id, to: folderId)
            default: break
            }
        }
        return true
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
                    Label(folder.name, systemImage: "folder")
                        .draggable(node.id)
                        .dropDestination(for: String.self) { tags, _ in
                            for tag in tags {
                                switch state.item(for: tag) {
                                case .connection(let connection): state.move(connectionId: connection.id, to: folder.id)
                                case .folder(let moved) where moved.id != folder.id: state.move(folderId: moved.id, to: folder.id)
                                default: break
                                }
                            }
                            expanded.insert(folder.id)
                            return true
                        }
                }
                .tag(node.id)
            case .connection(let connection):
                ConnectionRow(connection: connection, subtitle: nil)
                    .draggable(node.id)
                    .tag(node.id)
            }
        }
    }
}

struct ConnectionRow: View {
    let connection: Connection
    let subtitle: String?

    var body: some View {
        HStack(spacing: 8) {
            ConnectionIcon(type: connection.type)
            VStack(alignment: .leading, spacing: 1) {
                Text(connection.name).lineLimit(1)
                if let subtitle, !subtitle.isEmpty {
                    Text(subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer(minLength: 4)
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
        Image(systemName: type == .rdp ? "display" : "apple.terminal")
            .foregroundStyle(type == .rdp ? Color.blue : Color.green)
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
            Button("Kapcsolódás") { state.connect(connection) }
            Divider()
            Button("Szerkesztés…") { state.edit(connection) }
            Button("Duplikálás") { state.duplicate(connection) }
            Button(connection.isFavorite ? "Eltávolítás a kedvencekből" : "Hozzáadás a kedvencekhez") {
                state.toggleFavorite(connection)
            }
            MoveMenu(currentFolderId: connection.folderId, excluding: nil) { state.move(connectionId: connection.id, to: $0) }
            Divider()
            Button("Jelszó másolása") { state.copyPassword(connection) }
            Button("Host másolása") { Clipboard.copy(connection.host, clearAfter: nil) }
            Divider()
            Button("Törlés", role: .destructive) { state.delete(connection) }
        case .folder(let folder):
            Button("Új RDP kapcsolat itt") { state.selection = tag; state.newConnection(.rdp) }
            Button("Új SSH kapcsolat itt") { state.selection = tag; state.newConnection(.ssh) }
            Button("Új almappa") { state.selection = tag; state.newFolder() }
            Divider()
            Button("Átnevezés…") { state.folderEditor = folder }
            MoveMenu(currentFolderId: folder.parentId, excluding: folder.id) { state.move(folderId: folder.id, to: $0) }
            Divider()
            Button("Mappa törlése", role: .destructive) { state.deleteFolder(folder) }
        case .credential(let credential):
            Button("Szerkesztés…") { state.edit(credential) }
            Divider()
            Button("Credential törlése", role: .destructive) { state.requestDelete(credential) }
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
        Menu("Áthelyezés") {
            Button("(Gyökér)") { move(nil) }.disabled(currentFolderId == nil)
            ForEach(LibraryTree.flattenedFolders(state.library, excluding: excluding), id: \.folder.id) { entry in
                Button(String(repeating: "   ", count: entry.depth) + entry.folder.name) { move(entry.folder.id) }
                    .disabled(entry.folder.id == currentFolderId)
            }
        }
    }
}
