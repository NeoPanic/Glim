//! Streaming SVG preparation for system thumbnails and isolated browser images.
use crate::{limits, work::Cancellation};
use anyhow::{Result, ensure};
use quick_xml::{
    Reader, Writer,
    events::{BytesStart, Event},
};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::Path,
};

fn local_css(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.contains(['\\', '@'])
        || lower.contains("expression")
        || lower.contains("image-set")
        || lower.contains("/*")
    {
        return false;
    }
    let mut rest = lower.as_str();
    while let Some(start) = rest.find("url") {
        rest = rest[start + 3..].trim_start();
        let Some(argument) = rest.strip_prefix('(') else {
            return false;
        };
        let Some(end) = argument.find(')') else {
            return false;
        };
        let id = argument[..end].trim().trim_matches(['\'', '"']);
        if !id.starts_with('#')
            || !id[1..]
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.'))
        {
            return false;
        }
        rest = &argument[end + 1..];
    }
    true
}
// Keep data images as vectors/rasters in the isolated browser image document.
// Inspect only the header; decoding and re-encoding would discard nested SVGs
// and needlessly allocate another copy of an embedded image.
fn embedded_image_uri(value: &str) -> bool {
    value.split_once(',').is_some_and(|(header, _)| {
        header
            .get(..11)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:image/"))
    })
}
fn embedded(value: &str, cancel: &Cancellation) -> Result<Option<String>> {
    use base64::Engine;
    let Some((header, bytes)) = value.split_once(",") else {
        return Ok(None);
    };
    if !header.ends_with(";base64") || !header.starts_with("data:image/") {
        return Ok(None);
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(bytes)?;
    if image::guess_format(&bytes).is_err() {
        return Ok(None);
    }
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(&bytes)?;
    drop(bytes);
    let png = crate::inline_images::thumbnail_png_cancellable(
        file.path(),
        limits::INLINE_IMAGE_PREVIEW_EDGE.min(
            ((limits::REMOTE_IMAGE_SVG_EMBEDDED_BYTES / 8) as f64)
                .sqrt()
                .max(1.) as u32,
        ),
        cancel,
    )?;
    Ok(Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    )))
}
// Percent-sized exports have no intrinsic image dimensions. Resolve their
// viewport from the viewBox instead of accepting the browser's 300×150 default.
fn intrinsic_size(
    start: &BytesStart<'_>,
    decoder: quick_xml::encoding::Decoder,
) -> Result<Option<(f64, f64)>> {
    let (mut width, mut height, mut view_box) = (None, None, None);
    for attribute in start.attributes() {
        let attribute = attribute?;
        let value = attribute.decode_and_unescape_value(decoder)?;
        match attribute.key.as_ref() {
            b"width" => width = absolute_length(&value),
            b"height" => height = absolute_length(&value),
            b"viewBox" => {
                let mut parts = value
                    .split(|ch: char| ch.is_ascii_whitespace() || ch == ',')
                    .filter(|part| !part.is_empty());
                let mut values = [0.; 4];
                let valid = values.iter_mut().all(|value| {
                    if let Some(parsed) = parts.next().and_then(|part| part.parse::<f64>().ok()) {
                        *value = parsed;
                        parsed.is_finite()
                    } else {
                        false
                    }
                });
                if valid && parts.next().is_none() && values[2] > 0. && values[3] > 0. {
                    view_box = Some((values[2], values[3]));
                }
            }
            _ => {}
        }
    }
    if width.is_some() && height.is_some() {
        return Ok(None);
    }
    let Some((view_width, view_height)) = view_box else {
        return Ok(None);
    };
    let size = match (width, height) {
        (Some(width), None) => (width, width * view_height / view_width),
        (None, Some(height)) => (height * view_width / view_height, height),
        _ => (view_width, view_height),
    };
    Ok((size.0.is_finite() && size.1.is_finite() && size.0 > 0. && size.1 > 0.).then_some(size))
}
fn absolute_length(value: &str) -> Option<f64> {
    let value = value.trim();
    // CSS absolute-unit conversions, not application reader thresholds.
    let (number, factor) = [
        ("px", 1.),
        ("pt", 96. / 72.),
        ("pc", 16.),
        ("in", 96.),
        ("cm", 96. / 2.54),
        ("mm", 96. / 25.4),
        ("q", 96. / 101.6),
    ]
    .into_iter()
    .find_map(|(suffix, factor)| value.strip_suffix(suffix).map(|value| (value, factor)))
    .unwrap_or((value, 1.));
    let pixels = number.trim().parse::<f64>().ok()? * factor;
    (pixels.is_finite() && pixels > 0.).then_some(pixels)
}
pub(crate) fn prepare(path: &Path, cancel: &Cancellation) -> Result<tempfile::NamedTempFile> {
    prepare_image(path, cancel, false)
}
/// Only for SVG loaded as a browser `<img>`, whose image-document sandbox
/// disables scripts, navigation and external resource loads. Preserve XHTML
/// labels and complete CSS here; thumbnail providers use the stricter path.
pub(crate) fn prepare_browser_image(
    path: &Path,
    cancel: &Cancellation,
) -> Result<tempfile::NamedTempFile> {
    prepare_image(path, cancel, true)
}
fn prepare_image(
    path: &Path,
    cancel: &Cancellation,
    browser_image: bool,
) -> Result<tempfile::NamedTempFile> {
    let input = crate::work::RecordReader::new(File::open(path)?, limits::SVG_EVENT_BYTES, cancel);
    let mut reader = Reader::from_reader(BufReader::with_capacity(limits::IO_BUFFER_BYTES, input));
    let mut output = tempfile::Builder::new().suffix(".svg").tempfile()?;
    let mut writer = Writer::new(BufWriter::new(output.as_file_mut()));
    let mut buffer = Vec::new();
    let mut skipped = 0usize;
    let mut root = false;
    let mut style = false;
    loop {
        cancel.check()?;
        let event = reader.read_event_into(&mut buffer)?;
        reader.get_mut().get_mut().reset(limits::SVG_EVENT_BYTES);
        match event {
            event @ (Event::Start(_) | Event::Empty(_)) => {
                let empty = matches!(event, Event::Empty(_));
                let start = match event {
                    Event::Start(start) | Event::Empty(start) => start,
                    _ => unreachable!(),
                };
                let name = String::from_utf8(start.name().as_ref().to_vec())?;
                let local = name
                    .rsplit(':')
                    .next()
                    .unwrap_or(&name)
                    .to_ascii_lowercase();
                let is_root = !root;
                if is_root {
                    ensure!(local == "svg", "Not an SVG image");
                    root = true;
                }
                if skipped > 0
                    || (!browser_image && local == "foreignobject")
                    || matches!(
                        local.as_str(),
                        "script"
                            | "iframe"
                            | "object"
                            | "embed"
                            | "animate"
                            | "animatemotion"
                            | "animatetransform"
                            | "set"
                            | "discard"
                    )
                {
                    if !empty {
                        skipped += 1;
                    }
                } else {
                    let intrinsic = if browser_image && is_root {
                        intrinsic_size(&start, reader.decoder())?
                    } else {
                        None
                    };
                    let mut cleaned = BytesStart::new(name.as_str());
                    for attribute in start.attributes() {
                        let attribute = attribute?;
                        let key = std::str::from_utf8(attribute.key.as_ref())?;
                        let local_key = key.rsplit(':').next().unwrap_or(key).to_ascii_lowercase();
                        let value = attribute
                            .decode_and_unescape_value(reader.decoder())?
                            .into_owned();
                        if intrinsic.is_some() && matches!(local_key.as_str(), "width" | "height") {
                            continue;
                        }
                        if local_key.starts_with("on")
                            || matches!(local_key.as_str(), "src" | "base")
                        {
                            continue;
                        }
                        if local_key == "href" {
                            if value.starts_with('#')
                                || (browser_image && embedded_image_uri(&value))
                            {
                                cleaned.push_attribute((key, value.as_str()));
                            } else if let Some(value) = embedded(&value, cancel).ok().flatten() {
                                cleaned.push_attribute((key, value.as_str()));
                            }
                        } else if local_key == "style" || value.to_ascii_lowercase().contains("url")
                        {
                            if browser_image || local_css(&value) {
                                cleaned.push_attribute((key, value.as_str()));
                            }
                        } else {
                            cleaned.push_attribute((key, value.as_str()));
                        }
                    }
                    if let Some((width, height)) = intrinsic {
                        cleaned.push_attribute(("width", width.to_string().as_str()));
                        cleaned.push_attribute(("height", height.to_string().as_str()));
                    }
                    style = local == "style" && !empty;
                    writer.write_event(if empty {
                        Event::Empty(cleaned)
                    } else {
                        Event::Start(cleaned)
                    })?;
                }
            }
            Event::End(end) => {
                if skipped > 0 {
                    skipped -= 1;
                } else {
                    style = false;
                    writer.write_event(Event::End(end))?;
                }
            }
            Event::Text(text) if skipped == 0 => {
                if browser_image || !style || local_css(&text.decode()?) {
                    writer.write_event(Event::Text(text))?;
                }
            }
            Event::CData(text) if skipped == 0 => {
                if browser_image || !style || local_css(&text.decode()?) {
                    writer.write_event(Event::CData(text))?;
                }
            }
            Event::GeneralRef(reference) if skipped == 0 && (browser_image || !style) => {
                writer.write_event(Event::GeneralRef(reference))?;
            }
            Event::DocType(declaration) => {
                // Exporters such as Matplotlib include the standard SVG DTD.
                // Drop the declaration so downstream readers never fetch it;
                // internal subsets could define entities and remain unsupported.
                ensure!(!root, "SVG document type must precede the root element");
                ensure!(
                    !declaration.contains(&b'['),
                    "SVG internal document type subsets are unsupported"
                );
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    ensure!(root && skipped == 0, "Incomplete SVG document");
    writer.into_inner().flush()?;
    Ok(output)
}
