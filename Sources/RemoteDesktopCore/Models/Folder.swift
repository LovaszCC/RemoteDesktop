import Foundation
import GRDB

public struct Folder: Codable, Hashable, Identifiable, Sendable {
    public var id: String
    public var parentId: String?
    public var name: String
    public var sortOrder: Int

    public init(id: String = UUID().uuidString, parentId: String? = nil, name: String, sortOrder: Int = 0) {
        self.id = id
        self.parentId = parentId
        self.name = name
        self.sortOrder = sortOrder
    }
}

extension Folder: FetchableRecord, PersistableRecord {
    public static let databaseTableName = "folder"
}
