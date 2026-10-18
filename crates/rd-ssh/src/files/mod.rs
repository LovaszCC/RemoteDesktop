//! The file browser (SCP/SFTP): remote listings over an exec channel, the local pane,
//! and transfers as tar streams (SFTP for single files on servers without `tar`).
//!
//! - [`listing`]: the `find -printf` / `ls -lnA` script and its parsers;
//! - [`path`]: POSIX path helpers and the shell's directory from a terminal title or OSC 7;
//! - [`local`]: the local folder listing;
//! - [`plan`]: the remote commands of a transfer and the conflict policy;
//! - [`archive`]: the local ends of a tar stream (safe unpacking, packing);
//! - [`remote`]: the connected remote side (listing, probes; SFTP on servers without `tar`);
//! - [`transfer`]: running a transfer with progress, conflicts, cancel and partial cleanup.

pub mod archive;
pub mod listing;
pub mod local;
pub mod path;
pub mod plan;
pub mod remote;
pub mod transfer;

pub use listing::{Entry, EntryKind, Listing, Unreadable};
pub use plan::{ConflictChoice, ConflictPolicy, Direction};
pub use remote::{FilesError, RemoteFiles, RemoteMode};
pub use transfer::{
    ConflictAnswer, ConflictQuestion, ItemReport, ItemStatus, Progress, TransferObserver, TransferReport,
    TransferRequest,
};
