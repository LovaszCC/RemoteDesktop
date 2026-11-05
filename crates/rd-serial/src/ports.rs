//! The plugged-in USB-serial adapters. Built-in serial ports are not listed (USB only), and on macOS only
//! the `/dev/cu.*` device of each adapter (its `/dev/tty.*` twin blocks on carrier detect).

use serde::Serialize;
use serialport::{SerialPortInfo, SerialPortType};

/// A USB-serial adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbPort {
    /// Device path (`/dev/cu.usbserial-…`, `/dev/ttyUSB0`, `COM3`).
    pub path: String,
    /// What the adapter calls itself: product, else manufacturer, else `USB serial`.
    pub label: String,
    /// USB vendor id.
    pub vid: u16,
    /// USB product id.
    pub pid: u16,
}

/// The USB-serial adapters plugged in now, sorted by path.
///
/// # Errors
/// When the OS can't enumerate ports.
pub fn list() -> serialport::Result<Vec<UsbPort>> {
    serialport::available_ports().map(usb_only)
}

/// Keeps the USB ports of `ports` (and not the macOS `/dev/tty.*` devices), sorted by path.
#[must_use]
pub fn usb_only(ports: Vec<SerialPortInfo>) -> Vec<UsbPort> {
    let mut usb: Vec<UsbPort> = ports
        .into_iter()
        .filter(|p| !p.port_name.starts_with("/dev/tty."))
        .filter_map(|p| match p.port_type {
            SerialPortType::UsbPort(info) => {
                let label = [info.product, info.manufacturer]
                    .into_iter()
                    .flatten()
                    .map(|s| s.trim().to_owned())
                    .find(|s| !s.is_empty())
                    .unwrap_or_else(|| "USB serial".to_owned());
                Some(UsbPort {
                    path: p.port_name,
                    label,
                    vid: info.vid,
                    pid: info.pid,
                })
            }
            _ => None,
        })
        .collect();
    usb.sort_by(|a, b| a.path.cmp(&b.path));
    usb.dedup_by(|a, b| a.path == b.path);
    usb
}

#[cfg(test)]
mod tests {
    use serialport::UsbPortInfo;

    use super::*;

    fn usb(name: &str, product: Option<&str>, manufacturer: Option<&str>) -> SerialPortInfo {
        SerialPortInfo {
            port_name: name.to_owned(),
            port_type: SerialPortType::UsbPort(UsbPortInfo {
                vid: 0x0403,
                pid: 0x6001,
                serial_number: None,
                manufacturer: manufacturer.map(str::to_owned),
                product: product.map(str::to_owned),
            }),
        }
    }

    #[test]
    fn keeps_usb_ports_only_and_drops_macos_tty_twins() {
        let ports = vec![
            usb("/dev/tty.usbserial-A10K", Some("FT232R"), None),
            usb("/dev/cu.usbserial-A10K", Some("FT232R"), Some("FTDI")),
            SerialPortInfo {
                port_name: "/dev/ttyS0".into(),
                port_type: SerialPortType::Unknown,
            },
            SerialPortInfo {
                port_name: "COM1".into(),
                port_type: SerialPortType::PciPort,
            },
            usb("/dev/ttyUSB0", None, Some("Prolific")),
            usb("COM7", Some("  "), None),
        ];
        let usb = usb_only(ports);
        assert_eq!(
            usb.iter()
                .map(|p| (p.path.as_str(), p.label.as_str()))
                .collect::<Vec<_>>(),
            [
                ("/dev/cu.usbserial-A10K", "FT232R"),
                ("/dev/ttyUSB0", "Prolific"),
                ("COM7", "USB serial"),
            ]
        );
        assert_eq!((usb[0].vid, usb[0].pid), (0x0403, 0x6001));
    }
}
