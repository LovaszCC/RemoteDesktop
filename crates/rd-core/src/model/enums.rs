//! Enums stored as the Swift raw string values.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A stored enum value this build doesn't know.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unknown {kind} value {value:?}")]
pub struct UnknownValue {
    pub(crate) kind: &'static str,
    pub(crate) value: String,
}

macro_rules! raw_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident = $raw:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name {
            $($(#[$vmeta])* #[serde(rename = $raw)] $variant),+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The raw value stored in the database.
            #[must_use]
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $raw),+ }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = UnknownValue;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($raw => Ok(Self::$variant),)+
                    _ => Err(UnknownValue { kind: stringify!($name), value: value.to_owned() }),
                }
            }
        }
    };
}

raw_enum! {
    /// Kind of connection.
    ConnectionType {
        /// Remote Desktop.
        Rdp = "rdp",
        /// SSH terminal.
        Ssh = "ssh",
        /// Web page; the URL is in `host`.
        Web = "web",
        /// MikroTik router (WinBox or SSH).
        Mikrotik = "mikrotik",
    }
}

impl ConnectionType {
    /// The type as the UI names it: `RDP`, `SSH`, `Web` or `MikroTik`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Rdp => "RDP",
            Self::Ssh => "SSH",
            Self::Web => "Web",
            Self::Mikrotik => "MikroTik",
        }
    }

    /// SSH and MikroTik (whose SSH mode opens a terminal); these can be split panes and tunnel forwards.
    #[must_use]
    pub fn uses_ssh(self) -> bool {
        matches!(self, Self::Ssh | Self::Mikrotik)
    }

    /// Port used when the connection has none.
    #[must_use]
    pub fn default_port(self) -> u16 {
        match self {
            Self::Rdp => 3389,
            Self::Ssh | Self::Mikrotik => 22,
            Self::Web => 443,
        }
    }
}

raw_enum! {
    /// Where an SSH session runs.
    SshTerminal {
        /// A tab inside the app.
        Embedded = "embedded",
        /// The user's external terminal app.
        External = "external",
    }
}

raw_enum! {
    /// Which client opens an RDP connection.
    RdpClient {
        /// The embedded RDP tab.
        Embedded = "embedded",
        /// The system RDP client (Windows App on macOS, mstsc on Windows, Remmina/xfreerdp on Linux).
        WindowsApp = "windowsApp",
    }
}

raw_enum! {
    /// What double-click does on a MikroTik connection.
    MikrotikMode {
        /// Launch WinBox.
        Winbox = "winbox",
        /// Open an SSH session.
        Ssh = "ssh",
    }
}

raw_enum! {
    /// Tag color for folders, connections and tabs.
    LabelColor {
        /// Red.
        Red = "red",
        /// Orange.
        Orange = "orange",
        /// Yellow.
        Yellow = "yellow",
        /// Green.
        Green = "green",
        /// Blue.
        Blue = "blue",
        /// Purple.
        Purple = "purple",
        /// Pink.
        Pink = "pink",
        /// Gray.
        Gray = "gray",
    }
}

impl LabelColor {
    /// The color every entry shares; `None` when they differ, any entry has none, or there are none.
    #[must_use]
    pub fn common(colors: impl IntoIterator<Item = Option<Self>>) -> Option<Self> {
        let mut colors = colors.into_iter();
        let first = colors.next()??;
        colors.all(|c| c == Some(first)).then_some(first)
    }
}

pub(crate) use raw_enum;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_values_match_swift() {
        assert_eq!(
            ConnectionType::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            ["rdp", "ssh", "web", "mikrotik"]
        );
        assert_eq!(RdpClient::WindowsApp.as_str(), "windowsApp");
        assert_eq!(SshTerminal::External.to_string(), "external");
        assert_eq!(MikrotikMode::Winbox.as_str(), "winbox");
        assert_eq!(LabelColor::ALL.len(), 8);
    }

    #[test]
    fn parses_and_rejects_raw_values() {
        assert_eq!("mikrotik".parse::<ConnectionType>(), Ok(ConnectionType::Mikrotik));
        assert_eq!("gray".parse::<LabelColor>(), Ok(LabelColor::Gray));
        assert!("ghostty".parse::<SshTerminal>().is_err());
        assert_eq!(
            serde_json::to_string(&RdpClient::WindowsApp).unwrap(),
            "\"windowsApp\""
        );
    }

    #[test]
    fn label_color_order_and_common() {
        use LabelColor::{Blue, Red};
        assert_eq!(
            LabelColor::ALL.iter().map(|c| c.as_str()).collect::<Vec<_>>(),
            [
                "red", "orange", "yellow", "green", "blue", "purple", "pink", "gray"
            ]
        );
        assert_eq!(LabelColor::common([Some(Red), Some(Red)]), Some(Red));
        assert_eq!(LabelColor::common([Some(Red), Some(Blue)]), None);
        assert_eq!(LabelColor::common([Some(Red), None]), None);
        assert_eq!(LabelColor::common([None, None]), None);
        assert_eq!(LabelColor::common([]), None);
    }

    #[test]
    fn default_ports() {
        assert_eq!(ConnectionType::Rdp.default_port(), 3389);
        assert_eq!(ConnectionType::Ssh.default_port(), 22);
        assert_eq!(ConnectionType::Web.default_port(), 443);
        assert_eq!(ConnectionType::Mikrotik.default_port(), 22);
    }
}
