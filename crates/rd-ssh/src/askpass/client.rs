//! The helper side: what the app executable does when `ssh` starts it as `SSH_ASKPASS`.

use std::io::{self, Write};

use interprocess::local_socket::Stream;
use interprocess::local_socket::prelude::*;
use zeroize::Zeroizing;

use super::{PROMPT_HINT_ENV, SOCKET_ENV, TOKEN_ENV, WireRequest, read_response, write_request};

/// Whether this process was started by `ssh` as the askpass helper. The app checks this first thing in
/// `main`, before any UI starts.
#[must_use]
pub fn helper_requested() -> bool {
    std::env::var_os(TOKEN_ENV).is_some()
}

/// Opens the server endpoint: a socket path, or a pipe name on Windows.
pub(crate) fn connect(endpoint: &str) -> io::Result<Stream> {
    #[cfg(windows)]
    let name = endpoint.to_ns_name::<interprocess::local_socket::GenericNamespaced>()?;
    #[cfg(not(windows))]
    let name = endpoint.to_fs_name::<interprocess::local_socket::GenericFilePath>()?;
    Stream::connect(name)
}

/// Sends one prompt to the server at `endpoint` and returns its answer (`None`: refused).
///
/// # Errors
/// When the server can't be reached or answers garbage.
pub fn ask(endpoint: &str, request: &WireRequest) -> io::Result<Option<Zeroizing<String>>> {
    let mut stream = connect(endpoint)?;
    write_request(&mut stream, request)?;
    read_response(&mut stream)
}

/// Runs the helper: asks the app for the prompt in `argv[1]` and prints the answer with a newline. Returns
/// the exit code: 0 with an answer, 1 when refused, empty or unreachable (`ssh` then fails the attempt).
#[must_use]
pub fn run_helper() -> i32 {
    let token = std::env::var(TOKEN_ENV).unwrap_or_default();
    let endpoint = std::env::var(SOCKET_ENV).unwrap_or_default();
    let request = WireRequest {
        token,
        hint: std::env::var(PROMPT_HINT_ENV).unwrap_or_default(),
        prompt: std::env::args().nth(1).unwrap_or_default(),
    };
    match ask(&endpoint, &request) {
        Ok(Some(answer)) if !answer.is_empty() => {
            let mut line = Zeroizing::new(String::with_capacity(answer.len() + 1));
            line.push_str(&answer);
            line.push('\n');
            let mut stdout = io::stdout().lock();
            let written = stdout.write_all(line.as_bytes()).and_then(|()| stdout.flush());
            i32::from(written.is_err())
        }
        _ => 1,
    }
}
