//! Devolutions Remote Desktop Manager `.rdm` XML exports.
//!
//! Each `<Connection>` element is flattened to `path → text` (for example `RDP/UserName`) and mapped with
//! fallbacks, since field locations differ between RDM versions and connection types.

use std::collections::HashMap;

use quick_xml::Reader;
use quick_xml::events::Event;

use super::{
    ImportError, ImportedConnection, ImportedCredential, ParsedImport, SkipReason, Skipped, trim_all,
};
use crate::model::{ConnectionType, new_id};

const RDP_TYPES: &[&str] = &["RDPConfigured", "RDP", "RDPFilename"];
const SSH_TYPES: &[&str] = &["SSHShell", "SSH", "Putty", "SSHTunnel", "Terminal"];

type Fields = HashMap<String, String>;

/// Looks `keys` up under `section/` first, then at the top level; the first non-empty value wins.
fn lookup<'a>(fields: &'a Fields, section: &str, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        fields
            .get(&format!("{section}/{key}"))
            .or_else(|| fields.get(*key))
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    })
}

/// The credential in `section` of an entry; `None` for a credentials folder without one. Counts an
/// encrypted password (`SafePassword`, which can't be read) in `encrypted`.
fn credential(
    fields: &Fields,
    section: &str,
    name: String,
    encrypted: &mut usize,
) -> Option<ImportedCredential> {
    let value = |keys: &[&str]| lookup(fields, section, keys).map(str::to_owned);
    let username = value(&["UserName", "Username"]);
    let password = value(&["Password", "ClearPassword"]);
    let safe = value(&["SafePassword"]).is_some();
    if section == "GroupDetails" && username.is_none() && password.is_none() && !safe {
        return None;
    }
    if password.is_none() && safe {
        *encrypted += 1;
    }
    Some(ImportedCredential {
        reference: fields.get("ID").cloned().unwrap_or_else(new_id),
        name: if name.is_empty() {
            username.clone().unwrap_or_else(|| "Credential".to_owned())
        } else {
            name
        },
        username: username.unwrap_or_default(),
        domain: value(&["Domain"]).unwrap_or_default(),
        password,
        ssh_key_path: value(&["PrivateKeyPath", "PrivateKeyFileName"]),
    })
}

/// Parses an export.
///
/// # Errors
/// [`ImportError::RdmUnreadable`] when the data isn't well-formed XML.
pub fn parse(data: &[u8]) -> Result<ParsedImport, ImportError> {
    let mut result = ParsedImport::default();
    for fields in collect(data)? {
        let kind = fields.get("ConnectionType").map_or("", String::as_str);
        let name = fields.get("Name").cloned().unwrap_or_default();
        let group = split_group(
            fields
                .get("Group")
                .or_else(|| fields.get("GroupName"))
                .map_or("", String::as_str),
        );

        // A folder of RDM's "Credentials" type holds a credential of its own (and credential entries,
        // which are imported on their own): it becomes a credential, not an empty folder. A connection
        // below it still creates the folder through its path.
        let credential_folder = kind == "Group"
            && fields.get("GroupDetails/GroupType").map(String::as_str) == Some("Credentials");
        if kind == "Credential" || credential_folder {
            let section = if credential_folder {
                "GroupDetails"
            } else {
                "Credentials"
            };
            if let Some(credential) = credential(&fields, section, name, &mut result.encrypted_passwords) {
                result.credentials.push(credential);
            }
            continue;
        }

        if kind == "Group" {
            // A folder entry's Group already contains its own name.
            result
                .empty_groups
                .push(if group.is_empty() { vec![name] } else { group });
            continue;
        }

        let connection_type = if RDP_TYPES.contains(&kind) {
            ConnectionType::Rdp
        } else if SSH_TYPES.contains(&kind) {
            ConnectionType::Ssh
        } else {
            result.skipped.push(Skipped {
                name,
                reason: SkipReason::Unsupported {
                    kind: kind.to_owned(),
                },
            });
            continue;
        };

        let section = if connection_type == ConnectionType::Rdp {
            "RDP"
        } else {
            "Terminal"
        };
        let value = |keys: &[&str]| lookup(&fields, section, keys).map(str::to_owned);

        let mut host = value(&["Host", "Url", "HostName", "ComputerName"]).unwrap_or_default();
        let mut port = value(&["HostPort", "Port"]).and_then(|p| p.parse::<i64>().ok());
        if let Some((split_host, split_port)) = split_host_port(&host) {
            host = split_host;
            port = port.or(split_port);
        }
        if host.is_empty() {
            result.skipped.push(Skipped {
                name,
                reason: SkipReason::NoHost,
            });
            continue;
        }

        let password = value(&["Password", "ClearPassword"]);
        if password.is_none() && value(&["SafePassword"]).is_some() {
            result.encrypted_passwords += 1;
        }
        let port = port
            .and_then(|p| u16::try_from(p).ok())
            .filter(|&p| p != connection_type.default_port());
        let admin = value(&["AdminMode", "UseConsoleSession"])
            .unwrap_or_default()
            .to_lowercase();

        let mut item = ImportedConnection::new(
            connection_type,
            if name.is_empty() { host.clone() } else { name },
            host,
        );
        item.group_path = group;
        item.port = port;
        item.username = value(&["UserName", "Username", "User"]).unwrap_or_default();
        item.domain = value(&["Domain"]).unwrap_or_default();
        item.password = password;
        item.ssh_key_path = if connection_type == ConnectionType::Ssh {
            value(&["PrivateKeyPath", "PrivateKeyFileName"])
        } else {
            None
        };
        item.notes = value(&["Description", "Notes"]).unwrap_or_default();
        item.rdp_admin_session = connection_type == ConnectionType::Rdp && (admin == "true" || admin == "1");
        item.credential_ref = fields.get("CredentialConnectionID").cloned();
        result.connections.push(item);
    }
    Ok(result)
}

/// `A\B/C` → `["A", "B", "C"]`, trimming spaces and dropping empty parts.
#[must_use]
pub fn split_group(group: &str) -> Vec<String> {
    group
        .split(['\\', '/'])
        .map(super::trim_spaces)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `host:port` → split; bare IPv6 addresses (several colons, no brackets) are left alone (`None`).
#[must_use]
pub fn split_host_port(value: &str) -> Option<(String, Option<i64>)> {
    if let Some(inner) = value.strip_prefix('[')
        && let Some(close) = inner.find(']')
    {
        let rest = &inner[close + 1..];
        let port = rest.strip_prefix(':').and_then(|p| p.parse().ok());
        return Some((inner[..close].to_owned(), port));
    }
    let parts: Vec<&str> = value.split(':').collect();
    match parts.as_slice() {
        [host, port] => port.parse().ok().map(|p| ((*host).to_owned(), Some(p))),
        _ => None,
    }
}

fn unreadable(error: impl std::fmt::Display) -> ImportError {
    ImportError::RdmUnreadable(error.to_string())
}

/// The text of the file: UTF-8 (with or without a BOM) or UTF-16 with a BOM.
fn decode(data: &[u8]) -> Result<String, ImportError> {
    let utf16 = |bytes: &[u8], from: fn([u8; 2]) -> u16| {
        let units = bytes.as_chunks::<2>().0.iter().map(|c| from(*c));
        char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .map_err(unreadable)
    };
    if let Some(rest) = data.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8(rest.to_vec()).map_err(unreadable)
    } else if let Some(rest) = data.strip_prefix(&[0xFF, 0xFE]) {
        utf16(rest, u16::from_le_bytes)
    } else if let Some(rest) = data.strip_prefix(&[0xFE, 0xFF]) {
        utf16(rest, u16::from_be_bytes)
    } else {
        String::from_utf8(data.to_vec()).map_err(unreadable)
    }
}

/// The value of a predefined entity or character reference.
fn resolve_reference(name: &str) -> Option<char> {
    match name {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "apos" => Some('\''),
        "quot" => Some('"'),
        _ => None,
    }
}

/// The Swift SAX collector: every `<Connection>` element becomes a flat map of `path → text`. Only the first
/// non-empty value of a path is kept.
fn collect(data: &[u8]) -> Result<Vec<Fields>, ImportError> {
    let text_data = decode(data)?;
    let mut reader = Reader::from_str(&text_data);
    reader.config_mut().check_end_names = true;

    let mut entries = Vec::new();
    let mut current: Option<Fields> = None;
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut depth = 0_usize;
    let mut saw_root = false;

    loop {
        let event = reader.read_event().map_err(unreadable)?;
        match event {
            Event::Start(start) => {
                depth += 1;
                saw_root = true;
                text.clear();
                let name = start.name().as_ref().to_owned();
                if current.is_none() {
                    if name == "Connection" {
                        current = Some(Fields::new());
                        path.clear();
                    }
                } else {
                    path.push(name);
                }
            }
            Event::Empty(start) => {
                saw_root = true;
                // `<x/>`: a start and an end with no text.
                text.clear();
                let name = start.name().as_ref().to_owned();
                if current.is_none() && name == "Connection" {
                    entries.push(Fields::new());
                }
            }
            Event::End(end) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| unreadable("unexpected end tag"))?;
                let Some(fields) = current.as_mut() else {
                    continue;
                };
                if path.is_empty() {
                    let name = end.name().as_ref().to_owned();
                    if name == "Connection"
                        && let Some(done) = current.take()
                    {
                        entries.push(done);
                    }
                    continue;
                }
                let key = path.join("/");
                let value = trim_all(&text);
                if !value.is_empty() && !fields.contains_key(&key) {
                    fields.insert(key, value.to_owned());
                }
                path.pop();
                text.clear();
            }
            Event::Text(content) => text.push_str(&content.xml10_content()),
            Event::CData(content) => text.push_str(&content.xml10_content()),
            Event::GeneralRef(reference) => {
                let resolved = match reference.resolve_char_ref().map_err(unreadable)? {
                    Some(c) => c,
                    None => resolve_reference(&reference)
                        .ok_or_else(|| unreadable(format!("unknown entity &{};", &*reference)))?,
                };
                text.push(resolved);
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
        }
    }
    if depth != 0 || !saw_root {
        return Err(unreadable("the document is incomplete"));
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../tests/fixtures/rdm-sample.rdm");

    #[test]
    fn parses_entries() {
        let result = parse(SAMPLE.as_bytes()).unwrap();

        assert_eq!(result.empty_groups, [["Customers", "Empty"]]);
        assert_eq!(result.skipped.len(), 2);
        assert_eq!(result.skipped[0].to_string(), "portal (WebBrowser)");
        assert_eq!(result.skipped[1].reason, SkipReason::NoHost);
        assert_eq!(result.skipped[1].name, "nohost");
        assert_eq!(result.encrypted_passwords, 1);
        assert_eq!(result.connections.len(), 3);

        let dc = &result.connections[0];
        assert_eq!(dc.kind, ConnectionType::Rdp);
        assert_eq!(dc.group_path, ["Customers", "Example"]);
        assert_eq!(dc.host, "192.0.2.10");
        assert_eq!(dc.port, Some(3390));
        assert_eq!(dc.username, "administrator");
        assert_eq!(dc.domain, "EXAMPLE");
        assert_eq!(dc.password.as_deref(), Some("Secret1"));
        assert_eq!(dc.notes, "Domain controller");

        let web = &result.connections[1];
        assert_eq!(web.kind, ConnectionType::Ssh);
        assert_eq!(web.host, "web.example.com");
        assert_eq!(web.port, None);
        assert_eq!(web.username, "deploy");
        assert_eq!(web.ssh_key_path.as_deref(), Some("~/.ssh/example"));
        assert_eq!(web.password, None);

        let root = &result.connections[2];
        assert!(root.group_path.is_empty());
        assert_eq!(root.username, "me");
    }

    #[test]
    fn credential_folders_become_credentials() {
        let xml = r"<RDMExport><Connections>
          <Connection>
            <ConnectionType>Group</ConnectionType>
            <Group>Logins</Group>
            <ID>FOLDER-1</ID>
            <Name>Logins</Name>
            <GroupDetails>
              <GroupType>Credentials</GroupType>
              <SafePassword>ENC==</SafePassword>
              <UserName>jdoe</UserName>
            </GroupDetails>
            <MetaInformation><PasswordHistory><PasswordHistory>
              <SafePassword>OLD==</SafePassword>
            </PasswordHistory></PasswordHistory></MetaInformation>
          </Connection>
          <Connection>
            <ConnectionType>Group</ConnectionType>
            <Group>Empty logins</Group>
            <Name>Empty logins</Name>
            <GroupDetails><GroupType>Credentials</GroupType></GroupDetails>
          </Connection>
          <Connection>
            <ConnectionType>Group</ConnectionType>
            <Group>Servers</Group>
            <Name>Servers</Name>
            <GroupDetails><GroupType>Folder</GroupType></GroupDetails>
          </Connection>
        </Connections></RDMExport>";
        let result = parse(xml.as_bytes()).unwrap();
        assert_eq!(
            result.credentials.len(),
            1,
            "an empty credentials folder imports nothing"
        );
        let login = &result.credentials[0];
        assert_eq!(
            (
                login.reference.as_str(),
                login.name.as_str(),
                login.username.as_str()
            ),
            ("FOLDER-1", "Logins", "jdoe")
        );
        assert_eq!(login.password, None);
        assert_eq!(result.encrypted_passwords, 1);
        assert_eq!(result.empty_groups, [["Servers"]], "other folders stay folders");
    }

    #[test]
    fn credentials_and_links() {
        let xml = r"<ArrayOfConnection>
          <Connection>
            <ConnectionType>Credential</ConnectionType>
            <ID>CRED-1</ID>
            <Name>Shared admin</Name>
            <Credentials><UserName>admin</UserName><Domain>EXAMPLE</Domain><Password>pw</Password></Credentials>
          </Connection>
          <Connection>
            <ConnectionType>Credential</ConnectionType>
            <Credentials><UserName>ops</UserName><SafePassword>ENC==</SafePassword></Credentials>
          </Connection>
          <Connection>
            <ConnectionType>RDP</ConnectionType>
            <Name>linked</Name>
            <Url>[2001:db8::5]:3389</Url>
            <CredentialConnectionID>CRED-1</CredentialConnectionID>
            <RDP><AdminMode>True</AdminMode></RDP>
          </Connection>
        </ArrayOfConnection>";
        let result = parse(xml.as_bytes()).unwrap();
        assert_eq!(result.credentials.len(), 2);
        let shared = &result.credentials[0];
        assert_eq!(
            (
                shared.reference.as_str(),
                shared.name.as_str(),
                shared.username.as_str()
            ),
            ("CRED-1", "Shared admin", "admin")
        );
        assert_eq!(shared.domain, "EXAMPLE");
        assert_eq!(shared.password.as_deref(), Some("pw"));
        let unnamed = &result.credentials[1];
        assert_eq!(unnamed.name, "ops");
        assert_eq!(unnamed.password, None);
        assert!(!unnamed.reference.is_empty());
        assert_eq!(result.encrypted_passwords, 1);
        let linked = &result.connections[0];
        assert_eq!(linked.host, "2001:db8::5");
        assert_eq!(linked.port, None, "the default port is dropped");
        assert!(linked.rdp_admin_session);
        assert_eq!(linked.credential_ref.as_deref(), Some("CRED-1"));
    }

    #[test]
    fn invalid_xml_fails() {
        for bad in [
            "<ArrayOfConnection><Connection>",
            "<a></b>",
            "",
            "<a>&nope;</a>",
            "</a>",
        ] {
            assert!(
                matches!(parse(bad.as_bytes()), Err(ImportError::RdmUnreadable(_))),
                "{bad:?}"
            );
        }
        assert!(parse(&[0xFF, 0x00, 0x41]).is_err());
    }

    #[test]
    fn entities_cdata_boms_and_empty_elements() {
        let xml = "<A><Connection><ConnectionType>SSH</ConnectionType><Name>a &amp; b&#33;</Name>\
                   <Host><![CDATA[h.example.com]]></Host><Notes/></Connection><Connection/></A>";
        let result = parse(xml.as_bytes()).unwrap();
        assert_eq!(result.connections[0].name, "a & b!");
        assert_eq!(result.connections[0].host, "h.example.com");
        // `<Connection/>` is an entry with no type.
        assert_eq!(result.skipped.len(), 1);

        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(xml.as_bytes());
        assert_eq!(parse(&bom).unwrap().connections.len(), 1);
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(parse(&utf16).unwrap().connections.len(), 1);
        let mut utf16be = vec![0xFE, 0xFF];
        utf16be.extend(xml.encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(parse(&utf16be).unwrap().connections.len(), 1);
    }

    #[test]
    fn helpers() {
        assert_eq!(split_group("A\\ B /C\\\\"), ["A", "B", "C"]);
        assert_eq!(split_host_port("h:22"), Some(("h".into(), Some(22))));
        assert_eq!(
            split_host_port("[2001:db8::1]:3390"),
            Some(("2001:db8::1".into(), Some(3390)))
        );
        assert_eq!(
            split_host_port("[2001:db8::1]"),
            Some(("2001:db8::1".into(), None))
        );
        assert_eq!(split_host_port("2001:db8::1"), None);
        assert_eq!(split_host_port("host"), None);
        assert_eq!(split_host_port("host:x"), None);
    }
}
