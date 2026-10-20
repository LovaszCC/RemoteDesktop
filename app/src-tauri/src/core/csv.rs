//! CSV import of a server list (`folder;name;host;type`): the sample file, the preview and the commit. The
//! file is parsed again on commit, like the other importers; the rows go through the export import's
//! preview and writer.

use std::path::Path;

use rd_core::transfer::{CsvError, CsvImport, parse_csv, sample_csv};

use super::import::read_file;
use super::transfer::preview_rows;
use super::{AppCore, CommandError};
use crate::dto::{CsvPreviewDto, TransferDecisionDto, TransferResultDto};

impl From<CsvError> for CommandError {
    fn from(error: CsvError) -> Self {
        match error {
            CsvError::Encoding => Self::new(
                "csvEncoding",
                "The CSV file isn't UTF-8. Save it as \"CSV UTF-8\".",
            ),
            CsvError::Empty => Self::new("importEmpty", "The file has nothing to import."),
            CsvError::Unterminated => Self::new(
                "csvUnreadable",
                "The CSV file is damaged: a quote is never closed.",
            ),
            CsvError::MissingColumns(columns) => {
                Self::new("csvMissingColumns", "The CSV file is missing columns: %@")
                    .with_detail(columns.join(", "))
            }
        }
    }
}

fn parse_file(path: &Path) -> Result<CsvImport, CommandError> {
    let parsed = parse_csv(&read_file(path)?)?;
    if parsed.payload.entries.is_empty() && parsed.skipped.is_empty() {
        return Err(CsvError::Empty.into());
    }
    Ok(parsed)
}

impl AppCore {
    /// Writes the sample CSV (header in the app's language) to `path`.
    ///
    /// # Errors
    /// `exportWrite`.
    pub fn save_csv_sample(&self, path: &Path) -> Result<(), CommandError> {
        let language = self.inner().settings.language.clone();
        std::fs::write(path, sample_csv(&language))
            .map_err(|e| CommandError::new("exportWrite", "Can't write the file.").with_detail(e.to_string()))
    }

    /// Reads a CSV file: the rows of its entries (existing ones flagged) and the rows it left out.
    ///
    /// # Errors
    /// `locked`, `readOnly`, `importRead`, `importTooLarge`, `importEmpty`, `csvEncoding`, `csvUnreadable` or
    /// `csvMissingColumns`.
    pub fn csv_preview(&self, path: &Path) -> Result<CsvPreviewDto, CommandError> {
        let library = {
            let inner = self.inner();
            let (db, _) = Self::writable(&inner)?;
            db.library()?
        };
        let parsed = parse_file(path)?;
        Ok(CsvPreviewDto {
            file_name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            rows: preview_rows(&parsed.payload, &library),
            skipped: parsed.skipped,
        })
    }

    /// Parses the CSV file again and imports its rows as decided.
    ///
    /// # Errors
    /// `locked`, `readOnly`, the [`csv_preview`](Self::csv_preview) file errors, or `database`.
    pub fn csv_commit(
        &self,
        path: &Path,
        decisions: &[TransferDecisionDto],
    ) -> Result<TransferResultDto, CommandError> {
        Self::writable(&self.inner())?;
        let parsed = parse_file(path)?;
        self.commit_payload(&parsed.payload, decisions)
    }
}
