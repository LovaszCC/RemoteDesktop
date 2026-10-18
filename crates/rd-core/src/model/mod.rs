//! Records stored in `connections.sqlite`, with the Swift app's field names, defaults and encodings.

mod enums;
mod records;
mod values;

pub(crate) use enums::raw_enum;
pub use enums::{ConnectionType, LabelColor, MikrotikMode, RdpClient, SshTerminal, UnknownValue};
pub use records::{Connection, Credential, Folder, PortForward, SavedSplit};
pub use values::{Sealed, Timestamp, TimestampError, new_id};
