//! The temporary file holding a serial tab's whole output, so "Copy output" and "Save to file" get more
//! than the terminal's scrollback (a complete boot log). Not a logging feature: the file lives in the
//! app's temp directory and is deleted with the tab ([`Drop`]); [`sweep`] clears what a crash left.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The default size cap: 64 MiB.
pub const DEFAULT_CAP: u64 = 64 * 1024 * 1024;

/// The app's directory for serial output files: `<temp>/remotedesktop-serial`.
#[must_use]
pub fn default_dir() -> PathBuf {
    std::env::temp_dir().join("remotedesktop-serial")
}

/// Deletes every file in `dir` (left behind by a crash or a killed app). A missing directory is fine.
///
/// # Errors
/// When the directory can't be read.
pub fn sweep(dir: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_file()) {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

struct State {
    file: File,
    len: u64,
}

/// One tab's output file. Appends raw bytes; past the cap the oldest half is dropped.
pub struct OutputLog {
    path: PathBuf,
    cap: u64,
    state: Mutex<State>,
}

impl std::fmt::Debug for OutputLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutputLog")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl OutputLog {
    /// Creates `<dir>/<id>.log` (and `dir`), empty, with the default cap.
    ///
    /// # Errors
    /// When the directory or file can't be created.
    pub fn create(dir: &Path, id: &str) -> io::Result<Self> {
        Self::with_cap(dir, id, DEFAULT_CAP)
    }

    /// [`create`](Self::create) with a size cap of `cap` bytes.
    ///
    /// # Errors
    /// When the directory or file can't be created.
    pub fn with_cap(dir: &Path, id: &str, cap: u64) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let path = dir.join(format!("{id}.log"));
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)?;
        Ok(Self {
            path,
            cap: cap.max(2),
            state: Mutex::new(State { file, len: 0 }),
        })
    }

    /// Where the file is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends `bytes`; when the file grows past the cap, keeps only its newest half-cap bytes.
    ///
    /// # Errors
    /// The write error.
    pub fn append(&self, bytes: &[u8]) -> io::Result<()> {
        let mut state = lock(&self.state);
        state.file.seek(SeekFrom::End(0))?;
        state.file.write_all(bytes)?;
        state.len += bytes.len() as u64;
        if state.len > self.cap {
            let keep = self.cap / 2;
            let mut tail = Vec::new();
            let start = state.len - keep;
            state.file.seek(SeekFrom::Start(start))?;
            (&mut state.file).take(keep).read_to_end(&mut tail)?;
            state.file.set_len(0)?;
            state.file.seek(SeekFrom::Start(0))?;
            state.file.write_all(&tail)?;
            state.len = tail.len() as u64;
        }
        Ok(())
    }

    /// The raw bytes so far.
    ///
    /// # Errors
    /// The read error.
    pub fn bytes(&self) -> io::Result<Vec<u8>> {
        let mut state = lock(&self.state);
        let mut out = Vec::new();
        state.file.seek(SeekFrom::Start(0))?;
        state.file.read_to_end(&mut out)?;
        Ok(out)
    }

    /// The output so far as plain text ([`to_plain`]).
    ///
    /// # Errors
    /// The read error.
    pub fn plain_text(&self) -> io::Result<String> {
        self.bytes().map(|b| to_plain(&b))
    }
}

impl Drop for OutputLog {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Default)]
struct Plain {
    text: String,
}

impl vte::Perform for Plain {
    fn print(&mut self, c: char) {
        self.text.push(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.text.push('\n'),
            b'\t' => self.text.push('\t'),
            // Backspace erases the last character of the line (line editing on a console).
            0x08 if !self.text.ends_with('\n') => {
                self.text.pop();
            }
            _ => {}
        }
    }
}

/// Terminal output as plain text: escape sequences (colors, cursor movement, titles) and control
/// characters other than newline and tab are removed, CR is dropped (CRLF → LF), backspace erases.
#[must_use]
pub fn to_plain(bytes: &[u8]) -> String {
    let mut parser = vte::Parser::new();
    let mut plain = Plain::default();
    parser.advance(&mut plain, bytes);
    plain.text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_strips_escapes_and_normalises_line_ends() {
        let raw = b"\x1b[1;32mOPNsense\x1b[0m 24.7\r\n\x1b]0;title\x07login:\troot\r\nabcd\x08\x08ef\r\n\xc3\xa1rv\xc3\xadz";
        assert_eq!(
            to_plain(raw),
            "OPNsense 24.7\nlogin:\troot\nabef\n\u{e1}rv\u{ed}z"
        );
    }

    #[test]
    fn appends_reads_and_deletes_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let log = OutputLog::create(dir.path(), "S1").unwrap();
        log.append(b"boot\r\n").unwrap();
        log.append(b"\x1b[2Jok\r\n").unwrap();
        assert_eq!(log.plain_text().unwrap(), "boot\nok\n");
        let path = log.path().to_path_buf();
        assert_eq!(path, dir.path().join("S1.log"));
        assert!(path.exists());
        drop(log);
        assert!(!path.exists());
    }

    #[test]
    fn past_the_cap_the_oldest_half_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let log = OutputLog::with_cap(dir.path(), "S2", 10).unwrap();
        log.append(b"012345").unwrap();
        log.append(b"6789AB").unwrap();
        assert_eq!(log.bytes().unwrap(), b"789AB");
        log.append(b"C").unwrap();
        assert_eq!(log.bytes().unwrap(), b"789ABC");
    }

    #[test]
    fn sweep_empties_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("old.log"), b"x").unwrap();
        fs::write(dir.path().join("older.log"), b"y").unwrap();
        sweep(dir.path()).unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        sweep(&dir.path().join("missing")).unwrap();
    }
}
