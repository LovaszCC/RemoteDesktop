//! Mapping between records and rows, with the Swift app's column names and encodings.

use std::str::FromStr;

use rd_core::{
    Connection, Credential, Entry, EntryVpn, Folder, PortForward, SavedSplit, Sealed, Timestamp, VpnTestMode,
};
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use rusqlite::{Row, ToSql};

/// Column list for `SELECT`/`INSERT` on `connection`, in [`connection_from_row`] order.
pub(crate) const CONNECTION_COLUMNS: &str = "id, folderId, credentialId, type, name, host, port, username, \
    domain, passwordEnc, sshKeyPath, sshKeyPassphraseEnc, isFavorite, rdpAdminSession, browserBundleId, \
    mikrotikMode, sshTerminal, rdpClient, color, notes, createdAt, updatedAt, entryId, label, showAsButton, \
    sortOrder, webScheme, webPath";
/// Column list for `SELECT`/`INSERT` on `entry`, in [`entry_from_row`] order.
pub(crate) const ENTRY_COLUMNS: &str = "id, folderId, name, host, notes, color, isFavorite, defaultConnectionId, \
    createdAt, updatedAt, vpnConnectionId, vpnTestMode, vpnTestAddress";
pub(crate) const CREDENTIAL_COLUMNS: &str = "id, name, username, domain, passwordEnc, sshKeyPath, \
    sshKeyPassphraseEnc, sshKeyUnlockedEnc, notes, createdAt, updatedAt";
pub(crate) const FOLDER_COLUMNS: &str = "id, parentId, name, sortOrder, color";
pub(crate) const PORT_FORWARD_COLUMNS: &str =
    "id, connectionId, name, localPort, remoteHost, remotePort, createdAt";
pub(crate) const SAVED_SPLIT_COLUMNS: &str = "id, name, connectionIds, sortOrder, createdAt";

/// Wraps a value so it maps to and from its stored text form.
pub(crate) struct Text<T>(pub T);

impl<T: FromStr> FromSql for Text<T> {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        text.parse()
            .map(Text)
            .map_err(|_| FromSqlError::Other(format!("unexpected value {text:?}").into()))
    }
}

impl<T: ToString> ToSql for Text<T> {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.0.to_string()))
    }
}

pub(crate) fn text<T: FromStr>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    row.get::<_, Text<T>>(index).map(|t| t.0)
}

pub(crate) fn opt_text<T: FromStr>(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<T>> {
    row.get::<_, Option<Text<T>>>(index).map(|t| t.map(|t| t.0))
}

pub(crate) fn timestamp(row: &Row<'_>, index: usize) -> rusqlite::Result<Timestamp> {
    let text: String = row.get(index)?;
    Timestamp::parse(&text)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, e.into()))
}

pub(crate) fn sealed(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<Sealed>> {
    Ok(row.get::<_, Option<Vec<u8>>>(index)?.map(Sealed::from_bytes))
}

pub(crate) fn sealed_sql(value: Option<&Sealed>) -> Option<&[u8]> {
    value.map(Sealed::as_bytes)
}

fn port(row: &Row<'_>, index: usize) -> rusqlite::Result<u16> {
    let value: i64 = row.get(index)?;
    u16::try_from(value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Integer, e.into())
    })
}

pub(crate) fn connection_from_row(row: &Row<'_>) -> rusqlite::Result<Connection> {
    let id: String = row.get(0)?;
    Ok(Connection {
        // A row written without an entry (by an older build) stands for its own entry.
        entry_id: row.get::<_, Option<String>>(22)?.unwrap_or_else(|| id.clone()),
        id,
        folder_id: row.get(1)?,
        credential_id: row.get(2)?,
        kind: text(row, 3)?,
        name: row.get(4)?,
        host: row.get(5)?,
        port: match row.get::<_, Option<i64>>(6)? {
            None => None,
            Some(_) => Some(port(row, 6)?),
        },
        username: row.get(7)?,
        domain: row.get(8)?,
        password_enc: sealed(row, 9)?,
        ssh_key_path: row.get(10)?,
        ssh_key_passphrase_enc: sealed(row, 11)?,
        is_favorite: row.get(12)?,
        rdp_admin_session: row.get(13)?,
        browser_bundle_id: row.get(14)?,
        mikrotik_mode: text(row, 15)?,
        ssh_terminal: text(row, 16)?,
        rdp_client: text(row, 17)?,
        color: opt_text(row, 18)?,
        notes: row.get(19)?,
        created_at: timestamp(row, 20)?,
        updated_at: timestamp(row, 21)?,
        label: row.get(23)?,
        show_as_button: row.get(24)?,
        sort_order: row.get(25)?,
        web_scheme: row.get(26)?,
        web_path: row.get(27)?,
    })
}

pub(crate) fn entry_from_row(row: &Row<'_>) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        name: row.get(2)?,
        host: row.get(3)?,
        notes: row.get(4)?,
        color: opt_text(row, 5)?,
        is_favorite: row.get(6)?,
        default_connection_id: row.get(7)?,
        created_at: timestamp(row, 8)?,
        updated_at: timestamp(row, 9)?,
        vpn: entry_vpn(row, 10)?,
    })
}

/// The VPN columns from `index` on; an unknown test mode (a newer build's) reads as the default.
fn entry_vpn(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<EntryVpn>> {
    let Some(connection_id) = row.get::<_, Option<String>>(index)? else {
        return Ok(None);
    };
    let mode: String = row.get(index + 1)?;
    Ok(Some(EntryVpn {
        connection_id,
        test_mode: mode.parse().unwrap_or(VpnTestMode::Host),
        test_address: row.get(index + 2)?,
    }))
}

pub(crate) fn credential_from_row(row: &Row<'_>) -> rusqlite::Result<Credential> {
    Ok(Credential {
        id: row.get(0)?,
        name: row.get(1)?,
        username: row.get(2)?,
        domain: row.get(3)?,
        password_enc: sealed(row, 4)?,
        ssh_key_path: row.get(5)?,
        ssh_key_passphrase_enc: sealed(row, 6)?,
        ssh_key_unlocked_enc: sealed(row, 7)?,
        notes: row.get(8)?,
        created_at: timestamp(row, 9)?,
        updated_at: timestamp(row, 10)?,
    })
}

pub(crate) fn folder_from_row(row: &Row<'_>) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: row.get(0)?,
        parent_id: row.get(1)?,
        name: row.get(2)?,
        sort_order: row.get(3)?,
        color: opt_text(row, 4)?,
    })
}

pub(crate) fn port_forward_from_row(row: &Row<'_>) -> rusqlite::Result<PortForward> {
    Ok(PortForward {
        id: row.get(0)?,
        connection_id: row.get(1)?,
        name: row.get(2)?,
        local_port: port(row, 3)?,
        remote_host: row.get(4)?,
        remote_port: port(row, 5)?,
        created_at: timestamp(row, 6)?,
    })
}

pub(crate) fn saved_split_from_row(row: &Row<'_>) -> rusqlite::Result<SavedSplit> {
    let ids: String = row.get(2)?;
    Ok(SavedSplit {
        id: row.get(0)?,
        name: row.get(1)?,
        connection_ids: serde_json::from_str(&ids).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, e.into())
        })?,
        sort_order: row.get(3)?,
        created_at: timestamp(row, 4)?,
    })
}

/// GRDB's JSON for `[String]`: compact, no spaces.
pub(crate) fn ids_json(ids: &[String]) -> String {
    serde_json::to_string(ids).expect("a list of strings always serializes")
}
