//! Preserve inline DOCX images that textutil drops, using temporary text anchors.
use super::*;

pub(super) fn prepare(
    path: &Path,
    directory: &Path,
    cancel: &Cancellation,
) -> Result<(PathBuf, Vec<(String, String)>)> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("docx"))
    {
        return Ok((path.to_owned(), Vec::new()));
    }
    let mut archive = zip::ZipArchive::new(File::open(path)?)?;
    let content = scans::xml(&mut archive, "word/document.xml", cancel)?;
    let document = roxmltree::Document::parse(&content)?;
    let relationships = scans::xml(&mut archive, "word/_rels/document.xml.rels", cancel)?;
    let relationships = roxmltree::Document::parse(&relationships)?;
    const WORD: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    const DRAWING: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    const VML: &str = "urn:schemas-microsoft-com:vml";
    let mut replacements = Vec::new();
    let mut images = Vec::new();
    let nonce: String = directory
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    for node in document
        .descendants()
        .filter(|n| n.has_tag_name((WORD, "drawing")) || n.has_tag_name((WORD, "pict")))
    {
        cancel.check()?;
        let id = node.descendants().find_map(|n| {
            if n.has_tag_name((DRAWING, "blip")) {
                n.attribute((REL, "embed"))
            } else if n.has_tag_name((VML, "imagedata")) {
                n.attribute((REL, "id"))
            } else {
                None
            }
        });
        let Some(id) = id else { continue };
        let relation = relationships
            .descendants()
            .find(|n| n.attribute("Id") == Some(id))
            .context("Missing Word image relationship")?;
        if relation.attribute("TargetMode") == Some("External") {
            continue;
        }
        let target = relation
            .attribute("Target")
            .context("Missing Word image target")?;
        ensure!(
            target.starts_with("media/")
                && !target.contains('\\')
                && !target.split('/').any(|p| p == ".."),
            "Unsupported Word image target"
        );
        let index = images.len();
        let original = directory.join(format!("inline-source-{index}"));
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
        let filename = format!("inline-{index}.png");
        std::fs::write(directory.join(&filename), png)?;
        let marker = format!("GLIMIMAGE{nonce}N{index}END");
        replacements.push((
            node.range(),
            format!("<w:t xmlns:w=\"{WORD}\">{marker}</w:t>"),
        ));
        images.push((marker, format!("<img src=\"{filename}\" loading=\"lazy\" alt=\"\" style=\"max-width:100%;height:auto;background:white\">")));
    }
    if images.is_empty() {
        return Ok((path.to_owned(), images));
    }
    let mut patched = content;
    for (range, replacement) in replacements.into_iter().rev() {
        patched.replace_range(range, &replacement);
    }
    let prepared = directory.join("images.docx");
    let mut output = zip::ZipWriter::new(File::create(&prepared)?);
    for index in 0..archive.len() {
        cancel.check()?;
        let mut entry = archive.by_index(index)?;
        if entry.name() == "word/document.xml" {
            output.start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )?;
            output.write_all(patched.as_bytes())?;
        } else {
            let options =
                zip::write::SimpleFileOptions::default().compression_method(entry.compression());
            output.start_file(entry.name(), options)?;
            cancel.copy(&mut entry, &mut output)?;
        }
    }
    output.finish()?;
    Ok((prepared, images))
}
