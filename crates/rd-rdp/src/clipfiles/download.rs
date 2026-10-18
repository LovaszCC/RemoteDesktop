//! Downloads a server file list into a fresh folder, one request at a time (Swift
//! `RDPClipboardDownloader`). Writes only below that folder, never following links and never replacing
//! anything, stamps the modification time and marks the files as downloaded (macOS quarantine, Windows
//! Mark of the Web), so the OS checks executables that came from the remote machine.
//!
//! The downloader is a state machine without threads: the session loop sends each [`FileRequest`] it
//! returns, feeds the server's answers to [`Downloader::receive`] and calls [`Downloader::check_stall`]
//! now and then (the 30 s watchdog).
//!
//! Safe creation:
//! - Unix: everything is opened relative to the job folder's descriptor (`mkdirat`/`openat` with
//!   `O_NOFOLLOW`, files with `O_CREAT|O_EXCL`), like the Swift app.
//! - Windows: there is no `openat`; each folder is created or checked (`symlink_metadata`: a folder, not a
//!   reparse point) on the way down, and files are created with `CREATE_NEW` and
//!   `FILE_FLAG_OPEN_REPARSE_POINT`. Device names (`CON`, `NUL`, `COM1` …) are refused. Only this
//!   process writes into the fresh job folder, so nothing can swap a folder for a link meanwhile.

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::file_list::{self, FileRange, RemoteFile};

/// The error of a cancelled download.
pub const CANCELLED: &str = "cancelled";

/// [MS-RDPECLIP] `FILECONTENTS_SIZE` / `FILECONTENTS_RANGE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentsKind {
    /// The file's size (8 bytes).
    Size,
    /// A range of its bytes.
    Range,
}

/// A File Contents Request to send to the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileRequest {
    /// Matches the response to the request.
    pub stream_id: u32,
    /// The entry in the server's list.
    pub index: u32,
    /// Size or range.
    pub kind: ContentsKind,
    /// The range (`{0, 0, 8}` for a size request).
    pub range: FileRange,
}

/// What the session does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Send this request.
    Request(FileRequest),
    /// The download ended: the folder holding the files (paths as in the list), or an error message
    /// ([`CANCELLED`] when cancelled). Partial files are removed on error.
    Finished(Result<PathBuf, String>),
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    stream_id: u32,
    kind: ContentsKind,
    length: u32,
}

#[derive(Debug)]
struct Job {
    files: Vec<RemoteFile>,
    folder: PathBuf,
    dir: fsops::Dir,
    index: usize,
    size: u64,
    offset: u64,
    file: Option<File>,
    pending: Option<Pending>,
    last_progress: Instant,
}

/// Downloads one file list at a time into `<root>/<job>`.
#[derive(Debug)]
pub struct Downloader {
    root: PathBuf,
    chunk_size: u32,
    allow_huge_files: bool,
    stall_timeout: Duration,
    next_stream_id: u32,
    job: Option<Job>,
    bytes_done: u64,
    bytes_total: u64,
}

impl Downloader {
    /// `root` is created (0700) when a download starts; each job gets its own `<root>/<uuid>`.
    #[must_use]
    pub fn new(root: PathBuf, chunk_size: u32, allow_huge_files: bool, stall_timeout: Duration) -> Self {
        Self {
            root,
            chunk_size: chunk_size.max(1),
            allow_huge_files,
            stall_timeout,
            next_stream_id: 1,
            job: None,
            bytes_done: 0,
            bytes_total: 0,
        }
    }

    /// Whether files over 4 GB may be downloaded (the server supports huge files).
    pub fn set_allow_huge_files(&mut self, allow: bool) {
        self.allow_huge_files = allow;
    }

    /// The folder holding the jobs.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A download is running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }

    /// Bytes written and expected in the running download (0 / 0 when idle; expected grows when sizes
    /// are asked).
    #[must_use]
    pub fn progress(&self) -> (u64, u64) {
        (self.bytes_done, self.bytes_total)
    }

    /// The running download's list.
    #[must_use]
    pub fn files(&self) -> Option<&[RemoteFile]> {
        self.job.as_ref().map(|job| job.files.as_slice())
    }

    fn too_big(file: &RemoteFile) -> String {
        format!(
            "{} is larger than 4 GB, which this server does not support",
            file.path
        )
    }

    /// Starts downloading `files`, dropping any running download (its partial files are removed).
    pub fn start(&mut self, files: Vec<RemoteFile>) -> Step {
        let _ = self.cancel();
        self.bytes_done = 0;
        self.bytes_total = 0;
        let mut total = 0u64;
        for file in &files {
            if !self.allow_huge_files && file.size > u64::from(u32::MAX) {
                return Step::Finished(Err(Self::too_big(file)));
            }
            total = total.saturating_add(file.size);
        }
        if let Err(e) = fsops::create_private_dir_all(&self.root) {
            return Step::Finished(Err(e.to_string()));
        }
        if fsops::available_space(&self.root).is_some_and(|available| available < total) {
            return Step::Finished(Err("not enough free disk space".into()));
        }
        let folder = self.root.join(rd_core::new_id());
        let dir = match fsops::create_job_dir(&folder) {
            Ok(dir) => dir,
            Err(e) => return Step::Finished(Err(format!("cannot create download folder ({e})"))),
        };
        self.bytes_total = total;
        self.job = Some(Job {
            files,
            folder,
            dir,
            index: 0,
            size: 0,
            offset: 0,
            file: None,
            pending: None,
            last_progress: Instant::now(),
        });
        self.start_entry()
    }

    /// Stops the running download, removes its partial files and reports [`CANCELLED`]; `None` when
    /// idle.
    pub fn cancel(&mut self) -> Option<Step> {
        self.job.as_ref()?;
        Some(self.finish(Some(CANCELLED.into())))
    }

    /// The watchdog: fails the download when the server sent nothing for the stall timeout.
    pub fn check_stall(&mut self, now: Instant) -> Option<Step> {
        let job = self.job.as_ref()?;
        if now.saturating_duration_since(job.last_progress) > self.stall_timeout {
            Some(self.finish(Some("the server stopped sending the file".into())))
        } else {
            None
        }
    }

    /// A File Contents Response. `None` when it isn't the one awaited (stale, or no download running).
    pub fn receive(&mut self, stream_id: u32, ok: bool, data: &[u8]) -> Option<Step> {
        let job = self.job.as_mut()?;
        let pending = job.pending.filter(|p| p.stream_id == stream_id)?;
        job.pending = None;
        let path = job.files[job.index].path.clone();
        if !ok {
            return Some(self.finish(Some(format!("the server refused to send {path}"))));
        }
        job.last_progress = Instant::now();

        if pending.kind == ContentsKind::Size {
            let Ok(bytes) = <[u8; 8]>::try_from(data) else {
                return Some(self.finish(Some("invalid file size from the server".into())));
            };
            let size = u64::from_le_bytes(bytes);
            if !self.allow_huge_files && size > u64::from(u32::MAX) {
                let message = Self::too_big(&job.files[job.index]);
                return Some(self.finish(Some(message)));
            }
            job.size = size;
            self.bytes_total = self.bytes_total.saturating_add(size);
            return Some(self.request_next_range());
        }

        if data.is_empty() || data.len() > pending.length as usize {
            return Some(self.finish(Some(format!("invalid data from the server for {path}"))));
        }
        let written = job.file.as_mut().map(|file| file.write_all(data));
        if let Some(Err(e)) = written {
            return Some(self.finish(Some(format!("cannot write {path} ({e})"))));
        }
        job.offset += data.len() as u64;
        self.bytes_done += data.len() as u64;
        Some(self.request_next_range())
    }

    fn send(&mut self, kind: ContentsKind, range: FileRange) -> Step {
        let stream_id = self.next_stream_id;
        self.next_stream_id = self.next_stream_id.wrapping_add(1).max(1);
        let Some(job) = self.job.as_mut() else {
            return Step::Finished(Err(CANCELLED.into()));
        };
        job.pending = Some(Pending {
            stream_id,
            kind,
            length: range.length,
        });
        Step::Request(FileRequest {
            stream_id,
            index: u32::try_from(job.index).unwrap_or(u32::MAX),
            kind,
            range,
        })
    }

    fn start_entry(&mut self) -> Step {
        loop {
            let Some(job) = self.job.as_mut() else {
                return Step::Finished(Err(CANCELLED.into()));
            };
            let Some(file) = job.files.get(job.index) else {
                return self.finish(None);
            };
            if file.is_directory {
                if let Err(e) = fsops::create_dir(&job.dir, &file.path) {
                    let message = format!("cannot create {} ({e})", file.path);
                    return self.finish(Some(message));
                }
                job.index += 1;
                continue;
            }
            match fsops::create_file(&job.dir, &file.path) {
                Ok(handle) => job.file = Some(handle),
                Err(e) => {
                    let message = format!("cannot create {} ({e})", file.path);
                    return self.finish(Some(message));
                }
            }
            job.offset = 0;
            if file.has_size {
                job.size = file.size;
                return self.request_next_range();
            }
            return self.send(
                ContentsKind::Size,
                FileRange {
                    position_low: 0,
                    position_high: 0,
                    length: 8,
                },
            );
        }
    }

    fn request_next_range(&mut self) -> Step {
        let Some(job) = self.job.as_mut() else {
            return Step::Finished(Err(CANCELLED.into()));
        };
        let range = file_list::range_for(job.size, job.offset, self.chunk_size);
        if range.length > 0 {
            return self.send(ContentsKind::Range, range);
        }
        if let Some(handle) = job.file.take() {
            let file = &job.files[job.index];
            fsops::mark_downloaded(&handle, &job.dir, &file.path);
            if let Some(modified) = file.modified {
                let _ = handle.set_modified(modified);
            }
        }
        job.index += 1;
        self.start_entry()
    }

    /// Ends the running job: `None` = success, else its folder is removed.
    fn finish(&mut self, error: Option<String>) -> Step {
        let Some(job) = self.job.take() else {
            return Step::Finished(Err(error.unwrap_or_else(|| CANCELLED.into())));
        };
        self.bytes_done = 0;
        self.bytes_total = 0;
        let Job {
            folder, file, dir, ..
        } = job;
        drop(file);
        drop(dir);
        match error {
            None => Step::Finished(Ok(folder)),
            Some(error) => {
                let _ = std::fs::remove_dir_all(&folder);
                Step::Finished(Err(error))
            }
        }
    }
}

impl Drop for Downloader {
    fn drop(&mut self) {
        let _ = self.cancel();
    }
}

#[cfg(unix)]
mod fsops {
    //! `openat`-relative creation below the job folder's descriptor.

    use std::fs::File;
    use std::io;
    use std::os::fd::OwnedFd;
    use std::os::unix::fs::DirBuilderExt as _;
    use std::path::Path;

    use rustix::fs::{CWD, Mode, OFlags};

    pub type Dir = OwnedFd;

    const DIR_FLAGS: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);

    pub fn create_private_dir_all(path: &Path) -> io::Result<()> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }

    pub fn create_job_dir(folder: &Path) -> io::Result<Dir> {
        std::fs::DirBuilder::new().mode(0o700).create(folder)?;
        Ok(rustix::fs::openat(CWD, folder, DIR_FLAGS, Mode::empty())?)
    }

    pub fn available_space(path: &Path) -> Option<u64> {
        let stat = rustix::fs::statvfs(path).ok()?;
        Some(stat.f_bavail.saturating_mul(stat.f_frsize))
    }

    fn mkdir(parent: &OwnedFd, name: &str) -> io::Result<()> {
        match rustix::fs::mkdirat(parent, name, Mode::from_raw_mode(0o755)) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Opens (creating as needed) the folder holding `path`'s last component; returns it and the leaf.
    fn open_parent<'a>(root: &OwnedFd, path: &'a str) -> io::Result<(OwnedFd, &'a str)> {
        let mut parts: Vec<&str> = path.split('/').collect();
        let leaf = parts.pop().unwrap_or_default();
        let mut dir = root.try_clone()?;
        for part in parts {
            mkdir(&dir, part)?;
            dir = rustix::fs::openat(&dir, part, DIR_FLAGS, Mode::empty())?;
        }
        Ok((dir, leaf))
    }

    pub fn create_dir(root: &Dir, path: &str) -> io::Result<()> {
        let (parent, leaf) = open_parent(root, path)?;
        mkdir(&parent, leaf)
    }

    pub fn create_file(root: &Dir, path: &str) -> io::Result<File> {
        let (parent, leaf) = open_parent(root, path)?;
        let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        Ok(File::from(rustix::fs::openat(
            &parent,
            leaf,
            flags,
            Mode::from_raw_mode(0o644),
        )?))
    }

    /// macOS: the quarantine attribute, so Gatekeeper checks what came from the remote machine. Linux has
    /// no equivalent.
    #[cfg(target_os = "macos")]
    pub fn mark_downloaded(file: &File, _root: &Dir, _path: &str) {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let value = format!("0081;{seconds:08x};RemoteDesktop;");
        let _ = rustix::fs::fsetxattr(
            file,
            "com.apple.quarantine",
            value.as_bytes(),
            rustix::fs::XattrFlags::empty(),
        );
    }

    #[cfg(not(target_os = "macos"))]
    pub fn mark_downloaded(_file: &File, _root: &Dir, _path: &str) {}
}

#[cfg(windows)]
mod fsops {
    //! Path-based creation with checks on the way down (no `openat` on Windows).

    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::path::{Path, PathBuf};

    pub type Dir = PathBuf;

    /// `FILE_FLAG_OPEN_REPARSE_POINT`: open a link itself rather than its target.
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

    pub fn create_private_dir_all(path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    pub fn create_job_dir(folder: &Path) -> io::Result<Dir> {
        std::fs::create_dir(folder)?;
        Ok(folder.to_path_buf())
    }

    pub fn available_space(_path: &Path) -> Option<u64> {
        None
    }

    fn checked(component: &str) -> io::Result<&str> {
        let stem = component.split('.').next().unwrap_or_default();
        if crate::clipfiles::file_list::is_windows_device_name(component)
            || crate::clipfiles::file_list::is_windows_device_name(stem)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "reserved device name",
            ));
        }
        Ok(component)
    }

    fn mkdir(path: &Path) -> io::Result<()> {
        match std::fs::create_dir(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = std::fs::symlink_metadata(path)?;
                if metadata.is_dir() && !metadata.file_type().is_symlink() {
                    Ok(())
                } else {
                    Err(io::Error::new(io::ErrorKind::AlreadyExists, "not a folder"))
                }
            }
            Err(e) => Err(e),
        }
    }

    fn open_parent(root: &Path, path: &str) -> io::Result<PathBuf> {
        let mut parts: Vec<&str> = path.split('/').collect();
        let leaf = parts.pop().unwrap_or_default();
        let mut dir = root.to_path_buf();
        for part in parts {
            dir.push(checked(part)?);
            mkdir(&dir)?;
        }
        Ok(dir.join(checked(leaf)?))
    }

    pub fn create_dir(root: &Dir, path: &str) -> io::Result<()> {
        mkdir(&open_parent(root, path)?)
    }

    pub fn create_file(root: &Dir, path: &str) -> io::Result<File> {
        let target = open_parent(root, path)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(target)
    }

    /// The Mark of the Web: a `Zone.Identifier` stream with the Internet zone. Written before the
    /// modification time is set (writing a stream touches it).
    pub fn mark_downloaded(_file: &File, root: &Dir, path: &str) {
        let target = root.join(path.replace('/', "\\"));
        let mut stream = target.into_os_string();
        stream.push(":Zone.Identifier");
        let _ = std::fs::write(stream, "[ZoneTransfer]\r\nZoneId=3\r\n");
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Fault {
        None,
        Refuse,
        Oversize,
        WrongStream,
        Silent,
    }

    /// Serves File Contents Requests from in-memory files, optionally misbehaving.
    struct FakeServer {
        contents: Vec<Vec<u8>>,
        fault: Fault,
        requests: Vec<FileRequest>,
    }

    impl FakeServer {
        fn new(contents: &[&[u8]], fault: Fault) -> Self {
            Self {
                contents: contents.iter().map(|c| c.to_vec()).collect(),
                fault,
                requests: Vec::new(),
            }
        }

        /// The answer to `request`: `(stream, ok, data)`, or nothing.
        fn answer(&mut self, request: FileRequest) -> Option<(u32, bool, Vec<u8>)> {
            self.requests.push(request);
            let file = &self.contents[request.index as usize];
            match self.fault {
                Fault::Silent => return None,
                Fault::Refuse => return Some((request.stream_id, false, Vec::new())),
                Fault::WrongStream => return Some((request.stream_id + 100, true, vec![1])),
                Fault::None | Fault::Oversize => {}
            }
            if request.kind == ContentsKind::Size {
                return Some((
                    request.stream_id,
                    true,
                    (file.len() as u64).to_le_bytes().to_vec(),
                ));
            }
            let offset = usize::try_from(request.range.position()).unwrap();
            let end = file.len().min(offset + request.range.length as usize);
            let mut chunk = file[offset..end].to_vec();
            if self.fault == Fault::Oversize {
                chunk.extend_from_slice(&[0, 0]);
            }
            Some((request.stream_id, true, chunk))
        }
    }

    fn file(path: &str, size: Option<u64>) -> RemoteFile {
        RemoteFile {
            path: path.into(),
            is_directory: false,
            has_size: size.is_some(),
            size: size.unwrap_or(0),
            modified: None,
        }
    }

    fn folder(path: &str) -> RemoteFile {
        RemoteFile {
            is_directory: true,
            ..file(path, Some(0))
        }
    }

    fn downloader(root: &Path, stall: Duration) -> Downloader {
        Downloader::new(root.to_path_buf(), 3, false, stall)
    }

    /// Runs a download against `server`, polling the watchdog while it waits.
    fn run(
        downloader: &mut Downloader,
        files: Vec<RemoteFile>,
        server: &mut FakeServer,
    ) -> Result<PathBuf, String> {
        let mut step = downloader.start(files);
        loop {
            match step {
                Step::Finished(result) => return result,
                Step::Request(request) => {
                    let reply = server
                        .answer(request)
                        .and_then(|(stream, ok, data)| downloader.receive(stream, ok, &data));
                    step = match reply {
                        Some(next) => next,
                        None => loop {
                            std::thread::sleep(Duration::from_millis(50));
                            if let Some(next) = downloader.check_stall(Instant::now()) {
                                break next;
                            }
                        },
                    };
                }
            }
        }
    }

    #[test]
    fn downloads_folders_and_files_in_chunks() {
        let root = tempfile::tempdir().unwrap();
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let mut server = FakeServer::new(&[b"", b"hello world", b"x"], Fault::None);
        let mut hello = file("dir/hello.txt", Some(11));
        hello.modified = Some(modified);
        let base = run(
            &mut downloader(root.path(), Duration::from_secs(5)),
            vec![folder("dir"), hello, file("sized-by-request.txt", None)],
            &mut server,
        )
        .unwrap();
        let hello_path = base.join("dir/hello.txt");
        assert_eq!(std::fs::read_to_string(&hello_path).unwrap(), "hello world");
        assert_eq!(
            std::fs::read_to_string(base.join("sized-by-request.txt")).unwrap(),
            "x"
        );
        assert_eq!(
            std::fs::metadata(&hello_path).unwrap().modified().unwrap(),
            modified
        );
        #[cfg(target_os = "macos")]
        assert!(rustix::fs::getxattr(&hello_path, "com.apple.quarantine", &mut [0u8; 0][..]).unwrap() > 0);
        #[cfg(windows)]
        assert!(
            std::fs::read_to_string(format!("{}:Zone.Identifier", hello_path.display()))
                .unwrap()
                .contains("ZoneId=3")
        );
        // 11 bytes in chunks of 3 → 4 range requests; the unsized file asks its size first.
        assert_eq!(server.requests.iter().filter(|r| r.index == 1).count(), 4);
        assert_eq!(
            server.requests.iter().find(|r| r.index == 2).map(|r| r.kind),
            Some(ContentsKind::Size)
        );
    }

    #[test]
    fn refused_or_broken_responses_fail_and_leave_nothing() {
        let root = tempfile::tempdir().unwrap();
        for fault in [Fault::Refuse, Fault::Oversize] {
            let mut server = FakeServer::new(&[b"data"], fault);
            let result = run(
                &mut downloader(root.path(), Duration::from_secs(5)),
                vec![file("a.txt", Some(4))],
                &mut server,
            );
            assert!(result.is_err());
        }
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn stale_streams_are_ignored_and_the_watchdog_fires() {
        let root = tempfile::tempdir().unwrap();
        let mut server = FakeServer::new(&[b"data"], Fault::WrongStream);
        let error = run(
            &mut downloader(root.path(), Duration::from_millis(400)),
            vec![file("a.txt", Some(4))],
            &mut server,
        )
        .unwrap_err();
        assert!(error.contains("stopped"), "{error}");
    }

    #[test]
    fn cancel_stops_the_download() {
        let root = tempfile::tempdir().unwrap();
        let mut server = FakeServer::new(&[b"data"], Fault::Silent);
        let mut downloader = downloader(root.path(), Duration::from_secs(30));
        let Step::Request(request) = downloader.start(vec![file("a.txt", Some(4))]) else {
            panic!("expected a request");
        };
        assert!(server.answer(request).is_none());
        assert_eq!(downloader.cancel(), Some(Step::Finished(Err(CANCELLED.into()))));
        assert!(!downloader.is_running());
    }

    #[test]
    fn huge_files_need_server_support() {
        let root = tempfile::tempdir().unwrap();
        let mut server = FakeServer::new(&[b""], Fault::None);
        let error = run(
            &mut downloader(root.path(), Duration::from_secs(5)),
            vec![file("big.iso", Some(5 << 30))],
            &mut server,
        )
        .unwrap_err();
        assert!(error.contains("4 GB"), "{error}");
        assert!(server.requests.is_empty());
    }

    #[test]
    fn writes_only_into_a_fresh_folder_below_the_root() {
        let root = tempfile::tempdir().unwrap();
        let mut server = FakeServer::new(&[b"", b"x"], Fault::None);
        let mut downloader = downloader(root.path(), Duration::from_secs(5));
        let list = || vec![folder("dir"), file("dir/x", Some(1))];
        let a = run(&mut downloader, list(), &mut server).unwrap();
        let b = run(&mut downloader, list(), &mut server).unwrap();
        assert_ne!(a, b);
        for folder in [a, b] {
            assert_eq!(folder.parent(), Some(root.path()));
        }
    }
}
