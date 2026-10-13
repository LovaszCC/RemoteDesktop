import AppKit
import RemoteDesktopCore
import SwiftUI

/// Window root: creates the session for its target and closes it with the window.
struct FileBrowserWindow: View {
    @Environment(AppState.self) private var state
    let target: FileBrowserTarget?
    @State private var session: FileBrowserSession?
    @State private var local = LocalBrowser()

    var body: some View {
        Group {
            if let session {
                FileBrowserView(session: session, local: local)
                    .navigationTitle(Text("Files – \(session.connectionName)"))
            } else {
                ContentUnavailableView("Connection not found", systemImage: "questionmark.folder")
            }
        }
        .frame(minWidth: 820, minHeight: 460)
        .onAppear {
            guard session == nil, let target, let created = state.makeFileBrowser(target) else { return }
            session = created
            created.transfers.onItemFinished = { [weak created, weak local] direction in
                switch direction {
                case .download: local?.reload()
                case .upload: Task { await created?.refresh() }
                }
            }
            created.connect()
        }
        .onDisappear {
            if let session { state.closeFileBrowser(session) }
        }
    }
}

/// Remote pane | local pane, transfers below.
private struct FileBrowserView: View {
    @Bindable var session: FileBrowserSession
    @Bindable var local: LocalBrowser
    @State private var remoteSelection = Set<RemoteEntry.ID>()
    @State private var localSelection = Set<LocalBrowser.Entry.ID>()

    private var queue: TransferQueue { session.transfers }

    var body: some View {
        @Bindable var queue = session.transfers
        VStack(spacing: 0) {
            switch session.status {
            case .connecting:
                ProgressView("Connecting…").frame(maxWidth: .infinity, maxHeight: .infinity)
            case .failed(let message):
                ContentUnavailableView {
                    Label("Can't connect", systemImage: "exclamationmark.triangle")
                } description: {
                    Text(message)
                } actions: {
                    Button("Retry") { session.connect() }
                }
            case .ready, .closed:
                VSplitView {
                    HSplitView {
                        RemotePane(session: session, showsHidden: local.showsHidden, selection: $remoteSelection) { urls in
                            upload(urls.map { $0.standardizedFileURL })
                        }
                        transferButtons
                        LocalPane(local: local, sessionId: session.id, selection: $localSelection) { names in
                            queue.enqueue(names: names, direction: .download, from: session.remotePath, to: local.directory.path)
                        }
                    }
                    .frame(minHeight: 240)
                    if !queue.items.isEmpty {
                        TransferListView(queue: queue)
                            .frame(minHeight: 90, idealHeight: 150)
                    }
                }
            }
        }
        .toolbar {
            ToolbarItemGroup {
                Toggle(isOn: $queue.compress) {
                    Label("Compress", systemImage: "archivebox")
                }
                .help("Compress transfers with gzip (faster for text over slow links, can be slower for already compressed files)")
                Toggle(isOn: $local.showsHidden) {
                    Label("Hidden files", systemImage: "eye")
                }
                .help("Show hidden files")
                Button("Refresh", systemImage: "arrow.clockwise") {
                    local.reload()
                    Task { await session.refresh() }
                }
            }
        }
        .sheet(item: $queue.pendingConflict) { conflict in
            ConflictSheet(conflict: conflict, queue: queue)
        }
    }

    /// Download / upload the selected rows (several at once; dragging moves one row).
    private var transferButtons: some View {
        VStack(spacing: 10) {
            Spacer()
            Button {
                let names = session.remoteEntries.filter { remoteSelection.contains($0.id) }.map(\.name)
                queue.enqueue(names: names, direction: .download, from: session.remotePath, to: local.directory.path)
            } label: {
                Image(systemName: "arrow.right")
            }
            .help("Download the selected items into the local folder")
            .disabled(remoteSelection.isEmpty || !session.isReady)
            Button {
                upload(local.entries.filter { localSelection.contains($0.id) }.map(\.url))
            } label: {
                Image(systemName: "arrow.left")
            }
            .help("Upload the selected items into the remote folder")
            .disabled(localSelection.isEmpty || !session.isReady)
            Spacer()
        }
        .frame(width: 36)
    }

    /// Groups dropped / selected files by their folder (Finder drops can come from anywhere).
    private func upload(_ urls: [URL]) {
        guard session.isReady else { return }
        let byFolder = Dictionary(grouping: urls) { $0.deletingLastPathComponent().path }
        for (folder, files) in byFolder {
            queue.enqueue(names: files.map(\.lastPathComponent), direction: .upload, from: folder, to: session.remotePath)
        }
    }
}

// MARK: Panes

/// Path field with an Up button above a pane's table.
private struct PathBar: View {
    let title: LocalizedStringKey
    let path: String
    let goUp: () -> Void
    let go: (String) -> Void
    @State private var text = ""

    var body: some View {
        HStack(spacing: 6) {
            Text(title).font(.headline)
            Button(action: goUp) { Image(systemName: "arrow.up") }
                .buttonStyle(.borderless)
                .help("Parent folder")
            TextField("Path", text: $text)
                .textFieldStyle(.roundedBorder)
                .onSubmit { go(text) }
        }
        .padding(8)
        .onAppear { text = path }
        .onChange(of: path) { text = path }
    }
}

private func formatBytes(_ count: Int64) -> String {
    ByteCountFormatter.string(fromByteCount: count, countStyle: .file)
}

/// Drag payload for a remote row: a plain string, recognized by the local pane.
private enum RemoteDrag {
    static let prefix = "remotedesktop-remote-item:"

    static func payload(sessionId: String, name: String) -> String { prefix + sessionId + "/" + name }

    static func name(from payload: String, sessionId: String) -> String? {
        let expected = prefix + sessionId + "/"
        return payload.hasPrefix(expected) ? String(payload.dropFirst(expected.count)) : nil
    }
}

private struct RemotePane: View {
    let session: FileBrowserSession
    let showsHidden: Bool
    @Binding var selection: Set<RemoteEntry.ID>
    let upload: ([URL]) -> Void
    @State private var sortOrder = [KeyPathComparator(\RemoteEntry.name, comparator: .localizedStandard)]

    private var rows: [RemoteEntry] {
        let visible = showsHidden ? session.remoteEntries : session.remoteEntries.filter { !$0.name.hasPrefix(".") }
        return visible.sorted(using: sortOrder).sorted { $0.isDirectory && !$1.isDirectory }
    }

    var body: some View {
        VStack(spacing: 0) {
            PathBar(title: "Remote", path: session.remotePath) {
                Task { await session.goUp() }
            } go: { path in
                Task { await session.list(path) }
            }
            Table(rows, selection: $selection, sortOrder: $sortOrder) {
                TableColumn("Name", value: \.name, comparator: .localizedStandard) { entry in
                    Label {
                        Text(entry.name)
                    } icon: {
                        Image(systemName: entry.isDirectory ? "folder.fill" : entry.kind == .symlink ? "link" : "doc")
                            .foregroundStyle(entry.isDirectory ? Color.accentColor : .secondary)
                    }
                    .draggable(RemoteDrag.payload(sessionId: session.id, name: entry.name))
                }
                TableColumn("Size", value: \.size) { entry in
                    Text(entry.isDirectory ? "—" : formatBytes(entry.size))
                        .foregroundStyle(.secondary)
                }
                .width(min: 60, ideal: 80)
                TableColumn("Modified", value: \.sortDate) { entry in
                    Text(entry.modified.map { $0.formatted(date: .abbreviated, time: .shortened) } ?? "")
                        .foregroundStyle(.secondary)
                }
                .width(min: 100, ideal: 140)
                TableColumn("Permissions", value: \.permissions) { entry in
                    Text(entry.permissions).font(.caption.monospaced()).foregroundStyle(.secondary)
                }
                .width(min: 80, ideal: 90)
            }
            .contextMenu(forSelectionType: RemoteEntry.ID.self) { _ in
            } primaryAction: { ids in
                if let id = ids.first, let entry = session.remoteEntries.first(where: { $0.id == id }) {
                    Task { await session.open(entry) }
                }
            }
            .dropDestination(for: URL.self) { urls, _ in
                let files = urls.filter(\.isFileURL)
                upload(files)
                return !files.isEmpty
            }
            .overlay {
                if session.isListing { ProgressView().controlSize(.small) }
            }
            if let error = session.listError {
                Text(error).font(.caption).foregroundStyle(.red).padding(6)
            }
        }
        .frame(minWidth: 360)
        .onChange(of: session.remotePath) { selection = [] }
    }
}

private struct LocalPane: View {
    @Bindable var local: LocalBrowser
    /// Remote rows of this window only can be dropped here.
    let sessionId: String
    @Binding var selection: Set<LocalBrowser.Entry.ID>
    let download: ([String]) -> Void
    @State private var sortOrder = [KeyPathComparator(\LocalBrowser.Entry.name, comparator: .localizedStandard)]

    private var rows: [LocalBrowser.Entry] {
        local.entries.sorted(using: sortOrder).sorted { $0.isDirectory && !$1.isDirectory }
    }

    var body: some View {
        VStack(spacing: 0) {
            PathBar(title: "This Mac", path: local.directory.path(percentEncoded: false)) {
                local.goUp()
            } go: { path in
                local.go(to: URL(filePath: (path as NSString).expandingTildeInPath))
            }
            Table(rows, selection: $selection, sortOrder: $sortOrder) {
                TableColumn("Name", value: \.name, comparator: .localizedStandard) { entry in
                    Label {
                        Text(entry.name)
                    } icon: {
                        Image(nsImage: NSWorkspace.shared.icon(forFile: entry.url.path))
                            .resizable()
                            .frame(width: 16, height: 16)
                    }
                    .draggable(entry.url)
                }
                TableColumn("Size", value: \.size) { entry in
                    Text(entry.isDirectory ? "—" : formatBytes(entry.size))
                        .foregroundStyle(.secondary)
                }
                .width(min: 60, ideal: 80)
                TableColumn("Modified", value: \.sortDate) { entry in
                    Text(entry.modified.map { $0.formatted(date: .abbreviated, time: .shortened) } ?? "")
                        .foregroundStyle(.secondary)
                }
                .width(min: 100, ideal: 140)
                TableColumn("Permissions", value: \.permissions) { entry in
                    Text(entry.permissions).font(.caption.monospaced()).foregroundStyle(.secondary)
                }
                .width(min: 80, ideal: 90)
            }
            .contextMenu(forSelectionType: LocalBrowser.Entry.ID.self) { ids in
                Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting(Array(ids)) }
            } primaryAction: { ids in
                if let id = ids.first, let entry = local.entries.first(where: { $0.id == id }) { local.open(entry) }
            }
            .dropDestination(for: String.self) { payloads, _ in
                let names = payloads.compactMap { RemoteDrag.name(from: $0, sessionId: sessionId) }
                download(names)
                return !names.isEmpty
            }
            if let error = local.listError {
                Text(error).font(.caption).foregroundStyle(.red).padding(6)
            }
        }
        .frame(minWidth: 360)
        .onChange(of: local.directory) { selection = [] }
    }
}

extension RemoteEntry {
    var sortDate: Date { modified ?? .distantPast }
}

extension LocalBrowser.Entry {
    var sortDate: Date { modified ?? .distantPast }
}

// MARK: Transfers

private struct TransferListView: View {
    let queue: TransferQueue

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Transfers").font(.headline)
                Spacer()
                Button("Clear finished") { queue.clearFinished() }
                    .disabled(!queue.items.contains(where: \.isFinished))
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            List(queue.items) { item in
                TransferRow(item: item) { queue.cancel(item) }
            }
            .listStyle(.plain)
        }
        .background(.bar)
    }
}

private struct TransferRow: View {
    let item: TransferQueue.Item
    let cancel: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: item.direction == .download ? "arrow.down.circle" : "arrow.up.circle")
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 3) {
                Text(item.name).lineLimit(1)
                switch item.status {
                case .running:
                    if let fraction = item.fraction {
                        ProgressView(value: fraction)
                    } else {
                        ProgressView().progressViewStyle(.linear)
                    }
                case .failed(let message):
                    Text(message).font(.caption).foregroundStyle(.red).lineLimit(2)
                default:
                    EmptyView()
                }
            }
            Spacer()
            Text(statusText).font(.caption).foregroundStyle(.secondary)
            if !item.isFinished {
                Button(action: cancel) { Image(systemName: "xmark.circle.fill") }
                    .buttonStyle(.borderless)
                    .help("Cancel")
            }
        }
    }

    private var statusText: String {
        switch item.status {
        case .waiting: String(localized: "Waiting")
        case .running:
            if let total = item.bytesTotal {
                "\(formatBytes(item.bytesDone)) / \(formatBytes(total))"
            } else {
                formatBytes(item.bytesDone)
            }
        case .done: String(localized: "Done")
        case .skipped: String(localized: "Skipped")
        case .cancelled: String(localized: "Cancelled")
        case .failed: String(localized: "Failed")
        }
    }
}

private struct ConflictSheet: View {
    let conflict: TransferQueue.Conflict
    let queue: TransferQueue
    @State private var applyToAll = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("“\(conflict.name)” already exists").font(.headline)
            Text(conflict.isFolder
                 ? "Replacing a folder merges it: files with the same name are overwritten, other files stay."
                 : "Replacing overwrites the existing file.")
                .foregroundStyle(.secondary)
            Toggle("Apply to all", isOn: $applyToAll)
            HStack {
                Spacer()
                Button("Skip") { queue.answerConflict(.skip, applyToAll: applyToAll) }
                    .keyboardShortcut(.cancelAction)
                Button("Replace") { queue.answerConflict(.replace, applyToAll: applyToAll) }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 400)
    }
}
