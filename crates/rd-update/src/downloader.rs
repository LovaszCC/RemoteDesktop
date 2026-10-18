//! Downloads a release's disk image and checks it against the release's `.sha256` file (Swift
//! `UpdateDownloader`).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::checker::{
    Fetch, HttpRequest, TransportError, UpdateError, UpdateRelease, check_status, fetch_data,
};

/// Downloads `url` to a new temporary file the caller then owns; returns it with the HTTP status.
pub type Download = Box<dyn Fn(&str) -> Result<(PathBuf, u16), TransportError> + Send + Sync>;

/// Removes a temporary download when dropped.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn io_error(error: &std::io::Error) -> UpdateError {
    UpdateError::Io(error.to_string())
}

/// Saves verified disk images.
pub struct UpdateDownloader {
    fetch: Fetch,
    download: Download,
}

impl UpdateDownloader {
    /// A downloader that reads checksum files with `fetch` and images with `download`.
    #[must_use]
    pub fn new(fetch: Fetch, download: Download) -> Self {
        Self { fetch, download }
    }

    /// Saves the release's DMG into `directory` (replacing a stale copy) and returns its location. A copy
    /// already there with the right checksum is reused without downloading.
    ///
    /// # Errors
    /// [`UpdateError::NoDownload`] without assets, [`UpdateError::ChecksumMismatch`] for a corrupted image,
    /// and the transport/HTTP errors of [`crate::UpdateChecker`].
    pub fn download(&self, release: &UpdateRelease, directory: &Path) -> Result<PathBuf, UpdateError> {
        let (Some(dmg_url), Some(checksum_url)) = (&release.dmg_url, &release.checksum_url) else {
            return Err(UpdateError::NoDownload);
        };
        let expected = checksum_from_file(&fetch_data(&self.fetch, &HttpRequest::get(checksum_url))?)?;
        let name = dmg_url.rsplit('/').next().unwrap_or(dmg_url);
        let destination = directory.join(name);
        if sha256_of(&destination).is_ok_and(|hash| hash == expected) {
            return Ok(destination);
        }

        let (temporary, status) = (self.download)(dmg_url)?;
        let temporary = TempFile(temporary);
        check_status(status)?;
        if sha256_of(&temporary.0).map_err(|e| io_error(&e))? != expected {
            return Err(UpdateError::ChecksumMismatch);
        }

        fs::create_dir_all(directory).map_err(|e| io_error(&e))?;
        let _ = fs::remove_file(&destination);
        if fs::rename(&temporary.0, &destination).is_err() {
            // Another volume: copy, then the temp file goes on drop.
            fs::copy(&temporary.0, &destination).map_err(|e| io_error(&e))?;
        }
        Ok(destination)
    }
}

/// The hash from a `shasum -a 256` line: `<hex>  <file name>`, lowercased.
///
/// # Errors
/// [`UpdateError::BadResponse`] unless the first word is 64 hex digits.
pub fn checksum_from_file(data: &[u8]) -> Result<String, UpdateError> {
    let text = String::from_utf8_lossy(data);
    let hash = text
        .split_whitespace()
        .next()
        .map(str::to_ascii_lowercase)
        .ok_or(UpdateError::BadResponse)?;
    if hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(hash)
    } else {
        Err(UpdateError::BadResponse)
    }
}

/// Lowercase hex SHA-256 of a file, read in chunks.
///
/// # Errors
/// When the file can't be read.
pub fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::AppVersion;
    use crate::checker::tests::stub_fetch;

    const IMAGE: &[u8] = b"disk image bytes";

    struct Fixture {
        directory: tempfile::TempDir,
        scratch: tempfile::TempDir,
        release: UpdateRelease,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
                scratch: tempfile::tempdir().unwrap(),
                release: UpdateRelease {
                    version: AppVersion::parse("1.2.0").unwrap(),
                    notes: String::new(),
                    page_url: "https://github.com/LovaszCC/RemoteDesktop/releases/tag/v1.2.0".into(),
                    dmg_url: Some("https://example.com/RemoteDesktop-1.2.0.dmg".into()),
                    checksum_url: Some("https://example.com/RemoteDesktop-1.2.0.dmg.sha256".into()),
                },
            }
        }

        /// The download target (a subfolder, so the downloader has to create it).
        fn dir(&self) -> PathBuf {
            self.directory.path().join("Downloads")
        }

        fn destination(&self) -> PathBuf {
            self.dir().join("RemoteDesktop-1.2.0.dmg")
        }

        /// Serves `data` as a fresh temporary file, like `URLSession.download(from:)`.
        fn serving(&self, data: &'static [u8], status: u16) -> Download {
            let scratch = self.scratch.path().to_owned();
            let counter = AtomicUsize::new(0);
            Box::new(move |_| {
                let file = scratch.join(format!("dl-{}", counter.fetch_add(1, Ordering::SeqCst)));
                fs::write(&file, data).map_err(|e| TransportError::Failed(e.to_string()))?;
                Ok((file, status))
            })
        }
    }

    fn sha256(data: &[u8]) -> String {
        hex(&Sha256::digest(data))
    }

    /// `shasum -a 256` output for `data`.
    fn checksum_fetch(data: &[u8]) -> Fetch {
        stub_fetch(200, &format!("{}  RemoteDesktop-1.2.0.dmg\n", sha256(data)))
    }

    #[test]
    fn saves_verified_image() {
        let f = Fixture::new();
        let downloader = UpdateDownloader::new(checksum_fetch(IMAGE), f.serving(IMAGE, 200));
        let saved = downloader.download(&f.release, &f.dir()).unwrap();
        assert_eq!(saved, f.destination());
        assert_eq!(fs::read(&saved).unwrap(), IMAGE);
        assert_eq!(
            fs::read_dir(f.scratch.path()).unwrap().count(),
            0,
            "temp file left"
        );
    }

    #[test]
    fn rejects_corrupted_image() {
        let f = Fixture::new();
        let downloader = UpdateDownloader::new(checksum_fetch(IMAGE), f.serving(b"tampered", 200));
        assert_eq!(
            downloader.download(&f.release, &f.dir()),
            Err(UpdateError::ChecksumMismatch)
        );
        assert!(!f.destination().exists());
        assert_eq!(
            fs::read_dir(f.scratch.path()).unwrap().count(),
            0,
            "temp file left"
        );
    }

    #[test]
    fn reuses_verified_copy() {
        let f = Fixture::new();
        fs::create_dir_all(f.dir()).unwrap();
        fs::write(f.destination(), IMAGE).unwrap();
        let downloader = UpdateDownloader::new(
            checksum_fetch(IMAGE),
            Box::new(|_| panic!("a verified copy must not be downloaded again")),
        );
        assert_eq!(downloader.download(&f.release, &f.dir()), Ok(f.destination()));
    }

    #[test]
    fn replaces_stale_copy() {
        let f = Fixture::new();
        fs::create_dir_all(f.dir()).unwrap();
        fs::write(f.destination(), b"older download").unwrap();
        let downloader = UpdateDownloader::new(checksum_fetch(IMAGE), f.serving(IMAGE, 200));
        downloader.download(&f.release, &f.dir()).unwrap();
        assert_eq!(fs::read(f.destination()).unwrap(), IMAGE);
    }

    #[test]
    fn release_without_image_has_no_download() {
        let f = Fixture::new();
        let bare = UpdateRelease {
            dmg_url: None,
            checksum_url: None,
            ..f.release.clone()
        };
        let downloader = UpdateDownloader::new(stub_fetch(200, ""), f.serving(IMAGE, 200));
        assert_eq!(downloader.download(&bare, &f.dir()), Err(UpdateError::NoDownload));
    }

    #[test]
    fn download_http_error_is_reported() {
        let f = Fixture::new();
        let downloader = UpdateDownloader::new(checksum_fetch(IMAGE), f.serving(IMAGE, 404));
        assert_eq!(
            downloader.download(&f.release, &f.dir()),
            Err(UpdateError::BadResponse)
        );
        assert_eq!(
            fs::read_dir(f.scratch.path()).unwrap().count(),
            0,
            "temp file left"
        );
    }

    #[test]
    fn download_transport_error_is_offline() {
        let f = Fixture::new();
        let downloader = UpdateDownloader::new(
            checksum_fetch(IMAGE),
            Box::new(|_| Err(TransportError::Failed("timed out".into()))),
        );
        assert_eq!(
            downloader.download(&f.release, &f.dir()),
            Err(UpdateError::Offline)
        );
    }

    #[test]
    fn parses_checksum_file() {
        for (text, hash) in [
            (
                format!("{}  RemoteDesktop-1.2.0.dmg\n", "a".repeat(64)),
                "a".repeat(64),
            ),
            ("AB".repeat(32), "ab".repeat(32)),
        ] {
            assert_eq!(checksum_from_file(text.as_bytes()), Ok(hash), "{text:?}");
        }
    }

    #[test]
    fn rejects_bad_checksum_file() {
        for text in [String::new(), "abc  file.dmg".to_owned(), "z".repeat(64)] {
            assert_eq!(
                checksum_from_file(text.as_bytes()),
                Err(UpdateError::BadResponse),
                "{text:?}"
            );
        }
    }
}
