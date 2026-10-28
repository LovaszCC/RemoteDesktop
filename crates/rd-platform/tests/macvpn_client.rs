//! The copied MacVPN control client against a fake MacVPN on a Unix socket.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Duration;

use rd_platform::macvpn::{Client, ErrorCode, State};

const PROFILE: &str = "6F1C2A4E-0B6D-4E43-9C55-1D2A7B8E9F01";

/// A one-client fake MacVPN: answers each request line with the next scripted lines (`{id}` is replaced
/// with the request's id). Short path: macOS limits socket paths to 104 bytes.
fn serve(script: Vec<Vec<&'static str>>) -> (tempfile::TempDir, PathBuf, JoinHandle<Vec<String>>) {
    let dir = tempfile::Builder::new().prefix("mv").tempdir_in("/tmp").unwrap();
    let path = dir.path().join("s.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        answer(stream, script)
    });
    (dir, path, server)
}

fn answer(mut stream: UnixStream, script: Vec<Vec<&'static str>>) -> Vec<String> {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut requests = Vec::new();
    for lines in script {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        let id = request["id"].as_u64().map_or(String::new(), |id| id.to_string());
        requests.push(request["op"].as_str().unwrap().to_owned());
        for reply in lines {
            if reply == "<wait-for-close>" {
                let mut rest = String::new();
                let _ = reader.read_line(&mut rest);
                continue;
            }
            stream.write_all(reply.replace("{id}", &id).as_bytes()).unwrap();
            stream.write_all(b"\n").unwrap();
        }
    }
    requests
}

#[test]
fn hello_then_list_parses_connections() {
    let (_dir, path, server) = serve(vec![
        vec![r#"{"id":{id},"ok":true,"api":1}"#],
        vec![
            r#"{"id":{id},"ok":true,"connections":[{"id":"6F1C2A4E-0B6D-4E43-9C55-1D2A7B8E9F01","name":"Office","kind":"openvpn","state":"connected","extra":1}]}"#,
        ],
    ]);
    let mut client = Client::open(&path, "t".repeat(43).as_str()).unwrap();
    let connections = client.list().unwrap();
    drop(client);
    assert_eq!(connections.len(), 1);
    assert_eq!(
        (
            connections[0].id.as_str(),
            connections[0].name.as_str(),
            &connections[0].state
        ),
        (PROFILE, "Office", &State::Connected)
    );
    assert_eq!(server.join().unwrap(), ["hello", "list"]);
}

#[test]
fn up_reports_events_then_connected() {
    let (_dir, path, server) = serve(vec![
        vec![r#"{"id":{id},"ok":true,"api":1}"#],
        vec![
            r#"{"event":"state","connection":"6F1C2A4E-0B6D-4E43-9C55-1D2A7B8E9F01","state":"connecting"}"#,
            r#"{"event":"state","connection":"6F1C2A4E-0B6D-4E43-9C55-1D2A7B8E9F01","state":"waiting_for_mfa"}"#,
            r#"{"event":"state","connection":"6F1C2A4E-0B6D-4E43-9C55-1D2A7B8E9F01","state":"connected"}"#,
            r#"{"id":{id},"ok":true,"state":"connected"}"#,
        ],
    ]);
    let mut client = Client::open(&path, "t".repeat(43).as_str()).unwrap();
    let mut seen = Vec::new();
    client
        .up(PROFILE, |event| seen.push(event.state.clone()))
        .unwrap();
    drop(client);
    assert_eq!(seen, [State::Connecting, State::WaitingForMfa, State::Connected]);
    assert_eq!(server.join().unwrap(), ["hello", "up"]);
}

#[test]
fn a_failed_up_carries_the_code_and_message() {
    let (_dir, path, server) = serve(vec![
        vec![r#"{"id":{id},"ok":true,"api":1}"#],
        vec![r#"{"id":{id},"ok":false,"error":"setup_required","message":"Finish System Status"}"#],
    ]);
    let mut client = Client::open(&path, "t".repeat(43).as_str()).unwrap();
    let error = client.up(PROFILE, |_| {}).unwrap_err();
    drop(client);
    assert_eq!(error.code(), Some(ErrorCode::SetupRequired));
    assert!(error.to_string().contains("Finish System Status"), "{error}");
    server.join().unwrap();
}

#[test]
fn closer_unblocks_a_pending_up() {
    let (_dir, path, server) = serve(vec![
        vec![r#"{"id":{id},"ok":true,"api":1}"#],
        vec![
            r#"{"event":"state","connection":"6F1C2A4E-0B6D-4E43-9C55-1D2A7B8E9F01","state":"waiting_for_mfa"}"#,
            "<wait-for-close>",
        ],
    ]);
    let mut client = Client::open(&path, "t".repeat(43).as_str()).unwrap();
    let closer = client.closer().unwrap();
    let cancel = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        closer.close();
    });
    assert!(client.up(PROFILE, |_| {}).is_err());
    cancel.join().unwrap();
    server.join().unwrap();
}
