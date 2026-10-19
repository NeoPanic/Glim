//! Disk-backed container reports, with bounded page display and cancellable streaming.
use crate::{
    limits as reader_limits,
    paged::{self, TextPage},
    work::Cancellation,
};
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
};
pub fn supports(path: &Path) -> bool {
    matches!(
        extension(path).as_str(),
        "zip" | "tar" | "gz" | "bz2" | "xz" | "epub" | "odt" | "ods" | "odp" | "npy"
    )
}
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub fn read_preview(path: &Path) -> Result<TextPage> {
    read_preview_cancellable(path, &Cancellation::default())
}
pub fn read_preview_cancellable(path: &Path, cancel: &Cancellation) -> Result<TextPage> {
    let ext = extension(path);
    if let Some(compression) = match ext.as_str() {
        "gz" => Some(crate::streamed::Compression::Gzip),
        "bz2" => Some(crate::streamed::Compression::Bzip2),
        "xz" => Some(crate::streamed::Compression::Xz),
        _ => None,
    } {
        return paged::compressed(path, compression, cancel);
    }
    let mut output = tempfile::NamedTempFile::new()?;
    {
        let mut text = BufWriter::new(output.as_file_mut());
        writeln!(
            text,
            "{} · {}\n",
            ext.to_uppercase(),
            crate::locale::tr("Read-only preview")
        )?;
        match ext.as_str() {
            "zip" | "epub" | "odt" | "ods" | "odp" => {
                let mut archive = zip::ZipArchive::new(File::open(path)?)
                    .context("Invalid or truncated ZIP container")?;
                for index in 0..archive.len() {
                    cancel.check()?;
                    let mut entry = archive.by_index(index)?;
                    writeln!(
                        text,
                        "{} · {} bytes{}",
                        entry.name(),
                        entry.size(),
                        if entry.enclosed_name().is_some() {
                            String::new()
                        } else {
                            crate::locale::tr(" · Unsafe extraction path")
                        }
                    )?;
                    if (matches!(ext.as_str(), "odt" | "ods" | "odp")
                        && entry.name() == "content.xml")
                        || (ext == "epub" && entry.name().ends_with(".xhtml"))
                    {
                        // Preserve large XML completely in source form rather than rejecting it.
                        let mut prefix = Vec::new();
                        (&mut entry)
                            .take(reader_limits::ARCHIVE_BYTES + 1)
                            .read_to_end(&mut prefix)?;
                        if prefix.len() as u64 <= reader_limits::ARCHIVE_BYTES {
                            text.write_all(xml_text(&prefix)?.as_bytes())?;
                        } else {
                            text.write_all(&prefix)?;
                            cancel.copy(entry, &mut text)?;
                        }
                        writeln!(text)?;
                    }
                }
            }
            "tar" => {
                let mut archive = tar::Archive::new(BufReader::new(File::open(path)?));
                for entry in archive.entries()? {
                    cancel.check()?;
                    let entry = entry?;
                    let name = entry.path()?;
                    let safe = !name.is_absolute()
                        && !name
                            .components()
                            .any(|c| matches!(c, std::path::Component::ParentDir));
                    writeln!(
                        text,
                        "{} · {} bytes{}",
                        name.display(),
                        entry.size(),
                        if safe {
                            String::new()
                        } else {
                            crate::locale::tr(" · Unsafe extraction path")
                        }
                    )?;
                }
            }
            "npy" => numpy_stream(path, &mut text, cancel)?,
            _ => anyhow::bail!("Unsupported container"),
        }
        text.flush()?;
    }
    paged::generated(path, output)
}
fn xml_text(bytes: &[u8]) -> Result<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut text = String::new();
    loop {
        match reader.read_event()? {
            Event::Text(value) => {
                text.push_str(&quick_xml::escape::unescape(&value.decode()?)?);
            }
            Event::End(_) => text.push('\n'),
            Event::DocType(_) => anyhow::bail!("Document type declarations are unsupported"),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(text)
}

fn numpy_stream(path: &Path, output: &mut impl Write, cancel: &Cancellation) -> Result<()> {
    let mut input = BufReader::new(File::open(path)?);
    let total = input.get_ref().metadata()?.len();
    let mut signature = [0; 8];
    input.read_exact(&mut signature)?;
    ensure!(
        signature.starts_with(b"\x93NUMPY"),
        "Invalid NumPy signature"
    );
    let version = signature[6];
    let length = match version {
        1 => {
            let mut n = [0; 2];
            input.read_exact(&mut n)?;
            u64::from(u16::from_le_bytes(n))
        }
        2 | 3 => {
            let mut n = [0; 4];
            input.read_exact(&mut n)?;
            u64::from(u32::from_le_bytes(n))
        }
        _ => anyhow::bail!("Unsupported NumPy version"),
    };
    let offset = input.stream_position()?;
    let data_offset = offset
        .checked_add(length)
        .filter(|end| *end <= total)
        .context("Truncated NumPy header")?;
    let mut metadata = NumpyMetadata::default();
    let mut remaining = length;
    let mut buffer = vec![0; reader_limits::IO_BUFFER_BYTES];
    writeln!(output, "NumPy v{version}")?;
    while remaining > 0 {
        cancel.check()?;
        let count = usize::try_from(remaining.min(buffer.len() as u64))?;
        input.read_exact(&mut buffer[..count])?;
        for byte in &buffer[..count] {
            metadata.push(*byte);
        }
        if version == 3 || buffer[..count].is_ascii() {
            output.write_all(&buffer[..count])?;
        } else {
            let text = buffer[..count]
                .iter()
                .map(|byte| char::from(*byte))
                .collect::<String>();
            output.write_all(text.as_bytes())?;
        }
        remaining -= count as u64;
    }
    writeln!(output)?;
    let descriptor = metadata.descriptor.as_deref();
    let size = match descriptor {
        Some("<f8" | "<i8") => 8,
        Some("<f4" | "<i4") => 4,
        Some("|u1") => 1,
        _ => 0,
    };
    if size == 0 {
        writeln!(
            output,
            "{} · {} bytes",
            crate::locale::tr("Numeric preview unavailable for this data type"),
            total - data_offset
        )?;
        return Ok(());
    }
    let Some(count) = metadata.elements.filter(|_| metadata.shape) else {
        writeln!(
            output,
            "{}",
            crate::locale::tr("Numeric preview unavailable for this data type")
        )?;
        return Ok(());
    };
    ensure!(
        count.checked_mul(size as u64) == Some(total - data_offset),
        "NumPy data length does not match shape"
    );
    input.seek(SeekFrom::Start(data_offset))?;
    let mut buffer = [0; 8];
    for index in 0..count {
        cancel.check()?;
        input.read_exact(&mut buffer[..size])?;
        let value = match descriptor {
            Some("<f8") => f64::from_le_bytes(buffer).to_string(),
            Some("<i8") => i64::from_le_bytes(buffer).to_string(),
            Some("<f4") => f32::from_le_bytes(buffer[..4].try_into()?).to_string(),
            Some("<i4") => i32::from_le_bytes(buffer[..4].try_into()?).to_string(),
            _ => buffer[0].to_string(),
        };
        writeln!(output, "[{index}] {value}")?;
    }
    Ok(())
}

#[derive(Default)]
struct NumpyMetadata {
    depth: usize,
    quote: u8,
    escaped: bool,
    token: String,
    key: String,
    reading_key: bool,
    descriptor: Option<String>,
    shape: bool,
    in_shape: bool,
    dimension: Option<u64>,
    digit: bool,
    elements: Option<u64>,
}
impl NumpyMetadata {
    fn dimension(&mut self) {
        if self.digit {
            self.elements = if self.dimension == Some(0) {
                Some(0)
            } else {
                self.elements
                    .and_then(|count| count.checked_mul(self.dimension?))
            };
        }
        self.dimension = Some(0);
        self.digit = false;
    }
    fn push(&mut self, byte: u8) {
        if self.quote != 0 {
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == self.quote {
                self.quote = 0;
                if self.depth == 1 {
                    if self.reading_key {
                        self.key.clone_from(&self.token);
                    } else if self.key == "descr" {
                        self.descriptor = Some(self.token.clone());
                    }
                }
                return;
            }
            if self.token.len() < reader_limits::NUMPY_HEADER_BYTES {
                self.token.push(char::from(byte));
            }
            return;
        }
        match byte {
            b'\'' | b'"' => {
                self.quote = byte;
                self.token.clear();
            }
            b'{' => {
                self.depth += 1;
                if self.depth == 1 {
                    self.reading_key = true;
                }
            }
            b':' if self.depth == 1 => self.reading_key = false,
            b',' if self.depth == 1 => {
                self.reading_key = true;
                self.key.clear();
            }
            b'(' | b'[' => {
                if self.depth == 1 && self.key == "shape" {
                    self.in_shape = true;
                    self.shape = true;
                    self.elements = Some(1);
                    self.dimension = Some(0);
                }
                self.depth += 1;
            }
            b')' | b']' | b'}' => {
                if self.in_shape && self.depth == 2 {
                    self.dimension();
                    self.in_shape = false;
                }
                self.depth = self.depth.saturating_sub(1);
            }
            b',' if self.in_shape => self.dimension(),
            b'0'..=b'9' if self.in_shape => {
                self.digit = true;
                self.dimension = self
                    .dimension
                    .and_then(|value| value.checked_mul(10)?.checked_add(u64::from(byte - b'0')));
            }
            _ => {}
        }
    }
}
