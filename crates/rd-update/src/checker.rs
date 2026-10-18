//! The GitHub release check (Swift `UpdateChecker`, `UpdateRelease`, `UpdateError`).
//!
//! The transport is injected ([`Fetch`]), so the check is tested without a network and the crate needs no
//! HTTP client of its own.

use std::time::Duration;

use serde::Deserialize;

use crate::AppVersion;

/// The releases endpoint the Swift app reads. The Rust app's own updates use the Tauri updater instead.
pub const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/LovaszCC/RemoteDesktop/releases/latest";

/// A GET request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// The URL.
    pub url: String,
    /// Extra request headers.
    pub headers: Vec<(String, String)>,
    /// Give up after this long.
    pub timeout: Duration,
}

impl HttpRequest {
    /// A GET of `url` with the 15 s timeout the Swift check uses.
    #[must_use]
    pub fn get(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            headers: Vec::new(),
            timeout: Duration::from_secs(15),
        }
    }

    /// The value of header `name` (case-insensitive).
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A response: status code and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// The body.
    pub body: Vec<u8>,
}

/// Why a request didn't produce a response.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// The caller cancelled it; not reported as offline.
    #[error("cancelled")]
    Cancelled,
    /// No connection, DNS, TLS, timeout, ….
    #[error("transport failed: {0}")]
    Failed(String),
}

/// Performs one GET.
pub type Fetch = Box<dyn Fn(&HttpRequest) -> Result<HttpResponse, TransportError> + Send + Sync>;

/// A failed check or download.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpdateError {
    /// GitHub couldn't be reached.
    #[error("Couldn't reach GitHub. Check your internet connection.")]
    Offline,
    /// HTTP 403 or 429.
    #[error("GitHub is limiting requests right now. Try again later.")]
    RateLimited,
    /// Any other status, or a body that isn't a release.
    #[error("GitHub sent an unexpected response.")]
    BadResponse,
    /// The release has no disk image (or no checksum for it).
    #[error("This release has no downloadable disk image.")]
    NoDownload,
    /// The download doesn't match its `.sha256`.
    #[error("The download is corrupted. Try again.")]
    ChecksumMismatch,
    /// The caller cancelled the request.
    #[error("The request was cancelled.")]
    Cancelled,
    /// The downloaded file couldn't be saved.
    #[error("The download couldn't be saved: {0}")]
    Io(String),
}

impl From<TransportError> for UpdateError {
    fn from(error: TransportError) -> Self {
        match error {
            TransportError::Cancelled => Self::Cancelled,
            TransportError::Failed(_) => Self::Offline,
        }
    }
}

/// Maps an HTTP status to success or an [`UpdateError`].
///
/// # Errors
/// [`UpdateError::RateLimited`] for 403/429, [`UpdateError::BadResponse`] for any other non-2xx.
pub fn check_status(status: u16) -> Result<(), UpdateError> {
    match status {
        200..=299 => Ok(()),
        403 | 429 => Err(UpdateError::RateLimited),
        _ => Err(UpdateError::BadResponse),
    }
}

/// The body of a successful request; transport and HTTP failures become [`UpdateError`]s.
///
/// # Errors
/// See [`check_status`] and [`UpdateError::Offline`] / [`UpdateError::Cancelled`].
pub fn fetch_data(fetch: &Fetch, request: &HttpRequest) -> Result<Vec<u8>, UpdateError> {
    let response = fetch(request)?;
    check_status(response.status)?;
    Ok(response.body)
}

/// The parts of a GitHub release the update check needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRelease {
    /// The release's version (from its tag).
    pub version: AppVersion,
    /// Release notes (Markdown).
    pub notes: String,
    /// The release page.
    pub page_url: String,
    /// The `.dmg` asset.
    pub dmg_url: Option<String>,
    /// The `.dmg.sha256` asset.
    pub checksum_url: Option<String>,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    body: Option<String>,
    html_url: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

impl UpdateRelease {
    /// Decodes a `GET /repos/{owner}/{repo}/releases/latest` response.
    ///
    /// # Errors
    /// [`UpdateError::BadResponse`] when it isn't a release or the tag isn't a version.
    pub fn decode_github_json(data: &[u8]) -> Result<Self, UpdateError> {
        let release: GitHubRelease = serde_json::from_slice(data).map_err(|_| UpdateError::BadResponse)?;
        let version = AppVersion::parse(&release.tag_name).ok_or(UpdateError::BadResponse)?;
        let asset = |suffix: &str| {
            release
                .assets
                .iter()
                .find(|a| a.name.ends_with(suffix))
                .map(|a| a.browser_download_url.clone())
        };
        Ok(Self {
            dmg_url: asset(".dmg"),
            checksum_url: asset(".dmg.sha256"),
            version,
            notes: release.body.unwrap_or_default(),
            page_url: release.html_url,
        })
    }
}

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheckResult {
    /// A newer release.
    Available(UpdateRelease),
    /// Carries the running version as shown to the user.
    UpToDate(String),
}

/// Asks GitHub for the latest published release (drafts and prereleases excluded) and compares it with the
/// running version.
pub struct UpdateChecker {
    current_version: String,
    fetch: Fetch,
}

impl UpdateChecker {
    /// A checker for the running `current_version`.
    #[must_use]
    pub fn new(current_version: impl Into<String>, fetch: Fetch) -> Self {
        Self {
            current_version: current_version.into(),
            fetch,
        }
    }

    /// Fetches the latest release and compares versions. A running version that doesn't parse (dev builds)
    /// is never outdated.
    ///
    /// # Errors
    /// [`UpdateError::Offline`], [`UpdateError::RateLimited`], [`UpdateError::BadResponse`] or
    /// [`UpdateError::Cancelled`].
    pub fn check(&self) -> Result<UpdateCheckResult, UpdateError> {
        let mut request = HttpRequest::get(LATEST_RELEASE_URL);
        request
            .headers
            .push(("Accept".to_owned(), "application/vnd.github+json".to_owned()));
        let release = UpdateRelease::decode_github_json(&fetch_data(&self.fetch, &request)?)?;
        match AppVersion::parse(&self.current_version) {
            Some(current) if release.version > current => Ok(UpdateCheckResult::Available(release)),
            _ => Ok(UpdateCheckResult::UpToDate(self.current_version.clone())),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Stubbed HTTP: one status code and body for every request.
    pub(crate) fn stub_fetch(status: u16, body: &str) -> Fetch {
        let body = body.as_bytes().to_vec();
        Box::new(move |_| {
            Ok(HttpResponse {
                status,
                body: body.clone(),
            })
        })
    }

    /// A `releases/latest` response shaped like GitHub's.
    fn release_json(tag: &str, with_assets: bool) -> String {
        let base = format!("https://github.com/LovaszCC/RemoteDesktop/releases/download/{tag}");
        let version = tag.strip_prefix('v').unwrap_or(tag);
        let assets = if with_assets {
            format!(
                r#"[{{"name": "RemoteDesktop-{version}.dmg", "browser_download_url": "{base}/RemoteDesktop-{version}.dmg"}},
                   {{"name": "RemoteDesktop-{version}.dmg.sha256", "browser_download_url": "{base}/RemoteDesktop-{version}.dmg.sha256"}}]"#
            )
        } else {
            "[]".to_owned()
        };
        format!(
            r###"{{"tag_name": "{tag}", "name": "RemoteDesktop {version}", "draft": false, "prerelease": false,
                "html_url": "https://github.com/LovaszCC/RemoteDesktop/releases/tag/{tag}",
                "body": "## What's Changed\n* Fix launch crash", "assets": {assets}}}"###
        )
    }

    fn checker(current: &str, fetch: Fetch) -> UpdateChecker {
        UpdateChecker::new(current, fetch)
    }

    fn last_path_component(url: Option<&String>) -> Option<&str> {
        url.and_then(|u| u.rsplit('/').next())
    }

    #[test]
    fn newer_release_is_available() {
        let result = checker("1.1.1", stub_fetch(200, &release_json("v1.2.0", true)))
            .check()
            .unwrap();
        let UpdateCheckResult::Available(release) = result else {
            panic!("expected available, got {result:?}");
        };
        assert_eq!(release.version, AppVersion::parse("1.2.0").unwrap());
        assert_eq!(release.notes, "## What's Changed\n* Fix launch crash");
        assert_eq!(
            release.page_url,
            "https://github.com/LovaszCC/RemoteDesktop/releases/tag/v1.2.0"
        );
        assert_eq!(
            last_path_component(release.dmg_url.as_ref()),
            Some("RemoteDesktop-1.2.0.dmg")
        );
        assert_eq!(
            last_path_component(release.checksum_url.as_ref()),
            Some("RemoteDesktop-1.2.0.dmg.sha256")
        );
    }

    #[test]
    fn not_newer_is_up_to_date() {
        for current in ["1.2.0", "1.2", "1.3.0", "0.0.0-dev.12", ""] {
            let result = checker(current, stub_fetch(200, &release_json("v1.2.0", true)))
                .check()
                .unwrap();
            assert_eq!(
                result,
                UpdateCheckResult::UpToDate(current.to_owned()),
                "{current:?}"
            );
        }
    }

    #[test]
    fn release_without_assets_has_no_download() {
        let result = checker("1.0.0", stub_fetch(200, &release_json("v1.2.0", false)))
            .check()
            .unwrap();
        let UpdateCheckResult::Available(release) = result else {
            panic!("expected available");
        };
        assert_eq!(release.dmg_url, None);
        assert_eq!(release.checksum_url, None);
    }

    #[test]
    fn sends_github_json_request() {
        let body = release_json("v1.0.0", true);
        let fetch: Fetch = Box::new(move |request| {
            assert_eq!(request.url, LATEST_RELEASE_URL);
            assert_eq!(request.header("accept"), Some("application/vnd.github+json"));
            assert_eq!(request.timeout, Duration::from_secs(15));
            Ok(HttpResponse {
                status: 200,
                body: body.as_bytes().to_vec(),
            })
        });
        checker("1.0.0", fetch).check().unwrap();
    }

    #[test]
    fn http_errors() {
        for (status, error) in [
            (403, UpdateError::RateLimited),
            (429, UpdateError::RateLimited),
            (404, UpdateError::BadResponse),
            (500, UpdateError::BadResponse),
        ] {
            assert_eq!(
                checker("1.0.0", stub_fetch(status, "")).check(),
                Err(error),
                "{status}"
            );
        }
    }

    #[test]
    fn unexpected_body_is_bad_response() {
        for body in [
            "not json",
            r#"{"tag_name": "nightly", "html_url": "https://x", "assets": []}"#,
            "{}",
        ] {
            assert_eq!(
                checker("1.0.0", stub_fetch(200, body)).check(),
                Err(UpdateError::BadResponse),
                "{body:?}"
            );
        }
    }

    #[test]
    fn transport_error_is_offline() {
        let fetch: Fetch = Box::new(|_| Err(TransportError::Failed("not connected".into())));
        assert_eq!(checker("1.0.0", fetch).check(), Err(UpdateError::Offline));
    }

    #[test]
    fn cancellation_is_not_reported_as_offline() {
        let fetch: Fetch = Box::new(|_| Err(TransportError::Cancelled));
        assert_eq!(checker("1.0.0", fetch).check(), Err(UpdateError::Cancelled));
    }

    #[test]
    fn errors_have_messages() {
        for error in [
            UpdateError::Offline,
            UpdateError::RateLimited,
            UpdateError::BadResponse,
            UpdateError::NoDownload,
            UpdateError::ChecksumMismatch,
        ] {
            assert!(!error.to_string().is_empty(), "{error:?}");
        }
    }
}
