//! The Windows low-level keyboard hook. The hook procedure is a plain function the OS calls on the
//! thread that installed it, so the sink and the key state live in statics; one capture runs at a time.
#![allow(unsafe_code)]

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread::JoinHandle;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_ALTDOWN, LLKHF_EXTENDED,
    LLKHF_INJECTED, MSG, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
};

use super::{CaptureError, CapturedKey, Held, KeySink, vk, windows_should_capture};

/// The sink of the running capture.
static SINK: Mutex<Option<KeySink>> = Mutex::new(None);
/// Keys whose press was captured: their release is captured too.
static CAPTURED: Mutex<Option<HashSet<u32>>> = Mutex::new(None);
static CTRL: AtomicBool = AtomicBool::new(false);
static WIN: AtomicBool = AtomicBool::new(false);

const VK_CONTROL_KEYS: [u32; 3] = [0x11, 0xA2, 0xA3];

/// The hook procedure.
unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION.cast_signed() {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, `lparam` points to a valid KBDLLHOOKSTRUCT for the
        // duration of this call (documented contract of LowLevelKeyboardProc).
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if handle(info, u32::try_from(wparam).unwrap_or_default()) {
            return 1;
        }
    }
    // SAFETY: passing the call on unchanged, as the hook contract requires; the hook handle argument is
    // ignored by the OS.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Updates the modifier state and forwards the key when it is captured; `true` swallows it.
fn handle(info: &KBDLLHOOKSTRUCT, message: u32) -> bool {
    let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
    let key = info.vkCode;
    if VK_CONTROL_KEYS.contains(&key) {
        CTRL.store(down, Ordering::SeqCst);
    }
    if info.flags & LLKHF_INJECTED != 0 {
        return false;
    }
    let held = Held {
        alt: info.flags & LLKHF_ALTDOWN != 0,
        ctrl: CTRL.load(Ordering::SeqCst),
        win: WIN.load(Ordering::SeqCst),
    };
    if matches!(key, vk::LWIN | vk::RWIN) {
        WIN.store(down, Ordering::SeqCst);
    }
    let mut captured = CAPTURED.lock().unwrap_or_else(PoisonError::into_inner);
    let set = captured.get_or_insert_with(HashSet::new);
    let take = if down {
        windows_should_capture(key, held) && {
            set.insert(key);
            true
        }
    } else {
        set.remove(&key)
    };
    drop(captured);
    if !take {
        return false;
    }
    let mut scancode = (info.scanCode & 0xFF) as u16;
    if info.flags & LLKHF_EXTENDED != 0 {
        scancode |= 0x100;
    }
    let sink = SINK.lock().unwrap_or_else(PoisonError::into_inner).clone();
    if let Some(sink) = sink {
        sink(CapturedKey { scancode, down });
    }
    true
}

/// The hook thread; dropping it unhooks.
pub struct Capture {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    pub fn start(sink: KeySink) -> Result<Self, CaptureError> {
        *SINK.lock().unwrap_or_else(PoisonError::into_inner) = Some(sink);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("rdp-keyboard-hook".into())
            .spawn(move || {
                // SAFETY: GetModuleHandleW(null) returns this executable's module handle and has no
                // preconditions.
                let module = unsafe { GetModuleHandleW(std::ptr::null()) };
                // SAFETY: `hook` matches the LowLevelKeyboardProc signature and stays valid for the
                // program's lifetime; a low-level hook is installed for this thread's message loop.
                let handle = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), module, 0) };
                // SAFETY: no preconditions.
                let thread_id = unsafe { GetCurrentThreadId() };
                if handle.is_null() {
                    let _ = ready_tx.send(Err(std::io::Error::last_os_error().to_string()));
                    return;
                }
                let _ = ready_tx.send(Ok(thread_id));
                // SAFETY: an all-zero MSG is a valid value to be filled by GetMessageW.
                let mut message: MSG = unsafe { std::mem::zeroed() };
                // SAFETY: `message` is a valid, writable MSG; the loop ends on WM_QUIT (0) or error (-1).
                while unsafe { GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0) } > 0 {
                    // SAFETY: `message` was filled by GetMessageW.
                    unsafe {
                        TranslateMessage(&raw const message);
                        DispatchMessageW(&raw const message);
                    }
                }
                // SAFETY: `handle` is the hook installed above, not yet removed.
                unsafe { UnhookWindowsHookEx(handle) };
            })
            .map_err(|e| CaptureError(e.to_string()))?;
        match ready_rx.recv() {
            Ok(Ok(thread_id)) => Ok(Self {
                thread_id,
                thread: Some(thread),
            }),
            Ok(Err(error)) => Err(CaptureError(error)),
            Err(e) => Err(CaptureError(e.to_string())),
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // SAFETY: posting WM_QUIT to the hook thread's queue, which exists while it runs its loop.
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        *SINK.lock().unwrap_or_else(PoisonError::into_inner) = None;
        *CAPTURED.lock().unwrap_or_else(PoisonError::into_inner) = None;
        WIN.store(false, Ordering::SeqCst);
    }
}
