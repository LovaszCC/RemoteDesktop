//! One serial console: the open port, a reader thread that sends its output to a [`Sink`] (and the
//! [`OutputLog`]) under the same flow control as the PTY sessions, and the writer for typed input.
//!
//! When the port goes away (adapter unplugged, read error, sleep) the session reports
//! [`SessionEvent::Exit`] once, with no code; the UI shows "Connection lost" and Reconnect.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rd_core::{SerialFlowControl, SerialParity, SerialStopBits};
use rd_ssh::{FlowControl, SessionEvent, Sink};
use serialport::{DataBits, FlowControl as PortFlow, Parity, SerialPort, StopBits};

use crate::{OutputLog, SerialSettings};

/// How long an idle read waits; also the longest `close` waits for the reader.
const READ_TIMEOUT: Duration = Duration::from_millis(200);
/// Output that keeps coming is gathered this long (or up to [`CHUNK`]) into one message.
const COALESCE: Duration = Duration::from_millis(6);
/// The most output one message carries.
const CHUNK: usize = 64 * 1024;
/// How often the reader checks that the device still exists.
const PRESENCE_INTERVAL: Duration = Duration::from_secs(1);

/// Why a port can't be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// The device doesn't exist (unplugged, renamed).
    Missing,
    /// Another program holds the port.
    Busy,
    /// The user may not open the device (Linux: not in `dialout`).
    PermissionDenied,
    /// The settings are not supported by the device.
    InvalidSettings(String),
    /// Anything else, with the OS message.
    Other(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("the device is not connected"),
            Self::Busy => f.write_str("the port is in use by another program"),
            Self::PermissionDenied => f.write_str("permission denied"),
            Self::InvalidSettings(m) | Self::Other(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for OpenError {}

impl From<serialport::Error> for OpenError {
    fn from(error: serialport::Error) -> Self {
        let text = error.description.to_lowercase();
        match error.kind {
            serialport::ErrorKind::NoDevice | serialport::ErrorKind::Io(io::ErrorKind::NotFound) => {
                Self::Missing
            }
            // Windows refuses a port that is open elsewhere with "Access is denied".
            serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied) if cfg!(windows) => Self::Busy,
            serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied) => Self::PermissionDenied,
            serialport::ErrorKind::InvalidInput => Self::InvalidSettings(error.description),
            _ if text.contains("busy") || text.contains("in use") => Self::Busy,
            _ if text.contains("permission denied") => Self::PermissionDenied,
            _ if text.contains("no such file") || text.contains("not found") => Self::Missing,
            _ => Self::Other(error.description),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn data_bits(bits: u8) -> DataBits {
    match bits {
        5 => DataBits::Five,
        6 => DataBits::Six,
        7 => DataBits::Seven,
        _ => DataBits::Eight,
    }
}

fn parity(parity: SerialParity) -> Parity {
    match parity {
        SerialParity::None => Parity::None,
        SerialParity::Odd => Parity::Odd,
        SerialParity::Even => Parity::Even,
    }
}

fn stop_bits(bits: SerialStopBits) -> StopBits {
    match bits {
        SerialStopBits::One => StopBits::One,
        SerialStopBits::Two => StopBits::Two,
    }
}

fn flow(flow: SerialFlowControl) -> PortFlow {
    match flow {
        SerialFlowControl::None => PortFlow::None,
        SerialFlowControl::Software => PortFlow::Software,
        SerialFlowControl::Hardware => PortFlow::Hardware,
    }
}

fn configure(port: &mut dyn SerialPort, settings: SerialSettings) -> serialport::Result<()> {
    port.set_baud_rate(settings.baud)?;
    port.set_data_bits(data_bits(settings.data_bits))?;
    port.set_parity(parity(settings.parity))?;
    port.set_stop_bits(stop_bits(settings.stop_bits))?;
    port.set_flow_control(flow(settings.flow_control))
}

struct Shared {
    sink: Arc<dyn Sink>,
    log: Arc<OutputLog>,
    flow_control: Option<FlowControl>,
    /// Ended: closed by the app or lost; the exit event is sent at most once.
    ended: AtomicBool,
    writer: Mutex<Option<Box<dyn SerialPort>>>,
    unacked: Mutex<usize>,
    acked: Condvar,
}

impl Shared {
    /// Ends the session; reports the exit when `report` and it hadn't ended yet.
    fn end(&self, report: bool) {
        if self.ended.swap(true, Ordering::SeqCst) {
            return;
        }
        lock(&self.writer).take();
        self.acked.notify_all();
        if report {
            self.sink.event(SessionEvent::Exit {
                code: None,
                auth_failed: false,
                host_key_changed: None,
            });
        }
    }

    fn deliver(&self, bytes: &[u8]) {
        let _ = self.log.append(bytes);
        if let Some(flow) = self.flow_control {
            let deadline = Instant::now() + flow.ack_timeout;
            let mut unacked = lock(&self.unacked);
            while *unacked >= flow.window && !self.ended.load(Ordering::SeqCst) {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                unacked = self
                    .acked
                    .wait_timeout(unacked, deadline - now)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
            *unacked += bytes.len();
        }
        self.sink.output(bytes.to_vec());
    }
}

/// An open serial console.
pub struct SerialSession {
    shared: Arc<Shared>,
    path: String,
    reader: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for SerialSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SerialSession")
            .field("path", &self.path)
            .field("running", &self.is_running())
            .finish_non_exhaustive()
    }
}

/// The device at `path` is gone. Only Unix can tell from the path; Windows relies on read errors.
fn gone(path: &str) -> bool {
    cfg!(unix) && !std::path::Path::new(path).exists()
}

/// A read that found nothing (as opposed to a port error).
fn quiet(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
    )
}

/// Reads the rest of a burst into `chunk`: whatever arrives within [`COALESCE`] of the last bytes, up
/// to [`CHUNK`]. `false` when the port failed (what was read is still in `chunk`).
fn gather(reader: &mut dyn SerialPort, chunk: &mut Vec<u8>) -> bool {
    if reader.set_timeout(COALESCE).is_err() {
        return true;
    }
    let mut ok = true;
    while chunk.len() < CHUNK {
        let start = chunk.len();
        chunk.resize(CHUNK, 0);
        match reader.read(&mut chunk[start..]) {
            Ok(n) if n > 0 => chunk.truncate(start + n),
            Ok(_) => {
                chunk.truncate(start);
                break;
            }
            Err(e) => {
                chunk.truncate(start);
                ok = quiet(&e);
                break;
            }
        }
    }
    let _ = reader.set_timeout(READ_TIMEOUT);
    ok
}

fn read_loop(shared: &Shared, mut reader: Box<dyn SerialPort>, path: &str) {
    let mut chunk = Vec::with_capacity(CHUNK);
    let mut checked = Instant::now();
    loop {
        if shared.ended.load(Ordering::SeqCst) {
            return;
        }
        chunk.clear();
        chunk.resize(CHUNK, 0);
        match reader.read(&mut chunk) {
            Ok(0) => {}
            Ok(n) => {
                chunk.truncate(n);
                let ok = gather(reader.as_mut(), &mut chunk);
                shared.deliver(&chunk);
                if !ok {
                    break;
                }
            }
            Err(e) if quiet(&e) => {}
            Err(_) => break,
        }
        if checked.elapsed() >= PRESENCE_INTERVAL {
            checked = Instant::now();
            if gone(path) {
                break;
            }
        }
    }
    drop(reader);
    shared.end(true);
}

impl SerialSession {
    /// Opens `path` with `settings` and starts reading.
    ///
    /// # Errors
    /// Why the port can't be opened.
    pub fn open(
        path: &str,
        settings: SerialSettings,
        sink: Arc<dyn Sink>,
        log: Arc<OutputLog>,
        flow_control: Option<FlowControl>,
    ) -> Result<Self, OpenError> {
        let port = serialport::new(path, settings.baud)
            .data_bits(data_bits(settings.data_bits))
            .parity(parity(settings.parity))
            .stop_bits(stop_bits(settings.stop_bits))
            .flow_control(flow(settings.flow_control))
            .timeout(READ_TIMEOUT)
            .open()?;
        Self::from_port(port, path, sink, log, flow_control)
    }

    /// Starts a session on an already open port (tests: a pseudo-terminal).
    ///
    /// # Errors
    /// When the port can't be cloned for the reader or the thread can't start.
    pub fn from_port(
        mut port: Box<dyn SerialPort>,
        path: &str,
        sink: Arc<dyn Sink>,
        log: Arc<OutputLog>,
        flow_control: Option<FlowControl>,
    ) -> Result<Self, OpenError> {
        port.set_timeout(READ_TIMEOUT)?;
        let reader = port.try_clone()?;
        let shared = Arc::new(Shared {
            sink,
            log,
            flow_control,
            ended: AtomicBool::new(false),
            writer: Mutex::new(Some(port)),
            unacked: Mutex::new(0),
            acked: Condvar::new(),
        });
        let thread = {
            let shared = Arc::clone(&shared);
            let path = path.to_owned();
            std::thread::Builder::new()
                .name("serial-reader".into())
                .spawn(move || read_loop(&shared, reader, &path))
                .map_err(|e| OpenError::Other(e.to_string()))?
        };
        Ok(Self {
            shared,
            path: path.to_owned(),
            reader: Mutex::new(Some(thread)),
        })
    }

    /// The device path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The output file.
    #[must_use]
    pub fn log(&self) -> &Arc<OutputLog> {
        &self.shared.log
    }

    /// Not closed and not lost.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.shared.ended.load(Ordering::SeqCst)
    }

    /// Writes typed input to the port.
    ///
    /// # Errors
    /// `BrokenPipe` after the session ended, or the write error.
    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let mut writer = lock(&self.shared.writer);
        let writer = writer.as_mut().ok_or(io::ErrorKind::BrokenPipe)?;
        // No flush: the port is unbuffered, and its flush is tcdrain, which waits for the line to drain.
        writer.write_all(bytes)
    }

    /// The receiver processed `bytes` of output.
    pub fn ack(&self, bytes: usize) {
        let mut unacked = lock(&self.shared.unacked);
        *unacked = unacked.saturating_sub(bytes);
        self.shared.acked.notify_all();
    }

    /// Applies new settings to the open port.
    ///
    /// # Errors
    /// `BrokenPipe` after the session ended, or the setting the device refused.
    pub fn reconfigure(&self, settings: SerialSettings) -> Result<(), OpenError> {
        let mut writer = lock(&self.shared.writer);
        let writer = writer
            .as_mut()
            .ok_or_else(|| OpenError::Other("the session has ended".into()))?;
        configure(writer.as_mut(), settings).map_err(OpenError::from)
    }

    /// Closes the port without reporting an exit (tab closed, reconnect). Returns once the port is
    /// released, so it can be opened again right away.
    pub fn close(&self) {
        self.shared.end(false);
        self.join();
    }

    /// Closes the port and reports the exit (another tab takes the port over).
    pub fn disconnect(&self) {
        self.shared.end(true);
        self.join();
    }

    fn join(&self) {
        let thread = lock(&self.reader).take();
        if let Some(thread) = thread
            && thread.thread().id() != std::thread::current().id()
        {
            let _ = thread.join();
        }
    }
}

impl Drop for SerialSession {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::sync::mpsc;

    use serialport::TTYPort;

    use super::*;

    struct TestSink {
        output: Mutex<Vec<u8>>,
        messages: std::sync::atomic::AtomicUsize,
        events: mpsc::Sender<SessionEvent>,
    }

    impl Sink for TestSink {
        fn output(&self, bytes: Vec<u8>) {
            self.messages.fetch_add(1, Ordering::SeqCst);
            lock(&self.output).extend(bytes);
        }

        fn event(&self, event: SessionEvent) {
            let _ = self.events.send(event);
        }
    }

    fn setup() -> (
        TTYPort,
        SerialSession,
        Arc<TestSink>,
        mpsc::Receiver<SessionEvent>,
        tempfile::TempDir,
    ) {
        let (master, slave) = TTYPort::pair().unwrap();
        let name = slave.name().unwrap_or_default();
        let (tx, rx) = mpsc::channel();
        let sink = Arc::new(TestSink {
            output: Mutex::default(),
            messages: std::sync::atomic::AtomicUsize::new(0),
            events: tx,
        });
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(OutputLog::create(dir.path(), "T").unwrap());
        let session = SerialSession::from_port(
            Box::new(slave),
            &name,
            Arc::clone(&sink) as Arc<dyn Sink>,
            log,
            Some(FlowControl::WEBVIEW),
        )
        .unwrap();
        (master, session, sink, rx, dir)
    }

    fn wait_for(sink: &TestSink, needle: &[u8]) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if lock(&sink.output).windows(needle.len()).any(|w| w == needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn output_reaches_the_sink_and_the_log_and_input_reaches_the_port() {
        let (mut master, session, sink, _rx, _dir) = setup();
        master.write_all(b"OPNsense login: ").unwrap();
        assert!(wait_for(&sink, b"OPNsense login: "));
        assert!(session.log().plain_text().unwrap().contains("OPNsense login: "));

        session.write(b"root").unwrap();
        master.set_timeout(Duration::from_secs(2)).unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 64];
        while !got.windows(4).any(|w| w == b"root") {
            let n = master.read(&mut buf).unwrap();
            got.extend_from_slice(&buf[..n]);
        }
        session.ack(1024);
        assert!(session.is_running());
    }

    #[test]
    fn a_burst_of_small_writes_arrives_as_one_message() {
        let (mut master, _session, sink, _rx, _dir) = setup();
        for line in ["boot ", "line ", "by line\r\n"] {
            master.write_all(line.as_bytes()).unwrap();
            master.flush().unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(wait_for(&sink, b"boot line by line"));
        assert!(
            sink.messages.load(Ordering::SeqCst) <= 2,
            "{}",
            sink.messages.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn losing_the_device_reports_one_exit() {
        let (master, session, _sink, rx, _dir) = setup();
        drop(master);
        let event = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(
            event,
            SessionEvent::Exit {
                code: None,
                auth_failed: false,
                host_key_changed: None
            }
        );
        assert!(!session.is_running());
        assert!(session.write(b"x").is_err());
        session.disconnect();
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn close_is_silent_and_disconnect_reports() {
        let (_master, session, _sink, rx, _dir) = setup();
        session.close();
        assert!(!session.is_running());
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());

        let (_master, session, _sink, rx, _dir) = setup();
        session.disconnect();
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(1)),
            Ok(SessionEvent::Exit { .. })
        ));
    }

    #[test]
    fn open_errors_map_to_kinds() {
        let missing = serialport::Error::new(serialport::ErrorKind::NoDevice, "gone");
        assert_eq!(OpenError::from(missing), OpenError::Missing);
        let busy = serialport::Error::new(serialport::ErrorKind::Unknown, "Resource busy");
        assert_eq!(OpenError::from(busy), OpenError::Busy);
        let denied = serialport::Error::new(
            serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied),
            "Permission denied",
        );
        assert_eq!(OpenError::from(denied), OpenError::PermissionDenied);
        assert_eq!(
            SerialSession::open(
                "/dev/does-not-exist-serial",
                SerialSettings::default(),
                Arc::new(TestSink {
                    output: Mutex::default(),
                    messages: std::sync::atomic::AtomicUsize::new(0),
                    events: mpsc::channel().0,
                }),
                Arc::new(OutputLog::create(tempfile::tempdir().unwrap().path(), "X").unwrap()),
                None,
            )
            .unwrap_err(),
            OpenError::Missing
        );
    }
}
