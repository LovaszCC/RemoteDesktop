//! Files on the RDP clipboard ([MS-RDPECLIP] file streams), both ways (Swift `RDPClipboard`,
//! `RDPClipboardFileList`, `RDPClipboardDownloader`): the strict parser of the server's file list, the
//! downloader, the local files offered to the server and the limits.

pub mod download;
pub mod file_list;
pub mod upload;

use std::time::Duration;

pub use download::{ContentsKind, Downloader, FileRequest, Step};
pub use file_list::{FileRange, RemoteFile};
pub use upload::LocalFiles;

/// Most entries accepted in a server's file list.
pub const MAX_REMOTE_FILES: usize = 100_000;
/// Most entries offered from the local clipboard (folders count with their contents).
pub const MAX_LOCAL_FILES: usize = 50_000;
/// Bytes asked per `FILECONTENTS_RANGE` request.
pub const CHUNK_SIZE: u32 = 1024 * 1024;
/// A download fails when the server sends nothing for this long.
pub const STALL_TIMEOUT: Duration = Duration::from_secs(30);
