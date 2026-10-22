//! Image-only DOCX pages, in drawing order rather than ZIP member order.
use super::*;
use std::io::Read;

fn xml(archive: &mut zip::ZipArchive<File>, name: &str, cancel: &Cancellation) -> Result<String> {
    let entry = archive.by_name(name)?;
    ensure!(
        entry.size() <= limits::IO_RECORD_BYTES as u64,
        "Word image metadata exceeds working budget"
    );
    let mut text = String::new();
    crate::work::Reader {
        inner: entry,
        cancel: cancel.clone(),
    }
    .take(limits::IO_RECORD_BYTES as u64 + 1)
    .read_to_string(&mut text)?;
    ensure!(
        text.len() <= limits::IO_RECORD_BYTES,
        "Word image metadata exceeds working budget"
    );
    Ok(text)
}
pub(super) fn prepare(
    path: &Path,
    directory: &Path,
    destination: &Path,
    cancel: &Cancellation,
) -> Result<bool> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("docx"))
    {
        return Ok(false);
    }
    let mut archive = zip::ZipArchive::new(File::open(path)?)?;
    let content = xml(&mut archive, "word/document.xml", cancel)?;
    let document = roxmltree::Document::parse(&content)?;
    const WORD: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    const DRAWING: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    const VML: &str = "urn:schemas-microsoft-com:vml";
    if document
        .descendants()
        .any(|n| n.has_tag_name((WORD, "t")) && n.text().is_some_and(|s| !s.trim().is_empty()))
    {
        return Ok(false);
    }
    let images: Vec<_> = document
        .descendants()
        .filter_map(|n| {
            if n.has_tag_name((DRAWING, "blip")) {
                n.attribute((REL, "embed"))
            } else if n.has_tag_name((VML, "imagedata")) {
                n.attribute((REL, "id"))
            } else {
                None
            }
        })
        .collect();
    if images.is_empty() {
        return Ok(false);
    }
    let relationships = xml(&mut archive, "word/_rels/document.xml.rels", cancel)?;
    let relationships = roxmltree::Document::parse(&relationships)?;
    let converted = directory.join("scan-pages.html");
    let mut output = BufWriter::new(File::create(&converted)?);
    writeln!(
        output,
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">"
    )?;
    writeln!(output, "</head><body>")?;
    for (index, id) in images.iter().enumerate() {
        cancel.check()?;
        let relation = relationships
            .descendants()
            .find(|n| n.attribute("Id") == Some(*id))
            .context("Missing Word image relationship")?;
        ensure!(
            relation.attribute("TargetMode") != Some("External"),
            "External Word image is unsupported"
        );
        let target = relation
            .attribute("Target")
            .context("Missing Word image target")?;
        ensure!(
            !target.contains('\\')
                && !target.split('/').any(|s| s == "..")
                && target.starts_with("media/"),
            "Unsupported Word image target"
        );
        let original = directory.join(format!("scan-source-{index}"));
        cancel.copy(
            archive.by_name(&format!("word/{target}"))?,
            File::create(&original)?,
        )?;
        let png = crate::inline_images::thumbnail_png_cancellable(
            &original,
            limits::INLINE_IMAGE_PREVIEW_EDGE,
            cancel,
        )?;
        std::fs::remove_file(original)?;
        std::fs::write(directory.join(format!("scan-{index}.png")), png)?;
        writeln!(
            output,
            "<p style=\"margin:0 0 24px;text-align:center\"><img src=\"scan-{index}.png\" loading=\"lazy\" alt=\"{}\" style=\"display:block;width:100%;max-width:100%;height:auto;background:white\"></p>",
            index + 1
        )?;
    }
    writeln!(output, "</body></html>")?;
    output.flush()?;
    super::decorate(&converted, destination, cancel)?;
    Ok(true)
}
