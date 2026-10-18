//! The port forward engine (Swift `PortForwardSession`, without the `ssh -N -L` process): a listener on
//! `127.0.0.1:<local port>` and one `direct-tcpip` channel per accepted connection.
//!
//! States: `starting` → `active` once the listener is bound and the connection is authenticated, or
//! `failed` (port in use, connect/auth/host key error, 30 s without getting there, connection lost);
//! `stopped` after [`Tunnel::stop`]. The 30 s don't run while the user looks at a host key prompt.

use std::future::Future;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

use super::{BackgroundError, Client};

/// How long a tunnel may take to become active (Swift `PortForwardSession.startTimeout`).
pub const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a port held by a tunnel that is shutting down is waited for.
const PORT_RETRY: Duration = Duration::from_secs(1);

/// A tunnel's state, as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ForwardState {
    /// Connecting.
    Starting,
    /// Listening and authenticated.
    Active,
    /// Ended with an error.
    Failed {
        /// Stable code the UI translates: `portInUse`, `timeout`, `disconnected` or a
        /// [`BackgroundError::code`].
        code: String,
        /// English message.
        error: String,
    },
    /// Stopped by the user or the app.
    Stopped,
}

impl ForwardState {
    /// A failure from a code and message.
    #[must_use]
    pub fn failed(code: &str, error: impl Into<String>) -> Self {
        Self::Failed {
            code: code.to_owned(),
            error: error.into(),
        }
    }

    /// Starting or active.
    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Starting | Self::Active)
    }
}

impl From<BackgroundError> for ForwardState {
    fn from(error: BackgroundError) -> Self {
        Self::failed(error.code(), error.to_string())
    }
}

/// A byte stream through the server.
pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

/// Boxed future, as returned by [`Opener::open`].
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Opens forwarded streams over an established connection ([`Client`], or a fake in tests).
pub trait Opener: Send + Sync + 'static {
    /// A stream to `host:port` as seen from the server.
    fn open<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        originator_port: u16,
    ) -> BoxFuture<'a, io::Result<Box<dyn Stream>>>;

    /// Whether the connection is gone.
    fn is_closed(&self) -> bool;
}

impl Opener for Client {
    fn open<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        originator_port: u16,
    ) -> BoxFuture<'a, io::Result<Box<dyn Stream>>> {
        Box::pin(async move {
            let stream = self
                .open_direct_tcpip(host, port, originator_port)
                .await
                .map_err(io::Error::other)?;
            Ok(Box::new(stream) as Box<dyn Stream>)
        })
    }

    fn is_closed(&self) -> bool {
        Client::is_closed(self)
    }
}

/// The start deadline, paused while a host key prompt is open.
#[derive(Debug, Clone)]
pub struct StartClock(Arc<ClockInner>);

#[derive(Debug)]
struct ClockInner {
    paused: AtomicUsize,
    since: Mutex<Instant>,
}

/// Keeps the clock paused until dropped.
#[derive(Debug)]
pub struct Paused(StartClock);

impl Drop for Paused {
    fn drop(&mut self) {
        let inner = &(self.0).0;
        *inner.since.lock().unwrap_or_else(PoisonError::into_inner) = Instant::now();
        inner.paused.fetch_sub(1, Ordering::SeqCst);
    }
}

impl StartClock {
    fn new() -> Self {
        Self(Arc::new(ClockInner {
            paused: AtomicUsize::new(0),
            since: Mutex::new(Instant::now()),
        }))
    }

    /// Stops the clock until the guard is dropped; it then restarts from zero.
    #[must_use]
    pub fn pause(&self) -> Paused {
        self.0.paused.fetch_add(1, Ordering::SeqCst);
        Paused(self.clone())
    }

    fn expired(&self, limit: Duration) -> bool {
        self.0.paused.load(Ordering::SeqCst) == 0
            && self
                .0
                .since
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .elapsed()
                >= limit
    }
}

/// Where a tunnel listens and forwards to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelSpec {
    /// Port on 127.0.0.1.
    pub local_port: u16,
    /// Destination host as seen from the server.
    pub remote_host: String,
    /// Destination port.
    pub remote_port: u16,
}

/// Receives state changes.
pub type StateSink = Arc<dyn Fn(ForwardState) + Send + Sync>;

/// A running tunnel; [`stop`](Self::stop) or dropping it ends it.
#[derive(Debug)]
pub struct Tunnel {
    task: JoinHandle<()>,
    stopped: Arc<AtomicBool>,
}

/// Whether something already accepts connections on `127.0.0.1:port` (Swift `LocalPort.isListening`).
async fn is_listening(port: u16) -> bool {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    matches!(
        tokio::time::timeout(Duration::from_millis(300), TcpStream::connect(address)).await,
        Ok(Ok(_))
    )
}

/// Binds the port, waiting briefly for a tunnel that is still letting go of it.
async fn bind(port: u16) -> Option<TcpListener> {
    let deadline = Instant::now() + PORT_RETRY;
    loop {
        if !is_listening(port).await
            && let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await
        {
            return Some(listener);
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn forward(opener: Arc<dyn Opener>, mut socket: TcpStream, peer: SocketAddr, spec: Arc<TunnelSpec>) {
    let Ok(mut channel) = opener
        .open(&spec.remote_host, spec.remote_port, peer.port())
        .await
    else {
        return;
    };
    let _ = tokio::io::copy_bidirectional(&mut socket, &mut channel).await;
}

async fn run<F>(spec: TunnelSpec, connect: F, clock: StartClock, emit: StateSink, limit: Duration)
where
    F: Future<Output = Result<Arc<dyn Opener>, ForwardState>> + Send,
{
    emit(ForwardState::Starting);
    let Some(listener) = bind(spec.local_port).await else {
        emit(ForwardState::failed(
            "portInUse",
            format!("Local port {} is already in use.", spec.local_port),
        ));
        return;
    };
    tokio::pin!(connect);
    let opener = loop {
        tokio::select! {
            result = &mut connect => match result {
                Ok(opener) => break opener,
                Err(state) => {
                    emit(state);
                    return;
                }
            },
            () = tokio::time::sleep(Duration::from_millis(200)) => {
                if clock.expired(limit) {
                    emit(ForwardState::failed(
                        "timeout",
                        format!("The tunnel didn't open within {} seconds.", limit.as_secs()),
                    ));
                    return;
                }
            }
        }
    };
    emit(ForwardState::Active);
    let spec = Arc::new(spec);
    let mut connections = JoinSet::new();
    let mut check = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                if let Ok((socket, peer)) = accepted {
                    connections.spawn(forward(Arc::clone(&opener), socket, peer, Arc::clone(&spec)));
                }
            }
            _ = check.tick() => {
                if opener.is_closed() {
                    emit(BackgroundError::Disconnected.into());
                    return;
                }
            }
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

impl Tunnel {
    /// Starts a tunnel on `runtime`: binds the port, awaits `connect` (built with the start clock, so a
    /// host key prompt can pause it), then forwards. States go to `on_state` until [`stop`](Self::stop).
    pub fn start<C, F>(
        runtime: &tokio::runtime::Handle,
        spec: TunnelSpec,
        connect: C,
        on_state: StateSink,
    ) -> Self
    where
        C: FnOnce(StartClock) -> F,
        F: Future<Output = Result<Arc<dyn Opener>, ForwardState>> + Send + 'static,
    {
        Self::start_with_limit(runtime, spec, connect, on_state, START_TIMEOUT)
    }

    /// [`start`](Self::start) with another start timeout (tests).
    pub fn start_with_limit<C, F>(
        runtime: &tokio::runtime::Handle,
        spec: TunnelSpec,
        connect: C,
        on_state: StateSink,
        limit: Duration,
    ) -> Self
    where
        C: FnOnce(StartClock) -> F,
        F: Future<Output = Result<Arc<dyn Opener>, ForwardState>> + Send + 'static,
    {
        let stopped = Arc::new(AtomicBool::new(false));
        let gate = Arc::clone(&stopped);
        let emit: StateSink = Arc::new(move |state| {
            if !gate.load(Ordering::SeqCst) {
                on_state(state);
            }
        });
        let clock = StartClock::new();
        let connect = connect(clock.clone());
        let task = runtime.spawn(run(spec, connect, clock, emit, limit));
        Self { task, stopped }
    }

    /// Ends the tunnel: no more state changes are reported, the listener, the open connections and the
    /// SSH connection close.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.task.abort();
    }

    /// Whether the task has ended (failed or stopped).
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::background::runtime;

    /// Connects straight to the destination instead of going through a server.
    struct Direct {
        closed: AtomicBool,
    }

    impl Opener for Direct {
        fn open<'a>(
            &'a self,
            host: &'a str,
            port: u16,
            _originator_port: u16,
        ) -> BoxFuture<'a, io::Result<Box<dyn Stream>>> {
            Box::pin(async move {
                let stream = TcpStream::connect((host, port)).await?;
                Ok(Box::new(stream) as Box<dyn Stream>)
            })
        }

        fn is_closed(&self) -> bool {
            self.closed.load(Ordering::SeqCst)
        }
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn collector() -> (StateSink, mpsc::Receiver<ForwardState>) {
        let (sender, receiver) = mpsc::channel();
        let sender = Mutex::new(sender);
        (
            Arc::new(move |state| {
                let _ = sender.lock().unwrap().send(state);
            }),
            receiver,
        )
    }

    fn next(receiver: &mpsc::Receiver<ForwardState>) -> ForwardState {
        receiver.recv_timeout(Duration::from_secs(10)).unwrap()
    }

    /// An echo server on a free port.
    fn echo_server() -> u16 {
        let listener = runtime()
            .block_on(TcpListener::bind((Ipv4Addr::LOCALHOST, 0)))
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        runtime().spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let (mut read, mut write) = socket.split();
                    let _ = tokio::io::copy(&mut read, &mut write).await;
                });
            }
        });
        port
    }

    fn spec(local: u16, remote: u16) -> TunnelSpec {
        TunnelSpec {
            local_port: local,
            remote_host: "127.0.0.1".to_owned(),
            remote_port: remote,
        }
    }

    #[test]
    fn forwards_connections_and_stops() {
        let remote = echo_server();
        let local = free_port();
        let (sink, states) = collector();
        let direct = Arc::new(Direct {
            closed: AtomicBool::new(false),
        });
        let opener: Arc<dyn Opener> = direct.clone();
        let tunnel = Tunnel::start(
            runtime().handle(),
            spec(local, remote),
            move |_| async move { Ok(opener) },
            sink,
        );
        assert_eq!(next(&states), ForwardState::Starting);
        assert_eq!(next(&states), ForwardState::Active);

        runtime().block_on(async {
            for text in [&b"hello"[..], b"again"] {
                let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, local)).await.unwrap();
                client.write_all(text).await.unwrap();
                let mut buffer = vec![0; text.len()];
                client.read_exact(&mut buffer).await.unwrap();
                assert_eq!(buffer, text);
            }
        });

        tunnel.stop();
        // The port is released and nothing is reported after the stop.
        runtime().block_on(async {
            let deadline = Instant::now() + Duration::from_secs(5);
            while is_listening(local).await {
                assert!(Instant::now() < deadline, "port still open");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
        assert!(states.recv_timeout(Duration::from_millis(300)).is_err());
    }

    #[test]
    fn port_in_use_fails() {
        let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = busy.local_addr().unwrap().port();
        let (sink, states) = collector();
        let _tunnel = Tunnel::start(
            runtime().handle(),
            spec(port, 1),
            |_| async { Err(ForwardState::failed("x", "never")) },
            sink,
        );
        assert_eq!(next(&states), ForwardState::Starting);
        assert_eq!(
            next(&states),
            ForwardState::failed("portInUse", format!("Local port {port} is already in use."))
        );
    }

    #[test]
    fn connect_errors_are_reported_and_free_the_port() {
        let port = free_port();
        let (sink, states) = collector();
        let tunnel = Tunnel::start(
            runtime().handle(),
            spec(port, 1),
            |_| async { Err(BackgroundError::AuthFailed("password".to_owned()).into()) },
            sink,
        );
        assert_eq!(next(&states), ForwardState::Starting);
        assert_eq!(
            next(&states),
            ForwardState::failed("authFailed", "Permission denied (password).")
        );
        std::thread::sleep(Duration::from_millis(100));
        assert!(tunnel.is_finished());
        assert!(!runtime().block_on(is_listening(port)));
    }

    #[test]
    fn times_out_but_not_while_a_prompt_is_open() {
        let port = free_port();
        let (sink, states) = collector();
        let (release, released) = tokio::sync::oneshot::channel::<()>();
        let _tunnel = Tunnel::start_with_limit(
            runtime().handle(),
            spec(port, 1),
            move |clock| async move {
                let paused = clock.pause();
                // The "prompt" stays open longer than the limit.
                let _ = released.await;
                drop(paused);
                std::future::pending::<Result<Arc<dyn Opener>, ForwardState>>().await
            },
            sink,
            Duration::from_millis(400),
        );
        assert_eq!(next(&states), ForwardState::Starting);
        assert!(states.recv_timeout(Duration::from_millis(900)).is_err());
        release.send(()).unwrap();
        let started = Instant::now();
        assert_eq!(
            next(&states),
            ForwardState::failed("timeout", "The tunnel didn't open within 0 seconds.")
        );
        assert!(started.elapsed() >= Duration::from_millis(300));
    }

    #[test]
    fn lost_connection_fails() {
        let port = free_port();
        let (sink, states) = collector();
        let direct = Arc::new(Direct {
            closed: AtomicBool::new(false),
        });
        let opener: Arc<dyn Opener> = direct.clone();
        let _tunnel = Tunnel::start(
            runtime().handle(),
            spec(port, 1),
            move |_| async move { Ok(opener) },
            sink,
        );
        assert_eq!(next(&states), ForwardState::Starting);
        assert_eq!(next(&states), ForwardState::Active);
        direct.closed.store(true, Ordering::SeqCst);
        assert_eq!(
            next(&states),
            ForwardState::failed("disconnected", "The SSH connection was closed.")
        );
    }

    #[test]
    fn state_serialization() {
        assert_eq!(
            serde_json::to_value(ForwardState::failed("timeout", "t")).unwrap(),
            serde_json::json!({"state": "failed", "code": "timeout", "error": "t"})
        );
        assert_eq!(
            serde_json::to_value(ForwardState::Active).unwrap(),
            serde_json::json!({"state": "active"})
        );
        assert!(ForwardState::Starting.is_running());
        assert!(!ForwardState::Stopped.is_running());
    }
}
