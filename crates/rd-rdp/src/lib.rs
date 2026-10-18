//! Embedded RDP: the session engine on `FreeRDP` (crate `rd-freerdp`), framebuffer frames, input and the
//! clipboard, plus the app-owned certificate store. The pure session rules (resize, disconnect classification, keyboard
//! mapping, type clipboard) live in [`rd_core::rdp`].

pub mod clipfiles;
mod engine;
pub mod frame;
pub mod known_hosts;
mod pointer;
pub mod session;

pub use session::{
    CertificateChoice, CertificateRequest, Credentials, CredentialsRequest, InputEvent, MouseButton,
    RdpConfig, RdpEvent, RdpHandler, RdpSession,
};

/// The engine's name and version, for Settings.
pub const ENGINE_NAME: &str = "FreeRDP";
/// The `FreeRDP` release this build links.
pub const ENGINE_VERSION: &str = rd_freerdp::VERSION;
