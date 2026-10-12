import AppKit
import GRDB
import Observation
import RDPBridge
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
    var rdpClient: RDPClient = .embedded
    var browserBundleId: String?
    var mikrotikMode: MikrotikMode = .winbox
    var sshTerminal: SSHTerminal = .embedded

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
    /// Tag of a just-saved item to select once the observed library contains it.
    private var pendingSelection: String?
    var searchText = ""
    var editorDraft: ConnectionDraft?
    var folderEditor: Folder?
    var credentialEditor: CredentialDraft?
    var pendingCredentialDeletion: Credential?
    var isImporting = false

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

    func move(folderId: String, to parentId: String?) {
        perform {
            do { try repo?.moveFolder(id: folderId, to: parentId) }
            catch RepositoryError.folderCycle { showToast(String(localized: "A folder can't be moved into itself")) }
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
        case .ghostty:
            return try SSHLauncher.launch(try sshScript(for: connection))
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
        sessions.contains { $0.connectionId == connectionId && $0.isRunning }
    }

    var hasRunningSessions: Bool { runningSessionCount > 0 }

    /// Embedded sessions and port forward tunnels that quitting would close.
    var runningSessionCount: Int {
        sessions.filter(\.isRunning).count + portForwardSessions.values.filter(\.isRunning).count
    }

    private func openSession(for connection: Connection) throws {
        let snapshot = connection
        let session = TerminalSession(connection: connection) { [weak self] in
            guard let self else { throw VaultError.notInitialized }
            // Re-resolve on every (re)connect so edits to the entry or credential apply.
            let current = self.library.connections.first { $0.id == snapshot.id }.map(self.library.resolved) ?? snapshot
            return try self.sshScript(for: current).write()
        }
        session.onExit = { [weak self] session, code in
            if SessionTabs.closesTab(exitCode: code) { self?.closeSession(session.id) }
        }
        try session.start()
        sessions.append(.terminal(session))
        tabs.open(session.id)
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
            return configuration
        }
        session.onEnd = { [weak self] session, reason in
            if reason == .clean { self?.closeSession(session.id) }
        }
        try session.start()
        sessions.append(.rdp(session))
        tabs.open(session.id)
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
        stopAllPortForwards()
    }

    // MARK: Port forwards

    struct PendingHostKey: Identifiable {
        let forward: PortForward
        let target: SSHHostKeys.Target
        let keyLines: [String]
        let fingerprints: [String]
        var id: String { forward.id }
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
        perform { try repo?.save(forward) }
        if start { startPortForward(forward) }
        return nil
    }

    func deletePortForward(_ forward: PortForward) {
        stopPortForward(forward)
        perform { try repo?.deletePortForward(id: forward.id) }
    }

    /// Checks the gateway's host key first: an unknown one is shown for confirmation instead of starting.
    func startPortForward(_ forward: PortForward) {
        guard let connection = library.connections.first(where: { $0.id == forward.connectionId }) else { return }
        let resolved = library.resolved(connection)
        Task {
            do {
                let pending = try await Task.detached { () -> PendingHostKey? in
                    let keys = SSHHostKeys()
                    let target = try keys.target(for: resolved)
                    if try keys.isKnown(target) { return nil }
                    let lines = try keys.scan(target)
                    return PendingHostKey(forward: forward, target: target, keyLines: lines,
                                          fingerprints: try keys.fingerprints(of: lines))
                }.value
                if let pending { pendingHostKey = pending } else { launchPortForward(forward) }
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    func acceptPendingHostKey() {
        guard let pending = pendingHostKey else { return }
        pendingHostKey = nil
        do {
            try SSHHostKeys().trust(pending.keyLines, for: pending.target)
            launchPortForward(pending.forward)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func stopPortForward(_ forward: PortForward) {
        portForwardSessions.removeValue(forKey: forward.id)?.stop()
    }

    func stopAllPortForwards() {
        for session in portForwardSessions.values { session.stop() }
        portForwardSessions.removeAll()
    }

    private func launchPortForward(_ forward: PortForward) {
        portForwardSessions[forward.id]?.stop()
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
