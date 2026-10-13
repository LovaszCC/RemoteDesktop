import Foundation
import GRDB

public struct Folder: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var parentId: String?
    public var name: String
    public var sortOrder: Int
    /// Icon color in the sidebar.
    public var color: LabelColor?

    public init(id: String = UUID().uuidString, parentId: String? = nil, name: String, sortOrder: Int = 0, color: LabelColor? = nil) {
        self.id = id
        self.parentId = parentId
        self.name = name
        self.sortOrder = sortOrder
        self.color = color
    }
}

extension Folder: FetchableRecord, PersistableRecord {
    public static let databaseTableName = "folder"
}
