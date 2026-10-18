//! FreeRDP for the embedded RDP engine: a static FreeRDP build (see `build.rs`) behind a small C shim
//! (`csrc/rdshim.h`), wrapped in a safe API.
//!
//! A [`Connection`] is driven from one thread. Its [`Handler`] is called from that thread (inside
//! [`Connection::connect`] and [`Connection::poll`]) and from `FreeRDP`'s channel threads, always under the
//! shim's callback lock, which [`Connection::with_handler`] takes too. Other threads stop or wake it
//! through a [`Remote`].

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// The FreeRDP release this build links.
pub const VERSION: &str = env!("RD_FREERDP_VERSION");

/// `KBD_FLAGS_EXTENDED`.
pub const KBD_FLAGS_EXTENDED: u16 = 0x0100;
/// `KBD_FLAGS_DOWN`.
pub const KBD_FLAGS_DOWN: u16 = 0x4000;
/// `KBD_FLAGS_RELEASE`.
pub const KBD_FLAGS_RELEASE: u16 = 0x8000;
/// `PTR_FLAGS_HWHEEL`.
pub const PTR_FLAGS_HWHEEL: u16 = 0x0400;
/// `PTR_FLAGS_WHEEL`.
pub const PTR_FLAGS_WHEEL: u16 = 0x0200;
/// `PTR_FLAGS_WHEEL_NEGATIVE`.
pub const PTR_FLAGS_WHEEL_NEGATIVE: u16 = 0x0100;
/// `PTR_FLAGS_MOVE`.
pub const PTR_FLAGS_MOVE: u16 = 0x0800;
/// `PTR_FLAGS_DOWN`.
pub const PTR_FLAGS_DOWN: u16 = 0x8000;
/// `PTR_FLAGS_BUTTON1` (left).
pub const PTR_FLAGS_BUTTON1: u16 = 0x1000;
/// `PTR_FLAGS_BUTTON2` (right).
pub const PTR_FLAGS_BUTTON2: u16 = 0x2000;
/// `PTR_FLAGS_BUTTON3` (middle).
pub const PTR_FLAGS_BUTTON3: u16 = 0x4000;
/// `PTR_XFLAGS_DOWN`.
pub const PTR_XFLAGS_DOWN: u16 = 0x8000;
/// `PTR_XFLAGS_BUTTON1` (back).
pub const PTR_XFLAGS_BUTTON1: u16 = 0x0001;
/// `PTR_XFLAGS_BUTTON2` (forward).
pub const PTR_XFLAGS_BUTTON2: u16 = 0x0002;
/// `VERIFY_CERT_FLAG_MISMATCH`: the certificate's names don't match the host.
pub const VERIFY_CERT_FLAG_MISMATCH: u32 = 0x80;
/// `FILECONTENTS_SIZE`.
pub const FILECONTENTS_SIZE: u32 = 0x1;
/// `FILECONTENTS_RANGE`.
pub const FILECONTENTS_RANGE: u32 = 0x2;

/// A dirty rectangle (exclusive right and bottom edges).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub width: i32,
    /// Height.
    pub height: i32,
}

/// NLA credentials.
#[derive(Clone, Default)]
pub struct Credentials {
    /// User name.
    pub username: String,
    /// Domain, may be empty.
    pub domain: String,
    /// Password.
    pub password: String,
}

/// A clipboard format the server offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    /// Its id.
    pub id: u32,
    /// Its name (registered formats).
    pub name: Option<String>,
}

/// A File Contents Request ([MS-RDPECLIP] 2.2.5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentsRequest {
    /// The stream it opens or continues.
    pub stream_id: u32,
    /// The file's index in the list.
    pub index: u32,
    /// [`FILECONTENTS_SIZE`] or [`FILECONTENTS_RANGE`].
    pub flags: u32,
    /// Where the range starts.
    pub position: u64,
    /// Bytes asked for.
    pub requested: u32,
    /// The locked list it refers to.
    pub data_id: Option<u32>,
}

/// What the server's cliprdr channel sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipEvent {
    /// The channel is up: send the capabilities and the first Format List.
    MonitorReady,
    /// The server's general capability flags.
    Capabilities(u32),
    /// The remote clipboard changed (answer with a Format List Response).
    FormatList(Vec<Format>),
    /// The server wants the local clipboard's data in this format.
    DataRequest(u32),
    /// The data asked for, `None` when refused.
    DataResponse(Option<Vec<u8>>),
    /// The server reads one of the offered files.
    ContentsRequest(ContentsRequest),
    /// The answer to a File Contents Request, `None` when refused.
    ContentsResponse {
        /// The request it answers.
        stream_id: u32,
        /// The size (8 bytes) or the range's bytes.
        data: Option<Vec<u8>>,
    },
    /// The server locked (or unlocked) the file list it saw under this id.
    Lock {
        /// The lock's id.
        data_id: u32,
        /// Locked or unlocked.
        locked: bool,
    },
}

/// What the connection reports: from the connection's thread or `FreeRDP`'s channel threads, one call
/// at a time.
pub trait Handler: Send {
    /// A certificate OpenSSL didn't verify (PEM). `true` accepts it for this connection.
    fn verify_certificate(&mut self, pem: &[u8], host: &str, port: u16, flags: u32) -> bool;
    /// NLA needs credentials; `None` cancels.
    fn authenticate(&mut self) -> Option<Credentials>;
    /// Connected, or the desktop changed size.
    fn desktop(&mut self, width: u32, height: u32);
    /// These parts of the desktop changed.
    fn painted(&mut self, rects: &[Rect]);
    /// The pointer's shape: RGBA, not premultiplied.
    fn pointer_shape(&mut self, rgba: &[u8], width: u32, height: u32, hotspot_x: u32, hotspot_y: u32);
    /// The pointer is hidden.
    fn pointer_hidden(&mut self);
    /// The pointer is the system default.
    fn pointer_default(&mut self);
    /// Display Control is open: resizes can go out.
    fn display_control_ready(&mut self);
    /// A cliprdr message from the server.
    fn clip(&mut self, event: ClipEvent);
}

/// What to connect to.
#[derive(Debug, Clone, Copy)]
pub struct Config<'a> {
    /// Host name or address.
    pub host: &'a str,
    /// TCP port.
    pub port: u16,
    /// User name, may be empty.
    pub username: &'a str,
    /// Domain, may be empty.
    pub domain: &'a str,
    /// Password, may be empty.
    pub password: &'a str,
    /// The computer name the server sees.
    pub client_name: &'a str,
    /// `FreeRDP`'s own folder (its certificate store, which stays empty, and its license cache).
    pub config_dir: &'a str,
    /// Initial desktop width.
    pub width: u32,
    /// Initial desktop height.
    pub height: u32,
    /// The administrative (console) session.
    pub console_session: bool,
    /// Load the clipboard channel.
    pub clipboard: bool,
}

/// The desktop's pixels: RGBA rows. `FreeRDP` doesn't paint or resize while it is alive.
pub struct Framebuffer<'a> {
    shim: *mut ffi::RdShim,
    /// The rows.
    pub data: &'a [u8],
    /// Bytes per row.
    pub stride: usize,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl std::fmt::Debug for Framebuffer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Framebuffer")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

#[allow(unsafe_code)]
impl Drop for Framebuffer<'_> {
    fn drop(&mut self) {
        // SAFETY: the lock was taken by rdshim_lock_framebuffer() for this guard.
        unsafe { ffi::rdshim_unlock_framebuffer(self.shim) };
    }
}

#[allow(unsafe_code)]
mod ffi {
    use std::ffi::{c_char, c_int, c_void};

    use super::Rect;

    #[repr(C)]
    pub struct RdShim {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub struct Callbacks {
        pub user: *mut c_void,
        pub verify_certificate:
            Option<unsafe extern "C" fn(*mut c_void, *const u8, usize, *const c_char, u16, u32) -> c_int>,
        pub authenticate: Option<unsafe extern "C" fn(*mut c_void, *mut RdShim) -> c_int>,
        pub desktop: Option<unsafe extern "C" fn(*mut c_void, u32, u32)>,
        pub painted: Option<unsafe extern "C" fn(*mut c_void, *const Rect, u32)>,
        pub pointer_shape: Option<unsafe extern "C" fn(*mut c_void, *const u8, u32, u32, u32, u32)>,
        pub pointer_hidden: Option<unsafe extern "C" fn(*mut c_void)>,
        pub pointer_default: Option<unsafe extern "C" fn(*mut c_void)>,
        pub display_control_ready: Option<unsafe extern "C" fn(*mut c_void)>,
        pub clip_monitor_ready: Option<unsafe extern "C" fn(*mut c_void)>,
        pub clip_capabilities: Option<unsafe extern "C" fn(*mut c_void, u32)>,
        pub clip_format_list:
            Option<unsafe extern "C" fn(*mut c_void, *const u32, *const *const c_char, u32)>,
        pub clip_data_request: Option<unsafe extern "C" fn(*mut c_void, u32)>,
        pub clip_data_response: Option<unsafe extern "C" fn(*mut c_void, c_int, *const u8, u32)>,
        pub clip_contents_request:
            Option<unsafe extern "C" fn(*mut c_void, u32, u32, u32, u64, u32, c_int, u32)>,
        pub clip_contents_response: Option<unsafe extern "C" fn(*mut c_void, u32, c_int, *const u8, u32)>,
        pub clip_lock: Option<unsafe extern "C" fn(*mut c_void, u32, c_int)>,
    }

    #[repr(C)]
    #[allow(clippy::struct_field_names)] // mirrors RdShimConfig
    pub struct Config {
        pub host: *const c_char,
        pub port: u16,
        pub username: *const c_char,
        pub domain: *const c_char,
        pub password: *const c_char,
        pub client_name: *const c_char,
        pub config_dir: *const c_char,
        pub width: u32,
        pub height: u32,
        pub console_session: c_int,
        pub clipboard: c_int,
    }

    unsafe extern "C" {
        pub fn rdshim_version() -> *const c_char;
        pub fn rdshim_new(callbacks: *const Callbacks) -> *mut RdShim;
        pub fn rdshim_free(shim: *mut RdShim);
        pub fn rdshim_connect(shim: *mut RdShim, config: *const Config) -> c_int;
        pub fn rdshim_set_credentials(
            shim: *mut RdShim,
            username: *const c_char,
            domain: *const c_char,
            password: *const c_char,
        ) -> c_int;
        pub fn rdshim_abort(shim: *mut RdShim);
        pub fn rdshim_wake(shim: *mut RdShim);
        pub fn rdshim_poll(shim: *mut RdShim, timeout_ms: u32) -> c_int;
        pub fn rdshim_disconnect(shim: *mut RdShim);
        pub fn rdshim_last_error(shim: *mut RdShim) -> u32;
        pub fn rdshim_error_info(shim: *mut RdShim) -> u32;
        pub fn rdshim_last_error_string(code: u32) -> *const c_char;
        pub fn rdshim_error_info_string(code: u32) -> *const c_char;
        pub fn rdshim_lock_framebuffer(
            shim: *mut RdShim,
            data: *mut *const u8,
            stride: *mut u32,
            width: *mut u32,
            height: *mut u32,
        ) -> c_int;
        pub fn rdshim_unlock_framebuffer(shim: *mut RdShim);
        pub fn rdshim_lock_callbacks(shim: *mut RdShim);
        pub fn rdshim_unlock_callbacks(shim: *mut RdShim);
        pub fn rdshim_send_key(shim: *mut RdShim, flags: u16, code: u8) -> c_int;
        pub fn rdshim_send_unicode(shim: *mut RdShim, flags: u16, code: u16) -> c_int;
        pub fn rdshim_send_mouse(shim: *mut RdShim, flags: u16, x: u16, y: u16) -> c_int;
        pub fn rdshim_send_mouse_ex(shim: *mut RdShim, flags: u16, x: u16, y: u16) -> c_int;
        pub fn rdshim_resize(shim: *mut RdShim, width: u32, height: u32) -> c_int;
        pub fn rdshim_clip_capabilities(shim: *mut RdShim, flags: u32) -> c_int;
        pub fn rdshim_clip_format_list(
            shim: *mut RdShim,
            ids: *const u32,
            names: *const *const c_char,
            count: u32,
        ) -> c_int;
        pub fn rdshim_clip_format_list_response(shim: *mut RdShim, ok: c_int) -> c_int;
        pub fn rdshim_clip_data_request(shim: *mut RdShim, format: u32) -> c_int;
        pub fn rdshim_clip_data_response(shim: *mut RdShim, ok: c_int, data: *const u8, len: u32) -> c_int;
        pub fn rdshim_clip_contents_request(
            shim: *mut RdShim,
            stream_id: u32,
            index: u32,
            flags: u32,
            position: u64,
            requested: u32,
            has_data_id: c_int,
            data_id: u32,
        ) -> c_int;
        pub fn rdshim_clip_contents_response(
            shim: *mut RdShim,
            stream_id: u32,
            ok: c_int,
            data: *const u8,
            len: u32,
        ) -> c_int;
    }
}

/// The shim's own version string (`FREERDP_VERSION_FULL`).
#[must_use]
#[allow(unsafe_code)]
pub fn library_version() -> String {
    // SAFETY: a static NUL-terminated string.
    unsafe { text(ffi::rdshim_version()) }
}

/// The text of a `freerdp_get_last_error()` code.
#[must_use]
#[allow(unsafe_code)]
pub fn last_error_string(code: u32) -> String {
    // SAFETY: FreeRDP returns a static string for every code.
    unsafe { text(ffi::rdshim_last_error_string(code)) }
}

/// The text of a server error info code.
#[must_use]
#[allow(unsafe_code)]
pub fn error_info_string(code: u32) -> String {
    // SAFETY: FreeRDP returns a static string for every code.
    unsafe { text(ffi::rdshim_error_info_string(code)) }
}

/// # Safety
/// `p` is NULL or a NUL-terminated string.
#[allow(unsafe_code)]
unsafe fn text(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: see the function's contract.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

/// Stops or wakes a [`Connection`] from another thread.
#[derive(Debug)]
pub struct Remote {
    shim: Mutex<Option<ShimPtr>>,
}

#[derive(Debug, Clone, Copy)]
struct ShimPtr(NonNull<ffi::RdShim>);

// SAFETY: rdshim_abort() and rdshim_wake() are thread-safe, and the pointer is taken out (under the
// mutex) before the shim is freed.
#[allow(unsafe_code)]
unsafe impl Send for ShimPtr {}

impl Remote {
    /// Stops a running connect or session.
    #[allow(unsafe_code)]
    pub fn abort(&self) {
        if let Some(shim) = *self.shim.lock().unwrap_or_else(PoisonError::into_inner) {
            // SAFETY: the shim is alive while it is in the mutex.
            unsafe { ffi::rdshim_abort(shim.0.as_ptr()) };
        }
    }

    /// Makes a waiting [`Connection::poll`] return.
    #[allow(unsafe_code)]
    pub fn wake(&self) {
        if let Some(shim) = *self.shim.lock().unwrap_or_else(PoisonError::into_inner) {
            // SAFETY: the shim is alive while it is in the mutex.
            unsafe { ffi::rdshim_wake(shim.0.as_ptr()) };
        }
    }
}

/// One FreeRDP client; connects again after a disconnect.
pub struct Connection<H: Handler> {
    shim: NonNull<ffi::RdShim>,
    // Boxed: the shim keeps its address as the callbacks' user pointer.
    handler: Box<H>,
    remote: Arc<Remote>,
}

impl<H: Handler> std::fmt::Debug for Connection<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection").finish_non_exhaustive()
    }
}

fn cstring(s: &str) -> Option<CString> {
    CString::new(s).ok()
}

#[allow(unsafe_code)]
impl<H: Handler> Connection<H> {
    /// A client reporting to `handler`; `None` when FreeRDP couldn't be set up.
    pub fn new(handler: H) -> Option<Self> {
        let mut handler = Box::new(handler);
        let callbacks = ffi::Callbacks {
            user: std::ptr::from_mut::<H>(&mut handler).cast::<c_void>(),
            verify_certificate: Some(verify_certificate::<H>),
            authenticate: Some(authenticate::<H>),
            desktop: Some(desktop::<H>),
            painted: Some(painted::<H>),
            pointer_shape: Some(pointer_shape::<H>),
            pointer_hidden: Some(pointer_hidden::<H>),
            pointer_default: Some(pointer_default::<H>),
            display_control_ready: Some(display_control_ready::<H>),
            clip_monitor_ready: Some(clip_monitor_ready::<H>),
            clip_capabilities: Some(clip_capabilities::<H>),
            clip_format_list: Some(clip_format_list::<H>),
            clip_data_request: Some(clip_data_request::<H>),
            clip_data_response: Some(clip_data_response::<H>),
            clip_contents_request: Some(clip_contents_request::<H>),
            clip_contents_response: Some(clip_contents_response::<H>),
            clip_lock: Some(clip_lock::<H>),
        };
        // SAFETY: the shim copies the callbacks; the user pointer stays valid as long as the box.
        let shim = NonNull::new(unsafe { ffi::rdshim_new(&raw const callbacks) })?;
        Some(Self {
            shim,
            handler,
            remote: Arc::new(Remote {
                shim: Mutex::new(Some(ShimPtr(shim))),
            }),
        })
    }

    fn raw(&self) -> *mut ffi::RdShim {
        self.shim.as_ptr()
    }

    /// The handle other threads stop or wake this connection with.
    #[must_use]
    pub fn remote(&self) -> Arc<Remote> {
        Arc::clone(&self.remote)
    }

    /// Runs `f` with the handler, under the callback lock (no callback runs meanwhile, on any thread).
    pub fn with_handler<R>(&mut self, f: impl FnOnce(&mut H) -> R) -> R {
        // SAFETY: the shim is alive; the lock is released below.
        unsafe { ffi::rdshim_lock_callbacks(self.raw()) };
        let result = f(&mut self.handler);
        // SAFETY: taken above.
        unsafe { ffi::rdshim_unlock_callbacks(self.raw()) };
        result
    }

    /// Connects (blocking; the handler is asked about the certificate and credentials meanwhile).
    pub fn connect(&mut self, config: &Config<'_>) -> bool {
        let strings = [
            config.host,
            config.username,
            config.domain,
            config.password,
            config.client_name,
            config.config_dir,
        ]
        .map(cstring);
        let [
            Some(host),
            Some(username),
            Some(domain),
            Some(password),
            Some(client_name),
            Some(config_dir),
        ] = strings
        else {
            return false;
        };
        let raw = ffi::Config {
            host: host.as_ptr(),
            port: config.port,
            username: username.as_ptr(),
            domain: domain.as_ptr(),
            password: password.as_ptr(),
            client_name: client_name.as_ptr(),
            config_dir: config_dir.as_ptr(),
            width: config.width,
            height: config.height,
            console_session: c_int::from(config.console_session),
            clipboard: c_int::from(config.clipboard),
        };
        // SAFETY: the strings outlive the call; the handler is only reached through the user pointer
        // while `self` is borrowed mutably.
        let connected = unsafe { ffi::rdshim_connect(self.raw(), &raw const raw) } == 1;
        // The password's copy is wiped; FreeRDP keeps its own in its settings.
        let mut bytes = password.into_bytes();
        bytes.fill(0);
        connected
    }

    /// Waits up to `timeout` for data (or [`Remote::wake`]) and processes it; `false` once disconnected.
    pub fn poll(&mut self, timeout: Duration) -> bool {
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: see `connect`.
        unsafe { ffi::rdshim_poll(self.raw(), ms) == 1 }
    }

    /// Sends the disconnect and drops the connection.
    pub fn disconnect(&mut self) {
        // SAFETY: on the connection's thread.
        unsafe { ffi::rdshim_disconnect(self.raw()) };
    }

    /// `freerdp_get_last_error()` of the last connection.
    #[must_use]
    pub fn last_error(&self) -> u32 {
        // SAFETY: on the connection's thread.
        unsafe { ffi::rdshim_last_error(self.raw()) }
    }

    /// The server's error info of the last connection.
    #[must_use]
    pub fn error_info(&self) -> u32 {
        // SAFETY: on the connection's thread.
        unsafe { ffi::rdshim_error_info(self.raw()) }
    }

    /// The desktop, while connected; painting waits until the guard is dropped.
    #[must_use]
    pub fn framebuffer(&self) -> Option<Framebuffer<'_>> {
        let (mut data, mut stride, mut width, mut height) = (std::ptr::null(), 0u32, 0u32, 0u32);
        // SAFETY: out pointers to locals; on success the update lock is held until the guard drops.
        let ok = unsafe {
            ffi::rdshim_lock_framebuffer(
                self.raw(),
                &raw mut data,
                &raw mut stride,
                &raw mut width,
                &raw mut height,
            )
        };
        if ok != 1 {
            return None;
        }
        let len = (stride as usize).checked_mul(height as usize);
        let data: &[u8] = match (data.is_null(), len) {
            // SAFETY: the GDI buffer is `stride * height` bytes and can't be freed or reallocated while
            // the update lock is held (resizes take it, see rdshim.c).
            (false, Some(len)) => unsafe { std::slice::from_raw_parts(data, len) },
            _ => &[],
        };
        Some(Framebuffer {
            shim: self.raw(),
            data,
            stride: stride as usize,
            width,
            height,
        })
    }

    /// A scancode event (`KBD_FLAGS_*`).
    pub fn send_key(&mut self, flags: u16, code: u8) -> bool {
        // SAFETY: on the connection's thread.
        unsafe { ffi::rdshim_send_key(self.raw(), flags, code) == 1 }
    }

    /// A Unicode key event (`KBD_FLAGS_*`).
    pub fn send_unicode(&mut self, flags: u16, code: u16) -> bool {
        // SAFETY: on the connection's thread.
        unsafe { ffi::rdshim_send_unicode(self.raw(), flags, code) == 1 }
    }

    /// A mouse event (`PTR_FLAGS_*`).
    pub fn send_mouse(&mut self, flags: u16, x: u16, y: u16) -> bool {
        // SAFETY: on the connection's thread.
        unsafe { ffi::rdshim_send_mouse(self.raw(), flags, x, y) == 1 }
    }

    /// An extended mouse event (`PTR_XFLAGS_*`).
    pub fn send_mouse_ex(&mut self, flags: u16, x: u16, y: u16) -> bool {
        // SAFETY: on the connection's thread.
        unsafe { ffi::rdshim_send_mouse_ex(self.raw(), flags, x, y) == 1 }
    }

    /// A Display Control resize; `false` while the channel isn't open.
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        // SAFETY: on the connection's thread.
        unsafe { ffi::rdshim_resize(self.raw(), width, height) == 1 }
    }

    /// The client's cliprdr capabilities.
    pub fn clip_capabilities(&mut self, flags: u32) -> bool {
        // SAFETY: on the connection's thread.
        unsafe { ffi::rdshim_clip_capabilities(self.raw(), flags) == 1 }
    }

    /// A Format List.
    pub fn clip_format_list(&mut self, formats: &[Format]) -> bool {
        let ids: Vec<u32> = formats.iter().map(|f| f.id).collect();
        let names: Vec<Option<CString>> = formats
            .iter()
            .map(|f| f.name.as_deref().and_then(cstring))
            .collect();
        let pointers: Vec<*const c_char> = names
            .iter()
            .map(|n| n.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()))
            .collect();
        let Ok(count) = u32::try_from(formats.len()) else {
            return false;
        };
        // SAFETY: the arrays outlive the call.
        unsafe { ffi::rdshim_clip_format_list(self.raw(), ids.as_ptr(), pointers.as_ptr(), count) == 1 }
    }

    /// A Format List Response.
    pub fn clip_format_list_response(&mut self, ok: bool) -> bool {
        // SAFETY: on the connection's thread.
        unsafe { ffi::rdshim_clip_format_list_response(self.raw(), c_int::from(ok)) == 1 }
    }

    /// A Format Data Request.
    pub fn clip_data_request(&mut self, format: u32) -> bool {
        // SAFETY: on the connection's thread.
        unsafe { ffi::rdshim_clip_data_request(self.raw(), format) == 1 }
    }

    /// A Format Data Response; `None` refuses.
    pub fn clip_data_response(&mut self, data: Option<&[u8]>) -> bool {
        let (ok, ptr, len) = parts(data);
        // SAFETY: the data outlives the call.
        unsafe { ffi::rdshim_clip_data_response(self.raw(), ok, ptr, len) == 1 }
    }

    /// A File Contents Request.
    pub fn clip_contents_request(&mut self, request: &ContentsRequest) -> bool {
        // SAFETY: on the connection's thread.
        unsafe {
            ffi::rdshim_clip_contents_request(
                self.raw(),
                request.stream_id,
                request.index,
                request.flags,
                request.position,
                request.requested,
                c_int::from(request.data_id.is_some()),
                request.data_id.unwrap_or(0),
            ) == 1
        }
    }

    /// A File Contents Response; `None` refuses.
    pub fn clip_contents_response(&mut self, stream_id: u32, data: Option<&[u8]>) -> bool {
        let (ok, ptr, len) = parts(data);
        // SAFETY: the data outlives the call.
        unsafe { ffi::rdshim_clip_contents_response(self.raw(), stream_id, ok, ptr, len) == 1 }
    }
}

fn parts(data: Option<&[u8]>) -> (c_int, *const u8, u32) {
    match data {
        Some(data) => (1, data.as_ptr(), u32::try_from(data.len()).unwrap_or(u32::MAX)),
        None => (0, std::ptr::null(), 0),
    }
}

#[allow(unsafe_code)]
impl<H: Handler> Drop for Connection<H> {
    fn drop(&mut self) {
        *self.remote.shim.lock().unwrap_or_else(PoisonError::into_inner) = None;
        // SAFETY: no other reference to the shim is left.
        unsafe { ffi::rdshim_free(self.raw()) };
    }
}

// ---- callbacks: the user pointer is the connection's boxed handler --------------------------------

/// # Safety
/// `user` is the `Box<H>` of a live connection, and no other reference to it exists (the shim only
/// calls back from inside `connect`/`poll`, which borrow the connection mutably).
#[allow(unsafe_code)]
unsafe fn handler<'a, H: Handler>(user: *mut c_void) -> &'a mut H {
    // SAFETY: see the function's contract.
    unsafe { &mut *user.cast::<H>() }
}

/// # Safety
/// `data` is NULL or points at `len` readable bytes.
#[allow(unsafe_code)]
unsafe fn bytes<'a>(data: *const u8, len: usize) -> &'a [u8] {
    if data.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: see the function's contract.
        unsafe { std::slice::from_raw_parts(data, len) }
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn verify_certificate<H: Handler>(
    user: *mut c_void,
    pem: *const u8,
    len: usize,
    host: *const c_char,
    port: u16,
    flags: u32,
) -> c_int {
    // SAFETY: the shim passes its live handler and FreeRDP's PEM and host strings.
    let (handler, pem, host) = unsafe { (handler::<H>(user), bytes(pem, len), text(host)) };
    c_int::from(handler.verify_certificate(pem, &host, port, flags))
}

#[allow(unsafe_code)]
unsafe extern "C" fn authenticate<H: Handler>(user: *mut c_void, shim: *mut ffi::RdShim) -> c_int {
    // SAFETY: as above.
    let handler = unsafe { handler::<H>(user) };
    let Some(credentials) = handler.authenticate() else {
        return 0;
    };
    let [Some(username), Some(domain), Some(password)] = [
        credentials.username.as_str(),
        credentials.domain.as_str(),
        credentials.password.as_str(),
    ]
    .map(cstring) else {
        return 0;
    };
    // SAFETY: the shim copies the strings.
    unsafe { ffi::rdshim_set_credentials(shim, username.as_ptr(), domain.as_ptr(), password.as_ptr()) };
    let mut bytes = password.into_bytes();
    bytes.fill(0);
    1
}

#[allow(unsafe_code)]
unsafe extern "C" fn desktop<H: Handler>(user: *mut c_void, width: u32, height: u32) {
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.desktop(width, height);
}

#[allow(unsafe_code)]
unsafe extern "C" fn painted<H: Handler>(user: *mut c_void, rects: *const Rect, count: u32) {
    // SAFETY: the shim passes `count` rectangles.
    let rects = if rects.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(rects, count as usize) }
    };
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.painted(rects);
}

#[allow(unsafe_code)]
unsafe extern "C" fn pointer_shape<H: Handler>(
    user: *mut c_void,
    rgba: *const u8,
    width: u32,
    height: u32,
    hotspot_x: u32,
    hotspot_y: u32,
) {
    let len = width as usize * height as usize * 4;
    // SAFETY: the shim passes `width * height * 4` bytes.
    let rgba = unsafe { bytes(rgba, len) };
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.pointer_shape(rgba, width, height, hotspot_x, hotspot_y);
}

#[allow(unsafe_code)]
unsafe extern "C" fn pointer_hidden<H: Handler>(user: *mut c_void) {
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.pointer_hidden();
}

#[allow(unsafe_code)]
unsafe extern "C" fn pointer_default<H: Handler>(user: *mut c_void) {
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.pointer_default();
}

#[allow(unsafe_code)]
unsafe extern "C" fn display_control_ready<H: Handler>(user: *mut c_void) {
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.display_control_ready();
}

#[allow(unsafe_code)]
unsafe extern "C" fn clip_monitor_ready<H: Handler>(user: *mut c_void) {
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.clip(ClipEvent::MonitorReady);
}

#[allow(unsafe_code)]
unsafe extern "C" fn clip_capabilities<H: Handler>(user: *mut c_void, flags: u32) {
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.clip(ClipEvent::Capabilities(flags));
}

#[allow(unsafe_code)]
unsafe extern "C" fn clip_format_list<H: Handler>(
    user: *mut c_void,
    ids: *const u32,
    names: *const *const c_char,
    count: u32,
) {
    let count = count as usize;
    let formats = (0..count)
        .map(|i| {
            // SAFETY: the shim passes `count` ids and names (a name may be NULL).
            let (id, name) = unsafe { (*ids.add(i), *names.add(i)) };
            Format {
                id,
                // SAFETY: NULL or a NUL-terminated string.
                name: (!name.is_null()).then(|| unsafe { text(name) }),
            }
        })
        .collect();
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.clip(ClipEvent::FormatList(formats));
}

#[allow(unsafe_code)]
unsafe extern "C" fn clip_data_request<H: Handler>(user: *mut c_void, format: u32) {
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.clip(ClipEvent::DataRequest(format));
}

#[allow(unsafe_code)]
unsafe extern "C" fn clip_data_response<H: Handler>(user: *mut c_void, ok: c_int, data: *const u8, len: u32) {
    // SAFETY: the shim passes `len` bytes.
    let data = (ok != 0).then(|| unsafe { bytes(data, len as usize) }.to_vec());
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.clip(ClipEvent::DataResponse(data));
}

#[allow(unsafe_code)]
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn clip_contents_request<H: Handler>(
    user: *mut c_void,
    stream_id: u32,
    index: u32,
    flags: u32,
    position: u64,
    requested: u32,
    has_data_id: c_int,
    data_id: u32,
) {
    let request = ContentsRequest {
        stream_id,
        index,
        flags,
        position,
        requested,
        data_id: (has_data_id != 0).then_some(data_id),
    };
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.clip(ClipEvent::ContentsRequest(request));
}

#[allow(unsafe_code)]
unsafe extern "C" fn clip_contents_response<H: Handler>(
    user: *mut c_void,
    stream_id: u32,
    ok: c_int,
    data: *const u8,
    len: u32,
) {
    // SAFETY: the shim passes `len` bytes.
    let data = (ok != 0).then(|| unsafe { bytes(data, len as usize) }.to_vec());
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.clip(ClipEvent::ContentsResponse { stream_id, data });
}

#[allow(unsafe_code)]
unsafe extern "C" fn clip_lock<H: Handler>(user: *mut c_void, data_id: u32, locked: c_int) {
    // SAFETY: as above.
    unsafe { handler::<H>(user) }.clip(ClipEvent::Lock {
        data_id,
        locked: locked != 0,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Nothing;

    impl Handler for Nothing {
        fn verify_certificate(&mut self, _: &[u8], _: &str, _: u16, _: u32) -> bool {
            false
        }
        fn authenticate(&mut self) -> Option<Credentials> {
            None
        }
        fn desktop(&mut self, _: u32, _: u32) {}
        fn painted(&mut self, _: &[Rect]) {}
        fn pointer_shape(&mut self, _: &[u8], _: u32, _: u32, _: u32, _: u32) {}
        fn pointer_hidden(&mut self) {}
        fn pointer_default(&mut self) {}
        fn display_control_ready(&mut self) {}
        fn clip(&mut self, _: ClipEvent) {}
    }

    #[test]
    fn links_the_pinned_freerdp() {
        assert!(library_version().starts_with(VERSION), "{}", library_version());
        assert!(!last_error_string(0x0002_000B).is_empty());
    }

    #[test]
    fn a_refused_connection_fails_without_a_session() {
        let mut connection = Connection::new(Nothing).unwrap();
        let config_dir = std::env::temp_dir()
            .join("rd-freerdp-test")
            .to_string_lossy()
            .into_owned();
        let config = Config {
            host: "127.0.0.1",
            port: 1,
            username: "",
            domain: "",
            password: "",
            client_name: "test",
            config_dir: &config_dir,
            width: 800,
            height: 600,
            console_session: false,
            clipboard: false,
        };
        assert!(!connection.connect(&config));
        assert_ne!(connection.last_error(), 0);
        assert!(connection.framebuffer().is_none());
        connection.remote().wake();
        connection.disconnect();
    }
}
