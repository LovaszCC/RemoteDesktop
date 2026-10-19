//! The external launch flows against a recording launcher: nothing is started.

use std::sync::{Arc, Mutex, PoisonError};

use rd_core::ConnectionType;
use rd_core::draft::ConnectionDraft;
use rd_platform::launch::{LaunchError, Launcher};
use rd_platform::rdp::RdpOutcome;
use zeroize::Zeroizing;

use super::tests::Env;
use super::*;

/// One recorded side effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Call {
    Web {
        url: String,
        browser: Option<String>,
    },
    Url(String),
    Copy(Vec<String>),
    Winbox {
        configured: Option<String>,
        login: Vec<String>,
    },
    Rdp {
        file: String,
        password: Option<String>,
    },
}

/// Records the calls; `fail` makes every launch fail with that error.
#[derive(Clone, Default)]
pub(super) struct Recorder {
    calls: Arc<Mutex<Vec<Call>>>,
    fail: Option<LaunchError>,
    /// What `launch_rdp` returns; it also calls the fallback with this key when set.
    rdp_outcome: Option<RdpOutcome>,
    rdp_fallback: Option<&'static str>,
}

impl Recorder {
    pub(super) fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn push(&self, call: Call) -> Result<(), LaunchError> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call);
        self.fail.clone().map_or(Ok(()), Err)
    }
}

impl Launcher for Recorder {
    fn launch_rdp(
        &self,
        connection: &rd_core::Connection,
        password: Option<Zeroizing<String>>,
        on_fallback: rd_platform::rdp::Fallback,
    ) -> Result<RdpOutcome, LaunchError> {
        self.push(Call::Rdp {
            file: rd_platform::rdp::rdp_file(connection),
            password: password.map(|p| p.to_string()),
        })?;
        if let Some(message) = self.rdp_fallback {
            on_fallback(message);
        }
        Ok(self.rdp_outcome.clone().unwrap_or(RdpOutcome::Launched))
    }

    fn open_web(&self, url: &str, browser_id: Option<&str>) -> Result<(), LaunchError> {
        self.push(Call::Web {
            url: url.to_owned(),
            browser: browser_id.map(str::to_owned),
        })
    }

    fn open_url(&self, url: &str) -> Result<(), LaunchError> {
        self.push(Call::Url(url.to_owned()))
    }

    fn copy_sequence(&self, values: Vec<Zeroizing<String>>) -> Result<(), LaunchError> {
        self.push(Call::Copy(values.iter().map(|v| v.to_string()).collect()))
    }

    fn launch_winbox(
        &self,
        configured: Option<&std::path::Path>,
        login: Vec<String>,
    ) -> Result<(), LaunchError> {
        self.push(Call::Winbox {
            configured: configured.map(|p| p.display().to_string()),
            login,
        })
    }
}

/// An unlocked core that records launches.
pub(super) fn recording(env: &Env, fail: Option<LaunchError>) -> (AppCore, Recorder) {
    let recorder = Recorder {
        fail,
        ..Recorder::default()
    };
    let core = env.unlocked().with_launcher(Box::new(recorder.clone()));
    (core, recorder)
}

pub(super) fn save(
    core: &AppCore,
    kind: ConnectionType,
    edit: impl FnOnce(&mut ConnectionDraft),
    password: Option<&str>,
) -> String {
    let mut draft = ConnectionDraft::new(kind, None);
    draft.name = "Target".into();
    draft.host = "host.example.com".into();
    edit(&mut draft);
    core.save_connection(None, &draft, password, None).unwrap()
}

#[test]
fn web_opens_the_browser_then_copies_password_and_username() {
    let env = Env::new();
    let (core, recorder) = recording(&env, None);
    let id = save(
        &core,
        ConnectionType::Web,
        |d| {
            d.host = " router.example/admin ".into();
            d.username = "bob".into();
            d.browser_bundle_id = Some("com.brave.Browser".into());
        },
        Some("pw"),
    );
    assert_eq!(core.connect_web(&id), Ok(LaunchOutcomeDto::LoginCopied));
    assert_eq!(
        recorder.calls(),
        [
            Call::Web {
                url: "https://router.example/admin".into(),
                browser: Some("com.brave.Browser".into())
            },
            Call::Copy(vec!["pw".into(), "bob".into()]),
        ]
    );
}

#[test]
fn web_http_opens_in_the_browser() {
    let env = Env::new();
    let (core, recorder) = recording(&env, None);
    let id = save(
        &core,
        ConnectionType::Web,
        |d| {
            d.port = "8443".into();
            d.web_path = "/admin".into();
            d.browser_bundle_id = Some("org.mozilla.firefox".into());
        },
        None,
    );
    assert_eq!(core.connect_web(&id), Ok(LaunchOutcomeDto::Launched));
    assert_eq!(
        recorder.calls(),
        [Call::Web {
            url: "https://host.example.com:8443/admin".into(),
            browser: Some("org.mozilla.firefox".into()),
        }]
    );
}

#[test]
fn web_non_http_scheme_opens_with_the_os() {
    let env = Env::new();
    let (core, recorder) = recording(&env, None);
    let id = save(
        &core,
        ConnectionType::Web,
        |d| {
            d.web_scheme = "smb".into();
            d.web_path = "/share".into();
            d.username = "bob".into();
        },
        Some("pw"),
    );
    assert_eq!(core.connect_web(&id), Ok(LaunchOutcomeDto::LoginCopied));
    assert_eq!(
        recorder.calls(),
        [
            Call::Url("smb://host.example.com/share".into()),
            Call::Copy(vec!["pw".into(), "bob".into()]),
        ]
    );
}

#[test]
fn web_without_login_only_opens() {
    let env = Env::new();
    let (core, recorder) = recording(&env, None);
    let id = save(
        &core,
        ConnectionType::Web,
        |d| {
            d.web_scheme = "http".into();
            d.host = "example.com".into();
            d.port = "8080".into();
            d.web_path = "/x".into();
        },
        None,
    );
    assert_eq!(core.connect_web(&id), Ok(LaunchOutcomeDto::Launched));
    assert_eq!(
        recorder.calls(),
        [Call::Web {
            url: "http://example.com:8080/x".into(),
            browser: None
        }]
    );
}

#[test]
fn web_uses_the_linked_credential() {
    let env = Env::new();
    let (core, recorder) = recording(&env, None);
    let credential = core
        .save_credential(
            None,
            &CredentialInput {
                name: "Shared".into(),
                username: "admin".into(),
                ..CredentialInput::default()
            },
            Some("secret"),
            None,
        )
        .unwrap();
    let id = save(
        &core,
        ConnectionType::Web,
        |d| {
            d.username = "own".into();
            d.credential_id = Some(credential);
        },
        Some("own-pw"),
    );
    core.connect_web(&id).unwrap();
    assert_eq!(
        recorder.calls()[1],
        Call::Copy(vec!["secret".into(), "admin".into()])
    );
}

#[test]
fn web_errors() {
    let env = Env::new();
    let (core, recorder) = recording(&env, Some(LaunchError::OpenFailed("no browser".into())));
    let web = save(&core, ConnectionType::Web, |_| {}, None);
    let error = core.connect_web(&web).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.detail.as_deref()),
        ("launchFailed", Some("no browser"))
    );

    let ssh = save(&core, ConnectionType::Ssh, |_| {}, None);
    assert_eq!(core.connect_web(&ssh).unwrap_err().code, "wrongType");
    assert_eq!(core.connect_web("missing").unwrap_err().code, "notFound");
    assert_eq!(recorder.calls().len(), 1);

    core.lock();
    assert_eq!(core.connect_web(&web).unwrap_err().code, "locked");
}

#[test]
fn launch_errors_map_to_codes() {
    let error = CommandError::from(LaunchError::AppNotInstalled("WinBox".into()));
    assert_eq!(
        (
            error.code.as_str(),
            error.message.as_str(),
            error.detail.as_deref()
        ),
        ("appNotInstalled", "%@ is not installed.", Some("WinBox"))
    );
    assert_eq!(
        CommandError::from(LaunchError::Clipboard("x".into())).code,
        "clipboard"
    );
}

#[test]
fn outcomes_serialize_with_a_kind() {
    assert_eq!(
        serde_json::to_value(LaunchOutcomeDto::PasswordCopied {
            reason: "Accessibility permission missing".into()
        })
        .unwrap(),
        serde_json::json!({"kind": "passwordCopied", "reason": "Accessibility permission missing"})
    );
    assert_eq!(
        serde_json::to_value(LaunchOutcomeDto::LoginCopied).unwrap(),
        serde_json::json!({"kind": "loginCopied"})
    );
}

#[test]
fn winbox_logs_in_with_host_user_and_password() {
    let env = Env::new();
    let (core, recorder) = recording(&env, None);
    let id = save(
        &core,
        ConnectionType::Mikrotik,
        |d| {
            d.host = " router.example ".into();
            d.port = "2222".into();
            d.username = "admin".into();
        },
        Some("pw"),
    );
    assert_eq!(core.connect_winbox(&id), Ok(LaunchOutcomeDto::Launched));
    let no_user = save(&core, ConnectionType::Mikrotik, |_| {}, Some("pw"));
    core.settings_set(&SettingsPatch {
        winbox_path: Some(" /opt/winbox/WinBox ".into()),
        ..SettingsPatch::default()
    })
    .unwrap();
    core.connect_winbox(&no_user).unwrap();
    assert_eq!(
        recorder.calls(),
        [
            Call::Winbox {
                configured: None,
                login: vec!["router.example".into(), "admin".into(), "pw".into()]
            },
            Call::Winbox {
                configured: Some("/opt/winbox/WinBox".into()),
                login: vec!["host.example.com".into()]
            },
        ]
    );
}

#[test]
fn winbox_errors() {
    let env = Env::new();
    let (core, _) = recording(&env, Some(LaunchError::AppNotInstalled("WinBox".into())));
    let id = save(&core, ConnectionType::Mikrotik, |_| {}, None);
    let error = core.connect_winbox(&id).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.detail.as_deref()),
        ("appNotInstalled", Some("WinBox"))
    );
    let web = save(&core, ConnectionType::Web, |_| {}, None);
    assert_eq!(core.connect_winbox(&web).unwrap_err().code, "wrongType");
}

#[test]
fn winbox_path_setting_and_status() {
    let env = Env::new();
    let core = env.unlocked();
    assert_eq!(core.settings_get().winbox_path, None);
    assert_eq!(core.winbox_status().configured_exists, None);
    let temp = tempfile::tempdir().unwrap();
    let exe = temp.path().join("winbox64.exe");
    std::fs::write(&exe, "").unwrap();
    let dto = core
        .settings_set(&SettingsPatch {
            winbox_path: Some(exe.display().to_string()),
            ..SettingsPatch::default()
        })
        .unwrap();
    assert_eq!(dto.winbox_path, Some(exe.display().to_string()));
    let status = core.winbox_status();
    assert_eq!(status.configured_exists, Some(true));
    assert_eq!(status.uses_wine, cfg!(not(any(target_os = "macos", windows))));
    let cleared = core
        .settings_set(&SettingsPatch {
            winbox_path: Some(String::new()),
            ..SettingsPatch::default()
        })
        .unwrap();
    assert_eq!(cleared.winbox_path, None);
}

#[test]
fn rdp_hands_the_resolved_connection_to_the_system_client() {
    let env = Env::new();
    let (core, recorder) = recording(&env, None);
    let id = save(
        &core,
        ConnectionType::Rdp,
        |d| {
            d.host = "dc.example.test".into();
            d.port = "3390".into();
            d.username = "admin".into();
            d.domain = "CORP".into();
            d.rdp_admin_session = true;
            d.rdp_client = rd_core::RdpClient::WindowsApp;
        },
        Some("pw"),
    );
    assert_eq!(core.connect_rdp_external(&id), Ok(LaunchOutcomeDto::Launched));
    let calls = recorder.calls();
    let [Call::Rdp { file, password }] = calls.as_slice() else {
        panic!("one RDP launch expected");
    };
    assert!(file.contains("full address:s:dc.example.test:3390\r\n"));
    assert!(file.contains("username:s:CORP\\admin\r\n"));
    assert!(file.contains("administrative session:i:1"));
    assert!(!file.contains("pw"));
    assert_eq!(password.as_deref(), Some("pw"));

    let ssh = save(&core, ConnectionType::Ssh, |_| {}, None);
    assert_eq!(core.connect_rdp_external(&ssh).unwrap_err().code, "wrongType");
}

#[test]
fn rdp_reports_the_clipboard_fallbacks() {
    let env = Env::new();
    let recorder = Recorder {
        rdp_outcome: Some(RdpOutcome::PasswordCopied(
            rd_platform::rdp::REASON_NO_ACCESSIBILITY,
        )),
        rdp_fallback: Some(rd_platform::rdp::NOTICE_NO_FIELD),
        ..Recorder::default()
    };
    let core = env.unlocked().with_launcher(Box::new(recorder.clone()));
    let notices = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&notices);
    core.set_launch_notifier(Arc::new(move |notice| sink.lock().unwrap().push(notice)));
    let id = save(&core, ConnectionType::Rdp, |d| d.name = "Office".into(), None);
    assert_eq!(
        core.connect_rdp_external(&id),
        Ok(LaunchOutcomeDto::PasswordCopied {
            reason: "Accessibility permission missing".into()
        })
    );
    let calls = recorder.calls();
    let [Call::Rdp { file, password: None }] = calls.as_slice() else {
        panic!("one RDP launch without a password expected");
    };
    assert!(file.starts_with("full address:s:host.example.com\r\n"));
    assert_eq!(
        notices.lock().unwrap().as_slice(),
        [LaunchNoticeDto {
            connection_name: "Office".into(),
            message: "Couldn't find the password field — password on the clipboard (30 s)".into()
        }]
    );
}

#[test]
fn accessibility_status_only_on_macos() {
    assert_eq!(
        AppCore::accessibility_status().is_some(),
        cfg!(target_os = "macos")
    );
}
