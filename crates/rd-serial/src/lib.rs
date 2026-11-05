//! USB-serial consoles: listing the plugged-in USB-serial adapters ([`ports`]), the session that
//! connects one to the embedded terminal ([`session`]) and the temporary file that keeps its whole
//! output for "Copy output" and "Save to file" ([`log`]).

pub mod log;
pub mod ports;
pub mod session;

use rd_core::{Connection, SerialFlowControl, SerialParity, SerialStopBits};
use serde::{Deserialize, Serialize};

pub use log::OutputLog;
pub use ports::UsbPort;
pub use serialport;
pub use session::{OpenError, SerialSession};

/// How the port is configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialSettings {
    /// Baud rate.
    pub baud: u32,
    /// Data bits, 5–8.
    pub data_bits: u8,
    /// Parity.
    pub parity: SerialParity,
    /// Stop bits.
    pub stop_bits: SerialStopBits,
    /// Flow control.
    pub flow_control: SerialFlowControl,
}

impl Default for SerialSettings {
    /// 115200 8N1, no flow control.
    fn default() -> Self {
        Self {
            baud: Connection::DEFAULT_SERIAL_BAUD,
            data_bits: Connection::DEFAULT_SERIAL_DATA_BITS,
            parity: SerialParity::None,
            stop_bits: SerialStopBits::One,
            flow_control: SerialFlowControl::None,
        }
    }
}

impl SerialSettings {
    /// The settings saved on a serial connection.
    #[must_use]
    pub fn of(connection: &Connection) -> Self {
        Self {
            baud: connection.serial_baud,
            data_bits: connection.serial_data_bits,
            parity: connection.serial_parity,
            stop_bits: connection.serial_stop_bits,
            flow_control: connection.serial_flow_control,
        }
    }

    /// Writes the settings into a serial connection.
    pub fn apply_to(self, connection: &mut Connection) {
        connection.serial_baud = self.baud;
        connection.serial_data_bits = self.data_bits;
        connection.serial_parity = self.parity;
        connection.serial_stop_bits = self.stop_bits;
        connection.serial_flow_control = self.flow_control;
    }

    /// The settings are usable: a positive baud rate and 5–8 data bits.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.baud > 0 && (5..=8).contains(&self.data_bits)
    }
}

#[cfg(test)]
mod tests {
    use rd_core::ConnectionType;

    use super::*;

    #[test]
    fn defaults_are_115200_8n1() {
        let s = SerialSettings::default();
        assert_eq!((s.baud, s.data_bits), (115_200, 8));
        assert_eq!(s.parity, SerialParity::None);
        assert_eq!(s.stop_bits, SerialStopBits::One);
        assert_eq!(s.flow_control, SerialFlowControl::None);
        assert!(s.is_valid());
    }

    #[test]
    fn round_trips_through_a_connection_and_json() {
        let mut c = Connection::new(ConnectionType::Serial, "OPNsense", "");
        let s = SerialSettings {
            baud: 9600,
            data_bits: 7,
            parity: SerialParity::Even,
            stop_bits: SerialStopBits::Two,
            flow_control: SerialFlowControl::Hardware,
        };
        s.apply_to(&mut c);
        assert_eq!(SerialSettings::of(&c), s);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            json,
            r#"{"baud":9600,"dataBits":7,"parity":"even","stopBits":"2","flowControl":"hardware"}"#
        );
        assert!(!SerialSettings { baud: 0, ..s }.is_valid());
        assert!(!SerialSettings { data_bits: 9, ..s }.is_valid());
    }
}
