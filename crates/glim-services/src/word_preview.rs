//! Read-only, reflowable Word preparation. System conversion runs in a cancellable
//! subprocess; generated HTML is decorated as a stream, not loaded into one string.
use crate::{limits, work::Cancellation};
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, TryLockError},
};

mod images;
mod scans;

static CONVERSION: Mutex<()> = Mutex::new(());
pub struct ReadingDocument {
    pub html: PathBuf,
    pub image_pages: bool,
    _directory: tempfile::TempDir,
}
pub fn supports(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "doc" | "docx" | "rtf"))
}
pub fn prepare(path: &Path, cancel: &Cancellation) -> Result<ReadingDocument> {
    let _conversion = loop {
        cancel.check()?;
        match CONVERSION.try_lock() {
            Ok(guard) => break guard,
            Err(TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            Err(TryLockError::Poisoned(_)) => anyhow::bail!("Word conversion was interrupted"),
        }
    };
    let directory = tempfile::tempdir()?;
    let converted = directory.path().join("converted.html");
    let html = directory.path().join("reading.html");
    match scans::prepare(path, directory.path(), &html, cancel) {
        Ok(true) => {
            return Ok(ReadingDocument {
                html: std::fs::canonicalize(html)?,
                image_pages: true,
                _directory: directory,
            });
        }
        Ok(false) => {}
        Err(_) => {
            cancel.check()?;
        }
    }
    let (conversion_source, images) = images::prepare(path, directory.path(), cancel)?;
    let mut command = Command::new("/usr/bin/textutil");
    command
        .args(["-convert", "html", "-output"])
        .arg(&converted)
        .arg(&conversion_source)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    ensure!(
        crate::work::run(&mut command, cancel)?.success(),
        "Cannot convert this document to reading view"
    );
    decorate_images(&converted, &html, cancel, &images)?;
    Ok(ReadingDocument {
        image_pages: false,
        html: std::fs::canonicalize(html)?,
        _directory: directory,
    })
}
fn relative_fonts(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    for declaration in line.split_inclusive(';') {
        let property = declaration
            .rsplit_once('{')
            .map_or(declaration, |(_, rest)| rest)
            .trim_start();
        if !property.starts_with("font:") && !property.starts_with("font-size:") {
            output.push_str(declaration);
            continue;
        }
        let bytes = declaration.as_bytes();
        let mut cursor = 0;
        let mut index = 0;
        let mut quote = None;
        while index < bytes.len() {
            let byte = bytes[index];
            if let Some(end) = quote {
                if byte == b'\\' {
                    index = (index + 2).min(bytes.len());
                    continue;
                }
                if byte == end {
                    quote = None;
                }
            } else if matches!(byte, b'\'' | b'"') {
                quote = Some(byte);
            } else if (byte.is_ascii_digit() || byte == b'.')
                && (index == 0 || matches!(bytes[index - 1], b':' | b' ' | b'\t' | b'/'))
            {
                let start = index;
                while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.')
                {
                    index += 1;
                }
                if bytes.get(index..index + 2) == Some(b"px")
                    && let Ok(size) = declaration[start..index].parse::<f64>()
                    && size.is_finite()
                {
                    output.push_str(&declaration[cursor..start]);
                    // textutil emits point-based fonts as px. Reading typography
                    // rebases the conventional 12-point body onto the root font.
                    output.push_str(&format!("{}rem", size / 12.0));
                    index += 2;
                    cursor = index;
                    continue;
                }
                continue;
            }
            index += 1;
        }
        output.push_str(&declaration[cursor..]);
    }
    output
}
fn decorate(source: &Path, destination: &Path, cancel: &Cancellation) -> Result<()> {
    decorate_images(source, destination, cancel, &[])
}
fn decorate_images(
    source: &Path,
    destination: &Path,
    cancel: &Cancellation,
    images: &[(String, String)],
) -> Result<()> {
    let input =
        crate::work::RecordReader::new(File::open(source)?, limits::IO_RECORD_BYTES, cancel);
    let mut input = BufReader::with_capacity(limits::IO_BUFFER_BYTES, input);
    let mut output = BufWriter::with_capacity(limits::IO_BUFFER_BYTES, File::create(destination)?);
    let mut buffer = Vec::new();
    let mut style = false;
    let mut decorated = false;
    loop {
        cancel.check()?;
        buffer.clear();
        input.get_mut().reset(limits::IO_RECORD_BYTES);
        if input.read_until(b'\n', &mut buffer)? == 0 {
            break;
        }
        let line =
            std::str::from_utf8(&buffer).context("Word conversion did not produce UTF-8 HTML")?;
        if line.contains("<style") {
            style = true;
        }
        if line.contains("</head>") {
            output.write_all(br#"<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline'; font-src 'self' data:;">
<style>
:root { color-scheme: light dark; font-size:18px; }
html { margin:0; padding:0; }
body { box-sizing:border-box; margin:0!important; padding:24px 32px 104px!important; width:auto!important; max-width:none!important; min-height:100vh; color:CanvasText; background:Canvas; overflow-wrap:anywhere; }
body p, body li { line-height:1.45!important; }
img, svg { max-width:100%; height:auto; }
table { max-width:100%; border-collapse:collapse; }
td, th { overflow-wrap:normal; }
/* Imported Word runs often explicitly specify black/gray text. Override those
   only in dark reading mode, including text on explicitly shaded cells. */
@media(prefers-color-scheme:dark) {
  body, body p, body li, body span, body td, body th, body div, body font {
    color:#e6e6e6!important; background-color:transparent!important;
  }
  html, body { background:#202020!important; }
  body a, body a span { color:#8ab4ff!important; }
  td, th { border-color:#777!important; }
}
@media(max-width:640px) { body { padding:16px 20px 88px!important; } }
</style>
"#)?;
            decorated = true;
        }
        let mut rendered = if style {
            relative_fonts(line)
        } else {
            line.to_owned()
        };
        for (marker, image) in images {
            rendered = rendered.replace(marker, image);
        }
        output.write_all(rendered.as_bytes())?;
        if line.contains("</style>") {
            style = false;
        }
    }
    ensure!(decorated, "Word conversion has no HTML document header");
    output.flush()?;
    Ok(())
}
