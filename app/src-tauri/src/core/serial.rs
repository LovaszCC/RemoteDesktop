//! Serial consoles over USB-serial adapters, in embedded terminal tabs. Separate from the SSH sessions
//! (no PTY, no askpass) but with the same output channel, ack flow control and `exit` event, so the tab
//! behaves the same.
//!
//! A session is a saved serial connection (settings from the library, re-read on reconnect unless the
//! tab changed them) or ad hoc (the settings it was opened with). The port is picked at connect time.
//! One port belongs to at most one tab: opening a held port fails with `portInUse` unless the caller
//! takes it over, which disconnects the other tab (it shows "Connection lost" and keeps its output).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rd_core::{ConnectionType, new_id};
use rd_serial::serialport::SerialPort;
use rd_serial::{OpenError, OutputLog, SerialSession, SerialSettings, UsbPort};
use rd_ssh::{FlowControl, Sink};
use serde::Serialize;

use super::{AppCore, CommandError};

/// Opens a port (test hook: a pseudo-terminal instead of a device).
pub type PortOpener =
    Arc<dyn Fn(&str, SerialSettings) -> Result<Box<dyn SerialPort>, OpenError> + Send + Sync>;
/// Lists the USB-serial ports (test hook).
pub type PortLister = Arc<dyn Fn() -> Vec<UsbPort> + Send + Sync>;

/// A port in the picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortDto {
    /// Device path.
    pub path: String,
    /// What the adapter calls itself.
    pub label: String,
    /// The title of the tab that holds the port, if one does.
    pub in_use_by: Option<String>,
}

struct Entry {
    title: String,
    /// The saved connection; `None` for an ad-hoc console.
    connection_id: Option<String>,
    settings: SerialSettings,
    /// The tab changed the settings of its saved connection without saving them: they win on reconnect.
    overridden: bool,
    path: String,
    sink: Arc<dyn Sink>,
    log: Arc<OutputLog>,
    run: Option<Arc<SerialSession>>,
}

impl Entry {
    fn holds(&self, path: &str) -> bool {
        self.path == path && self.run.as_ref().is_some_and(|r| r.is_running())
    }
}

/// The serial state of the app.
pub(crate) struct SerialShared {
    sessions: Mutex<HashMap<String, Entry>>,
    dir: Mutex<PathBuf>,
    opener: Mutex<Option<PortOpener>>,
    lister: Mutex<Option<PortLister>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SerialShared {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            sessions: Mutex::default(),
            dir: Mutex::new(rd_serial::log::default_dir()),
            opener: Mutex::default(),
            lister: Mutex::default(),
        })
    }
}

fn session_not_found(id: &str) -> CommandError {
    CommandError::new("sessionNotFound", "The session doesn't exist any more.").with_detail(id)
}

fn invalid_settings() -> CommandError {
    CommandError::new("invalidSerialSettings", "The serial settings are not valid.")
}

fn open_error(error: OpenError, path: &str) -> CommandError {
    match error {
        OpenError::Missing => {
            CommandError::new("portMissing", "The serial device is not connected.").with_detail(path)
        }
        OpenError::Busy => {
            CommandError::new("portBusy", "Another program is using the serial port.").with_detail(path)
        }
        OpenError::PermissionDenied => CommandError::new(
            "portPermission",
            "You may not open the serial port. On Linux, add your user to the dialout group.",
        )
        .with_detail(path),
        OpenError::InvalidSettings(m) => invalid_settings().with_detail(m),
        OpenError::Other(m) => {
            CommandError::new("serialFailed", "Couldn't open the serial port.").with_detail(m)
        }
    }
}

impl AppCore {
    /// Test hooks: open ports with `opener`, list them with `lister`, keep output files in `dir`.
    pub fn set_serial_hooks(
        &self,
        opener: Option<PortOpener>,
        lister: Option<PortLister>,
        dir: Option<PathBuf>,
    ) {
        *lock(&self.serial.opener) = opener;
        *lock(&self.serial.lister) = lister;
        if let Some(dir) = dir {
            *lock(&self.serial.dir) = dir;
        }
    }

    /// The USB-serial adapters plugged in now, each with the tab that holds it.
    ///
    /// # Errors
    /// `serialFailed` when the OS can't list ports.
    pub fn serial_ports(&self) -> Result<Vec<SerialPortDto>, CommandError> {
        let lister = lock(&self.serial.lister).clone();
        let ports = match lister {
            Some(list) => list(),
            None => rd_serial::ports::list().map_err(|e| {
                CommandError::new("serialFailed", "Couldn't list the serial ports.")
                    .with_detail(e.to_string())
            })?,
        };
        let sessions = lock(&self.serial.sessions);
        Ok(ports
            .into_iter()
            .map(|p| SerialPortDto {
                in_use_by: sessions
                    .values()
                    .find(|e| e.holds(&p.path))
                    .map(|e| e.title.clone()),
                path: p.path,
                label: p.label,
            })
            .collect())
    }

    /// The saved settings of serial connection `id`.
    fn serial_connection_settings(&self, id: &str) -> Result<SerialSettings, CommandError> {
        let inner = self.inner();
        let (db, _) = Self::unlocked(&inner)?;
        let connection = db
            .library()?
            .connections
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| CommandError::not_found(id))?;
        if connection.kind != ConnectionType::Serial {
            return Err(CommandError::new("notSerial", "This is not a serial connection.").with_detail(id));
        }
        Ok(SerialSettings::of(&connection))
    }

    /// Frees `path` for session `except`: `portInUse` (detail: the holder's title) unless `take_over`,
    /// which disconnects the holder.
    fn claim_port(&self, path: &str, except: Option<&str>, take_over: bool) -> Result<(), CommandError> {
        let holder = {
            let sessions = lock(&self.serial.sessions);
            sessions
                .iter()
                .find(|(id, e)| Some(id.as_str()) != except && e.holds(path))
                .map(|(_, e)| (e.title.clone(), e.run.clone()))
        };
        let Some((title, run)) = holder else {
            return Ok(());
        };
        if !take_over {
            return Err(
                CommandError::new("portInUse", "Another tab is using the serial port.").with_detail(title),
            );
        }
        if let Some(run) = run {
            run.disconnect();
        }
        Ok(())
    }

    fn open_port(
        &self,
        path: &str,
        settings: SerialSettings,
        sink: Arc<dyn Sink>,
        log: Arc<OutputLog>,
    ) -> Result<SerialSession, CommandError> {
        let flow = Some(FlowControl::WEBVIEW);
        let opener = lock(&self.serial.opener).clone();
        match opener {
            Some(open) => {
                open(path, settings).and_then(|port| SerialSession::from_port(port, path, sink, log, flow))
            }
            None => SerialSession::open(path, settings, sink, log, flow),
        }
        .map_err(|e| open_error(e, path))
    }

    /// Opens a serial console on `path`: with the settings of serial connection `connection_id`, else
    /// (ad hoc) with `settings`. `title` names the tab (for `portInUse`). Returns the session id.
    ///
    /// # Errors
    /// `locked`, `notFound`, `notSerial`, `invalidSerialSettings`, `portInUse`, `portMissing`, `portBusy`,
    /// `portPermission`, `serialFailed`.
    pub fn open_serial_session(
        &self,
        connection_id: Option<&str>,
        path: &str,
        settings: Option<SerialSettings>,
        title: &str,
        take_over: bool,
        make_sink: impl FnOnce(&str) -> Arc<dyn Sink>,
    ) -> Result<String, CommandError> {
        let settings = match connection_id {
            Some(id) => self.serial_connection_settings(id)?,
            None => settings.ok_or_else(invalid_settings)?,
        };
        if !settings.is_valid() {
            return Err(invalid_settings());
        }
        self.claim_port(path, None, take_over)?;
        let session_id = new_id();
        let dir = lock(&self.serial.dir).clone();
        let log = Arc::new(OutputLog::create(&dir, &session_id).map_err(|e| {
            CommandError::new("serialFailed", "Couldn't create the output file.").with_detail(e.to_string())
        })?);
        let sink = make_sink(&session_id);
        let run = self.open_port(path, settings, Arc::clone(&sink), Arc::clone(&log))?;
        lock(&self.serial.sessions).insert(
            session_id.clone(),
            Entry {
                title: title.to_owned(),
                connection_id: connection_id.map(str::to_owned),
                settings,
                overridden: false,
                path: path.to_owned(),
                sink,
                log,
                run: Some(Arc::new(run)),
            },
        );
        self.ssh_sessions_changed();
        Ok(session_id)
    }

    /// Opens the port again in the same tab (same output file): `path`, else the one it had. A saved
    /// connection's settings are read anew unless the tab changed them.
    ///
    /// # Errors
    /// `sessionNotFound`, `portMissing` (the device is gone or was renamed: pick a port), and those of
    /// [`open_serial_session`](Self::open_serial_session).
    pub fn serial_reconnect(
        &self,
        session_id: &str,
        path: Option<&str>,
        take_over: bool,
    ) -> Result<(), CommandError> {
        let (connection_id, overridden, mut settings, old_path, sink, log, old) = {
            let sessions = lock(&self.serial.sessions);
            let e = sessions
                .get(session_id)
                .ok_or_else(|| session_not_found(session_id))?;
            (
                e.connection_id.clone(),
                e.overridden,
                e.settings,
                e.path.clone(),
                Arc::clone(&e.sink),
                Arc::clone(&e.log),
                e.run.clone(),
            )
        };
        if let (Some(id), false) = (&connection_id, overridden) {
            settings = self.serial_connection_settings(id)?;
        }
        let path = path.unwrap_or(&old_path).to_owned();
        if let Some(old) = old {
            old.close();
        }
        self.claim_port(&path, Some(session_id), take_over)?;
        let language = self.inner().settings.language.clone();
        sink.output(
            format!(
                "\r\n\u{1b}[2m— {} —\u{1b}[0m\r\n",
                crate::menu::tr(&language, "Reconnecting")
            )
            .into_bytes(),
        );
        let run = self.open_port(&path, settings, Arc::clone(&sink), log)?;
        if let Some(e) = lock(&self.serial.sessions).get_mut(session_id) {
            e.run = Some(Arc::new(run));
            e.path = path;
            e.settings = settings;
        }
        self.ssh_sessions_changed();
        Ok(())
    }

    /// The settings session `session_id` runs with.
    ///
    /// # Errors
    /// `sessionNotFound`.
    pub fn serial_session_settings(&self, session_id: &str) -> Result<SerialSettings, CommandError> {
        lock(&self.serial.sessions)
            .get(session_id)
            .map(|e| e.settings)
            .ok_or_else(|| session_not_found(session_id))
    }

    /// "Settings…" on a tab: applies `settings` to the open port (the tab and its output stay). With
    /// `save` they are also written into the tab's saved connection. Returns whether the library changed.
    ///
    /// # Errors
    /// `sessionNotFound`, `invalidSerialSettings`, `readOnly`, `locked`, `notFound`, or the setting the
    /// device refused.
    pub fn serial_apply_settings(
        &self,
        session_id: &str,
        settings: SerialSettings,
        save: bool,
    ) -> Result<bool, CommandError> {
        if !settings.is_valid() {
            return Err(invalid_settings());
        }
        let (connection_id, run, path) = {
            let sessions = lock(&self.serial.sessions);
            let e = sessions
                .get(session_id)
                .ok_or_else(|| session_not_found(session_id))?;
            (e.connection_id.clone(), e.run.clone(), e.path.clone())
        };
        if let Some(run) = run.filter(|r| r.is_running()) {
            run.reconfigure(settings).map_err(|e| open_error(e, &path))?;
        }
        let saved = match (&connection_id, save) {
            (Some(id), true) => {
                let inner = self.inner();
                let (db, _) = Self::writable(&inner)?;
                let mut connection = db
                    .library()?
                    .connections
                    .into_iter()
                    .find(|c| c.id == *id)
                    .ok_or_else(|| CommandError::not_found(id))?;
                settings.apply_to(&mut connection);
                db.save_connection(&connection)?;
                true
            }
            _ => false,
        };
        if let Some(e) = lock(&self.serial.sessions).get_mut(session_id) {
            e.settings = settings;
            e.overridden = connection_id.is_some() && !saved;
        }
        Ok(saved)
    }

    fn serial_run(&self, session_id: &str) -> Result<Option<Arc<SerialSession>>, CommandError> {
        lock(&self.serial.sessions)
            .get(session_id)
            .map(|e| e.run.clone())
            .ok_or_else(|| session_not_found(session_id))
    }

    /// Typed input.
    ///
    /// # Errors
    /// `sessionNotFound`, or `sessionEnded` after the port went away.
    pub fn serial_input(&self, session_id: &str, data: &[u8]) -> Result<(), CommandError> {
        let ended = || CommandError::new("sessionEnded", "The session has ended.");
        let run = self.serial_run(session_id)?.ok_or_else(ended)?;
        run.write(data).map_err(|e| ended().with_detail(e.to_string()))
    }

    /// The terminal parsed `bytes` of output.
    ///
    /// # Errors
    /// `sessionNotFound`.
    pub fn serial_ack(&self, session_id: &str, bytes: usize) -> Result<(), CommandError> {
        if let Some(run) = self.serial_run(session_id)? {
            run.ack(bytes);
        }
        Ok(())
    }

    /// Closes the port and forgets the session; its output file is deleted.
    pub fn serial_close(&self, session_id: &str) {
        let entry = lock(&self.serial.sessions).remove(session_id);
        if let Some(run) = entry.as_ref().and_then(|e| e.run.as_ref()) {
            run.close();
        }
        drop(entry);
        self.ssh_sessions_changed();
    }

    /// The tab's whole output so far as plain text.
    ///
    /// # Errors
    /// `sessionNotFound`, `serialFailed` when the file can't be read.
    pub fn serial_output_text(&self, session_id: &str) -> Result<String, CommandError> {
        let log = lock(&self.serial.sessions)
            .get(session_id)
            .map(|e| Arc::clone(&e.log))
            .ok_or_else(|| session_not_found(session_id))?;
        log.plain_text().map_err(|e| {
            CommandError::new("serialFailed", "Couldn't read the output.").with_detail(e.to_string())
        })
    }

    /// Serial consoles whose port is open.
    #[must_use]
    pub fn running_serial_sessions(&self) -> usize {
        lock(&self.serial.sessions)
            .values()
            .filter(|e| e.run.as_ref().is_some_and(|r| r.is_running()))
            .count()
    }

    /// Closes every serial console (quit).
    pub fn close_all_serial(&self) {
        let entries: Vec<Entry> = lock(&self.serial.sessions).drain().map(|(_, e)| e).collect();
        for entry in &entries {
            if let Some(run) = &entry.run {
                run.close();
            }
        }
    }
}
