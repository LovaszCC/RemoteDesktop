//! The stored records. Field names follow the Swift models; secrets stay [`Sealed`].

use super::{
    ConnectionType, LabelColor, MikrotikMode, RdpClient, Sealed, SshTerminal, Timestamp, VpnTestMode, new_id,
};

/// A sidebar folder; folders nest through `parent_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// Uppercase UUID.
    pub id: String,
    /// Parent folder, `None` at the root.
    pub parent_id: Option<String>,
    /// Display name.
    pub name: String,
    /// Position among its siblings (ties sort by name).
    pub sort_order: i64,
    /// Icon color in the sidebar.
    pub color: Option<LabelColor>,
}

impl Folder {
    /// A new root folder with a fresh id.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            parent_id: None,
            name: name.into(),
            sort_order: 0,
            color: None,
        }
    }
}

/// A sidebar entry: one machine with its shared fields, owning one or more [`Connection`]s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Uppercase UUID; an upgraded entry has the id of its only connection.
    pub id: String,
    /// Containing folder, `None` at the root.
    pub folder_id: Option<String>,
    /// Display name.
    pub name: String,
    /// Host name or address, shared by every connection.
    pub host: String,
    /// Free-form notes.
    pub notes: String,
    /// Tag color, also the default color of the entry's tabs.
    pub color: Option<LabelColor>,
    /// Shown under Favorites.
    pub is_favorite: bool,
    /// The connection double-click opens; `None` (or a stale id) means the first one.
    pub default_connection_id: Option<String>,
    /// The MacVPN profile connects to this entry go through when it isn't reachable directly.
    pub vpn: Option<EntryVpn>,
    /// Creation time.
    pub created_at: Timestamp,
    /// Last change; the repository sets it on save.
    pub updated_at: Timestamp,
}

/// An entry's VPN setting: which MacVPN connection to bring up, and how to test for it first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryVpn {
    /// The MacVPN connection id, exactly as its control API lists it (case-sensitive).
    pub connection_id: String,
    /// What the gate pings.
    pub test_mode: VpnTestMode,
    /// The address pinged in [`VpnTestMode::Address`] (kept in the other mode, so switching back keeps it).
    pub test_address: String,
}

impl Entry {
    /// A new root entry with a fresh id and no default connection.
    #[must_use]
    pub fn new(name: impl Into<String>, host: impl Into<String>) -> Self {
        let now = Timestamp::now();
        Self {
            id: new_id(),
            folder_id: None,
            name: name.into(),
            host: host.into(),
            notes: String::new(),
            color: None,
            is_favorite: false,
            default_connection_id: None,
            vpn: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// The entry a lone connection stands for: the connection's id and shared fields, with it as the
    /// default.
    #[must_use]
    pub fn for_connection(connection: &Connection) -> Self {
        Self {
            id: connection.entry_id.clone(),
            folder_id: connection.folder_id.clone(),
            name: connection.name.clone(),
            host: connection.host.clone(),
            notes: connection.notes.clone(),
            color: connection.color,
            is_favorite: connection.is_favorite,
            default_connection_id: Some(connection.id.clone()),
            vpn: None,
            created_at: connection.created_at,
            updated_at: connection.updated_at,
        }
    }

    /// Copies the shared fields (name, host, folder, notes, color, favorite) onto one of the entry's
    /// connections.
    pub fn overlay(&self, connection: &mut Connection) {
        connection.folder_id.clone_from(&self.folder_id);
        connection.name.clone_from(&self.name);
        connection.host.clone_from(&self.host);
        connection.notes.clone_from(&self.notes);
        connection.color = self.color;
        connection.is_favorite = self.is_favorite;
    }
}

/// One saved connection. The auth fields may be overridden by a linked [`Credential`].
///
/// `folder_id`, `name`, `host`, `notes`, `color` and `is_favorite` mirror the owning [`Entry`]: the store
/// overlays them from the entry on load and keeps the stored columns equal to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// Uppercase UUID.
    pub id: String,
    /// Owning entry.
    pub entry_id: String,
    /// Containing folder, `None` at the root.
    pub folder_id: Option<String>,
    /// Linked credential that overrides the auth fields.
    pub credential_id: Option<String>,
    /// Kind of connection.
    pub kind: ConnectionType,
    /// Display name.
    pub name: String,
    /// Host name or address; for web connections the URL.
    pub host: String,
    /// Port; `None` means the type's default.
    pub port: Option<u16>,
    /// Login user.
    pub username: String,
    /// Windows domain (RDP).
    pub domain: String,
    /// Sealed password.
    pub password_enc: Option<Sealed>,
    /// Path to an SSH private key.
    pub ssh_key_path: Option<String>,
    /// Sealed SSH key passphrase.
    pub ssh_key_passphrase_enc: Option<Sealed>,
    /// Shown under Favorites.
    pub is_favorite: bool,
    /// RDP only: connect to the administrative session.
    pub rdp_admin_session: bool,
    /// Web only: browser id; `None` means the system default.
    pub browser_bundle_id: Option<String>,
    /// MikroTik only: what double-click opens.
    pub mikrotik_mode: MikrotikMode,
    /// SSH and MikroTik SSH: embedded tab or external terminal.
    pub ssh_terminal: SshTerminal,
    /// RDP: embedded tab or the system client.
    pub rdp_client: RdpClient,
    /// Tag color, also the default color of the connection's tabs.
    pub color: Option<LabelColor>,
    /// Free-form notes.
    pub notes: String,
    /// Optional label among the entry's connections; empty means "<TYPE> · <username>".
    pub label: String,
    /// Gets its own button on the entry's Details page.
    pub show_as_button: bool,
    /// Position among the entry's connections (ties sort by creation time).
    pub sort_order: i64,
    /// Web only: URL scheme, free text (`https`, `http`, `smb`, …).
    pub web_scheme: String,
    /// Web only: path after the host and port, empty for none.
    pub web_path: String,
    /// Creation time.
    pub created_at: Timestamp,
    /// Last change; the repository sets it on save.
    pub updated_at: Timestamp,
}

impl Connection {
    /// A new connection with a fresh id and the Swift defaults. It is its own entry (`entry_id == id`)
    /// until the caller points it at another one.
    #[must_use]
    pub fn new(kind: ConnectionType, name: impl Into<String>, host: impl Into<String>) -> Self {
        let now = Timestamp::now();
        let id = new_id();
        Self {
            entry_id: id.clone(),
            id,
            folder_id: None,
            credential_id: None,
            kind,
            name: name.into(),
            host: host.into(),
            port: None,
            username: String::new(),
            domain: String::new(),
            password_enc: None,
            ssh_key_path: None,
            ssh_key_passphrase_enc: None,
            is_favorite: false,
            rdp_admin_session: false,
            browser_bundle_id: None,
            mikrotik_mode: MikrotikMode::Winbox,
            ssh_terminal: SshTerminal::Embedded,
            rdp_client: RdpClient::Embedded,
            color: None,
            notes: String::new(),
            label: String::new(),
            show_as_button: false,
            sort_order: 0,
            web_scheme: "https".to_owned(),
            web_path: String::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// A copy for "Duplicate": a fresh id that is also its own entry, `copy_suffix` (the localised
    /// " (copy)") appended to the name, created and updated now. Everything else, the folder, credential,
    /// favorite and secrets included, is copied.
    #[must_use]
    pub fn duplicated(&self, copy_suffix: &str) -> Self {
        let now = Timestamp::now();
        let id = new_id();
        Self {
            entry_id: id.clone(),
            id,
            name: format!("{}{copy_suffix}", self.name),
            created_at: now,
            updated_at: now,
            ..self.clone()
        }
    }

    /// The port to connect to: the stored one or the type's default.
    #[must_use]
    pub fn effective_port(&self) -> u16 {
        self.port.unwrap_or_else(|| self.kind.default_port())
    }

    /// The connection with a linked credential's auth fields applied (Swift `Connection.resolved(with:)`).
    ///
    /// A credential with a username brings its username *and* domain, even an empty domain; one without a
    /// username keeps the connection's own. Password, key path and passphrase always come from the
    /// credential, even when it has none.
    #[must_use]
    pub fn resolved(&self, credential: Option<&Credential>) -> Self {
        let Some(credential) = credential else {
            return self.clone();
        };
        let mut resolved = self.clone();
        if !credential.username.is_empty() {
            resolved.username.clone_from(&credential.username);
            resolved.domain.clone_from(&credential.domain);
        }
        resolved.password_enc.clone_from(&credential.password_enc);
        resolved.ssh_key_path.clone_from(&credential.ssh_key_path);
        resolved
            .ssh_key_passphrase_enc
            .clone_from(&credential.ssh_key_passphrase_enc);
        resolved
    }
}

/// A reusable login that connections can link to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    /// Uppercase UUID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Login user; empty means linked connections keep their own user and domain.
    pub username: String,
    /// Windows domain.
    pub domain: String,
    /// Sealed password.
    pub password_enc: Option<Sealed>,
    /// Path to an SSH private key.
    pub ssh_key_path: Option<String>,
    /// Sealed SSH key passphrase.
    pub ssh_key_passphrase_enc: Option<Sealed>,
    /// The private key with its passphrase removed, sealed. Only for passphrase-protected keys.
    pub ssh_key_unlocked_enc: Option<Sealed>,
    /// Free-form notes.
    pub notes: String,
    /// Creation time.
    pub created_at: Timestamp,
    /// Last change; the repository sets it on save.
    pub updated_at: Timestamp,
}

impl Credential {
    /// A new, empty credential with a fresh id.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let now = Timestamp::now();
        Self {
            id: new_id(),
            name: name.into(),
            username: String::new(),
            domain: String::new(),
            password_enc: None,
            ssh_key_path: None,
            ssh_key_passphrase_enc: None,
            ssh_key_unlocked_enc: None,
            notes: String::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// `DOMAIN\user` style label for lists; empty without a username.
    #[must_use]
    pub fn display_user(&self) -> String {
        match (self.username.is_empty(), self.domain.is_empty()) {
            (true, _) => String::new(),
            (false, true) => self.username.clone(),
            (false, false) => format!("{}\\{}", self.domain, self.username),
        }
    }
}

/// An SSH local port forward that belongs to one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortForward {
    /// Uppercase UUID.
    pub id: String,
    /// Owning connection.
    pub connection_id: String,
    /// Optional label.
    pub name: String,
    /// Port on 127.0.0.1.
    pub local_port: u16,
    /// Destination host as seen from the SSH server.
    pub remote_host: String,
    /// Destination port.
    pub remote_port: u16,
    /// Creation time.
    pub created_at: Timestamp,
}

/// A saved set of up to 4 SSH connections opened together as a split view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSplit {
    /// Uppercase UUID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Connections in pane order; duplicates are allowed.
    pub connection_ids: Vec<String>,
    /// Position in the sidebar.
    pub sort_order: i64,
    /// Creation time.
    pub created_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sealed(byte: u8) -> Sealed {
        Sealed::from_bytes(vec![byte])
    }

    fn rdp() -> Connection {
        let mut connection = Connection::new(ConnectionType::Rdp, "dc", "dc.example.com");
        connection.username = "own".into();
        connection.domain = "OWN".into();
        connection.password_enc = Some(sealed(1));
        connection.ssh_key_path = Some("~/.ssh/own".into());
        connection.ssh_key_passphrase_enc = Some(sealed(2));
        connection
    }

    #[test]
    fn effective_port_falls_back_to_type_default() {
        let mut connection = Connection::new(ConnectionType::Ssh, "x", "h");
        assert_eq!(connection.effective_port(), 22);
        connection.port = Some(2222);
        assert_eq!(connection.effective_port(), 2222);
    }

    #[test]
    fn no_credential_leaves_connection_unchanged() {
        let connection = rdp();
        assert_eq!(connection.resolved(None), connection);
    }

    #[test]
    fn credential_without_username_keeps_own_user_and_domain() {
        let mut credential = Credential::new("key only");
        credential.password_enc = Some(sealed(9));
        let resolved = rdp().resolved(Some(&credential));
        assert_eq!(
            (resolved.username.as_str(), resolved.domain.as_str()),
            ("own", "OWN")
        );
        assert_eq!(resolved.password_enc, Some(sealed(9)));
    }

    #[test]
    fn credential_username_brings_its_domain_even_when_empty() {
        let mut credential = Credential::new("admin");
        credential.username = "bob".into();
        let resolved = rdp().resolved(Some(&credential));
        assert_eq!(
            (resolved.username.as_str(), resolved.domain.as_str()),
            ("bob", "")
        );
    }

    #[test]
    fn resolved_replaces_all_auth_fields_even_with_none() {
        let credential = Credential::new("empty");
        let resolved = rdp().resolved(Some(&credential));
        assert_eq!(resolved.password_enc, None);
        assert_eq!(resolved.ssh_key_path, None);
        assert_eq!(resolved.ssh_key_passphrase_enc, None);
        assert_eq!(resolved.name, "dc");
    }

    #[test]
    fn duplicate_copies_everything_but_id_name_and_dates() {
        let mut original = rdp();
        original.created_at = Timestamp::from_unix(0);
        original.updated_at = Timestamp::from_unix(0);
        original.is_favorite = true;
        original.credential_id = Some("K".into());
        let copy = original.duplicated(" (copy)");
        assert_ne!(copy.id, original.id);
        assert_eq!(copy.entry_id, copy.id);
        assert_eq!(copy.name, "dc (copy)");
        assert!(copy.created_at > original.created_at);
        assert_eq!(copy.updated_at, copy.created_at);
        let restored = Connection {
            id: original.id.clone(),
            entry_id: original.entry_id.clone(),
            name: original.name.clone(),
            created_at: original.created_at,
            updated_at: original.updated_at,
            ..copy
        };
        assert_eq!(restored, original);
    }

    #[test]
    fn new_connection_is_its_own_entry() {
        let connection = Connection::new(ConnectionType::Web, "w", "w.example.com");
        assert_eq!(connection.entry_id, connection.id);
        assert!(!connection.show_as_button);
        assert_eq!(
            (
                connection.label.as_str(),
                connection.sort_order,
                connection.web_scheme.as_str()
            ),
            ("", 0, "https")
        );
        assert_eq!(connection.web_path, "");
    }

    #[test]
    fn entry_for_a_connection_and_overlay() {
        let mut connection = rdp();
        connection.folder_id = Some("F".into());
        connection.notes = "n".into();
        connection.color = Some(LabelColor::Red);
        connection.is_favorite = true;
        let entry = Entry::for_connection(&connection);
        assert_eq!(entry.id, connection.entry_id);
        assert_eq!(
            entry.default_connection_id.as_deref(),
            Some(connection.id.as_str())
        );
        assert_eq!(
            (entry.name.as_str(), entry.host.as_str()),
            ("dc", "dc.example.com")
        );
        assert_eq!(entry.created_at, connection.created_at);

        let mut other = Connection::new(ConnectionType::Ssh, "x", "y");
        entry.overlay(&mut other);
        assert_eq!(
            (other.name.as_str(), other.host.as_str(), other.notes.as_str()),
            ("dc", "dc.example.com", "n")
        );
        assert_eq!(other.folder_id.as_deref(), Some("F"));
        assert_eq!(other.color, Some(LabelColor::Red));
        assert!(other.is_favorite);
        assert_eq!(other.username, "");

        let fresh = Entry::new("e", "h");
        assert_eq!(fresh.default_connection_id, None);
        assert_ne!(fresh.id, Entry::new("e", "h").id);
    }

    #[test]
    fn display_user() {
        let mut credential = Credential::new("c");
        assert_eq!(credential.display_user(), "");
        credential.username = "bob".into();
        assert_eq!(credential.display_user(), "bob");
        credential.domain = "CORP".into();
        assert_eq!(credential.display_user(), "CORP\\bob");
    }
}
