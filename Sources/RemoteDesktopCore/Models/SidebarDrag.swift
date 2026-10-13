import Foundation

/// Drag & drop of sidebar rows. Rows are identified by tags (`f:` folder, `c:` connection, `fav:` favorite
/// row of a connection, `k:` credential); dragging a row of a multi-selection carries every selected tag.
public enum SidebarDrag {
    /// What a dragged row carries: the whole selection when the row is part of it, else just the row.
    public static func payload(for tag: String, selection: Set<String>) -> String {
        guard selection.count > 1, selection.contains(tag) else { return tag }
        return selection.sorted().joined(separator: "\n")
    }

    public static func tags(from payloads: [String]) -> [String] {
        payloads.flatMap { $0.split(separator: "\n").map(String.init) }
    }

    /// Connections and folders to move (de-duplicated, credentials ignored).
    public static func movable(_ tags: [String]) -> (connectionIds: [String], folderIds: [String]) {
        var connections: [String] = []
        var folders: [String] = []
        for tag in tags {
            if tag.hasPrefix("f:") {
                folders.append(String(tag.dropFirst(2)))
            } else if tag.hasPrefix("c:") {
                connections.append(String(tag.dropFirst(2)))
            } else if tag.hasPrefix("fav:") {
                connections.append(String(tag.dropFirst(4)))
            }
        }
        return (unique(connections), unique(folders))
    }

    private static func unique(_ ids: [String]) -> [String] {
        var seen = Set<String>()
        return ids.filter { seen.insert($0).inserted }
    }
}
