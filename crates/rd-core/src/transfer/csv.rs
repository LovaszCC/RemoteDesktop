//! A server list from a spreadsheet: `folder;name;host;type` rows, read into an [`ExportPayload`] so the
//! export import can preview and write it. Rows with the same name become one entry with one connection
//! each. No logins or passwords: those are set in the app afterwards.

use serde::Serialize;

use super::{ExportPayload, ExportedConnection, ExportedEntry};
use crate::draft::split_web_url;
use crate::import::split_host_port;
use crate::model::{Connection, ConnectionType, Timestamp};

/// Why a CSV file can't be read at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CsvError {
    /// Not UTF-8 text.
    #[error("the file is not UTF-8 text")]
    Encoding,
    /// No header line.
    #[error("the file is empty")]
    Empty,
    /// A quoted field is never closed.
    #[error("a quoted field is not closed")]
    Unterminated,
    /// Required columns are missing from the header.
    #[error("missing columns: {}", .0.join(", "))]
    MissingColumns(Vec<String>),
}

/// Why a row was left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "reason", content = "detail")]
pub enum CsvSkipReason {
    /// The name cell is empty.
    NoName,
    /// The host cell is empty.
    NoHost,
    /// The type is not `rdp`, `ssh`, `web` or `mikrotik`; carries the cell.
    UnknownType(String),
    /// A web row whose host isn't a usable URL; carries the cell.
    BadUrl(String),
}

/// A row left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvSkipped {
    /// 1-based line number in the file (the header is line 1).
    pub line: usize,
    /// The row's name, possibly empty.
    pub name: String,
    /// Why.
    #[serde(flatten)]
    pub reason: CsvSkipReason,
}

/// A parsed CSV file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvImport {
    /// The entries, without credentials or passwords.
    pub payload: ExportPayload,
    /// Rows left out.
    pub skipped: Vec<CsvSkipped>,
}

/// Records of a CSV text with their 1-based starting line numbers.
fn records(text: &str, delimiter: char) -> Result<Vec<(usize, Vec<String>)>, CsvError> {
    let mut out = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut line = 1;
    let mut start = 1;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    field.push(c);
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            c if c == delimiter => record.push(std::mem::take(&mut field)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                record.push(std::mem::take(&mut field));
                out.push((start, std::mem::take(&mut record)));
                line += 1;
                start = line;
            }
            _ => field.push(c),
        }
    }
    if quoted {
        return Err(CsvError::Unterminated);
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        out.push((start, record));
    }
    // Blank lines carry nothing.
    out.retain(|(_, r)| r.iter().any(|f| !f.trim().is_empty()));
    Ok(out)
}

/// `;` when the header has more of them than commas (Excel with a Hungarian locale), else `,`.
fn delimiter(text: &str) -> char {
    let header = text.lines().next().unwrap_or_default();
    if header.matches(';').count() > header.matches(',').count() {
        ';'
    } else {
        ','
    }
}

const FOLDER: &[&str] = &["folder", "mappa", "group", "csoport"];
const NAME: &[&str] = &["name", "név", "nev"];
const HOST: &[&str] = &["host", "address", "cím", "cim", "url"];
const TYPE: &[&str] = &["type", "típus", "tipus"];

fn column(header: &[String], aliases: &[&str]) -> Option<usize> {
    header
        .iter()
        .position(|h| aliases.contains(&h.trim().to_lowercase().as_str()))
}

fn kind(cell: &str) -> Option<ConnectionType> {
    match cell.trim().to_lowercase().as_str() {
        "rdp" => Some(ConnectionType::Rdp),
        "ssh" => Some(ConnectionType::Ssh),
        "web" | "http" | "https" => Some(ConnectionType::Web),
        "mikrotik" | "winbox" => Some(ConnectionType::Mikrotik),
        _ => None,
    }
}

/// A connection of `kind` with the app's defaults for a new one.
fn connection(kind: ConnectionType) -> ExportedConnection {
    let defaults = Connection::new(kind, "", "");
    ExportedConnection {
        kind,
        label: String::new(),
        port: None,
        username: String::new(),
        domain: String::new(),
        password: None,
        credential_ref: None,
        rdp_admin_session: false,
        browser_bundle_id: None,
        mikrotik_mode: defaults.mikrotik_mode,
        ssh_terminal: defaults.ssh_terminal,
        rdp_client: defaults.rdp_client,
        show_as_button: false,
        web_scheme: defaults.web_scheme,
        web_path: String::new(),
        port_forwards: Vec::new(),
    }
}

/// Reads a CSV file (UTF-8, optional BOM; `;` or `,`).
///
/// # Errors
/// [`CsvError`] when the file isn't UTF-8, has no header, a quote is never closed or a column is missing.
pub fn parse_csv(data: &[u8]) -> Result<CsvImport, CsvError> {
    let text = std::str::from_utf8(data).map_err(|_| CsvError::Encoding)?;
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut rows = records(text, delimiter(text))?.into_iter();
    let (_, header) = rows.next().ok_or(CsvError::Empty)?;
    let columns = [
        ("folder", column(&header, FOLDER)),
        ("name", column(&header, NAME)),
        ("host", column(&header, HOST)),
        ("type", column(&header, TYPE)),
    ];
    let missing: Vec<String> = columns
        .iter()
        .filter(|(_, c)| c.is_none())
        .map(|(n, _)| (*n).to_owned())
        .collect();
    let [Some(folder), Some(name), Some(host), Some(kind_column)] = columns.map(|(_, c)| c) else {
        return Err(CsvError::MissingColumns(missing));
    };

    let mut entries: Vec<ExportedEntry> = Vec::new();
    let mut skipped = Vec::new();
    for (line, row) in rows {
        let cell = |i: usize| row.get(i).map_or("", |s| s.trim());
        let skip = |reason| CsvSkipped {
            line,
            name: cell(name).to_owned(),
            reason,
        };
        if cell(name).is_empty() {
            skipped.push(skip(CsvSkipReason::NoName));
            continue;
        }
        if cell(host).is_empty() {
            skipped.push(skip(CsvSkipReason::NoHost));
            continue;
        }
        let Some(kind) = kind(cell(kind_column)) else {
            skipped.push(skip(CsvSkipReason::UnknownType(cell(kind_column).to_owned())));
            continue;
        };
        let mut item = connection(kind);
        let address = if kind == ConnectionType::Web {
            let Some(parts) = split_web_url(cell(host)) else {
                skipped.push(skip(CsvSkipReason::BadUrl(cell(host).to_owned())));
                continue;
            };
            item.web_scheme = parts.scheme;
            item.port = parts.port;
            item.web_path = parts.path;
            parts.host
        } else if let Some((address, port)) = split_host_port(cell(host)) {
            item.port = port.and_then(|p| u16::try_from(p).ok()).filter(|p| *p > 0);
            address
        } else {
            cell(host).to_owned()
        };

        let key = cell(name).to_lowercase();
        if let Some(entry) = entries.iter_mut().find(|e| e.name.to_lowercase() == key) {
            entry.connections.push(item);
        } else {
            entries.push(ExportedEntry {
                reference: format!("e{}", entries.len() + 1),
                name: cell(name).to_owned(),
                host: address,
                notes: String::new(),
                color: None,
                is_favorite: false,
                folder_path: cell(folder)
                    .split('/')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
                default_connection: Some(0),
                connections: vec![item],
            });
        }
    }
    Ok(CsvImport {
        payload: ExportPayload {
            exported_at: Timestamp::now().to_string(),
            app_version: String::new(),
            credentials: Vec::new(),
            entries,
        },
        skipped,
    })
}

/// The sample file: `;`-separated (what Excel expects in Hungary), the header in `language` (`hu` or
/// English), one entry with two connections, a web URL with port and path, and a router at the root.
#[must_use]
pub fn sample_csv(language: &str) -> String {
    let (header, clients) = if language == "hu" {
        ("mappa;név;host;típus", "Ügyfelek/Acme")
    } else {
        ("folder;name;host;type", "Clients/Acme")
    };
    format!(
        "{header}\r\n\
         {clients};dc01;dc01.acme.local;rdp\r\n\
         {clients};dc01;dc01.acme.local;ssh\r\n\
         {clients};intranet;https://intranet.acme.local:8443/portal;web\r\n\
         ;core-router;192.0.2.1;mikrotik\r\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> CsvImport {
        parse_csv(text.as_bytes()).unwrap()
    }

    #[test]
    fn the_sample_reads_back_in_both_languages() {
        for (language, folder) in [("en", "Clients"), ("hu", "Ügyfelek")] {
            let parsed = parse(&sample_csv(language));
            assert!(parsed.skipped.is_empty());
            let entries = &parsed.payload.entries;
            assert_eq!(
                entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
                ["dc01", "intranet", "core-router"]
            );
            assert_eq!(entries[0].folder_path, [folder, "Acme"]);
            assert_eq!(
                entries[0].connections.iter().map(|c| c.kind).collect::<Vec<_>>(),
                [ConnectionType::Rdp, ConnectionType::Ssh]
            );
            let web = &entries[1].connections[0];
            assert_eq!(entries[1].host, "intranet.acme.local");
            assert_eq!(
                (web.web_scheme.as_str(), web.port, web.web_path.as_str()),
                ("https", Some(8443), "/portal")
            );
            assert!(entries[2].folder_path.is_empty());
            assert_eq!(entries[2].connections[0].kind, ConnectionType::Mikrotik);
        }
    }

    #[test]
    fn commas_quotes_bom_and_crlf() {
        let text = "\u{FEFF}Name,Host,Type,Folder,Notes\r\n\
                    \"web, main\",\"https://x.test/a?b=\"\"c\"\"\",web,\"A / B\",ignored\r\n\
                    \r\n\
                    box,10.0.0.1:2222,SSH,,\n";
        let parsed = parse(text);
        assert!(parsed.skipped.is_empty());
        let entries = &parsed.payload.entries;
        assert_eq!(entries[0].name, "web, main");
        assert_eq!(entries[0].folder_path, ["A", "B"]);
        assert_eq!(entries[0].connections[0].web_path, "/a?b=\"c\"");
        assert_eq!(entries[1].host, "10.0.0.1");
        assert_eq!(entries[1].connections[0].port, Some(2222));
        assert_eq!(entries[1].connections[0].kind, ConnectionType::Ssh);
    }

    #[test]
    fn rows_with_the_same_name_share_an_entry() {
        let parsed =
            parse("folder;name;host;type\nA;DC01;dc;rdp\nB;dc01;other;ssh\n;x;h;rdp\nC;Dc01;dc;mikrotik\n");
        let entries = &parsed.payload.entries;
        assert_eq!(entries.len(), 2);
        // The first row decides folder and host.
        assert_eq!(
            (entries[0].name.as_str(), entries[0].host.as_str()),
            ("DC01", "dc")
        );
        assert_eq!(entries[0].folder_path, ["A"]);
        assert_eq!(entries[0].connections.len(), 3);
        assert_eq!(entries[0].default_connection, Some(0));
        assert!(!parsed.payload.has_passwords());
    }

    #[test]
    fn bad_rows_are_skipped_with_their_line() {
        let parsed = parse(
            "mappa;név;host;típus\n;;h;rdp\n;a;;rdp\n;b;h;vnc\n;c;not a url;web\n;\"multi\nline\";h;ssh\n;d;h;telnet\n",
        );
        assert_eq!(
            parsed
                .skipped
                .iter()
                .map(|s| (s.line, s.name.as_str(), s.reason.clone()))
                .collect::<Vec<_>>(),
            [
                (2, "", CsvSkipReason::NoName),
                (3, "a", CsvSkipReason::NoHost),
                (4, "b", CsvSkipReason::UnknownType("vnc".into())),
                (5, "c", CsvSkipReason::BadUrl("not a url".into())),
                (8, "d", CsvSkipReason::UnknownType("telnet".into())),
            ]
        );
        assert_eq!(parsed.payload.entries[0].name, "multi\nline");
        assert_eq!(
            serde_json::to_value(&parsed.skipped[2]).unwrap(),
            serde_json::json!({"line": 4, "name": "b", "reason": "unknownType", "detail": "vnc"})
        );
    }

    #[test]
    fn unreadable_files() {
        assert_eq!(parse_csv(b"").unwrap_err(), CsvError::Empty);
        assert_eq!(parse_csv(&[0xff, 0xfe, 0x41]).unwrap_err(), CsvError::Encoding);
        assert_eq!(
            parse_csv(b"name;host;type\n\"x;h;rdp\n").unwrap_err(),
            CsvError::Unterminated
        );
        assert_eq!(
            parse_csv(b"name;Host\nx;h\n").unwrap_err(),
            CsvError::MissingColumns(vec!["folder".into(), "type".into()])
        );
        assert_eq!(
            CsvError::MissingColumns(vec!["folder".into(), "type".into()]).to_string(),
            "missing columns: folder, type"
        );
    }
}
