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

                Button("Import", systemImage: "square.and.arrow.down") { state.isImporting = true }
                    .help("Import from a Devolutions RDM export (.rdm)")
            }
        }
        .sheet(item: $state.editorDraft) { draft in
            ConnectionEditorView(draft: draft)
        }
        .sheet(item: $state.folderEditor) { folder in
            FolderEditorView(folder: folder)
        }
        .sheet(item: $state.credentialEditor) { draft in
            CredentialEditorView(draft: draft)
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
