import RemoteDesktopCore
import SwiftUI

/// Preview of a WinBox address book: pick the entries and the folder their WinBox groups go into.
struct WinboxImportSheet: View {
    @Environment(AppState.self) private var state
    @Environment(\.dismiss) private var dismiss
    @State var draft: WinboxImportDraft

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Import from WinBox").font(.headline)
            Form {
                FolderPicker(title: "Target folder", library: state.library, selection: $draft.targetFolderId)
                Text("WinBox groups become subfolders of the target folder.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .formStyle(.grouped)
            .frame(height: 110)
            .onChange(of: draft.targetFolderId) { state.markDuplicates(&draft) }

            Table($draft.rows) {
                TableColumn("") { $row in
                    Toggle("", isOn: $row.include).labelsHidden()
                }
                .width(24)
                TableColumn("Name") { $row in
                    HStack(spacing: 6) {
                        Text(row.item.name)
                        if row.exists {
                            Text("already exists")
                                .font(.caption2)
                                .padding(.horizontal, 5)
                                .background(.quaternary, in: Capsule())
                        }
                    }
                }
                TableColumn("Address") { $row in Text(row.item.host).foregroundStyle(.secondary) }
                TableColumn("User") { $row in Text(row.item.username).foregroundStyle(.secondary) }
                TableColumn("Group") { $row in Text(row.item.groupPath.joined(separator: " / ")).foregroundStyle(.secondary) }
                TableColumn("Password") { $row in
                    Image(systemName: row.item.password == nil ? "minus" : "checkmark")
                        .foregroundStyle(.secondary)
                }
                .width(60)
            }
            .frame(minHeight: 220)

            HStack {
                Button("Select all") { setAll(true) }
                Button("Select none") { setAll(false) }
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Import \(draft.includedCount)") { state.importWinbox(draft) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(draft.includedCount == 0)
            }
        }
        .padding(20)
        .frame(width: 720, height: 520)
    }

    private func setAll(_ include: Bool) {
        for index in draft.rows.indices { draft.rows[index].include = include }
    }
}
