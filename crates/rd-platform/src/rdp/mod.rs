//! The system RDP client (`rdpClient = windowsApp`), a port of Swift `RDPLauncher` and
//! `RDPFileBuilder`:
//! - macOS: a `.rdp` file opened in Windows App, then the password typed into its prompt through the
//!   Accessibility API ([`autofill`]); without the permission, or when no field shows up, the password goes
//!   on the clipboard (concealed, 30 s).
//! - Windows: `mstsc.exe` with a `.rdp` file; the password goes in through `CredWriteW` as a
//!   `TERMSRV/<host>` domain credential for the session (never on a command line) and is deleted 60 s
//!   after the start.
//! - Linux: Remmina with a `.remmina` file (the password on the clipboard, concealed, 30 s), else
//!   `xfreerdp3` / `xfreerdp` with `/from-stdin`, the password written to its standard input.
//!
//! No file holds the password. Files go into a private temporary folder (0700, files 0600 on Unix) that is
//! removed 60 s after the start. The builders are plain functions, tested on every OS.

#[cfg(target_os = "macos")]
pub mod autofill;
pub mod ax_tree;
#[cfg(windows)]
mod credentials;

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use rd_core::{Connection, ConnectionType};

use crate::launch::LaunchError;

/// Windows App's bundle id (Swift `ExternalApps.windowsAppBundleID`).
pub const WINDOWS_APP_BUNDLE_ID: &str = "com.microsoft.rdc.macos";

/// How long the launch folder and the Windows credential stay.
pub const CLEANUP_AFTER: Duration = Duration::from_secs(60);

/// What the user is told after a launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RdpOutcome {
    /// The client started (and, on macOS, the password is being typed in).
    Launched,
    /// The password is on the clipboard for 30 s; the reason is a catalog key.
    PasswordCopied(&'static str),
}

/// Reason: macOS without the Accessibility permission (Swift).
pub const REASON_NO_ACCESSIBILITY: &str = "Accessibility permission missing";
/// Reason: Remmina can't take the password from us.
pub const REASON_REMMINA: &str = "Remmina";
/// The toast when Windows App showed no password field in time (Swift).
pub const NOTICE_NO_FIELD: &str = "Couldn't find the password field — password on the clipboard (30 s)";

/// `DOMAIN\user`, unless the user name already carries a domain (`user@domain`, `DOMAIN\user`) (Swift
/// `RDPFileBuilder.qualifiedUsername`).
#[must_use]
pub fn qualified_username(username: &str, domain: &str) -> String {
    let username = username.trim();
    let domain = domain.trim();
    if username.is_empty() {
        return String::new();
    }
    if domain.is_empty() || username.contains('\\') || username.contains('@') {
        return username.to_owned();
    }
    format!("{domain}\\{username}")
}

/// User and domain for clients with separate fields: `DOMAIN\user` splits, `user@domain` keeps the domain
/// empty, otherwise the domain field is used (Swift `RDPSessionSettings`).
#[must_use]
pub fn split_username(username: &str, domain: &str) -> (String, String) {
    let username = username.trim();
    if let Some((domain, user)) = username.split_once('\\') {
        return (user.to_owned(), domain.to_owned());
    }
    if username.contains('@') {
        return (username.to_owned(), String::new());
    }
    (username.to_owned(), domain.trim().to_owned())
}

/// `host`, `host:port` for a non-default port, IPv6 in brackets (Swift `RDPFileBuilder.address`).
#[must_use]
pub fn address(host: &str, port: u16) -> String {
    let host = host.trim();
    let bracketed = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    if port == ConnectionType::Rdp.default_port() {
        bracketed
    } else {
        format!("{bracketed}:{port}")
    }
}

/// Values in the line-based files must stay on one line.
fn one_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

/// The `.rdp` file, CRLF lines, no password (Swift `RDPFileBuilder.contents`).
#[must_use]
pub fn rdp_file(connection: &Connection) -> String {
    let mut lines = vec![
        format!(
            "full address:s:{}",
            one_line(&address(&connection.host, connection.effective_port()))
        ),
        "screen mode id:i:2".to_owned(),
        "use multimon:i:0".to_owned(),
        "dynamic resolution:i:1".to_owned(),
        "session bpp:i:32".to_owned(),
        "prompt for credentials:i:0".to_owned(),
        "authentication level:i:2".to_owned(),
    ];
    let user = qualified_username(&connection.username, &connection.domain);
    if !user.is_empty() {
        lines.push(format!("username:s:{}", one_line(&user)));
    }
    if connection.rdp_admin_session {
        lines.push("administrative session:i:1".to_owned());
    }
    lines.join("\r\n") + "\r\n"
}

/// The `.remmina` file for Remmina's RDP plugin, no password. `console=1` is Remmina's admin session.
#[must_use]
pub fn remmina_file(connection: &Connection) -> String {
    let (user, domain) = split_username(&connection.username, &connection.domain);
    let mut lines = vec![
        "[remmina]".to_owned(),
        format!("name={}", one_line(&connection.name)),
        "protocol=RDP".to_owned(),
        format!(
            "server={}",
            one_line(&address(&connection.host, connection.effective_port()))
        ),
        format!("username={}", one_line(&user)),
        format!("domain={}", one_line(&domain)),
        "resolution_mode=2".to_owned(),
        "colordepth=99".to_owned(),
    ];
    if connection.rdp_admin_session {
        lines.push("console=1".to_owned());
    }
    lines.join("\n") + "\n"
}

/// The `FreeRDP` major version from the output of `xfreerdp /version`.
#[must_use]
pub fn freerdp_major(version_output: &str) -> Option<u32> {
    let rest = version_output.split("FreeRDP version").nth(1)?.trim_start();
    rest.split('.').next()?.trim().parse().ok()
}

/// `xfreerdp` arguments: address, user, domain, admin session, dynamic resolution, trust on first use
/// (`/cert:tofu` from `FreeRDP` 3, `/cert-tofu` before) and, with a password, `/from-stdin` so it is read
/// from standard input instead of the command line.
#[must_use]
pub fn xfreerdp_args(connection: &Connection, major: u32, with_password: bool) -> Vec<String> {
    let (user, domain) = split_username(&connection.username, &connection.domain);
    let mut args = vec![
        format!("/v:{}", address(&connection.host, connection.effective_port())),
        format!("/title:{}", connection.name),
    ];
    if !user.is_empty() {
        args.push(format!("/u:{user}"));
    }
    if !domain.is_empty() {
        args.push(format!("/d:{domain}"));
    }
    if connection.rdp_admin_session {
        args.push("/admin".to_owned());
    }
    args.push("/dynamic-resolution".to_owned());
    args.push(if major >= 3 { "/cert:tofu" } else { "/cert-tofu" }.to_owned());
    if with_password {
        args.push("/from-stdin".to_owned());
    }
    args
}

/// The Windows credential target `mstsc` looks up: `TERMSRV/<host>` (no port, no brackets).
#[must_use]
pub fn termsrv_target(host: &str) -> String {
    let host = host.trim();
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    format!("TERMSRV/{host}")
}

/// A safe file name from the connection name (Swift `RDPFileBuilder.fileName`): letters, digits, `-_. `
/// kept, the rest `_`, `connection` when empty.
#[must_use]
pub fn file_name(name: &str, extension: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim();
    let stem = if cleaned.is_empty() { "connection" } else { cleaned };
    format!("{stem}.{extension}")
}

/// Writes `contents` into a new private folder under the runtime or temp dir (0700; the file 0600 on Unix) and
/// returns the folder and the file.
///
/// # Errors
/// When the folder or the file can't be written.
pub fn write_private_file(name: &str, contents: &str) -> io::Result<(PathBuf, PathBuf)> {
    // The per-user runtime dir on Linux (0700); macOS and Windows temp dirs are per user already.
    let base = dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(crate::APP_IDENTITY.data_dir_name);
    std::fs::create_dir_all(&base)?;
    let mut builder = tempfile::Builder::new();
    builder.prefix("launch-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let dir = builder.tempdir_in(&base)?.keep();
    let file = dir.join(name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut handle = options.open(&file)?;
    handle.write_all(contents.as_bytes())?;
    handle.sync_all()?;
    Ok((dir, file))
}

/// Removes `dir` after [`CLEANUP_AFTER`], then runs `also` (Swift `scheduleRemoval`).
fn remove_later(dir: PathBuf, also: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        std::thread::sleep(CLEANUP_AFTER);
        also();
        let _ = std::fs::remove_dir_all(&dir);
    });
}

fn write_failed(error: &io::Error) -> LaunchError {
    LaunchError::OpenFailed(error.to_string())
}

/// Called with a catalog key when the launch falls back later (the macOS autofill found no field).
pub type Fallback = Box<dyn FnOnce(&'static str) + Send>;

/// Opens `connection` (resolved, with `password`) in the system RDP client.
///
/// # Errors
/// `AppNotInstalled` when there is no client, `OpenFailed` when it can't be started or the file can't be
/// written, `Clipboard` when the password can't be copied.
#[cfg_attr(not(target_os = "macos"), allow(clippy::needless_pass_by_value))]
pub fn launch(
    connection: &Connection,
    password: Option<zeroize::Zeroizing<String>>,
    on_fallback: Fallback,
) -> Result<RdpOutcome, LaunchError> {
    let password = password.filter(|p| !p.is_empty());
    #[cfg(target_os = "macos")]
    {
        launch_mac(connection, password, on_fallback)
    }
    #[cfg(windows)]
    {
        let _ = on_fallback;
        launch_windows(connection, password)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = on_fallback;
        launch_linux(connection, password)
    }
}

#[cfg(target_os = "macos")]
fn launch_mac(
    connection: &Connection,
    password: Option<zeroize::Zeroizing<String>>,
    on_fallback: Fallback,
) -> Result<RdpOutcome, LaunchError> {
    use crate::clipboard::{PASSWORD_LIFETIME, copy_concealed};

    if !crate::macos::is_installed(WINDOWS_APP_BUNDLE_ID) {
        return Err(LaunchError::AppNotInstalled("Windows App".to_owned()));
    }
    let (dir, file) = write_private_file(&file_name(&connection.name, "rdp"), &rdp_file(connection))
        .map_err(|e| write_failed(&e))?;
    remove_later(dir, || {});
    crate::launch::ProcessSpec::new(
        "/usr/bin/open",
        vec![
            "-b".to_owned(),
            WINDOWS_APP_BUNDLE_ID.to_owned(),
            file.display().to_string(),
        ],
    )
    .run()?;
    let Some(password) = password else {
        return Ok(RdpOutcome::Launched);
    };
    if !autofill::is_trusted(true) {
        copy_concealed(password, PASSWORD_LIFETIME)?;
        return Ok(RdpOutcome::PasswordCopied(REASON_NO_ACCESSIBILITY));
    }
    std::thread::spawn(move || {
        if !autofill::fill_password(&password, WINDOWS_APP_BUNDLE_ID, autofill::TIMEOUT) {
            let _ = copy_concealed(password, PASSWORD_LIFETIME);
            on_fallback(NOTICE_NO_FIELD);
        }
    });
    Ok(RdpOutcome::Launched)
}

#[cfg(windows)]
fn launch_windows(
    connection: &Connection,
    password: Option<zeroize::Zeroizing<String>>,
) -> Result<RdpOutcome, LaunchError> {
    let windir = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let mstsc = windir.join(r"System32\mstsc.exe");
    if !mstsc.is_file() {
        return Err(LaunchError::AppNotInstalled(
            "Remote Desktop Connection".to_owned(),
        ));
    }
    let (dir, file) = write_private_file(&file_name(&connection.name, "rdp"), &rdp_file(connection))
        .map_err(|e| write_failed(&e))?;
    let user = qualified_username(&connection.username, &connection.domain);
    let target = termsrv_target(&connection.host);
    let stored = match password {
        Some(password) if !user.is_empty() => {
            credentials::write(&target, &user, &password).map_err(|e| write_failed(&e))?;
            true
        }
        _ => false,
    };
    let started =
        crate::launch::ProcessSpec::new(mstsc.display().to_string(), vec![file.display().to_string()])
            .spawn();
    remove_later(dir, move || {
        if stored {
            let _ = credentials::delete(&target);
        }
    });
    started.map(|()| RdpOutcome::Launched)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn launch_linux(
    connection: &Connection,
    password: Option<zeroize::Zeroizing<String>>,
) -> Result<RdpOutcome, LaunchError> {
    use crate::launch::{ProcessSpec, which};

    if let Some(remmina) = which("remmina") {
        let (dir, file) =
            write_private_file(&file_name(&connection.name, "remmina"), &remmina_file(connection))
                .map_err(|e| write_failed(&e))?;
        remove_later(dir, || {});
        ProcessSpec::new(
            remmina.display().to_string(),
            vec!["-c".to_owned(), file.display().to_string()],
        )
        .spawn()?;
        return match password {
            Some(password) => {
                crate::clipboard::copy_concealed(password, crate::clipboard::PASSWORD_LIFETIME)?;
                Ok(RdpOutcome::PasswordCopied(REASON_REMMINA))
            }
            None => Ok(RdpOutcome::Launched),
        };
    }
    let freerdp = which("xfreerdp3")
        .or_else(|| which("xfreerdp"))
        .ok_or_else(|| LaunchError::AppNotInstalled("Remmina / FreeRDP".to_owned()))?;
    let version = std::process::Command::new(&freerdp)
        .arg("/version")
        .stdin(std::process::Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let major = freerdp_major(&version).unwrap_or(2);
    let command = ProcessSpec::new(
        freerdp.display().to_string(),
        xfreerdp_args(connection, major, password.is_some()),
    );
    match password {
        Some(password) => command.spawn_with_input(password)?,
        None => command.spawn()?,
    }
    Ok(RdpOutcome::Launched)
}

/// The macOS Accessibility permission, for Settings: `None` on the other OSes.
#[must_use]
pub fn accessibility_trusted(prompt: bool) -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        Some(autofill::is_trusted(prompt))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = prompt;
        None
    }
}

/// Opens System Settings at Privacy & Security › Accessibility (macOS), after asking for the permission
/// so the app is listed there (Swift `SettingsView`).
///
/// # Errors
/// When System Settings can't be opened; `OpenFailed` on the other OSes.
pub fn open_accessibility_settings() -> Result<(), LaunchError> {
    #[cfg(target_os = "macos")]
    {
        let _ = autofill::is_trusted(true);
        crate::launch::ProcessSpec::new(
            "/usr/bin/open",
            vec!["x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility".to_owned()],
        )
        .run()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(LaunchError::OpenFailed("only on macOS".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rdp(host: &str) -> Connection {
        Connection::new(ConnectionType::Rdp, "dc", host)
    }

    #[test]
    fn writes_address_and_domain_user() {
        let mut connection = rdp("dc.example.test");
        connection.username = "admin".into();
        connection.domain = "CORP".into();
        let text = rdp_file(&connection);
        assert!(text.contains("full address:s:dc.example.test\r\n"));
        assert!(text.contains("username:s:CORP\\admin\r\n"));
        assert!(text.contains("prompt for credentials:i:0"));
        assert!(!text.contains("password"));
        assert!(text.ends_with("\r\n"));
    }

    #[test]
    fn custom_port_and_ipv6() {
        assert_eq!(address("srv", 3390), "srv:3390");
        assert_eq!(address("fe80::1", 3389), "[fe80::1]");
        assert_eq!(address("[fe80::1]", 4000), "[fe80::1]:4000");
        assert_eq!(address(" srv ", 3389), "srv");
        let mut connection = rdp("srv");
        connection.port = Some(3390);
        assert!(rdp_file(&connection).starts_with("full address:s:srv:3390\r\n"));
    }

    #[test]
    fn qualified_username_variants() {
        assert_eq!(qualified_username("bob", ""), "bob");
        assert_eq!(qualified_username("bob", "CORP"), "CORP\\bob");
        assert_eq!(qualified_username("bob@corp.local", "CORP"), "bob@corp.local");
        assert_eq!(qualified_username("X\\bob", "CORP"), "X\\bob");
        assert_eq!(qualified_username(" ", "CORP"), "");
    }

    #[test]
    fn qualified_usernames_override_the_domain_field() {
        assert_eq!(
            split_username("AD\\alice", "OTHER"),
            ("alice".into(), "AD".into())
        );
        assert_eq!(
            split_username("alice@ad.local", "OTHER"),
            ("alice@ad.local".into(), String::new())
        );
        assert_eq!(
            split_username(" alice ", " CORP "),
            ("alice".into(), "CORP".into())
        );
    }

    #[test]
    fn omits_username_when_empty_and_admin_only_when_enabled() {
        let normal = rdp_file(&rdp("h"));
        assert!(!normal.contains("username:s:"));
        assert!(!normal.contains("administrative session"));
        let mut admin = rdp("h");
        admin.rdp_admin_session = true;
        assert!(rdp_file(&admin).contains("administrative session:i:1\r\n"));
    }

    #[test]
    fn file_name_is_sanitised() {
        assert_eq!(file_name("web/01: prod", "rdp"), "web_01_ prod.rdp");
        assert_eq!(file_name("", "rdp"), "connection.rdp");
        assert_eq!(file_name("  ", "remmina"), "connection.remmina");
        assert_eq!(file_name("Szerver ő", "rdp"), "Szerver ő.rdp");
    }

    #[test]
    fn remmina_file_without_password() {
        let mut connection = rdp("fe80::1");
        connection.name = "Office\nPC".into();
        connection.port = Some(3390);
        connection.username = "CORP\\bob".into();
        connection.domain = "OTHER".into();
        connection.rdp_admin_session = true;
        let text = remmina_file(&connection);
        assert_eq!(
            text,
            "[remmina]\nname=Office PC\nprotocol=RDP\nserver=[fe80::1]:3390\nusername=bob\ndomain=CORP\n\
             resolution_mode=2\ncolordepth=99\nconsole=1\n"
        );
        assert!(!text.contains("password"));
        assert!(!remmina_file(&rdp("h")).contains("console"));
    }

    #[test]
    fn freerdp_versions() {
        assert_eq!(
            freerdp_major("This is FreeRDP version 3.5.1 (n/a)\nBuild config: …"),
            Some(3)
        );
        assert_eq!(freerdp_major("This is FreeRDP version 2.11.7 (2.11.7)"), Some(2));
        assert_eq!(freerdp_major("garbage"), None);
    }

    #[test]
    fn xfreerdp_reads_the_password_from_stdin() {
        let mut connection = rdp("srv.example.test");
        connection.name = "Office PC".into();
        connection.username = "bob".into();
        connection.domain = "CORP".into();
        connection.rdp_admin_session = true;
        assert_eq!(
            xfreerdp_args(&connection, 3, true),
            [
                "/v:srv.example.test",
                "/title:Office PC",
                "/u:bob",
                "/d:CORP",
                "/admin",
                "/dynamic-resolution",
                "/cert:tofu",
                "/from-stdin"
            ]
        );
        let plain = xfreerdp_args(&rdp("srv"), 2, false);
        assert_eq!(
            plain,
            ["/v:srv", "/title:dc", "/dynamic-resolution", "/cert-tofu"]
        );
        assert!(
            xfreerdp_args(&connection, 3, true)
                .iter()
                .all(|a| !a.starts_with("/p:"))
        );
    }

    #[test]
    fn termsrv_targets() {
        assert_eq!(termsrv_target(" srv.example.test "), "TERMSRV/srv.example.test");
        assert_eq!(termsrv_target("[fe80::1]"), "TERMSRV/fe80::1");
    }

    #[test]
    fn private_files_are_private() {
        let (dir, file) = write_private_file("test.rdp", "full address:s:h\r\n").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "full address:s:h\r\n");
        assert_eq!(file.parent(), Some(dir.as_path()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
