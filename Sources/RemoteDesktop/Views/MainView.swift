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
        .searchable(text: $state.searchText, placement: .sidebar, prompt: "Keresés")
        .toolbar {
            ToolbarItemGroup {
                Menu {
                    ForEach(ConnectionType.allCases, id: \.self) { type in
                        Button(type.newTitle, systemImage: type.systemImage) { state.newConnection(type) }
                    }
                    Divider()
                    Button("Új mappa", systemImage: "folder.badge.plus") { state.newFolder() }
                    Button("Új credential", systemImage: "person.badge.key") { state.newCredential() }
                } label: {
                    Label("Új", systemImage: "plus")
                }
                .help("Új kapcsolat vagy mappa")

                Button("Importálás", systemImage: "square.and.arrow.down") { state.isImporting = true }
                    .help("Importálás Devolutions RDM exportból (.rdm)")
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
            "Credential törlése?",
            isPresented: Binding(
                get: { state.pendingCredentialDeletion != nil },
                set: { if !$0 { state.pendingCredentialDeletion = nil } }
            ),
            presenting: state.pendingCredentialDeletion
        ) { credential in
            Button("Törlés", role: .destructive) { state.confirmDelete(credential) }
            Button("Mégse", role: .cancel) {}
        } message: { credential in
            let count = state.library.connections(using: credential.id).count
            Text("\(count) kapcsolat használja a(z) „\(credential.name)” credentialt. Törlés után ezek a saját mentett adataikat használják.")
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
