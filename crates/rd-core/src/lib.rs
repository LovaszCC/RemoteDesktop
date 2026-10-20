//! Pure domain logic shared by every part of the app. No I/O apart from importers reading byte slices.

pub mod draft;
pub mod forward;
pub mod import;
pub mod library;
pub mod model;
pub mod natural;
pub mod rdp;
pub mod shortcuts;
pub mod split;
pub mod transfer;
pub mod tree;

pub use library::Library;
pub use model::*;
pub use natural::natural_cmp;
