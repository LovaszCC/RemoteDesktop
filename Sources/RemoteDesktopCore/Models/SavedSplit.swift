import Foundation
import GRDB

/// A split view saved for reopening: its panes' connections in order (the same one may appear more than
/// once). Broadcast settings are deliberately not saved.
public struct SavedSplit: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var name: String
    /// Stored as JSON text.
    public var connectionIds: [String]
    public var sortOrder: Int
    public var createdAt: Date

    public init(id: String = UUID().uuidString, name: String, connectionIds: [String], sortOrder: Int = 0, createdAt: Date = .now) {
        self.id = id
        self.name = name
        self.connectionIds = connectionIds
        self.sortOrder = sortOrder
        self.createdAt = createdAt
    }

    /// The panes' connections that still exist, in order, at most `SplitLayout.maxPanes`.
    public func connections(in library: Library) -> [Connection] {
        let byId = Dictionary(library.connections.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        return Array(connectionIds.compactMap { byId[$0] }.prefix(SplitLayout.maxPanes))
    }
}

extension SavedSplit: FetchableRecord, PersistableRecord {
    public static let databaseTableName = "savedSplit"
}
