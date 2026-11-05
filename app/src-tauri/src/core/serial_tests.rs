//! Serial consoles against pseudo-terminals standing in for USB-serial adapters.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use rd_core::draft::ConnectionDraft;
use rd_core::shortcuts::Platform;
use rd_core::{ConnectionType, SerialParity};
use rd_platform::Settings;
use rd_serial::serialport::TTYPort;
use rd_serial::{OpenError, SerialSettings, UsbPort};
use rd_ssh::{SessionEvent, Sink};
use rd_store::MemoryKeyCache;
use tempfile::TempDir;

use super::tests::{entry_draft, save_single};
use super::*;

const TIMEOUT: Duration = Duration::from_secs(5);
const PORT_A: &str = "/dev/cu.usbserial-A";
const PORT_B: &str = "/dev/cu.usbserial-B";

#[derive(Default)]
struct Collect {
    output: Mutex<Vec<u8>>,
    output_changed: Condvar,
    events: Mutex<Vec<SessionEvent>>,
    events_changed: Condvar,
}

impl Sink for Collect {
    fn output(&self, bytes: Vec<u8>) {
        self.output.lock().unwrap().extend(bytes);
        self.output_changed.notify_all();
    }
    fn event(&self, event: SessionEvent) {
        self.events.lock().unwrap().push(event);
        self.events_changed.notify_all();
    }
}

impl Collect {
    fn wait_output(&self, needle: &str) {
        let output = self
            .output_changed
            .wait_timeout_while(self.output.lock().unwrap(), TIMEOUT, |o| {
                !String::from_utf8_lossy(o).contains(needle)
            })
            .unwrap()
            .0;
        assert!(
            String::from_utf8_lossy(&output).contains(needle),
            "waiting for {needle:?}"
        );
    }

    fn wait_exits(&self, count: usize) {
        let events = self
            .events_changed
            .wait_timeout_while(self.events.lock().unwrap(), TIMEOUT, |e| e.len() < count)
            .unwrap()
            .0;
        assert_eq!(events.len(), count, "{events:?}");
        assert!(
            events
                .iter()
                .all(|e| matches!(e, SessionEvent::Exit { code: None, .. }))
        );
    }

    fn exits(&self) -> usize {
        self.events.lock().unwrap().len()
    }
}

/// The "device" side of each opened port, by path; the settings each open used.
#[derive(Default)]
struct Devices {
    masters: Mutex<HashMap<String, TTYPort>>,
    opened: Mutex<Vec<(String, SerialSettings)>>,
    unplugged: Mutex<Vec<String>>,
}

struct Lab {
    _dir: TempDir,
    logs: TempDir,
    core: AppCore,
    devices: Arc<Devices>,
}

impl Lab {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let settings_dir = dir.path().join("config");
        Settings {
            database_path: Some(dir.path().join("data").join("connections.sqlite")),
            ..Settings::default()
        }
        .save(&settings_dir)
        .unwrap();
        let core = AppCore::with_options(
            settings_dir,
            Box::new(MemoryKeyCache::default()),
            1_000,
            Platform::MacOS,
        );
        core.open_current();
        core.create_vault("master", false).unwrap();
        let devices = Arc::new(Devices::default());
        let logs = tempfile::tempdir().unwrap();
        let open_devices = Arc::clone(&devices);
        let list_devices = Arc::clone(&devices);
        core.set_serial_hooks(
            Some(Arc::new(move |path: &str, settings| {
                if open_devices.unplugged.lock().unwrap().iter().any(|p| p == path) {
                    return Err(OpenError::Missing);
                }
                let (master, slave) = TTYPort::pair().map_err(|e| OpenError::Other(e.to_string()))?;
                open_devices
                    .masters
                    .lock()
                    .unwrap()
                    .insert(path.to_owned(), master);
                open_devices
                    .opened
                    .lock()
                    .unwrap()
                    .push((path.to_owned(), settings));
                Ok(Box::new(slave) as Box<dyn rd_serial::serialport::SerialPort>)
            })),
            Some(Arc::new(move || {
                [PORT_A, PORT_B]
                    .into_iter()
                    .filter(|p| !list_devices.unplugged.lock().unwrap().iter().any(|u| u == p))
                    .map(|p| UsbPort {
                        path: p.to_owned(),
                        label: "FT232R".into(),
                        vid: 0x0403,
                        pid: 0x6001,
                    })
                    .collect()
            })),
            Some(logs.path().to_path_buf()),
        );
        Self {
            _dir: dir,
            logs,
            core,
            devices,
        }
    }

    fn device_says(&self, path: &str, text: &str) {
        self.devices
            .masters
            .lock()
            .unwrap()
            .get_mut(path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    fn unplug(&self, path: &str) {
        self.devices.unplugged.lock().unwrap().push(path.to_owned());
        self.devices.masters.lock().unwrap().remove(path);
    }

    fn last_settings(&self) -> SerialSettings {
        self.devices.opened.lock().unwrap().last().unwrap().1
    }

    fn serial_connection(&self, baud: u32) -> String {
        let mut draft = ConnectionDraft::new(ConnectionType::Serial);
        draft.serial_baud = baud;
        save_single(&self.core, None, &entry_draft("OPNsense", ""), draft, None, None).unwrap()
    }

    fn open(
        &self,
        connection: Option<&str>,
        path: &str,
        title: &str,
        take_over: bool,
    ) -> (String, Arc<Collect>) {
        let sink = Arc::new(Collect::default());
        let as_sink: Arc<dyn Sink> = Arc::clone(&sink) as Arc<dyn Sink>;
        let settings = connection.is_none().then(SerialSettings::default);
        let id = self
            .core
            .open_serial_session(connection, path, settings, title, take_over, |_| as_sink)
            .unwrap();
        (id, sink)
    }

    fn log_files(&self) -> usize {
        std::fs::read_dir(self.logs.path()).unwrap().count()
    }
}

#[test]
fn ad_hoc_console_streams_both_ways_and_close_deletes_the_output_file() {
    let lab = Lab::new();
    let (id, sink) = lab.open(None, PORT_A, "Serial Console", false);
    assert_eq!(lab.last_settings(), SerialSettings::default());
    lab.device_says(PORT_A, "\x1b[1mOPNsense\x1b[0m login: ");
    sink.wait_output("login: ");
    lab.core.serial_input(&id, b"root\r").unwrap();
    lab.core.serial_ack(&id, 1024).unwrap();
    assert_eq!(lab.core.serial_output_text(&id).unwrap(), "OPNsense login: ");
    assert_eq!(lab.core.running_serial_sessions(), 1);
    assert_eq!(lab.log_files(), 1);

    lab.core.serial_close(&id);
    assert_eq!(lab.log_files(), 0);
    assert_eq!(sink.exits(), 0);
    assert_eq!(
        lab.core.serial_input(&id, b"x").unwrap_err().code,
        "sessionNotFound"
    );
}

#[test]
fn saved_connection_opens_with_its_settings_and_needs_no_host() {
    let lab = Lab::new();
    let connection = lab.serial_connection(9600);
    let (id, _sink) = lab.open(Some(&connection), PORT_A, "OPNsense", false);
    assert_eq!(lab.last_settings().baud, 9600);
    assert_eq!(lab.core.serial_session_settings(&id).unwrap().baud, 9600);

    let ssh = save_single(
        &lab.core,
        None,
        &entry_draft("web", "web.example.com"),
        ConnectionDraft::new(ConnectionType::Ssh),
        None,
        None,
    )
    .unwrap();
    let error = lab
        .core
        .open_serial_session(Some(&ssh), PORT_B, None, "web", false, |_| {
            Arc::new(Collect::default())
        })
        .unwrap_err();
    assert_eq!(error.code, "notSerial");
}

#[test]
fn a_held_port_is_marked_and_taking_it_over_disconnects_the_holder() {
    let lab = Lab::new();
    let (a, sink_a) = lab.open(None, PORT_A, "OPNsense A", false);
    let ports = lab.core.serial_ports().unwrap();
    assert_eq!(ports[0].in_use_by.as_deref(), Some("OPNsense A"));
    assert_eq!(ports[1].in_use_by, None);

    let error = lab
        .core
        .open_serial_session(None, PORT_A, Some(SerialSettings::default()), "B", false, |_| {
            Arc::new(Collect::default())
        })
        .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.detail.as_deref()),
        ("portInUse", Some("OPNsense A"))
    );

    let (_b, _sink_b) = lab.open(None, PORT_A, "B", true);
    sink_a.wait_exits(1);
    assert_eq!(
        lab.core.serial_ports().unwrap()[0].in_use_by.as_deref(),
        Some("B")
    );
    // The disconnected tab keeps its output file.
    assert!(lab.core.serial_output_text(&a).is_ok());
    assert_eq!(lab.log_files(), 2);
}

#[test]
fn unplugging_reports_lost_and_reconnect_needs_a_port_when_the_device_is_gone() {
    let lab = Lab::new();
    let (id, sink) = lab.open(None, PORT_A, "Serial Console", false);
    lab.device_says(PORT_A, "boot log\r\n");
    sink.wait_output("boot log");
    lab.unplug(PORT_A);
    sink.wait_exits(1);
    assert_eq!(lab.core.running_serial_sessions(), 0);

    let error = lab.core.serial_reconnect(&id, None, false).unwrap_err();
    assert_eq!(error.code, "portMissing");

    lab.core.serial_reconnect(&id, Some(PORT_B), false).unwrap();
    lab.device_says(PORT_B, "after\r\n");
    sink.wait_output("after");
    // Output from before the disconnect is still there.
    let text = lab.core.serial_output_text(&id).unwrap();
    assert!(text.starts_with("boot log\n"), "{text:?}");
    assert!(text.contains("after"));
    assert_eq!(
        lab.core.serial_ports().unwrap()[0].in_use_by.as_deref(),
        Some("Serial Console")
    );
}

#[test]
fn reconnect_rereads_the_connection_unless_the_tab_changed_its_settings() {
    let lab = Lab::new();
    let connection = lab.serial_connection(115_200);
    let (id, sink) = lab.open(Some(&connection), PORT_A, "OPNsense", false);

    // Edited in the library: the reconnect picks it up.
    let mut draft = ConnectionDraft::new(ConnectionType::Serial);
    draft.serial_baud = 57_600;
    save_single(
        &lab.core,
        Some(&connection),
        &entry_draft("OPNsense", ""),
        draft,
        None,
        None,
    )
    .unwrap();
    lab.core.serial_reconnect(&id, None, false).unwrap();
    assert_eq!(lab.last_settings().baud, 57_600);

    // A pseudo-terminal refuses baud changes, so the port is gone while the settings change (they then
    // apply on the next open).
    lab.unplug(PORT_A);
    sink.wait_exits(1);

    // Changed on the tab only: the tab's settings win.
    let tab_only = SerialSettings {
        baud: 9600,
        parity: SerialParity::Even,
        ..SerialSettings::default()
    };
    assert!(!lab.core.serial_apply_settings(&id, tab_only, false).unwrap());
    lab.core.serial_reconnect(&id, Some(PORT_B), false).unwrap();
    assert_eq!(lab.last_settings(), tab_only);
    let saved = &lab.core.library().connections[0];
    assert_eq!(saved.serial_baud, 57_600);

    // Saved to the connection.
    let saved_settings = SerialSettings {
        baud: 38_400,
        ..SerialSettings::default()
    };
    lab.unplug(PORT_B);
    sink.wait_exits(2);
    assert!(lab.core.serial_apply_settings(&id, saved_settings, true).unwrap());
    lab.devices.unplugged.lock().unwrap().clear();
    assert_eq!(lab.core.library().connections[0].serial_baud, 38_400);
    lab.core.serial_reconnect(&id, None, false).unwrap();
    assert_eq!(lab.last_settings().baud, 38_400);

    let bad = SerialSettings {
        baud: 0,
        ..SerialSettings::default()
    };
    assert_eq!(
        lab.core.serial_apply_settings(&id, bad, false).unwrap_err().code,
        "invalidSerialSettings"
    );
}

#[test]
fn quitting_closes_every_console() {
    let lab = Lab::new();
    let (_a, sink_a) = lab.open(None, PORT_A, "A", false);
    let (_b, _sink_b) = lab.open(None, PORT_B, "B", false);
    assert_eq!(lab.core.running_count(), 2);
    lab.core.close();
    assert_eq!(lab.core.running_serial_sessions(), 0);
    assert_eq!(lab.log_files(), 0);
    assert_eq!(sink_a.exits(), 0);
}
