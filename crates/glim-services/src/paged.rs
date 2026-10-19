//! Bounded UTF-8 windows; offsets are byte offsets, never character indices.
use crate::limits as reader_limits;
use anyhow::{Result, ensure};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};
pub const PAGE_BYTES: u64 = reader_limits::TEXT_PAGE_BYTES;
pub const FULL_BYTES: u64 = reader_limits::TEXT_FULL_BYTES;
#[derive(Clone)]
pub struct PageSource {
    pub original: PathBuf,
    pub binary: bool,
    backing: Option<Arc<tempfile::NamedTempFile>>,
    stream: Option<Arc<std::sync::Mutex<crate::streamed::StreamedText>>>,
}
impl PageSource {
    pub fn is_editable(&self) -> bool {
        !self.binary && self.backing.is_none()
    }
    pub fn is_generated(&self) -> bool {
        self.backing.is_some()
    }
    pub fn raw(&self) -> Self {
        Self {
            original: self.original.clone(),
            binary: true,
            backing: None,
            stream: None,
        }
    }
    fn path(&self) -> &Path {
        self.backing
            .as_ref()
            .map_or(self.original.as_path(), |file| file.path())
    }
    pub fn read(&self, start: u64, expanded: bool) -> Result<TextPage> {
        self.read_cancellable(start, expanded, &crate::work::Cancellation::default())
    }
    pub fn read_cancellable(
        &self,
        start: u64,
        expanded: bool,
        cancel: &crate::work::Cancellation,
    ) -> Result<TextPage> {
        read_source_cancellable(self.clone(), start, expanded, cancel)
    }
}

pub struct TextPage {
    pub source: PageSource,
    pub owner: Option<crate::work::CancelOnDrop>,
    pub more: bool,
    pub total_known: bool,
    pub path: PathBuf,
    pub text: String,
    pub start: u64,
    pub end: u64,
    pub total: u64,
    pub modified: Option<SystemTime>,
    pub identity: (u64, u64),
}
pub fn read_page(path: &Path, start: u64, all: bool) -> Result<TextPage> {
    read_source(
        PageSource {
            original: std::path::absolute(path)?,
            binary: false,
            backing: None,
            stream: None,
        },
        start,
        all,
    )
}
pub fn read_binary_page(path: &Path, start: u64) -> Result<TextPage> {
    read_source(
        PageSource {
            original: std::path::absolute(path)?,
            binary: true,
            backing: None,
            stream: None,
        },
        start,
        false,
    )
}
pub fn generated(path: &Path, backing: tempfile::NamedTempFile) -> Result<TextPage> {
    generated_view(path, backing, false)
}
pub fn generated_binary(path: &Path, backing: tempfile::NamedTempFile) -> Result<TextPage> {
    generated_view(path, backing, true)
}
pub fn text_value(text: &str, cancel: &crate::work::Cancellation) -> Result<TextPage> {
    let mut file = tempfile::NamedTempFile::new()?;
    cancel.copy(std::io::Cursor::new(text.as_bytes()), file.as_file_mut())?;
    generated_view(Path::new("Value"), file, text.contains('\0'))
}
fn generated_view(path: &Path, backing: tempfile::NamedTempFile, binary: bool) -> Result<TextPage> {
    read_source(
        PageSource {
            original: std::path::absolute(path)?,
            binary,
            backing: Some(Arc::new(backing)),
            stream: None,
        },
        0,
        false,
    )
}
fn read_source(source: PageSource, start: u64, all: bool) -> Result<TextPage> {
    read_source_cancellable(source, start, all, &crate::work::Cancellation::default())
}
pub fn compressed(
    path: &Path,
    compression: crate::streamed::Compression,
    cancel: &crate::work::Cancellation,
) -> Result<TextPage> {
    compressed_view(path, compression, false, cancel)
}
fn compressed_view(
    path: &Path,
    compression: crate::streamed::Compression,
    binary: bool,
    cancel: &crate::work::Cancellation,
) -> Result<TextPage> {
    let (lifetime, owner) = crate::work::Cancellation::task();
    let stream =
        crate::streamed::StreamedText::new(std::path::absolute(path)?, compression, lifetime)?;
    let source = PageSource {
        original: std::path::absolute(path)?,
        binary,
        backing: Some(stream.file.clone()),
        stream: Some(Arc::new(std::sync::Mutex::new(stream))),
    };
    let mut page = read_source_cancellable(source, 0, false, cancel)?;
    page.owner = Some(owner);
    Ok(page)
}
fn read_source_cancellable(
    source: PageSource,
    start: u64,
    all: bool,
    cancel: &crate::work::Cancellation,
) -> Result<TextPage> {
    cancel.check()?;
    let mut base = 0;
    let mut stream_total = None;
    if let Some(stream) = &source.stream {
        let mut stream = stream
            .lock()
            .map_err(|_| anyhow::anyhow!("Reader interrupted"))?;
        let bytes = if source.binary {
            reader_limits::BINARY_PREVIEW_BYTES as u64
        } else if all {
            FULL_BYTES.min(reader_limits::IO_CACHE_BYTES)
        } else {
            PAGE_BYTES
        };
        stream.ensure_range(start, bytes + 3, cancel)?;
        base = stream.start;
        stream_total = Some((stream.end, stream.complete));
    }
    let mut file = File::open(source.path())?;
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "Not a regular file");
    let total = stream_total.map_or(metadata.len(), |value| value.0);
    ensure!(start <= total, "File changed; reopen it to restart reading");
    file.seek(SeekFrom::Start(start - base))?;
    let limit = if source.binary {
        reader_limits::BINARY_PREVIEW_BYTES as u64
    } else if source.stream.is_some() {
        if all {
            FULL_BYTES.min(reader_limits::IO_CACHE_BYTES)
        } else {
            PAGE_BYTES
        }
    } else if all {
        if source.is_editable() {
            FULL_BYTES.min(reader_limits::TEXT_EDITABLE_BYTES)
        } else {
            FULL_BYTES
        }
    } else {
        PAGE_BYTES
    };
    let mut bytes = Vec::new();
    (&mut file).take(limit).read_to_end(&mut bytes)?;
    let (text, end) = if source.binary {
        use std::fmt::Write;
        let mut text = String::new();
        for (index, row) in bytes.chunks(16).enumerate() {
            write!(text, "{:016x}  ", start + index as u64 * 16)?;
            for column in 0..16 {
                if let Some(byte) = row.get(column) {
                    write!(text, "{byte:02x} ")?;
                } else {
                    text.push_str("   ");
                }
            }
            text.push(' ');
            text.extend(row.iter().map(|byte| {
                if byte.is_ascii_graphic() || *byte == b' ' {
                    char::from(*byte)
                } else {
                    '.'
                }
            }));
            text.push('\n');
        }
        (text, start + bytes.len() as u64)
    } else {
        // Complete UTF-8 at a byte-page boundary; never silently drop a character.
        for _ in 0..3 {
            match std::str::from_utf8(&bytes) {
                Ok(_) => break,
                Err(e) if e.error_len().is_none() => {
                    let mut byte = [0];
                    if file.read(&mut byte)? == 0 {
                        break;
                    }
                    bytes.push(byte[0]);
                }
                Err(e) => return Err(e.into()),
            }
        }
        ensure!(
            !bytes.contains(&0),
            "Binary content cannot be displayed as UTF-8 text"
        );
        if all
            && let Some((offset, _)) = bytes
                .iter()
                .enumerate()
                .filter(|(_, b)| **b == b'\n')
                .nth(reader_limits::TEXT_FULL_NEWLINES.saturating_sub(1))
        {
            bytes.truncate(offset + 1);
        }
        if source.is_editable() {
            let text = std::str::from_utf8(&bytes)?;
            let mut line_start = 0;
            let mut newlines = 0;
            let mut boundary = bytes.len();
            for (offset, ch) in text.char_indices() {
                if ch == '\n' {
                    newlines += 1;
                    line_start = offset + 1;
                    if newlines >= reader_limits::TEXT_EDITABLE_NEWLINES {
                        boundary = offset + 1;
                        break;
                    }
                } else if offset - line_start >= reader_limits::TEXT_LINE_BYTES {
                    boundary = offset;
                    break;
                }
            }
            bytes.truncate(boundary);
        }
        let end = start + bytes.len() as u64;
        (String::from_utf8(bytes)?, end)
    };
    let after = file.metadata()?;
    ensure!(
        after.len() == metadata.len() && after.modified().ok() == metadata.modified().ok(),
        "File changed while reading; retry"
    );
    Ok(TextPage {
        path: source.original.clone(),
        owner: None,
        more: end < total || stream_total.is_some_and(|value| !value.1),
        total_known: stream_total.is_none_or(|value| value.1),
        source,
        text,
        start,
        end,
        total,
        modified: metadata.modified().ok(),
        identity: (metadata.dev(), metadata.ino()),
    })
}

impl TextPage {
    pub fn fingerprint(&self) -> String {
        format!("paged:{}:{:?}", self.total, self.modified)
    }
}

#[derive(Clone)]
pub struct PageRefresh {
    pub source: PageSource,
    pub start: u64,
    pub total: u64,
    pub modified: Option<SystemTime>,
    pub identity: (u64, u64),
}
impl PageRefresh {
    pub fn read(&self, cancel: &crate::work::Cancellation) -> Result<Option<TextPage>> {
        cancel.check()?;
        let metadata = std::fs::metadata(&self.source.original)?;
        if (
            metadata.len(),
            metadata.modified().ok(),
            (metadata.dev(), metadata.ino()),
        ) == (self.total, self.modified, self.identity)
        {
            return Ok(None);
        }
        let start = if self.start <= metadata.len() {
            self.start
        } else {
            0
        };
        self.source.read_cancellable(start, false, cancel).map(Some)
    }
}

#[derive(Clone)]
pub struct PageEdit {
    pub path: PathBuf,
    pub start: u64,
    pub end: u64,
    pub total: u64,
    pub modified: Option<SystemTime>,
    pub identity: (u64, u64),
    pub expected: String,
}
pub struct PageSaved {
    pub end: u64,
    pub total: u64,
    pub modified: Option<SystemTime>,
    pub identity: (u64, u64),
}
/// Replace one edited range through a same-directory temporary file. The rest
/// of the document is copied in fixed buffers; no whole-document allocation.
pub fn save_page(page: &PageEdit, text: &str) -> Result<PageSaved> {
    save_page_cancellable(page, text, &crate::work::Cancellation::default())
}
pub fn save_page_cancellable(
    page: &PageEdit,
    text: &str,
    cancel: &crate::work::Cancellation,
) -> Result<PageSaved> {
    use anyhow::Context;
    let _lock = crate::files::save_lock(cancel)?;
    ensure!(!text.contains('\0'), "Text contains a null byte");
    let target = page
        .path
        .canonicalize()
        .context("The file was moved or deleted; your edits have not been saved")?;
    let mut source = File::open(&target)?;
    let before = source.metadata()?;
    ensure!(
        before.is_file() && !before.permissions().readonly(),
        "File is read-only"
    );
    ensure!(
        before.len() == page.total
            && before.modified().ok() == page.modified
            && (before.dev(), before.ino()) == page.identity,
        "File changed on disk. Save canceled; your edits are still open."
    );
    ensure!(
        page.end >= page.start
            && page.end <= page.total
            && page.end - page.start == page.expected.len() as u64,
        "Invalid page snapshot"
    );
    source.seek(SeekFrom::Start(page.start))?;
    let mut buffer = vec![0; reader_limits::IO_BUFFER_BYTES];
    for expected in page
        .expected
        .as_bytes()
        .chunks(reader_limits::IO_BUFFER_BYTES)
    {
        cancel.check()?;
        source.read_exact(&mut buffer[..expected.len()])?;
        ensure!(
            &buffer[..expected.len()] == expected,
            "File changed on disk. Save canceled; your edits are still open."
        );
    }
    let mut output =
        tempfile::NamedTempFile::new_in(target.parent().context("File has no parent folder")?)?;
    source.seek(SeekFrom::Start(0))?;
    cancel.copy((&mut source).take(page.start), output.as_file_mut())?;
    cancel.copy(std::io::Cursor::new(text.as_bytes()), output.as_file_mut())?;
    source.seek(SeekFrom::Start(page.end))?;
    cancel.copy(&mut source, output.as_file_mut())?;
    let after = std::fs::metadata(&target)?;
    ensure!(
        after.len() == before.len()
            && after.modified().ok() == before.modified().ok()
            && after.ino() == before.ino()
            && after.dev() == before.dev(),
        "File changed while saving; your edits are still open."
    );
    output.as_file().set_permissions(before.permissions())?;
    output.as_file().sync_all()?;
    cancel.check()?;
    output.persist(&target).map_err(|error| error.error)?;
    let saved = std::fs::metadata(&target)?;
    Ok(PageSaved {
        end: page.start + text.len() as u64,
        total: saved.len(),
        modified: saved.modified().ok(),
        identity: (saved.dev(), saved.ino()),
    })
}

/// Read a text member without extracting its complete uncompressed contents.
pub fn zip_member(path: &Path, name: &str, cancel: &crate::work::Cancellation) -> Result<TextPage> {
    zip_member_view(path, name, false, cancel)
}
pub fn zip_member_bytes(
    path: &Path,
    name: &str,
    cancel: &crate::work::Cancellation,
) -> Result<TextPage> {
    zip_member_view(path, name, true, cancel)
}
fn zip_member_view(
    path: &Path,
    name: &str,
    binary: bool,
    cancel: &crate::work::Cancellation,
) -> Result<TextPage> {
    cancel.check()?;
    let mut archive = zip::ZipArchive::new(File::open(path)?)?;
    let entry = archive.by_name(name)?;
    let deflate = match entry.compression() {
        zip::CompressionMethod::Stored => false,
        zip::CompressionMethod::Deflated => true,
        _ => anyhow::bail!("Unsupported ZIP compression method"),
    };
    let compression = crate::streamed::Compression::Zip {
        offset: entry.data_start(),
        compressed: entry.compressed_size(),
        size: entry.size(),
        crc: entry.crc32(),
        deflate,
    };
    drop(entry);
    drop(archive);
    compressed_view(path, compression, binary, cancel)
}
