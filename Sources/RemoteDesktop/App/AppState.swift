import AppKit
import GRDB
import Observation
import RDPBridge
import RemoteDesktopCore
import UniformTypeIdentifiers

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
    var rdpClient: RDPClient = .embedded
    var browserBundleId: String?
    var mikrotikMode: MikrotikMode = .winbox
    var sshTerminal: SSHTerminal = .embedded
    var color: LabelColor?

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
        rdpClient = connection.rdpClient
        browserBundleId = connection.browserBundleId
        mikrotikMode = connection.mikrotikMode
        sshTerminal = connection.sshTerminal
        color = connection.color
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
    /// Sidebar selection (several rows with ⌘/⇧-click).
    var selectedTags: Set<String> = []
    /// The selected item when exactly one row is selected; setting it replaces the selection.
    var selection: String? {
        get { selectedTags.count == 1 ? selectedTags.first : nil }
        set { selectedTags = newValue.map { [$0] } ?? [] }
    }
    /// Tag of a just-saved item to select once the observed library contains it.
    private var pendingSelection: String?
    var searchText = ""
    var editorDraft: ConnectionDraft?
    var folderEditor: Folder?
    var credentialEditor: CredentialDraft?
    var pendingCredentialDeletion: Credential?
    var isImporting = false
    /// A parsed WinBox address book waiting for the user to pick entries and a target folder.
    var winboxImport: WinboxImportDraft?

    // Embedded SSH / RDP sessions shown as tabs in the detail area.
    private(set) var sessions: [AppSession] = []
    private(set) var tabs = SessionTabs()

    // SSH port forward tunnels by forward id; failed ones stay to show their error.
    private(set) var portForwardSessions: [String: PortForwardSession] = [:]
    /// An unknown gateway host key waiting for the user's decision before the tunnel starts.
    var pendingHostKey: PendingHostKey?

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
        stopAllPortForwards()
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
            phase = .failed(String(localized: "Can't open the database: \(error.localizedDescription)"))
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
            showToast(String(localized: "Master password changed"))
            return true
        } catch VaultError.wrongPassword {
            errorMessage = String(localized: "The current master password is wrong.")
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
            onChange: { [weak self] library in
                self?.library = library
                self?.applyPendingSelection()
            }
        )
    }

    /// Selects a newly saved item. The observation delivers the updated library asynchronously,
    /// and selecting a row the sidebar `List` does not have yet leaves a ghost duplicate row.
    func select(afterSave tag: String) {
        pendingSelection = tag
        applyPendingSelection()
    }

    private func applyPendingSelection() {
        guard let tag = pendingSelection, item(for: tag) != nil else { return }
        pendingSelection = nil
        selection = tag
    }

    // MARK: Selection

    enum SelectedItem {
        case folder(Folder)
        case connection(Connection)
        case credential(Credential)
        case savedSplit(SavedSplit)
    }

    /// Resolves sidebar tags: `f:<id>`, `c:<id>`, `fav:<id>`, `k:<id>` (credential), `s:<id>` (saved split).
    func item(for tag: String?) -> SelectedItem? {
        guard let tag, let colon = tag.firstIndex(of: ":") else { return nil }
        let id = String(tag[tag.index(after: colon)...])
        if tag.hasPrefix("s:") {
            return library.savedSplits.first { $0.id == id }.map(SelectedItem.savedSplit)
        }
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
        case .credential, .savedSplit, nil: nil
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
        switch item(for: tag) {
        case .connection(let connection): connect(connection)
        case .savedSplit(let saved): openSavedSplit(saved)
        default: break
        }
    }

    // MARK: Editing

    func draft(for connection: Connection) -> ConnectionDraft? {
        guard let vault else { return nil }
        do { return try ConnectionDraft(connection: connection, vault: vault) }
        catch { errorMessage = String(localized: "Couldn't decrypt: \(error.localizedDescription)"); return nil }
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
                sshKeyPath: draft.type.usesSSH && !trimmedKey.isEmpty ? trimmedKey : nil,
                sshKeyPassphraseEnc: draft.type.usesSSH ? try vault.encryptOptional(draft.sshKeyPassphrase) : nil,
                isFavorite: draft.isFavorite,
                rdpAdminSession: draft.type == .rdp && draft.rdpAdminSession,
                browserBundleId: draft.type == .web ? draft.browserBundleId : nil,
                mikrotikMode: draft.mikrotikMode,
                sshTerminal: draft.sshTerminal,
                rdpClient: draft.rdpClient,
                color: draft.color,
                notes: draft.notes,
                createdAt: existing?.createdAt ?? .now
            ))
        }
    }

    func duplicate(_ connection: Connection) {
        guard let repo else { return }
        var copy = connection
        copy.id = UUID().uuidString
        copy.name += String(localized: " (copy)")
        copy.createdAt = .now
        perform { try repo.save(copy) }
    }

    func delete(_ connection: Connection) {
        for session in portForwardSessions.values where session.forward.connectionId == connection.id {
            stopPortForward(session.forward)
        }
        perform { try repo?.deleteConnection(id: connection.id) }
    }

    func toggleFavorite(_ connection: Connection) {
        perform { try repo?.setFavorite(id: connection.id, !connection.isFavorite) }
    }

    func move(connectionId: String, to folderId: String?) {
        perform { try repo?.moveConnection(id: connectionId, to: folderId) }
    }

    /// Moves several sidebar rows (connections and folders) at once; a folder never goes into itself.
    func move(tags: [String], to folderId: String?) {
        let items = SidebarDrag.movable(tags)
        perform {
            guard let repo else { return }
            for id in items.connectionIds { try repo.moveConnection(id: id, to: folderId) }
            for id in items.folderIds where id != folderId {
                do { try repo.moveFolder(id: id, to: folderId) }
                catch RepositoryError.folderCycle { showToast(String(localized: "A folder can't be moved into itself")) }
            }
        }
    }

    /// Links (or with `nil` unlinks) a credential to connections; selected folders count with their whole subtree.
    func setCredential(_ credentialId: String?, forTags tags: [String]) {
        setCredential(credentialId, connectionIds: library.connectionIds(forTags: tags))
    }

    func setCredential(_ credentialId: String?, connectionIds: [String]) {
        perform {
            guard let repo else { return }
            let changed = try repo.setCredential(credentialId, connectionIds: connectionIds)
            showToast(credentialId == nil
                ? String(localized: "Linked credential removed — \(changed) connections")
                : String(localized: "Linked credential set — \(changed) connections"))
        }
    }

    /// Several sidebar items waiting for the delete confirmation.
    var pendingBulkDeletion: BulkDeletion?

    struct BulkDeletion: Identifiable {
        let id = UUID()
        let connectionIds: [String]
        let folderIds: [String]
        let credentialIds: [String]

        var count: Int { connectionIds.count + folderIds.count + credentialIds.count }
    }

    func requestDelete(tags: Set<String>) {
        let items = SidebarDrag.items(Array(tags))
        let deletion = BulkDeletion(connectionIds: items.connectionIds, folderIds: items.folderIds,
                                    credentialIds: items.credentialIds)
        if deletion.count > 0 { pendingBulkDeletion = deletion }
    }

    /// Connections in deleted folders (and their subfolders) are kept and move to the root.
    func confirmBulkDelete() {
        guard let deletion = pendingBulkDeletion else { return }
        pendingBulkDeletion = nil
        selectedTags = []
        perform {
            guard let repo else { return }
            for id in deletion.connectionIds { try repo.deleteConnection(id: id) }
            for id in deletion.folderIds { try repo.deleteFolder(id: id) }
            for id in deletion.credentialIds { try repo.deleteCredential(id: id) }
        }
    }

    /// Drag payload of a sidebar row (the whole selection when the row is part of it).
    func dragPayload(for tag: String) -> String {
        SidebarDrag.payload(for: tag, selection: selectedTags)
    }

    func move(folderId: String, to parentId: String?) {
        perform {
            do { try repo?.moveFolder(id: folderId, to: parentId) }
            catch RepositoryError.folderCycle { showToast(String(localized: "A folder can't be moved into itself")) }
        }
    }

    func saveFolder(_ folder: Folder) {
        perform { try repo?.save(folder) }
    }

    func setColor(_ color: LabelColor?, folderId: String) {
        guard var folder = library.folders.first(where: { $0.id == folderId }) else { return }
        folder.color = color
        saveFolder(folder)
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
        catch { errorMessage = String(localized: "Couldn't decrypt: \(error.localizedDescription)") }
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
            var message = String(localized: "Imported — connections: \(summary.imported), credentials: \(summary.credentialsImported), new folders: \(summary.foldersCreated).")
            if summary.withoutPassword > 0 {
                message += "\n" + String(localized: "\(summary.withoutPassword) entries had an encrypted password in the export; enter those by hand.")
            }
            if !summary.skipped.isEmpty {
                let skipped = summary.skipped.prefix(15).joined(separator: ", ")
                message += "\n" + String(localized: "Skipped (\(summary.skipped.count)): \(skipped)")
            }
            errorMessage = message
        }
    }

    // MARK: WinBox import

    /// Picks a WinBox address book with an AppKit open panel; a second SwiftUI `fileImporter`
    /// next to the RDM one did not present reliably on every macOS version.
    func chooseWinboxFile() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "cdb") ?? .data, .data]
        panel.canChooseDirectories = false
        panel.prompt = String(localized: "Open")
        if panel.runModal() == .OK, let url = panel.url {
            loadWinbox(from: url)
        }
    }

    func loadWinbox(from url: URL) {
        let accessing = url.startAccessingSecurityScopedResource()
        defer { if accessing { url.stopAccessingSecurityScopedResource() } }
        do {
            let items = try WinboxImporter.parse(Data(contentsOf: url))
            guard !items.isEmpty else {
                errorMessage = String(localized: "The file has no WinBox addresses.")
                return
            }
            var draft = WinboxImportDraft(rows: items.map { WinboxImportDraft.Row(item: $0) }, targetFolderId: selectedFolderId)
            markDuplicates(&draft)
            winboxImport = draft
        } catch {
            errorMessage = String(localized: "This isn't a WinBox address book (.cdb) or it is damaged.")
        }
    }

    /// Unchecks entries already in their destination folder (target / WinBox group).
    func markDuplicates(_ draft: inout WinboxImportDraft) {
        for index in draft.rows.indices {
            let item = draft.rows[index].item
            var folderId = draft.targetFolderId
            for name in item.groupPath {
                folderId = library.folders.first { $0.parentId == folderId && $0.name == name }?.id
                if folderId == nil { break }
            }
            let exists = (folderId != nil || item.groupPath.isEmpty)
                && WinboxImporter.isDuplicate(item, among: library.connections, folderId: folderId)
            draft.rows[index].exists = exists
            draft.rows[index].include = !exists
        }
    }

    func importWinbox(_ draft: WinboxImportDraft) {
        guard let repo, let vault else { return }
        winboxImport = nil
        let items = draft.rows.filter(\.include).map(\.item)
        perform {
            let result = try ImportWriter.write(connections: items, under: draft.targetFolderId, into: repo, vault: vault)
            showToast(String(localized: "Imported from WinBox — connections: \(result.imported), new folders: \(result.foldersCreated)"))
        }
    }

    // MARK: Launch

    /// `mode` overrides a MikroTik connection's default mode, `terminal` the SSH terminal and
    /// `rdpClient` the RDP client setting; other types ignore them.
    func connect(
        _ connection: Connection,
        mode: MikrotikMode? = nil,
        terminal: SSHTerminal? = nil,
        rdpClient: RDPClient? = nil
    ) {
        guard let vault else { return }
        let connection = library.resolved(connection)
        do {
            let password = try vault.decryptOptional(connection.passwordEnc)
            let outcome: LaunchOutcome
            switch connection.type {
            case .rdp:
                switch rdpClient ?? connection.rdpClient {
                case .embedded:
                    try openRDPSession(for: connection)
                    outcome = .embedded
                case .windowsApp:
                    outcome = try RDPLauncher.launch(connection, password: password) { [weak self] message in
                        self?.showToast(message)
                    }
                }
            case .ssh:
                outcome = try launchSSH(connection, terminal: terminal)
            case .mikrotik:
                switch mode ?? connection.mikrotikMode {
                case .ssh: outcome = try launchSSH(connection, terminal: terminal)
                case .winbox: outcome = try WinboxLauncher.launch(connection, password: password)
                }
            case .web:
                outcome = try WebLauncher.launch(connection, password: password)
            }
            switch outcome {
            case .launched: showToast(String(localized: "Connecting: \(connection.name)"))
            case .embedded: break
            case .passwordCopied(let reason): showToast(String(localized: "Password on the clipboard (\(reason))"))
            case .loginCopied: showToast(String(localized: "Opened: \(connection.name) · password, then username on the clipboard"))
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    /// `connection` is already resolved against its credential.
    private func launchSSH(_ connection: Connection, terminal: SSHTerminal?) throws -> LaunchOutcome {
        switch terminal ?? connection.sshTerminal {
        case .external:
            return try SSHLauncher.launch(try sshScript(for: connection), in: SSHLauncher.currentTerminal)
        case .embedded:
            try openSession(for: connection)
            return .embedded
        }
    }

    /// Launch script with freshly decrypted secrets for a resolved connection.
    private func sshScript(for connection: Connection) throws -> SSHLaunchScript {
        guard let vault else { throw VaultError.notInitialized }
        return SSHLaunchScript(
            connection: connection,
            password: try vault.decryptOptional(connection.passwordEnc),
            keyPassphrase: try vault.decryptOptional(connection.sshKeyPassphraseEnc),
            unlockedKey: try vault.decryptOptional(library.credential(id: connection.credentialId)?.sshKeyUnlockedEnc)
        )
    }

    // MARK: Embedded sessions

    var activeSession: AppSession? {
        sessions.first { $0.id == tabs.active }
    }

    func isConnected(_ connectionId: String) -> Bool {
        sessions.contains { session in
            switch session {
            case .split(let split): split.panes.contains { $0.connectionId == connectionId && $0.isRunning }
            default: session.connectionIds.contains(connectionId) && session.isRunning
            }
        }
    }

    var hasRunningSessions: Bool { runningSessionCount > 0 }

    /// Embedded sessions and port forward tunnels that quitting would close.
    var runningSessionCount: Int {
        sessions.map(\.runningCount).reduce(0, +) + portForwardSessions.values.filter(\.isRunning).count
            + fileBrowsers.values.map(\.transfers.activeCount).reduce(0, +)
    }

    private func openSession(for connection: Connection) throws {
        let session = makeTerminalSession(for: connection)
        session.onExit = { [weak self] session, code in
            if SessionTabs.closesTab(exitCode: code) { self?.closeSession(session.id) }
        }
        try session.start()
        sessions.append(.terminal(session))
        tabs.open(session.id)
    }

    /// Embedded SSH terminal for a resolved connection (not started).
    private func makeTerminalSession(for connection: Connection) -> TerminalSession {
        let snapshot = connection
        let session = TerminalSession(connection: connection) { [weak self] in
            guard let self else { throw VaultError.notInitialized }
            // Re-resolve on every (re)connect so edits to the entry or credential apply.
            let current = self.library.connections.first { $0.id == snapshot.id }.map(self.library.resolved) ?? snapshot
            return try self.sshScript(for: current).write()
        }
        session.onRecordingResult = { [weak self] result in self?.recordingEnded(result) }
        return session
    }

    // MARK: Split view

    /// SSH-capable connections among the selected sidebar rows, in library order.
    var selectedSSHConnections: [Connection] { sshConnections(forTags: selectedTags) }

    func sshConnections(forTags tags: Set<String>) -> [Connection] {
        let ids = Set(tags.compactMap(Self.connectionId(fromTag:)))
        return library.connections.filter { ids.contains($0.id) && $0.type.usesSSH }
    }

    /// `c:<id>` and `fav:<id>` rows are connections.
    static func connectionId(fromTag tag: String) -> String? {
        for prefix in ["c:", "fav:"] where tag.hasPrefix(prefix) { return String(tag.dropFirst(prefix.count)) }
        return nil
    }

    var activeSplit: SplitSession? {
        if case .split(let split) = activeSession { split } else { nil }
    }

    /// Opens 2–4 SSH connections as panes of one split tab (extra ones are left out with a toast).
    @discardableResult
    func openSplit(_ connections: [Connection]) -> SplitSession? {
        guard vault != nil, !connections.isEmpty else { return nil }
        let split = SplitSession()
        for connection in connections.prefix(SplitLayout.maxPanes) {
            do {
                try addPane(to: split, connection: connection)
            } catch {
                errorMessage = error.localizedDescription
            }
        }
        guard !split.panes.isEmpty else { return nil }
        if connections.count > SplitLayout.maxPanes {
            showToast(String(localized: "A split view holds at most \(SplitLayout.maxPanes) terminals."))
        }
        sessions.append(.split(split))
        tabs.open(split.id)
        return split
    }

    // MARK: Saved splits

    /// The name sheet for saving or renaming a saved split.
    struct SavedSplitPrompt: Identifiable {
        enum Source {
            case split(SplitSession)
            case connections([String])
            case rename(SavedSplit)
        }

        let id = UUID()
        var name: String
        let source: Source
    }

    var savedSplitPrompt: SavedSplitPrompt?

    func promptSaveSplit(_ split: SplitSession) {
        savedSplitPrompt = SavedSplitPrompt(name: split.title, source: .split(split))
    }

    func promptSaveSplit(connections: [Connection]) {
        let picked = Array(connections.prefix(SplitLayout.maxPanes))
        savedSplitPrompt = SavedSplitPrompt(name: picked.map(\.name).joined(separator: " + "),
                                            source: .connections(picked.map(\.id)))
    }

    func promptRename(_ saved: SavedSplit) {
        savedSplitPrompt = SavedSplitPrompt(name: saved.name, source: .rename(saved))
    }

    func commitSavedSplitPrompt(name: String) {
        guard let prompt = savedSplitPrompt, let repo else { return }
        savedSplitPrompt = nil
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }
        perform {
            switch prompt.source {
            case .split(let split):
                let saved = try repo.save(SavedSplit(name: trimmed, connectionIds: split.panes.map(\.connectionId)))
                split.savedSplitId = saved.id
                showToast(String(localized: "Saved split: \(trimmed)"))
            case .connections(let ids):
                try repo.save(SavedSplit(name: trimmed, connectionIds: ids))
                showToast(String(localized: "Saved split: \(trimmed)"))
            case .rename(var saved):
                saved.name = trimmed
                try repo.save(saved)
            }
        }
    }

    func savedSplit(for split: SplitSession) -> SavedSplit? {
        split.savedSplitId.flatMap { id in library.savedSplits.first { $0.id == id } }
    }

    /// The tab came from a saved split whose panes differ from the tab's now.
    func canUpdateSavedSplit(_ split: SplitSession) -> Bool {
        guard let saved = savedSplit(for: split) else { return false }
        return saved.connectionIds != split.panes.map(\.connectionId)
    }

    func updateSavedSplit(from split: SplitSession) {
        guard var saved = savedSplit(for: split), let repo else { return }
        saved.connectionIds = split.panes.map(\.connectionId)
        perform { try repo.save(saved) }
        showToast(String(localized: "Saved split updated: \(saved.name)"))
    }

    /// Opens the saved panes that still exist; broadcast always starts off.
    func openSavedSplit(_ saved: SavedSplit) {
        let connections = saved.connections(in: library)
        guard !connections.isEmpty else {
            showToast(String(localized: "None of this split's connections exist any more."))
            return
        }
        openSplit(connections)?.savedSplitId = saved.id
    }

    func deleteSavedSplit(_ saved: SavedSplit) {
        if selection == "s:\(saved.id)" { selection = nil }
        perform { try repo?.deleteSavedSplit(id: saved.id) }
    }

    func moveSavedSplits(from source: IndexSet, to destination: Int) {
        var ids = library.savedSplits.map(\.id)
        ids.move(fromOffsets: source, toOffset: destination)
        perform { try repo?.reorderSavedSplits(ids) }
    }

    /// Adds a pane to an open split (the same connection may appear in several panes).
    func addPane(to split: SplitSession, connectionId: String) {
        guard let connection = library.connections.first(where: { $0.id == connectionId }) else { return }
        do { try addPane(to: split, connection: connection) } catch { errorMessage = error.localizedDescription }
    }

    private func addPane(to split: SplitSession, connection: Connection) throws {
        guard !split.isFull else { return }
        let pane = makeTerminalSession(for: library.resolved(connection))
        pane.onExit = { [weak self, weak split] pane, code in
            guard let split, SessionTabs.closesTab(exitCode: code) else { return }
            self?.closePane(pane.id, in: split)
        }
        try pane.start()
        split.add(pane)
    }

    /// Closes one pane; closing the last one closes the tab.
    func closePane(_ paneId: String, in split: SplitSession) {
        split.remove(paneId: paneId)
        if split.panes.isEmpty { closeSession(split.id) }
    }

    /// Split terminal (⌘D by default): a terminal tab becomes a split (its running session stays) with a second pane on the same server;
    /// in a split, the focused pane's server is opened once more.
    func splitActiveTab() {
        switch activeSession {
        case .terminal(let session):
            guard let index = sessions.firstIndex(where: { $0.id == session.id }) else { return }
            let split = SplitSession()
            session.onExit = { [weak self, weak split] pane, code in
                guard let split, SessionTabs.closesTab(exitCode: code) else { return }
                self?.closePane(pane.id, in: split)
            }
            split.add(session)
            sessions[index] = .split(split)
            tabs.replace(session.id, with: split.id)
            addPane(to: split, connectionId: session.connectionId)
        case .split(let split):
            let pane = split.panes.first { $0.id == split.focusedPaneId } ?? split.panes.last
            if let pane { addPane(to: split, connectionId: pane.connectionId) }
        default:
            break
        }
    }

    var canSplitActiveTab: Bool {
        switch activeSession {
        case .terminal: true
        case .split(let split): !split.isFull
        default: false
        }
    }

    // MARK: Root warning

    /// A running terminal currently is root: per its terminal title, else its login user.
    func isRoot(_ terminal: TerminalSession) -> Bool {
        guard terminal.isRunning else { return false }
        let login = library.connections.first { $0.id == terminal.connectionId }.map { library.resolved($0).username } ?? ""
        return RootDetection.isRoot(loginUser: login, terminalTitle: terminal.terminalTitle)
    }

    func isRoot(_ session: AppSession) -> Bool {
        switch session {
        case .terminal(let terminal): isRoot(terminal)
        case .split(let split): split.panes.contains(where: isRoot)
        case .rdp: false
        }
    }

    // MARK: Tab colors

    func connectionColor(_ connectionId: String) -> LabelColor? {
        library.connections.first { $0.id == connectionId }?.color
    }

    /// The tab's color: chosen on the tab, else the entry's (a split: the color all its panes share).
    func tabColor(for session: AppSession) -> LabelColor? {
        let automatic: LabelColor? = switch session {
        case .split(let split): LabelColor.common(split.panes.map { connectionColor($0.connectionId) })
        default: session.connectionIds.first.flatMap(connectionColor)
        }
        return tabs.color(for: session.id, automatic: automatic)
    }

    func tabTitle(for session: AppSession) -> String {
        tabs.title(for: session.id, automatic: session.title)
    }

    func renameTab(_ sessionId: String, to title: String) {
        tabs.rename(sessionId, to: title)
    }

    func setTabColor(_ color: LabelColor?, for sessionId: String) {
        tabs.setColor(color, for: sessionId)
    }

    func resetTabColor(for sessionId: String) {
        tabs.resetColor(for: sessionId)
    }

    func toggleBroadcastInActiveSplit() {
        activeSplit?.toggleBroadcastAll()
    }

    private func openRDPSession(for connection: Connection) throws {
        let snapshot = connection
        let session = RDPSession(connection: connection) { [weak self] in
            guard let self, let vault = self.vault else { throw VaultError.notInitialized }
            let current = self.library.connections.first { $0.id == snapshot.id }.map(self.library.resolved) ?? snapshot
            let settings = RDPSessionSettings(connection: current)
            let size = RDPSession.initialSize()
            let configuration = RDPConnectionConfiguration()
            configuration.host = settings.host
            configuration.port = UInt16(clamping: settings.port)
            configuration.username = settings.username
            configuration.domain = settings.domain
            configuration.password = try vault.decryptOptional(current.passwordEnc)
            configuration.adminSession = settings.adminSession
            configuration.width = size.width
            configuration.height = size.height
            configuration.dynamicResolution = true
            configuration.fileTransferTitle = String(localized: "Copying files from the remote computer…")
            configuration.cancelTitle = String(localized: "Cancel")
            return configuration
        }
        session.onEnd = { [weak self] session, reason in
            if reason == .clean { self?.closeSession(session.id) }
        }
        session.onMessage = { [weak self] message in self?.showToast(message) }
        session.canSaveCredentials = connection.credentialId == nil
        session.onSaveCredentials = { [weak self] credentials in
            self?.saveRDPCredentials(credentials, connectionId: snapshot.id)
        }
        try session.start()
        sessions.append(.rdp(session))
        tabs.open(session.id)
    }

    /// Stores what was typed into the NLA prompt on the connection itself.
    private func saveRDPCredentials(_ credentials: RDPCredentials, connectionId: String) {
        guard let repo, let vault else { return }
        perform {
            guard var connection = try repo.connection(id: connectionId) else { return }
            connection.username = credentials.username
            connection.domain = credentials.domain
            connection.passwordEnc = try vault.encryptOptional(credentials.password)
            try repo.save(connection)
        }
    }

    func reconnect(_ session: TerminalSession) {
        do { try session.reconnect() } catch { errorMessage = error.localizedDescription }
    }

    func reconnect(_ session: RDPSession) {
        do { try session.reconnect() } catch { errorMessage = error.localizedDescription }
    }

    func closeSession(_ id: String) {
        guard let index = sessions.firstIndex(where: { $0.id == id }) else { return }
        sessions.remove(at: index).close()
        tabs.close(id)
    }

    var activeRDPSession: RDPSession? {
        if case .rdp(let session) = activeSession { session } else { nil }
    }

    /// Types the local clipboard text into the active RDP session (works even when clipboard
    /// redirection does not, e.g. on the logon screen).
    func typeClipboardIntoActiveSession() {
        guard let session = activeRDPSession, session.status == .connected, !session.isTyping else { return }
        guard let text = NSPasteboard.general.string(forType: .string), !text.isEmpty else {
            showToast(String(localized: "There's no text on the clipboard"))
            return
        }
        if text.count > RDPTyping.confirmationThreshold {
            let alert = NSAlert()
            alert.messageText = String(localized: "Type a long text?")
            alert.informativeText = String(localized: "\(text.count) characters, typing takes about \(text.count * 2 / 1000 + 1) s. Continue?")
            alert.addButton(withTitle: String(localized: "Type"))
            alert.addButton(withTitle: String(localized: "Cancel"))
            guard alert.runModal() == .alertFirstButtonReturn else { return }
        }
        session.type(text) { [weak self] finished in
            if !finished { self?.showToast(String(localized: "Typing was interrupted")) }
        }
    }

    func closeActiveSession() {
        if let id = tabs.active { closeSession(id) }
    }

    func selectTab(_ id: String?) {
        tabs.select(id)
    }

    func cycleTabs(forward: Bool) {
        tabs.cycle(forward: forward)
    }

    func terminateAllSessions() {
        for session in sessions { session.close() }
        for browser in fileBrowsers.values { browser.close() }
        stopAllPortForwards()
    }

    // MARK: Port forwards

    struct PendingHostKey: Identifiable {
        /// What to start once the key is trusted.
        enum Continuation {
            case portForward(PortForward)
            case files(FileBrowserTarget)
        }

        let continuation: Continuation
        let target: SSHHostKeys.Target
        let keyLines: [String]
        let fingerprints: [String]
        let id = UUID()
    }

    func portForwardSession(_ forward: PortForward) -> PortForwardSession? {
        portForwardSessions[forward.id]
    }

    func hasActivePortForward(_ connectionId: String) -> Bool {
        portForwardSessions.values.contains { $0.forward.connectionId == connectionId && $0.isRunning }
    }

    /// Saves the forward (optionally starting it); returns why it can't be saved.
    func savePortForward(_ forward: PortForward, start: Bool) -> String? {
        if let error = forward.validationError(among: library.portForwards) { return error.localizedDescription }
        // A running tunnel restarts with the edited settings.
        let wasRunning = portForwardSessions[forward.id]?.isRunning == true
        perform { try repo?.save(forward) }
        if start || wasRunning { startPortForward(forward) }
        return nil
    }

    func deletePortForward(_ forward: PortForward) {
        stopPortForward(forward)
        perform { try repo?.deletePortForward(id: forward.id) }
    }

    /// Checks the gateway's host key first: an unknown one is shown for confirmation instead of starting.
    func startPortForward(_ forward: PortForward) {
        guard let connection = library.connections.first(where: { $0.id == forward.connectionId }) else { return }
        checkHostKey(of: connection, then: .portForward(forward))
    }

    /// Background connections (tunnels, file browser) can't ask about an unknown host key on a terminal, so
    /// it is confirmed here first; known keys continue right away.
    private func checkHostKey(of connection: Connection, then continuation: PendingHostKey.Continuation) {
        let resolved = library.resolved(connection)
        Task {
            do {
                let pending = try await Task.detached { () -> PendingHostKey? in
                    let keys = SSHHostKeys()
                    let target = try keys.target(for: resolved)
                    if try keys.isKnown(target) { return nil }
                    let lines = try keys.scan(target)
                    return PendingHostKey(continuation: continuation, target: target, keyLines: lines,
                                          fingerprints: try keys.fingerprints(of: lines))
                }.value
                if let pending { pendingHostKey = pending } else { proceed(continuation) }
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    private func proceed(_ continuation: PendingHostKey.Continuation) {
        switch continuation {
        case .portForward(let forward): launchPortForward(forward)
        case .files(let target): filesWindowRequest = target
        }
    }

    func acceptPendingHostKey() {
        guard let pending = pendingHostKey else { return }
        pendingHostKey = nil
        do {
            try SSHHostKeys().trust(pending.keyLines, for: pending.target)
            proceed(pending.continuation)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    // MARK: File browser

    /// A file browser window to open; the main window's `openWindow` picks it up.
    var filesWindowRequest: FileBrowserTarget?
    /// Open file browser windows (their transfers count as running sessions on quit).
    private(set) var fileBrowsers: [String: FileBrowserSession] = [:]

    /// Opens the file browser for an SSH connection, in `startPath` (else the home folder).
    func openFiles(_ connection: Connection, startPath: String? = nil) {
        guard connection.type.usesSSH else { return }
        checkHostKey(of: connection, then: .files(FileBrowserTarget(connectionId: connection.id, startPath: startPath)))
    }

    /// The active tab's terminal: the terminal tab itself, or the focused pane of a split.
    var activeTerminal: TerminalSession? {
        switch activeSession {
        case .terminal(let session): session
        case .split(let split): split.panes.first { $0.id == split.focusedPaneId } ?? split.panes.first
        default: nil
        }
    }

    func clearActiveTerminal() {
        activeTerminal?.clear()
    }

    // MARK: Copy next command output

    var canRecordOutput: Bool { activeTerminal?.isRunning == true }

    /// Arms the active terminal's recorder; while armed or recording it stops and copies instead.
    func toggleRecordingInActiveTerminal() {
        activeTerminal?.toggleRecording()
    }

    private func recordingEnded(_ result: TerminalSession.RecordingResult) {
        switch result {
        case .copied(let text):
            Clipboard.copy(text, clearAfter: nil, concealed: false)
            let lines = text.split(separator: "\n", omittingEmptySubsequences: false).count
            let size = ByteCountFormatter.string(fromByteCount: Int64(text.utf8.count), countStyle: .file)
            showToast(String(localized: "Copied \(lines) lines (\(size))"))
        case .empty:
            showToast(String(localized: "The command printed nothing"))
        case .alternateScreen:
            showToast(String(localized: "Interactive program – recording stopped. Try cat or --no-pager."))
        case .tooLarge:
            showToast(String(localized: "Output larger than 10 MB – recording stopped"))
        }
    }

    /// Files for the active tab's terminal (its focused pane in a split), starting in the shell's directory.
    func openFilesForActiveTab() {
        if let terminal = activeTerminal { openFiles(for: terminal) } else if let connection = selectedSSHConnections.first, selectedTags.count == 1 {
            openFiles(connection)
        }
    }

    func openFiles(for terminal: TerminalSession) {
        guard let connection = library.connections.first(where: { $0.id == terminal.connectionId }) else { return }
        openFiles(connection, startPath: terminal.terminalTitle.flatMap(RemotePath.directory(fromTerminalTitle:)))
    }

    var canOpenFiles: Bool {
        switch activeSession {
        case .terminal, .split: true
        case .rdp: false
        case nil: selectedTags.count == 1 && !selectedSSHConnections.isEmpty
        }
    }

    func makeFileBrowser(_ target: FileBrowserTarget) -> FileBrowserSession? {
        guard let connection = library.connections.first(where: { $0.id == target.connectionId }) else { return nil }
        let resolved = library.resolved(connection)
        let session = FileBrowserSession(target: target, connection: resolved) { [weak self] controlPath in
            guard let self else { throw VaultError.notInitialized }
            var script = try self.sshScript(for: resolved)
            script.controlPath = controlPath
            return try script.write()
        }
        fileBrowsers[session.id] = session
        return session
    }

    func closeFileBrowser(_ session: FileBrowserSession) {
        session.close()
        fileBrowsers.removeValue(forKey: session.id)
    }

    func stopPortForward(_ forward: PortForward) {
        portForwardSessions.removeValue(forKey: forward.id)?.stop()
    }

    func stopAllPortForwards() {
        for session in portForwardSessions.values { session.stop() }
        portForwardSessions.removeAll()
    }

    private func launchPortForward(_ forward: PortForward) {
        if let previous = portForwardSessions.removeValue(forKey: forward.id), previous.isRunning {
            // Give the old ssh a moment to release the local port.
            previous.stop()
            Task {
                try? await Task.sleep(for: .milliseconds(500))
                launchPortForward(forward)
            }
            return
        }
        let session = PortForwardSession(forward: forward) { [weak self] in
            guard let self, let connection = self.library.connections.first(where: { $0.id == forward.connectionId })
            else { throw VaultError.notInitialized }
            var script = try self.sshScript(for: self.library.resolved(connection))
            script.forward = forward
            return try script.write()
        }
        portForwardSessions[forward.id] = session
        do {
            try session.start()
        } catch {
            portForwardSessions[forward.id] = nil
            errorMessage = String(localized: "Can't start the port forward: \(error.localizedDescription)")
        }
    }

    func copyPassword(_ connection: Connection) {
        guard let vault else { return }
        let connection = library.resolved(connection)
        perform {
            guard let password = try vault.decryptOptional(connection.passwordEnc) else {
                showToast(String(localized: "No saved password"))
                return
            }
            Clipboard.copy(password)
            showToast(String(localized: "Password on the clipboard (30 s)"))
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
