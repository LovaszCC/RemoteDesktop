//! The import flow (Swift `AppState.importRDM` / `loadWinbox` / `importWinbox`): read and parse the file,
//! preview it with duplicate flags, then write the chosen entries. The file is parsed again on commit, so
//! no plain-text password is kept between the two calls and none reaches the webview.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;

use rd_core::import::{self, ImportError, ImportKind, ParsedImport};
use rd_store::ImportBatch;

use super::{AppCore, CommandError};
use crate::dto::{ImportPreviewDto, ImportResultDto, ImportRowDto};

/// Larger files are refused: real exports are a few megabytes at most.
pub const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;

pub(super) fn read_file(path: &Path) -> Result<Vec<u8>, CommandError> {
    let unreadable = |e: std::io::Error| {
        CommandError::new("importRead", "Can't read the file.").with_detail(e.to_string())
    };
    let file = std::fs::File::open(path).map_err(unreadable)?;
    let mut data = Vec::new();
    file.take(MAX_IMPORT_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(unreadable)?;
    if data.len() as u64 > MAX_IMPORT_BYTES {
        return Err(CommandError::new(
            "importTooLarge",
            "The file is too large to import.",
        ));
    }
    Ok(data)
}

fn parse_file(kind: ImportKind, path: &Path) -> Result<ParsedImport, CommandError> {
    let data = read_file(path)?;
    let parsed = import::parse(kind, &data).map_err(|e| match e {
        ImportError::WinboxUnreadable => CommandError::new(
            "importUnreadable",
            "This isn't a WinBox address book (.cdb) or it is damaged.",
        ),
        ImportError::RdmUnreadable(detail) => CommandError::new(
            "importUnreadable",
            "This isn't a Devolutions RDM export (.rdm) or it is damaged.",
        )
        .with_detail(detail),
    })?;
    let nothing = parsed.connections.is_empty() && parsed.credentials.is_empty();
    match kind {
        ImportKind::Winbox if nothing => Err(CommandError::new(
            "importEmpty",
            "The file has no WinBox addresses.",
        )),
        ImportKind::Rdm if nothing && parsed.empty_groups.is_empty() && parsed.skipped.is_empty() => Err(
            CommandError::new("importEmpty", "The file has nothing to import."),
        ),
        _ => Ok(parsed),
    }
}

fn connection_id(index: usize) -> String {
    format!("c{index}")
}

fn credential_id(index: usize) -> String {
    format!("k{index}")
}

impl AppCore {
    /// Parses `path` and returns its secret-free rows; entries already in the library are flagged.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `importRead`, `importTooLarge`, `importUnreadable` or `importEmpty`.
    pub fn import_preview(&self, kind: ImportKind, path: &Path) -> Result<ImportPreviewDto, CommandError> {
        let library = {
            let inner = self.inner();
            let (db, _) = Self::writable(&inner)?;
            db.library()?
        };
        let parsed = parse_file(kind, path)?;
        let flags = parsed.duplicates(&library, None);
        let connections = parsed.connections.iter().zip(&flags.connections).enumerate().map(
            |(index, (item, &duplicate))| ImportRowDto {
                id: connection_id(index),
                kind: item.kind.as_str().to_owned(),
                name: item.name.clone(),
                host: item.host.clone(),
                port: item.port,
                user: item.username.clone(),
                group_path: item.group_path.clone(),
                has_password: item.password.is_some(),
                duplicate,
            },
        );
        let credentials = parsed.credentials.iter().zip(&flags.credentials).enumerate().map(
            |(index, (item, &duplicate))| ImportRowDto {
                id: credential_id(index),
                kind: "credential".to_owned(),
                name: item.name.clone(),
                host: String::new(),
                port: None,
                user: item.username.clone(),
                group_path: Vec::new(),
                has_password: item.password.is_some(),
                duplicate,
            },
        );
        Ok(ImportPreviewDto {
            kind,
            file_name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            rows: connections.chain(credentials).collect(),
            skipped: parsed.skipped.clone(),
            without_password: parsed.encrypted_passwords,
            empty_groups: parsed.empty_groups.len(),
        })
    }

    /// Parses `path` again and writes the rows whose ids are in `selected` (and RDM's empty folders) in
    /// one transaction. A selected connection linked to a credential that is not imported links to an
    /// existing credential with the same name, user and domain, if there is one.
    ///
    /// # Errors
    /// `locked`, `readOnly`, the [`import_preview`](Self::import_preview) file errors, or `database`.
    pub fn import_commit(
        &self,
        kind: ImportKind,
        path: &Path,
        selected: &[String],
    ) -> Result<ImportResultDto, CommandError> {
        // Fail fast before reading the file.
        Self::writable(&self.inner())?;
        let parsed = parse_file(kind, path)?;
        let selected: HashSet<&str> = selected.iter().map(String::as_str).collect();
        let connections: Vec<_> = parsed
            .connections
            .iter()
            .enumerate()
            .filter(|(i, _)| selected.contains(connection_id(*i).as_str()))
            .map(|(_, item)| item.clone())
            .collect();
        let (credentials, not_imported): (Vec<_>, Vec<_>) = parsed
            .credentials
            .iter()
            .enumerate()
            .partition(|(i, _)| selected.contains(credential_id(*i).as_str()));
        let credentials: Vec<_> = credentials.into_iter().map(|(_, c)| c.clone()).collect();

        let inner = self.inner();
        let (db, vault) = Self::writable(&inner)?;
        let library = db.library()?;
        let existing: HashMap<String, String> = not_imported
            .into_iter()
            .filter_map(|(_, item)| {
                library
                    .credentials
                    .iter()
                    .find(|c| import::credential_is_duplicate(item, std::slice::from_ref(c)))
                    .map(|c| (item.reference.clone(), c.id.clone()))
            })
            .collect();
        let counts = db.import(
            &ImportBatch {
                connections: &connections,
                credentials: &credentials,
                empty_groups: &parsed.empty_groups,
                parent_folder_id: None,
                existing_credentials: Some(&existing),
            },
            vault,
        )?;
        Ok(ImportResultDto {
            connections: counts.connections,
            credentials: counts.credentials,
            folders_created: counts.folders_created,
            skipped: parsed.skipped.len(),
            without_password: parsed.encrypted_passwords,
        })
    }
}
