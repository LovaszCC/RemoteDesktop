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
            DetailView()
        }
        .searchable(text: $state.searchText, placement: .sidebar, prompt: "Keresés")
        .toolbar {
            ToolbarItemGroup {
                Menu {
                    Button("Új RDP kapcsolat", systemImage: "display") { state.newConnection(.rdp) }
                    Button("Új SSH kapcsolat", systemImage: "terminal") { state.newConnection(.ssh) }
                    Divider()
                    Button("Új mappa", systemImage: "folder.badge.plus") { state.newFolder() }
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
