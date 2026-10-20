//! Export and import of the app's own format: write the picked entries and credentials to a file
//! (encrypted with its own password when one is given), preview a file with its conflicts, then import it
//! with the user's choice per record. As with the other importers, the file is read again on commit, so no
//! plain-text password is kept between the calls and none reaches the webview.

use std::path::Path;

use rd_core::Library;
use rd_core::transfer::{ExportPayload, ExportSelection, build_payload, conflicts};
use rd_store::{
    TransferAction, TransferDecisions, TransferError, VaultError, export_is_encrypted, read_export,
    write_export,
};
use secrecy::ExposeSecret;

use super::import::read_file;
use super::{AppCore, CommandError};
use crate::dto::{
    ExportResultDto, TransferDecisionDto, TransferPreviewDto, TransferResultDto, TransferRowDto,
};

impl From<TransferError> for CommandError {
    fn from(error: TransferError) -> Self {
        match error {
            TransferError::Unreadable => Self::new(
                "transferUnreadable",
                "This isn't a RemoteDesktop export or it is damaged.",
            ),
            TransferError::UnsupportedVersion(version) => Self::new(
                "transferNewer",
                "This export was made by a newer version of the app.",
            )
            .with_detail(version.to_string()),
            TransferError::NeedsPassword => Self::new(
                "transferNeedsPassword",
                "The export is encrypted. Enter its password.",
            ),
            TransferError::WrongPassword => Self::new("transferWrongPassword", "Wrong password"),
            TransferError::EmptyPassword => Self::new("emptyPassword", "The password is empty."),
            TransferError::PlainWithPasswords => Self::new(
                "transferPlainPasswords",
                "Passwords can only be exported with a password.",
            ),
        }
    }
}

fn entry_id(index: usize) -> String {
    format!("e{index}")
}

fn credential_id(index: usize) -> String {
    format!("k{index}")
}

fn read_payload(path: &Path, password: Option<&str>) -> Result<(ExportPayload, bool), CommandError> {
    let data = read_file(path)?;
    let encrypted = export_is_encrypted(&data)?;
    Ok((read_export(&data, password)?, encrypted))
}

/// The secret-free preview rows of a payload, entries then credentials, flagging the ones that already exist.
pub(super) fn preview_rows(payload: &ExportPayload, library: &Library) -> Vec<TransferRowDto> {
    let found = conflicts(payload, library);
    let entries = payload.entries.iter().enumerate().map(|(index, item)| {
        let mut credential_ids = Vec::new();
        for reference in item
            .connections
            .iter()
            .filter_map(|c| c.credential_ref.as_deref())
        {
            if let Some(position) = payload.credentials.iter().position(|c| c.reference == reference) {
                let id = credential_id(position);
                if !credential_ids.contains(&id) {
                    credential_ids.push(id);
                }
            }
        }
        let mut types = Vec::new();
        for connection in &item.connections {
            if !types.contains(&connection.kind) {
                types.push(connection.kind);
            }
        }
        TransferRowDto {
            id: entry_id(index),
            kind: "entry".to_owned(),
            name: item.name.clone(),
            host: item.host.clone(),
            user: item
                .connections
                .iter()
                .map(|c| c.username.as_str())
                .find(|u| !u.is_empty())
                .unwrap_or_default()
                .to_owned(),
            folder_path: item.folder_path.clone(),
            connection_types: types,
            credential_ids,
            has_password: item.connections.iter().any(|c| c.password.is_some()),
            conflict: found.entries[index].is_some(),
        }
    });
    let credentials = payload
        .credentials
        .iter()
        .enumerate()
        .map(|(index, item)| TransferRowDto {
            id: credential_id(index),
            kind: "credential".to_owned(),
            name: item.name.clone(),
            host: String::new(),
            user: item.username.clone(),
            folder_path: Vec::new(),
            connection_types: Vec::new(),
            credential_ids: Vec::new(),
            has_password: item.password.is_some(),
            conflict: found.credentials[index].is_some(),
        });
    entries.chain(credentials).collect()
}

impl AppCore {
    /// Writes the picked entries and credentials to `path`. With a password the file is encrypted and
    /// carries the passwords; without one it carries none. SSH key settings are never exported.
    ///
    /// # Errors
    /// `locked`, `emptyPassword`, `corruptData`, `exportWrite`, or `database`.
    pub fn export_library(
        &self,
        path: &Path,
        entry_ids: &[String],
        credential_ids: &[String],
        password: Option<&str>,
    ) -> Result<ExportResultDto, CommandError> {
        if password == Some("") {
            return Err(TransferError::EmptyPassword.into());
        }
        let (text, entries, credentials) = {
            let inner = self.inner();
            let (db, vault) = Self::unlocked(&inner)?;
            let library = db.library()?;
            let selection = ExportSelection {
                entries: entry_ids.to_vec(),
                credentials: credential_ids.to_vec(),
            };
            let mut open = |sealed: &rd_core::Sealed| -> Result<String, VaultError> {
                Ok(vault.open(sealed)?.expose_secret().to_owned())
            };
            let payload = build_payload(
                &library,
                &selection,
                &self.app_version,
                password.is_some().then_some(&mut open as _),
            )?;
            let text = write_export(&payload, password, self.iterations)?;
            (text, payload.entries.len(), payload.credentials.len())
        };
        std::fs::write(path, text.as_bytes()).map_err(|e| {
            CommandError::new("exportWrite", "Can't write the file.").with_detail(e.to_string())
        })?;
        Ok(ExportResultDto {
            entries,
            credentials,
            encrypted: password.is_some(),
        })
    }

    /// Reads an export and returns its secret-free rows, entries then credentials, flagging the ones that
    /// already exist.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `importRead`, `importTooLarge`, `transferUnreadable`, `transferNewer`,
    /// `transferNeedsPassword` or `transferWrongPassword`.
    pub fn transfer_preview(
        &self,
        path: &Path,
        password: Option<&str>,
    ) -> Result<TransferPreviewDto, CommandError> {
        let library = {
            let inner = self.inner();
            let (db, _) = Self::writable(&inner)?;
            db.library()?
        };
        let (payload, encrypted) = read_payload(path, password)?;
        Ok(TransferPreviewDto {
            file_name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            encrypted,
            exported_at: payload.exported_at.clone(),
            rows: preview_rows(&payload, &library),
        })
    }

    /// Reads the export again and imports it: each row by its decision, rows without one are skipped.
    ///
    /// # Errors
    /// `locked`, `readOnly`, the [`transfer_preview`](Self::transfer_preview) file errors, or `database`.
    pub fn transfer_commit(
        &self,
        path: &Path,
        password: Option<&str>,
        decisions: &[TransferDecisionDto],
    ) -> Result<TransferResultDto, CommandError> {
        Self::writable(&self.inner())?;
        let (payload, _) = read_payload(path, password)?;
        self.commit_payload(&payload, decisions)
    }

    /// Imports `payload` with the decisions of its preview rows; rows without one are skipped.
    pub(super) fn commit_payload(
        &self,
        payload: &ExportPayload,
        decisions: &[TransferDecisionDto],
    ) -> Result<TransferResultDto, CommandError> {
        let pick = |id: &str| {
            decisions
                .iter()
                .find(|d| d.id == id)
                .map_or(TransferAction::Skip, |d| d.action)
        };
        let decisions = TransferDecisions {
            entries: (0..payload.entries.len()).map(|i| pick(&entry_id(i))).collect(),
            credentials: (0..payload.credentials.len())
                .map(|i| pick(&credential_id(i)))
                .collect(),
        };
        let inner = self.inner();
        let (db, vault) = Self::writable(&inner)?;
        let counts = db.import_transfer(payload, &decisions, vault)?;
        Ok(TransferResultDto {
            entries_created: counts.entries_created,
            entries_updated: counts.entries_updated,
            credentials_created: counts.credentials_created,
            credentials_updated: counts.credentials_updated,
            folders_created: counts.folders_created,
        })
    }
}
