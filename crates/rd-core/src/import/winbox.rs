//! WinBox address books (`addresses.cdb` / an exported `.cdb`).
//!
//! The file is a `0D F0 1D C0` magic, then records of a `u32` LE length, `"M2"` and fields. A field is a
//! 3-byte LE id and a type byte, followed by its value. WinBox stores the passwords in plain text.

use std::collections::HashMap;

use super::{ImportError, ImportedConnection, trim_all, trim_spaces};
use crate::model::ConnectionType;

const MAGIC: [u8; 4] = [0x0D, 0xF0, 0x1D, 0xC0];

mod field {
    pub const ADDRESS: u32 = 0x01;
    pub const LOGIN: u32 = 0x02;
    pub const PASSWORD: u32 = 0x03;
    pub const NOTE: u32 = 0x04;
    pub const GROUP: u32 = 0x08;
    pub const DISPLAY_NAME: u32 = 0x00FE_0009;
}

/// The string fields of every record (other value types are read and dropped).
fn records(data: &[u8]) -> Result<Vec<HashMap<u32, String>>, ImportError> {
    let body = data.strip_prefix(&MAGIC).ok_or(ImportError::WinboxUnreadable)?;
    let mut records = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        let (length, after) = rest
            .split_first_chunk::<4>()
            .ok_or(ImportError::WinboxUnreadable)?;
        let length =
            usize::try_from(u32::from_le_bytes(*length)).map_err(|_| ImportError::WinboxUnreadable)?;
        if length < 2 || length > after.len() {
            return Err(ImportError::WinboxUnreadable);
        }
        let (record, next) = after.split_at(length);
        let fields = record.strip_prefix(b"M2").ok_or(ImportError::WinboxUnreadable)?;
        records.push(parse_fields(fields)?);
        rest = next;
    }
    Ok(records)
}

fn take<'a>(rest: &mut &'a [u8], count: usize) -> Result<&'a [u8], ImportError> {
    if rest.len() < count {
        return Err(ImportError::WinboxUnreadable);
    }
    let (head, tail) = rest.split_at(count);
    *rest = tail;
    Ok(head)
}

fn parse_fields(mut rest: &[u8]) -> Result<HashMap<u32, String>, ImportError> {
    let mut result = HashMap::new();
    while !rest.is_empty() {
        let header = take(&mut rest, 4)?;
        let id = u32::from(header[0]) | u32::from(header[1]) << 8 | u32::from(header[2]) << 16;
        match header[3] {
            // Bool: the value is in the type byte.
            0x00 | 0x01 => {}
            // u8.
            0x09 => {
                take(&mut rest, 1)?;
            }
            // u32.
            0x08 => {
                take(&mut rest, 4)?;
            }
            // Short string / bytes: u8 length.
            kind @ (0x21 | 0x31) => {
                let length = usize::from(take(&mut rest, 1)?[0]);
                let value = take(&mut rest, length)?;
                if kind == 0x21 {
                    result.insert(id, String::from_utf8_lossy(value).into_owned());
                }
            }
            _ => return Err(ImportError::WinboxUnreadable),
        }
    }
    Ok(result)
}

/// MikroTik connections, one per record with an address; the WinBox group becomes the folder.
///
/// # Errors
/// [`ImportError::WinboxUnreadable`] for a file without the magic or with a damaged record.
pub fn parse(data: &[u8]) -> Result<Vec<ImportedConnection>, ImportError> {
    Ok(records(data)?.into_iter().filter_map(connection).collect())
}

fn connection(mut fields: HashMap<u32, String>) -> Option<ImportedConnection> {
    let mut get = |id: u32| fields.remove(&id).unwrap_or_default();
    let address = get(field::ADDRESS);
    let address = trim_spaces(&address);
    if address.is_empty() {
        return None;
    }
    let (host, winbox_port) = split_address(address);
    // `[]` or `:8291` leave no host (found by fuzzing; Swift imported such entries without one).
    if host.is_empty() {
        return None;
    }
    let note = get(field::NOTE);
    let note = trim_all(&note);
    let display_name = get(field::DISPLAY_NAME);
    let display_name = trim_spaces(&display_name);
    let name = [display_name, note, host.as_str()]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or(&host)
        .to_owned();
    let mut notes = Vec::new();
    if !note.is_empty() && note != name {
        notes.push(note.to_owned());
    }
    if let Some(port) = winbox_port {
        notes.push(format!("WinBox port {port}"));
    }
    let group = get(field::GROUP);
    let group = trim_spaces(&group);
    let password = get(field::PASSWORD);
    let login = get(field::LOGIN);
    let mut item = ImportedConnection::new(ConnectionType::Mikrotik, name, host);
    item.group_path = if group.is_empty() {
        Vec::new()
    } else {
        vec![group.to_owned()]
    };
    trim_spaces(&login).clone_into(&mut item.username);
    item.password = (!password.is_empty()).then_some(password);
    item.notes = notes.join("\n");
    Some(item)
}

/// `192.0.2.1:8292` → host and WinBox port; IPv6 (`[2001:db8::1]:8292`) and bare addresses are kept.
#[must_use]
pub fn split_address(address: &str) -> (String, Option<String>) {
    if let Some(inner) = address.strip_prefix('[')
        && let Some(close) = inner.find(']')
    {
        let rest = &inner[close + 1..];
        return (
            inner[..close].to_owned(),
            rest.strip_prefix(':').map(str::to_owned),
        );
    }
    let parts: Vec<&str> = address.split(':').collect();
    if let [host, port] = parts.as_slice()
        && let Ok(port) = port.parse::<i64>()
        && port > 0
    {
        return ((*host).to_owned(), Some(port.to_string()));
    }
    (address.to_owned(), None)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds WinBox `.cdb` files like a real export (made-up data only).
    pub(crate) mod cdb {
        fn id(id: u32) -> [u8; 3] {
            let bytes = id.to_le_bytes();
            [bytes[0], bytes[1], bytes[2]]
        }

        pub fn string(field: u32, value: &str) -> Vec<u8> {
            let mut out = id(field).to_vec();
            out.push(0x21);
            out.push(u8::try_from(value.len()).unwrap());
            out.extend_from_slice(value.as_bytes());
            out
        }

        pub fn flag(field: u32, on: bool) -> Vec<u8> {
            let mut out = id(field).to_vec();
            out.push(u8::from(on));
            out
        }

        pub fn byte(field: u32, value: u8) -> Vec<u8> {
            let mut out = id(field).to_vec();
            out.extend([0x09, value]);
            out
        }

        pub fn record(fields: &[Vec<u8>]) -> Vec<u8> {
            let mut body = b"M2".to_vec();
            for f in fields {
                body.extend_from_slice(f);
            }
            let mut out = u32::try_from(body.len()).unwrap().to_le_bytes().to_vec();
            out.extend(body);
            out
        }

        pub fn file(records: &[Vec<u8>]) -> Vec<u8> {
            let mut out = vec![0x0D, 0xF0, 0x1D, 0xC0];
            for r in records {
                out.extend_from_slice(r);
            }
            out
        }

        /// Field order as WinBox writes it.
        pub fn entry(
            index: u8,
            name: &str,
            group: &str,
            password: &str,
            user: &str,
            address: &str,
        ) -> Vec<u8> {
            record(&[
                flag(7, true),
                byte(0x00FE_0001, index),
                string(0x0B, ""),
                string(0x0C, "<own>"),
                string(8, group),
                string(0x00FE_0009, name),
                string(4, name),
                string(3, password),
                string(2, user),
                string(1, address),
            ])
        }

        pub fn sample() -> Vec<u8> {
            file(&[
                entry(1, "core-rtr", "Branch East", "s3cret", "admin", "192.0.2.1"),
                entry(2, "Ágfalva", "Branch East", "", "admin", "192.0.2.2:8292"),
            ])
        }
    }

    #[test]
    fn parses_entries_like_a_real_export() {
        let items = parse(&cdb::sample()).unwrap();
        assert_eq!(items.len(), 2);
        let first = &items[0];
        assert_eq!(first.kind, ConnectionType::Mikrotik);
        assert_eq!(first.name, "core-rtr");
        assert_eq!(first.host, "192.0.2.1");
        assert_eq!(first.username, "admin");
        assert_eq!(first.password.as_deref(), Some("s3cret"));
        assert_eq!(first.group_path, ["Branch East"]);
        assert!(first.notes.is_empty());
        let second = &items[1];
        assert_eq!(second.name, "Ágfalva");
        assert_eq!(second.host, "192.0.2.2");
        assert_eq!(second.password, None);
        assert!(second.notes.contains("8292"));
    }

    #[test]
    fn name_falls_back_to_note_then_host_and_note_goes_to_notes() {
        let data = cdb::file(&[
            cdb::record(&[cdb::string(4, "rack 3"), cdb::string(1, "192.0.2.5")]),
            cdb::record(&[
                cdb::string(0x00FE_0009, "edge"),
                cdb::string(4, "rack 4"),
                cdb::string(1, "192.0.2.6"),
            ]),
            cdb::record(&[cdb::string(1, "192.0.2.7")]),
            cdb::record(&[cdb::string(2, "no address")]),
        ]);
        let items = parse(&data).unwrap();
        assert_eq!(
            items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
            ["rack 3", "edge", "192.0.2.7"]
        );
        assert_eq!(items[1].notes, "rack 4");
        assert!(items[2].group_path.is_empty());
    }

    #[test]
    fn skips_unknown_ids_and_other_value_types() {
        let uint32 = vec![0x0D, 0x00, 0x00, 0x08, 1, 2, 3, 4];
        let bytes = vec![0x0E, 0x00, 0x00, 0x31, 2, 0xAA, 0xBB];
        let data = cdb::file(&[cdb::record(&[
            uint32,
            bytes,
            cdb::string(0x77, "x"),
            cdb::string(1, "192.0.2.9"),
        ])]);
        let hosts: Vec<_> = parse(&data).unwrap().into_iter().map(|i| i.host).collect();
        assert_eq!(hosts, ["192.0.2.9"]);
    }

    #[test]
    fn rejects_damaged_files() {
        assert_eq!(parse(b"hello"), Err(ImportError::WinboxUnreadable));
        let mut truncated = cdb::sample();
        truncated.truncate(truncated.len() - 5);
        assert_eq!(parse(&truncated), Err(ImportError::WinboxUnreadable));
        let unknown_type = vec![0x01, 0x00, 0x00, 0x7F];
        assert_eq!(
            parse(&cdb::file(&[cdb::record(&[unknown_type])])),
            Err(ImportError::WinboxUnreadable)
        );
        assert_eq!(parse(&cdb::file(&[])), Ok(vec![]));
        // A record too short for "M2", a wrong record marker, a cut length and a huge length.
        assert!(parse(&cdb::file(&[vec![1, 0, 0, 0, b'M']])).is_err());
        assert!(parse(&cdb::file(&[vec![2, 0, 0, 0, b'X', b'2']])).is_err());
        assert!(parse(&cdb::file(&[vec![2, 0]])).is_err());
        assert!(parse(&cdb::file(&[vec![0xFF, 0xFF, 0xFF, 0xFF, b'M', b'2']])).is_err());
        // A field header cut short and a string longer than the record.
        assert!(parse(&cdb::file(&[cdb::record(&[vec![1, 0]])])).is_err());
        assert!(parse(&cdb::file(&[cdb::record(&[vec![1, 0, 0, 0x21, 9, b'a']])])).is_err());
    }

    #[test]
    fn splits_addresses() {
        let split = |a: &str| {
            let (host, port) = split_address(a);
            (host, port)
        };
        assert_eq!(split("192.0.2.1"), ("192.0.2.1".into(), None));
        assert_eq!(split("192.0.2.1:8292"), ("192.0.2.1".into(), Some("8292".into())));
        assert_eq!(
            split("[2001:db8::1]:8292"),
            ("2001:db8::1".into(), Some("8292".into()))
        );
        assert_eq!(split("2001:db8::1"), ("2001:db8::1".into(), None));
        assert_eq!(split("router.example.com"), ("router.example.com".into(), None));
        assert_eq!(
            split("router.example.com:0"),
            ("router.example.com:0".into(), None)
        );
        assert_eq!(
            split("router.example.com:x"),
            ("router.example.com:x".into(), None)
        );
    }

    #[test]
    fn addresses_without_a_host_are_skipped() {
        let data = cdb::file(&[
            cdb::record(&[cdb::string(1, "[]:8291")]),
            cdb::record(&[cdb::string(1, ":8291")]),
            cdb::record(&[cdb::string(1, "192.0.2.8")]),
        ]);
        let hosts: Vec<_> = parse(&data).unwrap().into_iter().map(|i| i.host).collect();
        assert_eq!(hosts, ["192.0.2.8"]);
    }

    #[test]
    fn invalid_utf8_is_replaced_not_rejected() {
        let mut record = vec![0x01, 0x00, 0x00, 0x21, 3, b'h', 0xFF, b'x'];
        record.extend(cdb::string(2, "admin"));
        let items = parse(&cdb::file(&[cdb::record(&[record])])).unwrap();
        assert_eq!(items[0].host, "h\u{FFFD}x");
    }
}
