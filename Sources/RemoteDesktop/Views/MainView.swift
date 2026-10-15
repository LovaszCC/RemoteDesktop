import RemoteDesktopCore
import SwiftUI
import UniformTypeIdentifiers

struct MainView: View {
    @Environment(AppState.self) private var state

    var body: some View {
        @Bindable var state = state
        NavigationSplitView {
            SidebarView()
                .navigationSplitViewColumnWidth(min: 220, ideal: 280, max: 420)
        } detail: {
            DetailArea()
        }
        .onChange(of: state.selection) { state.selectTab(nil) }
        .searchable(text: $state.searchText, placement: .sidebar, prompt: "Search")
        .toolbar {
            ToolbarItemGroup {
                Menu {
                    ForEach(ConnectionType.allCases, id: \.self) { type in
                        Button(type.newTitle, systemImage: type.systemImage) { state.newConnection(type) }
                    }
                    Divider()
                    Button("New folder", systemImage: "folder.badge.plus") { state.newFolder() }
                    Button("New credential", systemImage: "person.badge.key") { state.newCredential() }
                } label: {
                    Label("New", systemImage: "plus")
                }
                .help("New connection or folder")

                Menu {
                    Button("Import from RDM…") { state.isImporting = true }
                    Button("Import from WinBox…") { state.chooseWinboxFile() }
                } label: {
                    Label("Import", systemImage: "square.and.arrow.down")
                }
                .help("Import from a Devolutions RDM export (.rdm) or a WinBox address book (.cdb)")
            }
        }
        // Split up: one long modifier chain is too slow for the type checker on CI.
        .modifier(MainSheets())
        .modifier(MainDialogs())
        .modifier(MainImporters())
    }
}

/// Editors and other sheets of the main window.
private struct MainSheets: ViewModifier {
    @Environment(AppState.self) private var state

    func body(content: Content) -> some View {
        @Bindable var state = state
        content
            .sheet(item: $state.editorDraft) { draft in
                ConnectionEditorView(draft: draft)
            }
            .sheet(item: $state.folderEditor) { folder in
                FolderEditorView(folder: folder)
            }
            .sheet(item: $state.credentialEditor) { draft in
                CredentialEditorView(draft: draft)
            }
            .sheet(item: $state.savedSplitPrompt) { prompt in
                SavedSplitNameSheet(name: prompt.name, isRename: { if case .rename = prompt.source { true } else { false } }())
            }
            .sheet(item: $state.winboxImport) { draft in
                WinboxImportSheet(draft: draft)
            }
    }
}

/// Confirmations, the host key alert and opening file browser windows.
private struct MainDialogs: ViewModifier {
    @Environment(AppState.self) private var state
    @Environment(\.openWindow) private var openWindow

    func body(content: Content) -> some View {
        @Bindable var state = state
        content
            // Shown here, not on the port forward list: tunnels and file browsers start from anywhere.
            .alert("Unknown host key", isPresented: Binding(
                get: { state.pendingHostKey != nil },
                set: { if !$0 { state.pendingHostKey = nil } }
            ), presenting: state.pendingHostKey) { _ in
                Button("Accept and Start") { state.acceptPendingHostKey() }
                Button("Reject", role: .cancel) { state.pendingHostKey = nil }
            } message: { pending in
                Text("\(pending.target.lookupName) isn't in known_hosts yet. Accept only if these fingerprints match the server's keys:\n\n\(pending.fingerprints.joined(separator: "\n"))")
            }
            .onChange(of: state.filesWindowRequest) {
                guard let target = state.filesWindowRequest else { return }
                state.filesWindowRequest = nil
                openWindow(id: "files", value: target)
            }
            .confirmationDialog(
                Text("Delete \(state.pendingBulkDeletion?.count ?? 0) items?"),
                isPresented: Binding(
                    get: { state.pendingBulkDeletion != nil },
                    set: { if !$0 { state.pendingBulkDeletion = nil } }
                ),
                presenting: state.pendingBulkDeletion
            ) { _ in
                Button("Delete", role: .destructive) { state.confirmBulkDelete() }
                Button("Cancel", role: .cancel) {}
            } message: { deletion in
                Text(bulkDeletionMessage(deletion))
            }
            .confirmationDialog(
                "Delete credential?",
                isPresented: Binding(
                    get: { state.pendingCredentialDeletion != nil },
                    set: { if !$0 { state.pendingCredentialDeletion = nil } }
                ),
                presenting: state.pendingCredentialDeletion
            ) { credential in
                Button("Delete", role: .destructive) { state.confirmDelete(credential) }
                Button("Cancel", role: .cancel) {}
            } message: { credential in
                let count = state.library.connections(using: credential.id).count
                Text("\(count) connections use the credential “\(credential.name)”. After deleting it they use their own saved login.")
            }
    }

    private func bulkDeletionMessage(_ deletion: AppState.BulkDeletion) -> String {
        var lines = [String(localized: "Connections: \(deletion.connectionIds.count), folders: \(deletion.folderIds.count), credentials: \(deletion.credentialIds.count).")]
        if !deletion.folderIds.isEmpty {
            lines.append(String(localized: "Subfolders are deleted too; connections inside the folders are kept and move to Connections."))
        }
        if !deletion.credentialIds.isEmpty {
            lines.append(String(localized: "Connections using a deleted credential fall back to their own saved login."))
        }
        lines.append(String(localized: "This can't be undone."))
        return lines.joined(separator: "\n")
    }
}

/// File picker of the RDM import (WinBox uses an `NSOpenPanel`, see `AppState.chooseWinboxFile`).
private struct MainImporters: ViewModifier {
    @Environment(AppState.self) private var state

    func body(content: Content) -> some View {
        @Bindable var state = state
        content
            .fileImporter(
                isPresented: $state.isImporting,
                allowedContentTypes: [UTType(filenameExtension: "rdm") ?? .xml, .xml]
            ) { result in
                switch result {
                case .success(let url): state.importRDM(from: url)
                case .failure(let error): state.errorMessage = error.localizedDescription
                }
            }
    }
}
