import Foundation
import RemoteDesktopCore

/// Entries parsed from a WinBox address book, with the user's choices before importing.
struct WinboxImportDraft: Identifiable {
    struct Row: Identifiable {
        let id = UUID()
        let item: ImportedConnection
        var include = true
        /// Already in the destination folder (unchecked by default).
        var exists = false
    }

    let id = UUID()
    var rows: [Row]
    /// Folder the WinBox groups are created in (nil: the root).
    var targetFolderId: String?

    var includedCount: Int { rows.filter(\.include).count }
}
