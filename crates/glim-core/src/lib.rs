//! UI-independent values shared by the application and its services.

mod document;
pub mod update;
mod workspace;

pub use document::{Document, DocumentKind, language_for_path};
pub use workspace::DirectoryEntry;
