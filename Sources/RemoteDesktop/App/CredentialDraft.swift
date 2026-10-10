import Foundation
import RemoteDesktopCore

/// Editable, decrypted copy of a credential used by the credential editor sheet.
struct CredentialDraft: Identifiable, Equatable {
    var id: String
    var isNew: Bool
    var name = ""
    var username = ""
    var domain = ""
    var password = ""
    var sshKeyPath = ""
    var sshKeyPassphrase = ""
    var notes = ""

    init() {
        id = UUID().uuidString
        isNew = true
    }

    init(credential: Credential, vault: Vault) throws {
        id = credential.id
        isNew = false
        name = credential.name
        username = credential.username
        domain = credential.domain
        password = try vault.decryptOptional(credential.passwordEnc) ?? ""
        sshKeyPath = credential.sshKeyPath ?? ""
        sshKeyPassphrase = try vault.decryptOptional(credential.sshKeyPassphraseEnc) ?? ""
        notes = credential.notes
    }

    var isValid: Bool {
        !name.trimmingCharacters(in: .whitespaces).isEmpty
    }
}
