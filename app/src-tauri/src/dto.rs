//! What the webview receives. Every DTO is secret-free: sealed bytes and plaintexts never leave Rust, only
//! `has…` flags do. Field names are camelCase, enums are their raw strings.

use rd_core::draft::{EntryVpnDraft, web_url};
use rd_core::import::{ImportKind, Skipped};
use rd_core::shortcuts::{KeyShortcut, Platform, ShortcutCommand, ShortcutSettings};
use rd_core::tree::{self, TreeNode};
use rd_core::{
    Connection, ConnectionType, Credential, Entry, Folder, LabelColor, Library, MikrotikMode, PortForward,
    RdpClient, SavedSplit, SshTerminal, Timestamp,
};
use rd_store::LockHolder;
use serde::Serialize;

/// The login a connection uses once its linked credential is applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLoginDto {
    /// User name (the credential's when it has one).
    pub username: String,
    /// Windows domain.
    pub domain: String,
    /// A password is stored.
    pub has_password: bool,
    /// SSH key path.
    pub ssh_key_path: Option<String>,
    /// A key passphrase is stored.
    pub has_passphrase: bool,
}

/// A connection without its secrets.
#[allow(
    clippy::struct_excessive_bools,
    reason = "mirrors the record's flags for the UI"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDto {
    /// Record id.
    pub id: String,
    /// Owning entry.
    pub entry_id: String,
    /// Optional label among the entry's connections; empty when unset.
    pub label: String,
    /// The label, else `<TYPE> · <username>`, else `<TYPE>`.
    pub display_label: String,
    /// Gets its own button on the entry's Details page.
    pub show_as_button: bool,
    /// Position among the entry's connections.
    pub sort_order: i64,
    /// Web: the URL scheme.
    pub web_scheme: String,
    /// Web: the path after host and port; empty for none.
    pub web_path: String,
    /// Containing folder.
    pub folder_id: Option<String>,
    /// Linked credential.
    pub credential_id: Option<String>,
    /// `rdp`, `ssh`, `web` or `mikrotik`.
    #[serde(rename = "type")]
    pub kind: ConnectionType,
    /// Display name.
    pub name: String,
    /// Host (the entry's).
    pub host: String,
    /// Stored port; `None` means the type's default.
    pub port: Option<u16>,
    /// The port actually used.
    pub effective_port: u16,
    /// Web: the URL that opens; `None` for other types or an invalid URL.
    pub web_url: Option<String>,
    /// Own user name.
    pub username: String,
    /// Own Windows domain.
    pub domain: String,
    /// An own password is stored.
    pub has_password: bool,
    /// Own SSH key path.
    pub ssh_key_path: Option<String>,
    /// An own key passphrase is stored.
    pub has_passphrase: bool,
    /// The login after applying the linked credential.
    pub resolved: ResolvedLoginDto,
    /// Shown under Favorites.
    pub is_favorite: bool,
    /// RDP: administrative session.
    pub rdp_admin_session: bool,
    /// Web: browser id.
    pub browser_bundle_id: Option<String>,
    /// MikroTik: default mode.
    pub mikrotik_mode: MikrotikMode,
    /// SSH: embedded or external terminal.
    pub ssh_terminal: SshTerminal,
    /// RDP: embedded or the system client.
    pub rdp_client: RdpClient,
    /// Tag color.
    pub color: Option<LabelColor>,
    /// Notes.
    pub notes: String,
    /// `Parent / Child`; empty at the root.
    pub folder_path: String,
    /// Creation time.
    pub created_at: Timestamp,
    /// Last change.
    pub updated_at: Timestamp,
}

impl ConnectionDto {
    /// The DTO for one connection of `library`.
    #[must_use]
    pub fn new(connection: &Connection, library: &Library) -> Self {
        let resolved = library.resolved(connection);
        let label = connection.label.trim();
        let display_label = if !label.is_empty() {
            label.to_owned()
        } else if resolved.username.is_empty() {
            connection.kind.label().to_owned()
        } else {
            format!("{} · {}", connection.kind.label(), resolved.username)
        };
        Self {
            id: connection.id.clone(),
            entry_id: connection.entry_id.clone(),
            label: connection.label.clone(),
            display_label,
            show_as_button: connection.show_as_button,
            sort_order: connection.sort_order,
            web_scheme: connection.web_scheme.clone(),
            web_path: connection.web_path.clone(),
            folder_id: connection.folder_id.clone(),
            credential_id: connection.credential_id.clone(),
            kind: connection.kind,
            name: connection.name.clone(),
            host: connection.host.clone(),
            port: connection.port,
            effective_port: connection.effective_port(),
            web_url: (connection.kind == ConnectionType::Web)
                .then(|| {
                    web_url(
                        &connection.web_scheme,
                        &connection.host,
                        connection.port,
                        &connection.web_path,
                    )
                })
                .flatten(),
            username: connection.username.clone(),
            domain: connection.domain.clone(),
            has_password: connection.password_enc.is_some(),
            ssh_key_path: connection.ssh_key_path.clone(),
            has_passphrase: connection.ssh_key_passphrase_enc.is_some(),
            resolved: ResolvedLoginDto {
                username: resolved.username,
                domain: resolved.domain,
                has_password: resolved.password_enc.is_some(),
                ssh_key_path: resolved.ssh_key_path,
                has_passphrase: resolved.ssh_key_passphrase_enc.is_some(),
            },
            is_favorite: connection.is_favorite,
            rdp_admin_session: connection.rdp_admin_session,
            browser_bundle_id: connection.browser_bundle_id.clone(),
            mikrotik_mode: connection.mikrotik_mode,
            ssh_terminal: connection.ssh_terminal,
            rdp_client: connection.rdp_client,
            color: connection.color,
            notes: connection.notes.clone(),
            folder_path: tree::folder_path(library, connection.folder_id.as_deref()),
            created_at: connection.created_at,
            updated_at: connection.updated_at,
        }
    }
}

/// A sidebar entry: the shared fields and its connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDto {
    /// Record id.
    pub id: String,
    /// Containing folder.
    pub folder_id: Option<String>,
    /// `Parent / Child`; empty at the root.
    pub folder_path: String,
    /// Display name.
    pub name: String,
    /// Host shared by every connection.
    pub host: String,
    /// Notes.
    pub notes: String,
    /// Tag color.
    pub color: Option<LabelColor>,
    /// Shown under Favorites.
    pub is_favorite: bool,
    /// The connection double-click opens (a stale stored one already replaced by the first).
    pub default_connection_id: Option<String>,
    /// Its connections in order.
    pub connection_ids: Vec<String>,
    /// The MacVPN profile and its test; `null` for no VPN.
    pub vpn: Option<EntryVpnDraft>,
}

impl EntryDto {
    /// The DTO for one entry of `library`.
    #[must_use]
    pub fn new(entry: &Entry, library: &Library) -> Self {
        Self {
            id: entry.id.clone(),
            folder_id: entry.folder_id.clone(),
            folder_path: tree::folder_path(library, entry.folder_id.as_deref()),
            name: entry.name.clone(),
            host: entry.host.clone(),
            notes: entry.notes.clone(),
            color: entry.color,
            is_favorite: entry.is_favorite,
            default_connection_id: library.default_connection(&entry.id).map(|c| c.id.clone()),
            connection_ids: library
                .connections_of(&entry.id)
                .iter()
                .map(|c| c.id.clone())
                .collect(),
            vpn: entry.vpn.as_ref().map(|v| EntryVpnDraft {
                connection_id: v.connection_id.clone(),
                test_mode: v.test_mode,
                test_address: v.test_address.clone(),
            }),
        }
    }
}

/// A credential without its secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialDto {
    /// Record id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// User name.
    pub username: String,
    /// Windows domain.
    pub domain: String,
    /// `DOMAIN\user`, `user` or empty.
    pub display_user: String,
    /// A password is stored.
    pub has_password: bool,
    /// SSH key path.
    pub ssh_key_path: Option<String>,
    /// A key passphrase is stored.
    pub has_passphrase: bool,
    /// A passphrase-free copy of the key is stored.
    pub has_unlocked_key: bool,
    /// Notes.
    pub notes: String,
    /// How many connections link to it.
    pub usage_count: usize,
    /// Creation time.
    pub created_at: Timestamp,
    /// Last change.
    pub updated_at: Timestamp,
}

impl CredentialDto {
    /// The DTO for one credential of `library`.
    #[must_use]
    pub fn new(credential: &Credential, library: &Library) -> Self {
        Self {
            id: credential.id.clone(),
            name: credential.name.clone(),
            username: credential.username.clone(),
            domain: credential.domain.clone(),
            display_user: credential.display_user(),
            has_password: credential.password_enc.is_some(),
            ssh_key_path: credential.ssh_key_path.clone(),
            has_passphrase: credential.ssh_key_passphrase_enc.is_some(),
            has_unlocked_key: credential.ssh_key_unlocked_enc.is_some(),
            notes: credential.notes.clone(),
            usage_count: library.connections_using(&credential.id).count(),
            created_at: credential.created_at,
            updated_at: credential.updated_at,
        }
    }
}

/// A folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderDto {
    /// Record id.
    pub id: String,
    /// Parent folder; `None` at the root.
    pub parent_id: Option<String>,
    /// Display name.
    pub name: String,
    /// Sort position among siblings.
    pub sort_order: i64,
    /// Tag color.
    pub color: Option<LabelColor>,
    /// `Parent / Child` including itself.
    pub path: String,
}

impl FolderDto {
    /// The DTO for one folder of `library`.
    #[must_use]
    pub fn new(folder: &Folder, library: &Library) -> Self {
        Self {
            id: folder.id.clone(),
            parent_id: folder.parent_id.clone(),
            name: folder.name.clone(),
            sort_order: folder.sort_order,
            color: folder.color,
            path: tree::folder_path(library, Some(&folder.id)),
        }
    }
}

/// An SSH port forward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortForwardDto {
    /// Record id.
    pub id: String,
    /// The SSH connection it tunnels through.
    pub connection_id: String,
    /// Display name.
    pub name: String,
    /// Local listening port.
    pub local_port: u16,
    /// Target host as seen from the server.
    pub remote_host: String,
    /// Target port.
    pub remote_port: u16,
    /// Creation time.
    pub created_at: Timestamp,
}

impl From<&PortForward> for PortForwardDto {
    fn from(forward: &PortForward) -> Self {
        Self {
            id: forward.id.clone(),
            connection_id: forward.connection_id.clone(),
            name: forward.name.clone(),
            local_port: forward.local_port,
            remote_host: forward.remote_host.clone(),
            remote_port: forward.remote_port,
            created_at: forward.created_at,
        }
    }
}

/// A saved split layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSplitDto {
    /// Record id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Pane connections in order (some may no longer exist).
    pub connection_ids: Vec<String>,
    /// Sidebar position.
    pub sort_order: i64,
    /// Creation time.
    pub created_at: Timestamp,
}

impl From<&SavedSplit> for SavedSplitDto {
    fn from(split: &SavedSplit) -> Self {
        Self {
            id: split.id.clone(),
            name: split.name.clone(),
            connection_ids: split.connection_ids.clone(),
            sort_order: split.sort_order,
            created_at: split.created_at,
        }
    }
}

/// A sidebar tree node: a folder with its children, or an entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TreeNodeDto {
    /// A folder row.
    Folder {
        /// Folder id.
        id: String,
        /// Subfolders first, then entries.
        children: Vec<TreeNodeDto>,
    },
    /// An entry row.
    Entry {
        /// Entry id.
        id: String,
    },
}

impl From<&TreeNode<'_>> for TreeNodeDto {
    fn from(node: &TreeNode<'_>) -> Self {
        match node {
            TreeNode::Folder { folder, children } => Self::Folder {
                id: folder.id.clone(),
                children: children.iter().map(Into::into).collect(),
            },
            TreeNode::Entry(entry) => Self::Entry { id: entry.id.clone() },
        }
    }
}

/// A folder in picker order with its depth (root = 0).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlatFolderDto {
    /// Folder id.
    pub id: String,
    /// Nesting depth.
    pub depth: usize,
}

/// The whole library, plus the sidebar structure computed in Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDto {
    /// Folders in repository order.
    pub folders: Vec<FolderDto>,
    /// Entries by name.
    pub entries: Vec<EntryDto>,
    /// Connections by name.
    pub connections: Vec<ConnectionDto>,
    /// Credentials by name.
    pub credentials: Vec<CredentialDto>,
    /// Port forwards by local port.
    pub port_forwards: Vec<PortForwardDto>,
    /// Saved splits in sidebar order.
    pub saved_splits: Vec<SavedSplitDto>,
    /// The "Connections" tree.
    pub tree: Vec<TreeNodeDto>,
    /// Favorite entry ids in natural name order.
    pub favorites: Vec<String>,
    /// Every folder depth first for pickers and Move menus.
    pub flattened_folders: Vec<FlatFolderDto>,
}

impl LibraryDto {
    /// Builds the DTO; an empty library for a locked vault.
    #[must_use]
    pub fn new(library: &Library) -> Self {
        Self {
            folders: library
                .folders
                .iter()
                .map(|f| FolderDto::new(f, library))
                .collect(),
            entries: library
                .entries
                .iter()
                .map(|e| EntryDto::new(e, library))
                .collect(),
            connections: library
                .connections
                .iter()
                .map(|c| ConnectionDto::new(c, library))
                .collect(),
            credentials: library
                .credentials
                .iter()
                .map(|c| CredentialDto::new(c, library))
                .collect(),
            port_forwards: library.port_forwards.iter().map(Into::into).collect(),
            saved_splits: library.saved_splits.iter().map(Into::into).collect(),
            tree: tree::build(library).iter().map(Into::into).collect(),
            favorites: tree::favorites(library).iter().map(|e| e.id.clone()).collect(),
            flattened_folders: tree::flattened_folders(library, None)
                .into_iter()
                .map(|(f, depth)| FlatFolderDto {
                    id: f.id.clone(),
                    depth,
                })
                .collect(),
        }
    }
}

/// Search results as ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultDto {
    /// Matching entries in natural name order.
    pub entries: Vec<String>,
    /// Matching credentials in library order.
    pub credentials: Vec<String>,
}

/// Another instance that holds the database lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockHolderDto {
    /// Its host name.
    pub host: String,
    /// Its process id.
    pub pid: u32,
    /// Seconds since it last refreshed the lock.
    pub age_seconds: u64,
}

impl From<&LockHolder> for LockHolderDto {
    fn from(holder: &LockHolder) -> Self {
        Self {
            host: holder.host.clone(),
            pid: holder.pid,
            age_seconds: holder.age.as_secs(),
        }
    }
}

/// Which screen the app shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Phase {
    /// The database could not be opened; the error screen with Settings and Retry.
    NoDatabase {
        /// `openFailed`, `noPath` or `lockCancelled`.
        code: String,
        /// English description.
        error: String,
    },
    /// Another instance holds the lock; the UI asks read-only, open anyway, or cancel.
    LockConflict {
        /// Who holds it.
        holder: LockHolderDto,
    },
    /// The unlock or create screen.
    Locked {
        /// The database has no vault yet.
        needs_setup: bool,
    },
    /// The main window.
    Unlocked {
        /// Writes are refused.
        read_only: bool,
    },
    /// A newer app version has used the database: it isn't opened until this app is updated.
    NeedsUpdate {
        /// The newest version that opened the file.
        database_version: String,
        /// This app's version.
        app_version: String,
    },
}

/// The app's state for the root view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatusDto {
    /// Current screen.
    pub phase: Phase,
    /// The database file in use.
    pub database_path: Option<String>,
    /// Opened read-only.
    pub read_only: bool,
    /// The "remember" checkbox default.
    pub remember_master_key: bool,
    /// `macos`, `windows` or `linux`.
    pub platform: Platform,
    /// First launch after the Swift app (its settings were imported): show the welcome banner.
    pub handover_notice: bool,
}

/// The settings the UI can show and change.
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent settings, flat for the UI"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    /// The database file in use (configured or default).
    pub database_path: Option<String>,
    /// `en` or `hu`.
    pub language: String,
    /// Keep the vault key in the OS keyring.
    pub remember_master_key: bool,
    /// External terminal id; `None` means the OS default.
    pub external_terminal: Option<String>,
    /// Check for updates on launch.
    pub check_for_updates_automatically: bool,
    /// Last update check.
    pub last_update_check: Option<Timestamp>,
    /// Expanded sidebar folders.
    pub expanded_folders: Vec<String>,
    /// WinBox set in Settings; `None` means auto-detect.
    pub winbox_path: Option<String>,
    /// What a single click on a sidebar entry with an open connection shows: `details` or `tab`.
    pub selection_shows: String,
    /// Entries have launchers in the OS search.
    pub show_in_system_search: bool,
    /// macOS embedded RDP: swap the key left of 1 with the key next to left Shift; `None` = automatic.
    pub swap_iso_keys: Option<bool>,
    /// A connected keyboard is ISO (what automatic follows; always `false` off macOS).
    pub iso_keyboard: bool,
}

/// One row of the shortcut settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutDto {
    /// Command id (`newRDP`, `lock`, …).
    pub command: ShortcutCommand,
    /// `file` or `connection`.
    pub group: &'static str,
    /// Effective shortcut; `None` when removed.
    pub shortcut: Option<KeyShortcut>,
    /// The shortcut in the platform's notation.
    pub display: Option<String>,
    /// The default in the platform's notation (for "Restore default (X)").
    pub default_display: String,
    /// Differs from the default.
    pub customized: bool,
}

/// Every command's shortcut row on `settings`' platform, File menu first.
#[must_use]
pub fn shortcut_rows(settings: &ShortcutSettings) -> Vec<ShortcutDto> {
    let platform = settings.platform();
    let file = ShortcutCommand::FILE_MENU.iter().map(|&c| (c, "file"));
    let connection = ShortcutCommand::connection_menu()
        .into_iter()
        .map(|c| (c, "connection"));
    file.chain(connection)
        .map(|(command, group)| {
            let shortcut = settings.shortcut(command);
            ShortcutDto {
                command,
                group,
                display: shortcut.as_ref().map(|s| s.display(platform)),
                shortcut,
                default_display: command.default_shortcut(platform).display(platform),
                customized: settings.is_customized(command),
            }
        })
        .collect()
}

/// Result of a bulk delete.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCountsDto {
    /// Entries deleted through their `e:` / `fav:` tags.
    pub entries: usize,
    /// Connections deleted, those of deleted entries included.
    pub connections: usize,
    /// Folders deleted (subfolders not counted).
    pub folders: usize,
    /// Credentials deleted.
    pub credentials: usize,
    /// Saved splits deleted.
    pub splits: usize,
}

/// Result of a merge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeResultDto {
    /// Connections that moved into the target entry.
    pub moved_connections: usize,
}

/// Result of a move.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveResultDto {
    /// Items moved.
    pub moved: usize,
    /// Folders refused because the target is inside them ("A folder can't be moved into itself").
    pub cycle_refused: usize,
}

/// One row of the import preview. Secret-free: only whether the entry has a password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRowDto {
    /// Stable within one file: `c<n>` for connections, `k<n>` for credentials.
    pub id: String,
    /// `rdp`, `ssh` or `mikrotik`, or `credential` for an RDM credential entry.
    #[serde(rename = "type")]
    pub kind: String,
    /// Display name.
    pub name: String,
    /// Host; empty for credentials.
    pub host: String,
    /// Port; `None` means the type's default.
    pub port: Option<u16>,
    /// Login user.
    pub user: String,
    /// Folder names under the import root.
    pub group_path: Vec<String>,
    /// The export carries a usable (plain-text) password.
    pub has_password: bool,
    /// Already in the library (unchecked by default).
    pub duplicate: bool,
}

/// What `import_preview` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewDto {
    /// Which importer read the file.
    pub kind: ImportKind,
    /// The file name, for the sheet's summary.
    pub file_name: String,
    /// Connections, then credentials, in file order.
    pub rows: Vec<ImportRowDto>,
    /// Entries left out: `{name, reason: "unsupported" | "noHost", kind?}`.
    pub skipped: Vec<Skipped>,
    /// Entries whose password was only in encrypted form.
    pub without_password: usize,
    /// Empty RDM folders that will be created.
    pub empty_groups: usize,
}

/// What `import_commit` returns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResultDto {
    /// Connections created.
    pub connections: usize,
    /// Credentials created.
    pub credentials: usize,
    /// Folders created.
    pub folders_created: usize,
    /// Entries the importer left out.
    pub skipped: usize,
    /// Entries whose password was only in encrypted form.
    pub without_password: usize,
}

/// What `export_library` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResultDto {
    /// Entries written.
    pub entries: usize,
    /// Credentials written.
    pub credentials: usize,
    /// The file is encrypted (and carries the passwords).
    pub encrypted: bool,
}

/// One row of an export's import preview. Secret-free: only whether it carries a password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferRowDto {
    /// Stable within one file: `e<n>` for entries, `k<n>` for credentials.
    pub id: String,
    /// `entry` or `credential`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Display name.
    pub name: String,
    /// Host; empty for credentials.
    pub host: String,
    /// The credential's user, or the first connection user of an entry.
    pub user: String,
    /// Folder names from the root down.
    pub folder_path: Vec<String>,
    /// The entry's connection types, de-duplicated in order.
    pub connection_types: Vec<ConnectionType>,
    /// Rows of the credentials the entry's connections link to.
    pub credential_ids: Vec<String>,
    /// The file carries a password for it.
    pub has_password: bool,
    /// A matching record already exists; the user picks overwrite, keep both or skip.
    pub conflict: bool,
}

/// What `transfer_preview` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferPreviewDto {
    /// The file name, for the sheet's summary.
    pub file_name: String,
    /// The file is encrypted.
    pub encrypted: bool,
    /// When the file was written.
    pub exported_at: String,
    /// Entries, then credentials, in file order.
    pub rows: Vec<TransferRowDto>,
}

/// The user's choice for one preview row.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferDecisionDto {
    /// [`TransferRowDto::id`].
    pub id: String,
    /// `new`, `overwrite` or `skip`.
    pub action: rd_store::TransferAction,
}

/// What `transfer_commit` returns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferResultDto {
    /// Entries created.
    pub entries_created: usize,
    /// Entries replaced.
    pub entries_updated: usize,
    /// Credentials created.
    pub credentials_created: usize,
    /// Credentials replaced.
    pub credentials_updated: usize,
    /// Folders created.
    pub folders_created: usize,
}

/// What `csv_preview` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvPreviewDto {
    /// The file name, for the sheet's summary.
    pub file_name: String,
    /// The entries, in file order.
    pub rows: Vec<TransferRowDto>,
    /// Rows left out: `{line, name, reason, detail?}`.
    pub skipped: Vec<rd_core::transfer::CsvSkipped>,
}
