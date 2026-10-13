import Foundation
import GRDB

/// Writes imported connections and credentials in one transaction, creating their group folders under
/// `parentFolderId` (nil: the root) and reusing folders of the same name that already exist there.
public enum ImportWriter {
    public static func write(
        connections: [ImportedConnection],
        credentials: [ImportedCredential] = [],
        emptyGroups: [[String]] = [],
        under parentFolderId: String? = nil,
        into repo: ConnectionRepository,
        vault: Vault
    ) throws -> (imported: Int, foldersCreated: Int) {
        let encrypted = try connections.map { ($0, try vault.encryptOptional($0.password)) }
        let newCredentials = try credentials.map { item in
            (item.ref, Credential(
                name: item.name,
                username: item.username,
                domain: item.domain,
                passwordEnc: try vault.encryptOptional(item.password),
                sshKeyPath: item.sshKeyPath
            ))
        }
        let credentialIds = Dictionary(newCredentials.map { ($0.0, $0.1.id) }, uniquingKeysWith: { first, _ in first })
        let foldersBefore = try repo.library().folders.count

        try repo.transaction { db in
            var cache: [String: String] = [:]   // joined path → folder id

            func folderId(for path: [String]) throws -> String? {
                var parentId = parentFolderId
                for depth in path.indices {
                    let key = path[...depth].joined(separator: "\u{1F}")
                    if let cached = cache[key] { parentId = cached; continue }
                    let name = path[depth]
                    let existing = try Folder
                        .filter(Column("name") == name)
                        .filter(parentId == nil ? Column("parentId") == nil : Column("parentId") == parentId!)
                        .fetchOne(db)
                    let folder = try existing ?? {
                        let created = Folder(parentId: parentId, name: name)
                        try created.insert(db)
                        return created
                    }()
                    cache[key] = folder.id
                    parentId = folder.id
                }
                return parentId
            }

            for (_, credential) in newCredentials { try credential.insert(db) }
            for group in emptyGroups { _ = try folderId(for: group) }
            for (item, passwordEnc) in encrypted {
                try Connection(
                    folderId: try folderId(for: item.groupPath),
                    credentialId: item.credentialRef.flatMap { credentialIds[$0] },
                    type: item.type,
                    name: item.name,
                    host: item.host,
                    port: item.port,
                    username: item.username,
                    domain: item.domain,
                    passwordEnc: passwordEnc,
                    sshKeyPath: item.sshKeyPath,
                    rdpAdminSession: item.rdpAdminSession,
                    notes: item.notes
                ).insert(db)
            }
        }
        return (connections.count, try repo.library().folders.count - foldersBefore)
    }
}
