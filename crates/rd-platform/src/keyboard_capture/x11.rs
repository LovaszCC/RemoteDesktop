//! The X11 keyboard grab (pure Rust x11rb connection, no unsafe code). While grabbed, every key event of
//! the display comes here and is forwarded; the grab ends when the capture is dropped.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use x11rb::CURRENT_TIME;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{ConnectionExt as _, GrabMode, GrabStatus};

use super::{CaptureError, CapturedKey, KeySink, evdev_to_scancode};

/// The grab thread.
pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    pub fn start(sink: KeySink) -> Result<Self, CaptureError> {
        let (connection, screen) = x11rb::connect(None).map_err(|e| CaptureError(e.to_string()))?;
        let root = connection.setup().roots[screen].root;
        let reply = connection
            .grab_keyboard(false, root, CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)
            .map_err(|e| CaptureError(e.to_string()))?
            .reply()
            .map_err(|e| CaptureError(e.to_string()))?;
        if reply.status != GrabStatus::SUCCESS {
            return Err(CaptureError(format!("keyboard grab refused: {:?}", reply.status)));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("rdp-keyboard-grab".into())
            .spawn(move || {
                while !flag.load(Ordering::SeqCst) {
                    match connection.poll_for_event() {
                        Ok(Some(Event::KeyPress(e))) => forward(&sink, e.detail, true),
                        Ok(Some(Event::KeyRelease(e))) => forward(&sink, e.detail, false),
                        Ok(Some(_)) => {}
                        Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                        Err(_) => break,
                    }
                }
                let _ = connection.ungrab_keyboard(CURRENT_TIME);
                let _ = connection.flush();
            })
            .map_err(|e| CaptureError(e.to_string()))?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

fn forward(sink: &KeySink, keycode: u8, down: bool) {
    if let Some(scancode) = evdev_to_scancode(u16::from(keycode).saturating_sub(8)) {
        sink(CapturedKey { scancode, down });
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
