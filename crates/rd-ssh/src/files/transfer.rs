//! One transfer of a file browser (Swift `TransferQueue` + `RunningTransfer`): its items run one after the
//! other. For each item:
//! 1. the destination is checked; an existing one asks Replace/Skip through the observer, unless "Apply
//!    to all" already decided;
//! 2. the size is measured (`du` remotely, the files locally) for the progress bar;
//! 3. the item travels as one tar stream (remote `tar c[z]f -` → [`archive::unpack`], or
//!    [`archive::pack`] → remote `tar x[z]of -`), or over SFTP for a single file on servers without `tar`;
//! 4. a cancelled or failed copy into a destination that didn't exist before is removed.
//!
//! Progress counts uncompressed tar bytes (or file bytes over SFTP) against the measured size.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use russh::ChannelMsg;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::path as rpath;
use super::plan::{self, ConflictChoice, ConflictPolicy, Direction};
use super::remote::RemoteFiles;
use super::{archive, local};

/// How often progress is reported while bytes flow.
pub const PROGRESS_EVERY: Duration = Duration::from_millis(200);
const CHUNK: usize = 64 * 1024;

/// What to copy: `sources` are full paths on the source side (remote POSIX paths for a download, local
/// paths for an upload), `destination` the folder on the other side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferRequest {
    /// Download or upload.
    pub direction: Direction,
    /// The items to copy.
    pub sources: Vec<String>,
    /// The folder to copy into.
    pub destination: String,
    /// gzip the tar stream.
    pub compress: bool,
}

/// Where a transfer is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// The item being copied (0-based).
    pub index: usize,
    /// How many items the transfer has.
    pub count: usize,
    /// Its name.
    pub name: String,
    /// Bytes copied so far.
    pub bytes_done: u64,
    /// The item's size, when known.
    pub bytes_total: Option<u64>,
}

/// "Already exists": answer with the choice and "Apply to all".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictQuestion {
    /// The existing item's name.
    pub name: String,
    /// The source is a folder (replacing merges).
    pub is_folder: bool,
    /// The transfer's direction.
    pub direction: Direction,
}

/// The user's answer: the choice and whether it applies to the rest of the transfer.
pub type ConflictAnswer = (ConflictChoice, bool);

/// Receives a transfer's progress and asks about conflicts.
pub trait TransferObserver: Send + Sync {
    /// The current item's progress (throttled to [`PROGRESS_EVERY`]).
    fn progress(&self, progress: &Progress);
    /// Asks about an existing destination; a dropped sender skips.
    fn conflict(&self, question: ConflictQuestion) -> oneshot::Receiver<ConflictAnswer>;
}

/// How one item ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemStatus {
    /// Copied.
    Done,
    /// The destination existed and the user chose Skip.
    Skipped,
    /// Cancelled (also the items after a cancel).
    Cancelled,
    /// Failed; see the error.
    Failed,
}

/// One item's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemReport {
    /// The item's name.
    pub name: String,
    /// How it ended.
    pub status: ItemStatus,
    /// The failure (the remote tar's last error line, or a local error).
    pub error: Option<String>,
}

/// What a transfer did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferReport {
    /// Per item, in order.
    pub items: Vec<ItemReport>,
}

impl TransferReport {
    /// The transfer was cancelled.
    #[must_use]
    pub fn cancelled(&self) -> bool {
        self.items.iter().any(|i| i.status == ItemStatus::Cancelled)
    }

    /// The first failure, if any.
    #[must_use]
    pub fn first_error(&self) -> Option<&str> {
        self.items.iter().find_map(|i| i.error.as_deref())
    }
}

/// Why an item stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stop {
    Cancelled,
    Failed(String),
}

/// The message for folders on a server without `tar`.
pub const FOLDERS_NEED_TAR: &str = "Folders need tar on the server.";
/// The message for a failure without a message.
pub const TRANSFER_FAILED: &str = "The transfer failed.";

/// Runs a transfer to the end (or until `cancel`).
pub async fn run(
    remote: &RemoteFiles,
    request: &TransferRequest,
    observer: &dyn TransferObserver,
    cancel: &CancellationToken,
) -> TransferReport {
    let mut policy = ConflictPolicy::Ask;
    let mut items = Vec::with_capacity(request.sources.len());
    let count = request.sources.len();
    for (index, source) in request.sources.iter().enumerate() {
        let name = match request.direction {
            Direction::Download => rpath::name(source),
            Direction::Upload => Path::new(source)
                .file_name()
                .map_or_else(|| source.clone(), |n| n.to_string_lossy().into_owned()),
        };
        if cancel.is_cancelled() {
            items.push(report(name, Err(Stop::Cancelled)));
            continue;
        }
        let item = Item {
            remote,
            request,
            observer,
            cancel,
            index,
            count,
            name: name.clone(),
            source: source.clone(),
        };
        let result = item.run(&mut policy).await;
        items.push(report(name, result));
    }
    TransferReport { items }
}

fn report(name: String, result: Result<ItemStatus, Stop>) -> ItemReport {
    match result {
        Ok(status) => ItemReport {
            name,
            status,
            error: None,
        },
        Err(Stop::Cancelled) => ItemReport {
            name,
            status: ItemStatus::Cancelled,
            error: None,
        },
        Err(Stop::Failed(message)) => ItemReport {
            name,
            status: ItemStatus::Failed,
            error: Some(if message.is_empty() {
                TRANSFER_FAILED.to_owned()
            } else {
                message
            }),
        },
    }
}

struct Item<'a> {
    remote: &'a RemoteFiles,
    request: &'a TransferRequest,
    observer: &'a dyn TransferObserver,
    cancel: &'a CancellationToken,
    index: usize,
    count: usize,
    name: String,
    source: String,
}

impl Item<'_> {
    fn local_destination(&self) -> PathBuf {
        Path::new(&self.request.destination).join(&self.name)
    }

    fn remote_destination(&self) -> String {
        rpath::join(&self.request.destination, &self.name)
    }

    async fn run(&self, policy: &mut ConflictPolicy) -> Result<ItemStatus, Stop> {
        let (existed, folder) = match self.request.direction {
            Direction::Download => (
                local::exists(&self.local_destination()),
                self.remote.is_directory(&self.source).await,
            ),
            Direction::Upload => (
                self.remote.exists(&self.remote_destination()).await,
                std::fs::symlink_metadata(&self.source).is_ok_and(|m| m.is_dir()),
            ),
        };
        if folder && !self.remote.has_tar() {
            return Err(Stop::Failed(FOLDERS_NEED_TAR.to_owned()));
        }
        if self.ask(policy, existed, folder).await? == ConflictChoice::Skip {
            return Ok(ItemStatus::Skipped);
        }
        let total = match self.request.direction {
            Direction::Download => self.remote.size(&self.source).await,
            Direction::Upload => Some(local::size(Path::new(&self.source))),
        };
        let counter = Arc::new(AtomicU64::new(0));
        let reporter = Reporter {
            item: self,
            counter: Arc::clone(&counter),
            total,
        };
        reporter.report();
        let result = match (self.request.direction, self.remote.has_tar()) {
            (Direction::Download, true) => self.download_tar(&reporter).await,
            (Direction::Upload, true) => self.upload_tar(&reporter).await,
            (Direction::Download, false) => self.download_sftp(&reporter).await,
            (Direction::Upload, false) => self.upload_sftp(&reporter).await,
        };
        reporter.report();
        match result {
            Ok(()) => Ok(ItemStatus::Done),
            Err(stop) => {
                if !existed {
                    match self.request.direction {
                        Direction::Download => local::remove(&self.local_destination()),
                        Direction::Upload => self.remote.remove(&self.remote_destination()).await,
                    }
                }
                Err(stop)
            }
        }
    }

    async fn ask(
        &self,
        policy: &mut ConflictPolicy,
        exists: bool,
        folder: bool,
    ) -> Result<ConflictChoice, Stop> {
        if let Some(choice) = policy.resolve(exists) {
            return Ok(choice);
        }
        let answer = self.observer.conflict(ConflictQuestion {
            name: self.name.clone(),
            is_folder: folder,
            direction: self.request.direction,
        });
        let (choice, apply_to_all) = tokio::select! {
            () = self.cancel.cancelled() => return Err(Stop::Cancelled),
            answer = answer => answer.unwrap_or((ConflictChoice::Skip, false)),
        };
        if apply_to_all {
            *policy = ConflictPolicy::remembering(choice);
        }
        Ok(choice)
    }

    fn source_parts(&self) -> (String, String) {
        match self.request.direction {
            Direction::Download => (rpath::parent(&self.source), self.name.clone()),
            Direction::Upload => (
                Path::new(&self.source)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                self.name.clone(),
            ),
        }
    }

    /// Remote `tar c[z]f -` → (gunzip) → count → unpack.
    async fn download_tar(&self, reporter: &Reporter<'_>) -> Result<(), Stop> {
        let (directory, name) = self.source_parts();
        let compress = self.request.compress;
        let mut channel = self
            .remote
            .client()
            .exec_channel(&plan::download_command(&name, &directory, compress))
            .await
            .map_err(|e| Stop::Failed(e.to_string()))?;
        let (sender, receiver) = mpsc::channel::<Vec<u8>>(16);
        let destination = PathBuf::from(&self.request.destination);
        let counter = Arc::clone(&reporter.counter);
        let mut unpack_task = tokio::task::spawn_blocking(move || {
            let reader = ChannelReader::new(receiver);
            if compress {
                archive::unpack(
                    Counting::new(GzDecoder::new(reader), counter),
                    &destination,
                    &name,
                )
            } else {
                archive::unpack(Counting::new(reader, counter), &destination, &name)
            }
        });
        let mut sender = Some(sender);
        let mut unpacked: Option<io::Result<()>> = None;
        let mut stderr = Vec::new();
        let mut status = None;
        let mut ticker = tokio::time::interval(PROGRESS_EVERY);
        loop {
            tokio::select! {
                () = self.cancel.cancelled() => {
                    let _ = channel.close().await;
                    drop(sender);
                    let _ = unpack_task.await;
                    return Err(Stop::Cancelled);
                }
                _ = ticker.tick() => reporter.report(),
                message = channel.wait() => match message {
                    Some(ChannelMsg::Data { data }) => {
                        if let Some(s) = &sender && s.send(data.to_vec()).await.is_err() {
                            // The unpacker stopped: at the archive's end, or on an error.
                            sender = None;
                            let result = join(&mut unpack_task).await;
                            if let Err(e) = &result {
                                let _ = channel.close().await;
                                return Err(Stop::Failed(e.to_string()));
                            }
                            unpacked = Some(result);
                        }
                    }
                    Some(ChannelMsg::ExtendedData { data, ext: 1 }) => keep_tail(&mut stderr, &data),
                    Some(ChannelMsg::ExitStatus { exit_status }) => status = Some(exit_status),
                    Some(ChannelMsg::Eof) => sender = None,
                    Some(ChannelMsg::Close) | None => break,
                    _ => {}
                },
            }
        }
        drop(sender);
        let unpacked = match unpacked {
            Some(result) => result,
            None => join(&mut unpack_task).await,
        };
        finish(status, &stderr, unpacked)
    }

    /// pack → count → (gzip) → remote `tar x[z]of -`.
    async fn upload_tar(&self, reporter: &Reporter<'_>) -> Result<(), Stop> {
        let (directory, name) = self.source_parts();
        let compress = self.request.compress;
        let channel = self
            .remote
            .client()
            .exec_channel(&plan::upload_command(&self.request.destination, compress))
            .await
            .map_err(|e| Stop::Failed(e.to_string()))?;
        let (mut read_half, write_half) = channel.split();
        let (sender, mut receiver) = mpsc::channel::<Vec<u8>>(16);
        let counter = Arc::clone(&reporter.counter);
        let mut pack_task = tokio::task::spawn_blocking(move || -> io::Result<()> {
            let sink = ChunkWriter::new(sender);
            let directory = Path::new(&directory);
            let mut sink = if compress {
                let encoder = GzEncoder::new(sink, Compression::default());
                archive::pack(Counting::new(encoder, counter), directory, &name)?
                    .into_inner()
                    .finish()?
            } else {
                archive::pack(Counting::new(sink, counter), directory, &name)?.into_inner()
            };
            sink.flush()
        });
        let writer = async {
            while let Some(chunk) = receiver.recv().await {
                if write_half.data_bytes(chunk).await.is_err() {
                    return;
                }
            }
            let _ = write_half.eof().await;
        };
        let reader = async {
            let mut stderr = Vec::new();
            let mut status = None;
            while let Some(message) = read_half.wait().await {
                match message {
                    ChannelMsg::ExtendedData { data, ext: 1 } => keep_tail(&mut stderr, &data),
                    ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                    ChannelMsg::Close => break,
                    _ => {}
                }
            }
            (status, stderr)
        };
        let work = async { tokio::join!(writer, reader).1 };
        tokio::pin!(work);
        let mut ticker = tokio::time::interval(PROGRESS_EVERY);
        let (status, stderr) = loop {
            tokio::select! {
                () = self.cancel.cancelled() => {
                    // Close, then let the remote tar end before the partial copy is removed.
                    let _ = write_half.close().await;
                    let _ = tokio::time::timeout(Duration::from_secs(3), &mut work).await;
                    return Err(Stop::Cancelled);
                }
                _ = ticker.tick() => reporter.report(),
                done = &mut work => break done,
            }
        };
        match join(&mut pack_task).await {
            // A broken pipe only says the remote side stopped reading; its exit status says why.
            Err(e) if e.kind() != io::ErrorKind::BrokenPipe => Err(Stop::Failed(e.to_string())),
            result => finish(status, &stderr, result),
        }
    }

    async fn download_sftp(&self, reporter: &Reporter<'_>) -> Result<(), Stop> {
        let sftp = self
            .remote
            .sftp()
            .ok_or_else(|| Stop::Failed(FOLDERS_NEED_TAR.to_owned()))?;
        let mut remote = sftp
            .open(self.source.clone())
            .await
            .map_err(|e| Stop::Failed(e.to_string()))?;
        let mut file =
            std::fs::File::create(self.local_destination()).map_err(|e| Stop::Failed(e.to_string()))?;
        let mut buffer = vec![0u8; CHUNK];
        let mut last = tokio::time::Instant::now();
        loop {
            let read = tokio::select! {
                () = self.cancel.cancelled() => return Err(Stop::Cancelled),
                read = remote.read(&mut buffer) => read.map_err(|e| Stop::Failed(e.to_string()))?,
            };
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])
                .map_err(|e| Stop::Failed(e.to_string()))?;
            reporter.counter.fetch_add(read as u64, Ordering::Relaxed);
            if last.elapsed() >= PROGRESS_EVERY {
                reporter.report();
                last = tokio::time::Instant::now();
            }
        }
        let _ = remote.shutdown().await;
        Ok(())
    }

    async fn upload_sftp(&self, reporter: &Reporter<'_>) -> Result<(), Stop> {
        let sftp = self
            .remote
            .sftp()
            .ok_or_else(|| Stop::Failed(FOLDERS_NEED_TAR.to_owned()))?;
        let mut file = std::fs::File::open(&self.source).map_err(|e| Stop::Failed(e.to_string()))?;
        let mut remote = sftp
            .create(self.remote_destination())
            .await
            .map_err(|e| Stop::Failed(e.to_string()))?;
        let mut buffer = vec![0u8; CHUNK];
        let mut last = tokio::time::Instant::now();
        loop {
            let read = file.read(&mut buffer).map_err(|e| Stop::Failed(e.to_string()))?;
            if read == 0 {
                break;
            }
            tokio::select! {
                () = self.cancel.cancelled() => return Err(Stop::Cancelled),
                written = remote.write_all(&buffer[..read]) => written.map_err(|e| Stop::Failed(e.to_string()))?,
            }
            reporter.counter.fetch_add(read as u64, Ordering::Relaxed);
            if last.elapsed() >= PROGRESS_EVERY {
                reporter.report();
                last = tokio::time::Instant::now();
            }
        }
        remote.shutdown().await.map_err(|e| Stop::Failed(e.to_string()))
    }
}

/// Reports an item's progress from its byte counter.
struct Reporter<'a> {
    item: &'a Item<'a>,
    counter: Arc<AtomicU64>,
    total: Option<u64>,
}

impl Reporter<'_> {
    fn report(&self) {
        self.item.observer.progress(&Progress {
            index: self.item.index,
            count: self.item.count,
            name: self.item.name.clone(),
            bytes_done: self.counter.load(Ordering::Relaxed),
            bytes_total: self.total,
        });
    }
}

async fn join(handle: &mut tokio::task::JoinHandle<io::Result<()>>) -> io::Result<()> {
    handle
        .await
        .unwrap_or_else(|e| Err(io::Error::other(e.to_string())))
}

/// The remote exit status decides first (its error line is the clearest), then the local side.
fn finish(status: Option<u32>, stderr: &[u8], local: io::Result<()>) -> Result<(), Stop> {
    if status != Some(0) {
        let line = error_line(stderr);
        return Err(Stop::Failed(if line.is_empty() {
            local.err().map(|e| e.to_string()).unwrap_or_default()
        } else {
            line
        }));
    }
    local.map_err(|e| Stop::Failed(e.to_string()))
}

/// The last stderr line that says what went wrong: tar's closing "exiting with failure" summaries
/// (GNU, busybox) are passed over for the line before them.
fn error_line(stderr: &[u8]) -> String {
    const SUMMARIES: [&str; 2] = ["Exiting with failure status", "error exit delayed"];
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines
        .iter()
        .rev()
        .find(|l| !SUMMARIES.iter().any(|s| l.contains(s)))
        .or(lines.last())
        .map_or_else(String::new, |l| (*l).to_owned())
}

/// Keeps the last 8 KiB of stderr.
fn keep_tail(buffer: &mut Vec<u8>, data: &[u8]) {
    buffer.extend_from_slice(data);
    if buffer.len() > 8192 {
        buffer.drain(..buffer.len() - 4096);
    }
}

/// A blocking reader over the chunks an async task receives.
struct ChannelReader {
    receiver: mpsc::Receiver<Vec<u8>>,
    chunk: Vec<u8>,
    offset: usize,
}

impl ChannelReader {
    fn new(receiver: mpsc::Receiver<Vec<u8>>) -> Self {
        Self {
            receiver,
            chunk: Vec::new(),
            offset: 0,
        }
    }
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.offset >= self.chunk.len() {
            match self.receiver.blocking_recv() {
                Some(chunk) => {
                    self.chunk = chunk;
                    self.offset = 0;
                }
                None => return Ok(0),
            }
        }
        let n = buf.len().min(self.chunk.len() - self.offset);
        buf[..n].copy_from_slice(&self.chunk[self.offset..self.offset + n]);
        self.offset += n;
        Ok(n)
    }
}

/// A blocking writer sending [`CHUNK`]-sized pieces to an async task.
struct ChunkWriter {
    sender: mpsc::Sender<Vec<u8>>,
    buffer: Vec<u8>,
}

impl ChunkWriter {
    fn new(sender: mpsc::Sender<Vec<u8>>) -> Self {
        Self {
            sender,
            buffer: Vec::with_capacity(CHUNK),
        }
    }

    fn send(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let chunk = std::mem::replace(&mut self.buffer, Vec::with_capacity(CHUNK));
        self.sender
            .blocking_send(chunk)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "the remote side stopped reading"))
    }
}

impl Write for ChunkWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = buf.len().min(CHUNK - self.buffer.len());
        self.buffer.extend_from_slice(&buf[..n]);
        if self.buffer.len() >= CHUNK {
            self.send()?;
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.send()
    }
}

/// Counts the bytes that pass (the uncompressed tar stream).
struct Counting<T> {
    inner: T,
    counter: Arc<AtomicU64>,
}

impl<T> Counting<T> {
    fn new(inner: T, counter: Arc<AtomicU64>) -> Self {
        Self { inner, counter }
    }

    fn into_inner(self) -> T {
        self.inner
    }
}

impl<T: Read> Read for Counting<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.counter.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

impl<T: Write> Write for Counting<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.counter.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_and_messages() {
        let report = TransferReport {
            items: vec![
                super::report("a".into(), Ok(ItemStatus::Done)),
                super::report("b".into(), Err(Stop::Failed(String::new()))),
                super::report("c".into(), Err(Stop::Cancelled)),
            ],
        };
        assert!(report.cancelled());
        assert_eq!(report.first_error(), Some(TRANSFER_FAILED));
        assert_eq!(report.items[2].status, ItemStatus::Cancelled);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["items"][1]["status"], "failed");
    }

    #[test]
    fn finish_prefers_the_remote_error_line() {
        assert_eq!(finish(Some(0), b"", Ok(())), Ok(()));
        assert_eq!(
            finish(Some(2), b"tar: x: Cannot open: Permission denied\n", Ok(())),
            Err(Stop::Failed("tar: x: Cannot open: Permission denied".into()))
        );
        assert_eq!(
            finish(None, b"", Err(io::Error::other("disk full"))),
            Err(Stop::Failed("disk full".into()))
        );
        assert_eq!(
            finish(Some(0), b"", Err(io::Error::other("unsafe"))),
            Err(Stop::Failed("unsafe".into()))
        );
    }

    #[test]
    fn error_line_skips_tar_summaries() {
        let gnu = b"tar: ./missing: Cannot stat: No such file or directory\ntar: Exiting with failure status due to previous errors\n";
        assert_eq!(
            error_line(gnu),
            "tar: ./missing: Cannot stat: No such file or directory"
        );
        let busybox =
            b"tar: ./missing: No such file or directory\ntar: error exit delayed from previous errors\n";
        assert_eq!(error_line(busybox), "tar: ./missing: No such file or directory");
        assert_eq!(
            error_line(b"tar: error exit delayed from previous errors"),
            "tar: error exit delayed from previous errors"
        );
        assert_eq!(error_line(b""), "");
    }

    #[test]
    fn stderr_tail_is_bounded() {
        let mut buffer = Vec::new();
        keep_tail(&mut buffer, &[b'x'; 10_000]);
        assert_eq!(buffer.len(), 4096);
    }

    #[test]
    fn chunk_pipes_carry_a_tar_stream() {
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(source.path().join("d/sub")).unwrap();
        std::fs::write(source.path().join("d/sub/f.txt"), vec![b'a'; 200_000]).unwrap();
        let (sender, receiver) = mpsc::channel(4);
        let packed = Arc::new(AtomicU64::new(0));
        let unpacked = Arc::new(AtomicU64::new(0));
        let directory = source.path().to_path_buf();
        let pack_count = Arc::clone(&packed);
        let writer = std::thread::spawn(move || {
            let encoder = GzEncoder::new(ChunkWriter::new(sender), Compression::fast());
            let stream = archive::pack(Counting::new(encoder, pack_count), &directory, "d").unwrap();
            stream.into_inner().finish().unwrap().flush().unwrap();
        });
        let destination = tempfile::tempdir().unwrap();
        let reader = Counting::new(
            GzDecoder::new(ChannelReader::new(receiver)),
            Arc::clone(&unpacked),
        );
        archive::unpack(reader, destination.path(), "d").unwrap();
        writer.join().unwrap();
        assert_eq!(
            std::fs::read(destination.path().join("d/sub/f.txt"))
                .unwrap()
                .len(),
            200_000
        );
        let packed = packed.load(Ordering::Relaxed);
        assert!(packed > 200_000, "{packed}");
        assert!(unpacked.load(Ordering::Relaxed) > 200_000);
    }

    #[test]
    fn a_closed_pipe_is_a_broken_pipe() {
        let (sender, receiver) = mpsc::channel(1);
        drop(receiver);
        let mut writer = ChunkWriter::new(sender);
        writer.write_all(&[0u8; 10]).unwrap();
        assert_eq!(writer.flush().unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        writer.flush().unwrap();
    }
}
