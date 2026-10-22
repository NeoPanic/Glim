//! Blocking I/O services. Call from a background executor once the UI is running.

pub mod binary;
pub mod csv;
pub mod diagnostics;
pub mod executables;
pub mod files;
pub mod formats;
mod fragmented_mp4;
pub mod geo;
pub mod git;
pub mod graph;
pub mod inline_images;
pub mod json;
pub mod limits;
pub mod locale;
pub mod markdown_preview;
pub mod markup_images;
mod matroska;
pub mod media;
pub mod paged;
pub mod preferences;
pub mod preview;
mod qoi;
mod report;
pub mod safetensors;
pub mod spreadsheet;
pub mod sqlite;
pub mod streamed;
mod svg_preview;
pub mod telemetry;
pub mod text_formats;
pub mod vector_formats;
pub mod watch;
pub mod word_preview;
pub mod work;
pub mod workspace;
pub mod xmind;
mod xml;

pub mod updates;

pub mod books;

pub mod drawio;

pub mod opml;

pub mod plist;
