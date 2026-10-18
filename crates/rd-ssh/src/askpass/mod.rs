//! The askpass channel: the app's own executable is `ssh`'s `SSH_ASKPASS` program.
//!
//! `ssh` runs with `SSH_ASKPASS=<app exe>`, `SSH_ASKPASS_REQUIRE=force` and two variables of ours: a
//! one-per-session random token ([`TOKEN_ENV`]) and the local endpoint of the running app ([`SOCKET_ENV`]):
//! a Unix socket in a `0700` directory, or on Windows a named pipe only the current user may open. Started
//! that way, the executable runs [`client::run_helper`] instead of the app: it sends the token, the prompt
//! and `SSH_ASKPASS_PROMPT` to the [`server::AskpassServer`] and prints the answer.
//!
//! The server classifies the prompt ([`PromptKind`]). The first password and the first passphrase prompt
//! of a session are answered from the vault; every other prompt (a retry after a wrong password, a host
//! key confirmation, anything unknown) goes to the app's handler, which asks the user. There is no TTY
//! fallback: under `force`, `ssh` would send an empty answer as the password (Phase 0 spike).
//!
//! Wire format, both directions length-prefixed so it works without half-closing a named pipe:
//! request `RDA1` ‖ field(token) ‖ field(`SSH_ASKPASS_PROMPT`) ‖ field(prompt); response one status byte
//! (`1` answer, `0` refused) ‖ field(answer). A field is a little-endian `u32` length and UTF-8 bytes.

pub mod client;
pub mod server;

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::Path;

use serde::Serialize;
use zeroize::Zeroizing;

/// The session token variable; its presence starts the executable in helper mode.
pub const TOKEN_ENV: &str = "RD_ASKPASS_TOKEN";
/// The server endpoint variable: a socket path, or a pipe name on Windows.
pub const SOCKET_ENV: &str = "RD_ASKPASS_SOCKET";
/// Set by OpenSSH ≥ 8.4 for confirmation (`confirm`) and notice (`none`) prompts.
pub const PROMPT_HINT_ENV: &str = "SSH_ASKPASS_PROMPT";

const MAGIC: &[u8; 4] = b"RDA1";
const MAX_FIELD: usize = 64 * 1024;

/// What `ssh` is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PromptKind {
    /// A login password (`user@host's password:`, keyboard-interactive `Password:`).
    Password,
    /// A key passphrase (`Enter passphrase for key '…':`).
    Passphrase,
    /// A yes/no question, such as an unknown host key.
    Confirm,
    /// Anything else (one-time codes, notices).
    Other,
}

/// Classifies a prompt from its text and the `SSH_ASKPASS_PROMPT` hint.
#[must_use]
pub fn classify(prompt: &str, hint: &str) -> PromptKind {
    if hint.eq_ignore_ascii_case("confirm") {
        return PromptKind::Confirm;
    }
    let lower = prompt.to_lowercase();
    if lower.contains("passphrase") {
        PromptKind::Passphrase
    } else if lower.contains("password") {
        PromptKind::Password
    } else if lower.contains("(yes/no") || lower.contains("continue connecting") {
        PromptKind::Confirm
    } else {
        PromptKind::Other
    }
}

/// The environment that makes `ssh` ask `helper` (the app executable) through `endpoint` with `token`.
#[must_use]
pub fn askpass_env(helper: &Path, endpoint: &str, token: &str) -> Vec<(String, OsString)> {
    vec![
        ("SSH_ASKPASS".to_owned(), helper.as_os_str().to_owned()),
        ("SSH_ASKPASS_REQUIRE".to_owned(), "force".into()),
        (TOKEN_ENV.to_owned(), token.into()),
        (SOCKET_ENV.to_owned(), endpoint.into()),
    ]
}

/// A random 256-bit token as hex.
///
/// # Panics
/// If the OS random source fails, which leaves nothing safe to do.
#[must_use]
pub fn new_token() -> String {
    use std::fmt::Write as _;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random source is available");
    bytes.iter().fold(String::with_capacity(64), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// One prompt as sent by the helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireRequest {
    /// The session token.
    pub token: String,
    /// `SSH_ASKPASS_PROMPT` (empty when unset).
    pub hint: String,
    /// The prompt text.
    pub prompt: String,
}

fn write_field(w: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    let len = u32::try_from(bytes.len().min(MAX_FIELD)).unwrap_or(u32::MAX);
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&bytes[..len as usize])
}

fn read_field(r: &mut impl Read) -> io::Result<Zeroizing<Vec<u8>>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FIELD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "askpass field too long",
        ));
    }
    let mut bytes = Zeroizing::new(vec![0u8; len]);
    r.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn utf8(bytes: &[u8]) -> io::Result<String> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "askpass field is not UTF-8"))
}

/// Writes a request.
///
/// # Errors
/// I/O errors of `w`.
pub fn write_request(w: &mut impl Write, request: &WireRequest) -> io::Result<()> {
    w.write_all(MAGIC)?;
    write_field(w, request.token.as_bytes())?;
    write_field(w, request.hint.as_bytes())?;
    write_field(w, request.prompt.as_bytes())?;
    w.flush()
}

/// Reads a request.
///
/// # Errors
/// I/O errors, a wrong magic, an oversized field or invalid UTF-8.
pub fn read_request(r: &mut impl Read) -> io::Result<WireRequest> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not an askpass request",
        ));
    }
    Ok(WireRequest {
        token: utf8(&read_field(r)?)?,
        hint: utf8(&read_field(r)?)?,
        prompt: utf8(&read_field(r)?)?,
    })
}

/// Writes the answer, or a refusal for `None`.
///
/// # Errors
/// I/O errors of `w`.
pub fn write_response(w: &mut impl Write, answer: Option<&str>) -> io::Result<()> {
    w.write_all(&[u8::from(answer.is_some())])?;
    write_field(w, answer.unwrap_or_default().as_bytes())?;
    w.flush()
}

/// Reads the answer; `None` when refused.
///
/// # Errors
/// I/O errors, an oversized field or invalid UTF-8.
pub fn read_response(r: &mut impl Read) -> io::Result<Option<Zeroizing<String>>> {
    let mut status = [0u8; 1];
    r.read_exact(&mut status)?;
    let field = read_field(r)?;
    if status[0] == 0 {
        return Ok(None);
    }
    Ok(Some(Zeroizing::new(utf8(&field)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_openssh_prompts() {
        use PromptKind::{Confirm, Other, Passphrase, Password};
        assert_eq!(classify("tester@127.0.0.1's password: ", ""), Password);
        assert_eq!(classify("(tester@host) Password: ", ""), Password);
        assert_eq!(classify("Enter passphrase for key '/k': ", ""), Passphrase);
        assert_eq!(
            classify(
                "The authenticity of host 'h (::1)' can't be established.\nED25519 key fingerprint is \
                 SHA256:x.\nAre you sure you want to continue connecting (yes/no/[fingerprint])? ",
                ""
            ),
            Confirm
        );
        assert_eq!(classify("Allow use of key k?", "confirm"), Confirm);
        assert_eq!(classify("Verification code: ", ""), Other);
        assert_eq!(classify("Confirm user presence for key", "none"), Other);
    }

    #[test]
    fn env_points_ssh_at_the_helper() {
        let env = askpass_env(Path::new("/app/rd"), "/tmp/x/askpass.sock", "tok");
        let get = |k: &str| env.iter().find(|(name, _)| name == k).map(|(_, v)| v.clone());
        assert_eq!(get("SSH_ASKPASS"), Some("/app/rd".into()));
        assert_eq!(get("SSH_ASKPASS_REQUIRE"), Some("force".into()));
        assert_eq!(get(TOKEN_ENV), Some("tok".into()));
        assert_eq!(get(SOCKET_ENV), Some("/tmp/x/askpass.sock".into()));
    }

    #[test]
    fn tokens_are_random_hex() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn wire_round_trip() {
        let request = WireRequest {
            token: "t".into(),
            hint: String::new(),
            prompt: "jelszó:\n".into(),
        };
        let mut buf = Vec::new();
        write_request(&mut buf, &request).unwrap();
        assert_eq!(read_request(&mut buf.as_slice()).unwrap(), request);

        let mut buf = Vec::new();
        write_response(&mut buf, Some("pw")).unwrap();
        assert_eq!(
            read_response(&mut buf.as_slice())
                .unwrap()
                .as_deref()
                .map(String::as_str),
            Some("pw")
        );
        let mut buf = Vec::new();
        write_response(&mut buf, None).unwrap();
        assert!(read_response(&mut buf.as_slice()).unwrap().is_none());
    }

    #[test]
    fn rejects_garbage() {
        assert!(read_request(&mut b"HTTP/1.1".as_slice()).is_err());
        let mut huge = MAGIC.to_vec();
        huge.extend(u32::MAX.to_le_bytes());
        assert!(read_request(&mut huge.as_slice()).is_err());
        let mut bad_utf8 = MAGIC.to_vec();
        bad_utf8.extend(1u32.to_le_bytes());
        bad_utf8.push(0xFF);
        assert!(read_request(&mut bad_utf8.as_slice()).is_err());
    }
}
