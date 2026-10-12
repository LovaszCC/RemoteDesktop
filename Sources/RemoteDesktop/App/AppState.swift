import AppKit
import GRDB
import Observation
import RemoteDesktopCore

/// Editable, decrypted copy of a connection used by the editor sheet.
struct ConnectionDraft: Identifiable, Equatable {
    var id: String
    var isNew: Bool
    var type: ConnectionType
    var folderId: String?
    /// Linked credential; when set, the inline auth fields below are kept only as a fallback.
    var credentialId: String?
    var name = ""
    var host = ""
    var port = ""
    var username = ""
    var domain = ""
    var password = ""
    var sshKeyPath = ""
    var sshKeyPassphrase = ""
    var notes = ""
    var isFavorite = false
    var rdpAdminSession = false
    var browserBundleId: String?

    init(type: ConnectionType, folderId: String?) {
        id = UUID().uuidString
        isNew = true
        self.type = type
        self.folderId = folderId
    }

    init(connection: Connection, vault: Vault) throws {
        id = connection.id
        isNew = false
        type = connection.type
        folderId = connection.folderId
        credentialId = connection.credentialId
        name = connection.name
        host = connection.host
        port = connection.port.map(String.init) ?? ""
        username = connection.username
        domain = connection.domain
        password = try vault.decryptOptional(connection.passwordEnc) ?? ""
        sshKeyPath = connection.sshKeyPath ?? ""
        sshKeyPassphrase = try vault.decryptOptional(connection.sshKeyPassphraseEnc) ?? ""
        notes = connection.notes
        isFavorite = connection.isFavorite
        rdpAdminSession = connection.rdpAdminSession
        browserBundleId = connection.browserBundleId
    }

    var isValid: Bool {
        guard !name.trimmingCharacters(in: .whitespaces).isEmpty else { return false }
        if !type.usesPort { return WebLaunch.url(from: host) != nil }
        return !host.trimmingCharacters(in: .whitespaces).isEmpty
            && (port.isEmpty || Int(port).map { (1...65535).contains($0) } == true)
    }
}

@MainActor
@Observable
final class AppState {
    enum Phase: Equatable {
        case locked(needsSetup: Bool)
        case unlocked
        case failed(String)
    }

    private static let databasePathKey = "databasePath"
    private static let rememberKeyKey = "rememberMasterKey"

    private(set) var phase: Phase = .locked(needsSetup: false)
    private(set) var library = Library()
    private(set) var databaseURL: URL
    var errorMessage: String?
    var toast: String?

    // UI state shared with menu commands.
    var selection: String?
    var searchText = ""
    var editorDraft: ConnectionDraft?
    var folderEditor: Folder?
    var credentialEditor: CredentialDraft?
    var pendingCredentialDeletion: Credential?
    var isImporting = false

    @ObservationIgnored private var repo: ConnectionRepository?
    @ObservationIgnored private var vault: Vault?
    @ObservationIgnored private var observation: AnyDatabaseCancellable?
    @ObservationIgnored private let keychain = KeychainStore()
    @ObservationIgnored private var toastTask: Task<Void, Never>?

    var rememberKey: Bool {
        get { UserDefaults.standard.object(forKey: Self.rememberKeyKey) as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: Self.rememberKeyKey) }
    }

    init() {
        databaseURL = UserDefaults.standard.string(forKey: Self.databasePathKey).map { URL(filePath: $0) }
            ?? AppDatabase.defaultURL
        openDatabase()
    }

    // MARK: Database & vault

    private var keychainAccount: String { databaseURL.standardizedFileURL.path }

    func openDatabase() {
        lock(clearCachedKey: false)
        do {
            let repo = ConnectionRepository(database: try AppDatabase.open(at: databaseURL))
            self.repo = repo
            guard try Vault.isInitialized(repo) else {
                phase = .locked(needsSetup: true)
                return
            }
            if let keyData = keychain.load(account: keychainAccount),
               let vault = try? Vault.unlock(keyData: keyData, in: repo) {
                didUnlock(vault)
            } else {
                phase = .locked(needsSetup: false)
            }
        } catch {
            phase = .failed("Az adatbázis nem nyitható meg: \(error.localizedDescription)")
        }
    }

    func createVault(password: String, remember: Bool) {
        guard let repo else { return }
        perform {
            let vault = try Vault.create(password: password, in: repo)
            rememberKey = remember
            didUnlock(vault)
        }
    }

    /// Returns `false` on a wrong password so the unlock view can shake/clear.
    @discardableResult
    func unlock(password: String, remember: Bool) -> Bool {
        guard let repo else { return false }
        do {
            let vault = try Vault.unlock(password: password, in: repo)
            rememberKey = remember
            didUnlock(vault)
            return true
        } catch VaultError.wrongPassword {
            return false
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }

    private func didUnlock(_ vault: Vault) {
        self.vault = vault
        if rememberKey {
            try? keychain.save(vault.keyData, account: keychainAccount)
        } else {
            keychain.delete(account: keychainAccount)
        }
        phase = .unlocked
        startObserving()
    }

    func lock(clearCachedKey: Bool = true) {
        observation?.cancel()
        observation = nil
        vault = nil
        library = Library()
        if clearCachedKey {
            keychain.delete(account: keychainAccount)
        }
        if repo != nil { phase = .locked(needsSetup: false) }
    }

    func changeMasterPassword(current: String, new: String) -> Bool {
        guard let repo else { return false }
        do {
            let old = try Vault.unlock(password: current, in: repo)
            let updated = try old.changePassword(to: new, in: repo)
            didUnlock(updated)
            showToast("Mesterjelszó módosítva")
            return true
        } catch VaultError.wrongPassword {
            errorMessage = "A jelenlegi mesterjelszó hibás."
        } catch {
            errorMessage = error.localizedDescription
        }
        return false
    }

    /// Switches to another database file (existing or new) and locks.
    func useDatabase(at url: URL) {
        databaseURL = url
        UserDefaults.standard.set(url.path, forKey: Self.databasePathKey)
        openDatabase()
    }

    private func startObserving() {
        guard let repo else { return }
        observation = repo.observeLibrary().start(
            in: repo.database.writer,
            scheduling: .immediate,
            onError: { [weak self] error in self?.errorMessage = error.localizedDescription },
            onChange: { [weak self] library in self?.library = library }
        )
    }

    // MARK: Selection

    enum SelectedItem {
        case folder(Folder)
        case connection(Connection)
        case credential(Credential)
    }

    /// Resolves sidebar tags: `f:<id>`, `c:<id>`, `fav:<id>`, `k:<id>` (credential).
    func item(for tag: String?) -> SelectedItem? {
        guard let tag, let colon = tag.firstIndex(of: ":") else { return nil }
        let id = String(tag[tag.index(after: colon)...])
        if tag.hasPrefix("f:") {
            return library.folders.first { $0.id == id }.map(SelectedItem.folder)
        }
        if tag.hasPrefix("k:") {
            return library.credential(id: id).map(SelectedItem.credential)
        }
        return library.connections.first { $0.id == id }.map(SelectedItem.connection)
    }

    /// Folder new items should go into, based on the current selection.
    var selectedFolderId: String? {
        switch item(for: selection) {
        case .folder(let folder): folder.id
        case .connection(let connection): connection.folderId
        case .credential, nil: nil
        }
    }

    func newConnection(_ type: ConnectionType) {
        guard phase == .unlocked else { return }
        editorDraft = ConnectionDraft(type: type, folderId: selectedFolderId)
    }

    func newFolder() {
        guard phase == .unlocked else { return }
        folderEditor = Folder(parentId: selectedFolderId, name: "")
    }

    func edit(_ connection: Connection) {
        editorDraft = draft(for: connection)
    }

    /// Double-click / Return on a sidebar row.
    func activate(tag: String) {
        if case .connection(let connection) = item(for: tag) { connect(connection) }
    }

    // MARK: Editing

    func draft(for connection: Connection) -> ConnectionDraft? {
        guard let vault else { return nil }
        do { return try ConnectionDraft(connection: connection, vault: vault) }
        catch { errorMessage = "Nem sikerült visszafejteni: \(error.localizedDescription)"; return nil }
    }

    func save(_ draft: ConnectionDraft) {
        guard let repo, let vault else { return }
        perform {
            let existing = draft.isNew ? nil : try repo.connection(id: draft.id)
            let trimmedKey = draft.sshKeyPath.trimmingCharacters(in: .whitespaces)
            try repo.save(Connection(
                id: draft.id,
                folderId: draft.folderId,
                credentialId: draft.credentialId,
                type: draft.type,
                name: draft.name.trimmingCharacters(in: .whitespaces),
                host: draft.host.trimmingCharacters(in: .whitespaces),
                port: draft.type.usesPort ? Int(draft.port) : nil,
                username: draft.username.trimmingCharacters(in: .whitespaces),
                domain: draft.type == .rdp ? draft.domain.trimmingCharacters(in: .whitespaces) : "",
                passwordEnc: try vault.encryptOptional(draft.password),
                sshKeyPath: draft.type == .ssh && !trimmedKey.isEmpty ? trimmedKey : nil,
                sshKeyPassphraseEnc: draft.type == .ssh ? try vault.encryptOptional(draft.sshKeyPassphrase) : nil,
                isFavorite: draft.isFavorite,
                rdpAdminSession: draft.type == .rdp && draft.rdpAdminSession,
                browserBundleId: draft.type == .web ? draft.browserBundleId : nil,
                notes: draft.notes,
                createdAt: existing?.createdAt ?? .now
            ))
        }
    }

    func duplicate(_ connection: Connection) {
        guard let repo else { return }
        var copy = connection
        copy.id = UUID().uuidString
        copy.name += " (másolat)"
        copy.createdAt = .now
        perform { try repo.save(copy) }
    }

    func delete(_ connection: Connection) {
        perform { try repo?.deleteConnection(id: connection.id) }
    }

    func toggleFavorite(_ connection: Connection) {
        perform { try repo?.setFavorite(id: connection.id, !connection.isFavorite) }
    }

    func move(connectionId: String, to folderId: String?) {
        perform { try repo?.moveConnection(id: connectionId, to: folderId) }
    }

    func move(folderId: String, to parentId: String?) {
        perform {
            do { try repo?.moveFolder(id: folderId, to: parentId) }
            catch RepositoryError.folderCycle { showToast("Mappa nem helyezhető saját magába") }
        }
    }

    func saveFolder(_ folder: Folder) {
        perform { try repo?.save(folder) }
    }

    func deleteFolder(_ folder: Folder) {
        perform { try repo?.deleteFolder(id: folder.id) }
    }

    // MARK: Credentials

    func newCredential() {
        guard phase == .unlocked else { return }
        credentialEditor = CredentialDraft()
    }

    func edit(_ credential: Credential) {
        guard let vault else { return }
        do { credentialEditor = try CredentialDraft(credential: credential, vault: vault) }
        catch { errorMessage = "Nem sikerült visszafejteni: \(error.localizedDescription)" }
    }

    /// Throws when saving failed (e.g. wrong key passphrase) so the editor can stay open and show it.
    func save(_ draft: CredentialDraft) throws {
        guard let repo, let vault else { return }
        let existing = draft.isNew ? nil : try repo.credential(id: draft.id)
        let trimmedKey = draft.sshKeyPath.trimmingCharacters(in: .whitespaces)
        let unlockedKey = try unlockedKeyEnc(path: trimmedKey, passphrase: draft.sshKeyPassphrase, existing: existing, vault: vault)
        try repo.save(Credential(
            id: draft.id,
            name: draft.name.trimmingCharacters(in: .whitespaces),
            username: draft.username.trimmingCharacters(in: .whitespaces),
            domain: draft.domain.trimmingCharacters(in: .whitespaces),
            passwordEnc: try vault.encryptOptional(draft.password),
            sshKeyPath: trimmedKey.isEmpty ? nil : trimmedKey,
            sshKeyPassphraseEnc: try vault.encryptOptional(draft.sshKeyPassphrase),
            sshKeyUnlockedEnc: unlockedKey,
            notes: draft.notes,
            createdAt: existing?.createdAt ?? .now
        ))
    }

    /// Unlocks a passphrase-protected key so SSH can use it without prompting.
    /// Keeps the stored copy when the key file is gone but path and passphrase are unchanged.
    private func unlockedKeyEnc(path: String, passphrase: String, existing: Credential?, vault: Vault) throws -> Data? {
        guard !path.isEmpty, !passphrase.isEmpty else { return nil }
        do {
            return try vault.encrypt(SSHKeyUnlocker.unlock(keyPath: path, passphrase: passphrase))
        } catch SSHKeyUnlockError.fileUnreadable {
            if let existing, let stored = existing.sshKeyUnlockedEnc, existing.sshKeyPath == path,
               try vault.decryptOptional(existing.sshKeyPassphraseEnc) == passphrase {
                return stored
            }
            throw SSHKeyUnlockError.fileUnreadable(path)
        }
    }

    /// Asks for confirmation when connections still use the credential.
    func requestDelete(_ credential: Credential) {
        if library.connections(using: credential.id).isEmpty {
            confirmDelete(credential)
        } else {
            pendingCredentialDeletion = credential
        }
    }

    func confirmDelete(_ credential: Credential) {
        pendingCredentialDeletion = nil
        if selection == "k:\(credential.id)" { selection = nil }
        perform { try repo?.deleteCredential(id: credential.id) }
    }

    // MARK: Import

    func importRDM(from url: URL) {
        guard let repo, let vault else { return }
        perform {
            let accessing = url.startAccessingSecurityScopedResource()
            defer { if accessing { url.stopAccessingSecurityScopedResource() } }
            let summary = try RDMImporter.importFile(Data(contentsOf: url), into: repo, vault: vault)
            var message = "\(summary.imported) kapcsolat, \(summary.credentialsImported) credential, \(summary.foldersCreated) új mappa importálva."
            if summary.withoutPassword > 0 {
                message += "\n\(summary.withoutPassword) bejegyzés jelszava titkosított volt az exportban, ezeket kézzel kell megadni."
            }
            if !summary.skipped.isEmpty {
                message += "\nKihagyva (\(summary.skipped.count)): " + summary.skipped.prefix(15).joined(separator: ", ")
            }
            errorMessage = message
        }
    }

    // MARK: Launch

    func connect(_ connection: Connection) {
        guard let vault else { return }
        let connection = library.resolved(connection)
        do {
            let password = try vault.decryptOptional(connection.passwordEnc)
            let outcome: LaunchOutcome
            switch connection.type {
            case .rdp:
                outcome = try RDPLauncher.launch(connection, password: password) { [weak self] message in
                    self?.showToast(message)
                }
            case .ssh:
                outcome = try SSHLauncher.launch(
                    connection,
                    password: password,
                    keyPassphrase: try vault.decryptOptional(connection.sshKeyPassphraseEnc),
                    unlockedKey: try vault.decryptOptional(library.credential(id: connection.credentialId)?.sshKeyUnlockedEnc)
                )
            case .web:
                outcome = try WebLauncher.launch(connection, password: password)
            }
            switch outcome {
            case .launched: showToast("Kapcsolódás: \(connection.name)")
            case .passwordCopied(let reason): showToast("Jelszó a vágólapon (\(reason))")
            case .loginCopied: showToast("Megnyitva: \(connection.name) · jelszó, majd felhasználónév a vágólapon")
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func copyPassword(_ connection: Connection) {
        guard let vault else { return }
        let connection = library.resolved(connection)
        perform {
            guard let password = try vault.decryptOptional(connection.passwordEnc) else {
                showToast("Nincs mentett jelszó")
                return
            }
            Clipboard.copy(password)
            showToast("Jelszó a vágólapon (30 mp)")
        }
    }

    // MARK: Helpers

    func showToast(_ message: String) {
        toast = message
        toastTask?.cancel()
        toastTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(3.5))
            guard !Task.isCancelled else { return }
            self?.toast = nil
        }
    }

    private func perform(_ work: () throws -> Void) {
        do { try work() } catch { errorMessage = error.localizedDescription }
    }
}
