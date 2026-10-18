//! Interactive SSH with the system `ssh`: the argument list, the askpass channel (the app's own
//! executable as `SSH_ASKPASS`), unlocked-key temp files, finding `ssh`, and the PTY session with its
//! stream analyzer and output recorder (which also runs the local shell, [`local`]). Background
//! connections (russh) resolve `~/.ssh/config` ([`config`]) and check host keys ([`known_hosts`])
//! themselves.
//!

pub mod analyzer;
pub mod askpass;
pub mod background;
pub mod broadcast;
pub mod command;
pub mod config;
pub mod external;
pub mod files;
pub mod key_file;
pub mod known_hosts;
pub mod local;
pub mod locate;
pub mod recorder;
pub mod session;

pub use command::{ArgsError, SshSecrets, resolve, ssh_arguments};
pub use key_file::UnlockedKeyFile;
pub use locate::find_ssh;
pub use session::{SessionEvent, SessionKind, Sink, SpawnSpec, SshSession};
