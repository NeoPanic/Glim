//! Offline ebook structure and chapter resources. All reads/decoding run in the background.
use crate::limits as reader_limits;
pub mod preferences;

use anyhow::{Context, Result, ensure};
use roxmltree::{Document, Node};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    ops::Range,
    path::{Path, PathBuf},
};

const TEXT_LIMIT: u64 = reader_limits::BOOK_TEXT_BYTES;
const CHAPTER_LIMIT: u64 = reader_limits::BOOK_CHAPTER_BYTES;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Epub,
    FictionBook,
    Comic,
}
impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Self::Epub => "EPUB",
            Self::FictionBook => "FB2",
            Self::Comic => "CBZ",
        }
    }
}
#[derive(Clone)]
pub struct Chapter {
    pub href: String,
    pub title: String,
    pub html: Option<String>,
}
#[derive(Clone)]
pub struct TocEntry {
    pub title: String,
    pub href: String,
    pub depth: usize,
}
pub struct Book {
    pub path: PathBuf,
    pub fingerprint: String,
    pub format: Format,
    pub title: String,
    pub author: String,
    pub chapters: Vec<Chapter>,
    pub toc: Vec<TocEntry>,
    pub cover: Option<String>,
    binaries: HashMap<String, Vec<u8>>,
    source_pages: bool,
}
pub struct ChapterContent {
    pub html: String,
    pub images: HashMap<String, Vec<u8>>,
    pub anchors: HashMap<String, Range<usize>>,
    pub comic: Option<Vec<u8>>,
    pub page: Option<crate::paged::TextPage>,
}
pub fn supports(path: &Path) -> bool {
    path.file_name().and_then(|v| v.to_str()).is_some_and(|v| {
        let name = v.to_ascii_lowercase();
        name.ends_with(".epub")
            || name.ends_with(".fb2")
            || name.ends_with(".fb2.zip")
            || name.ends_with(".cbz")
    })
}
fn bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    reader_limits::preview_budget(
        bytes.len() as u64 <= limit,
        "Ebook content exceeds reading limit",
    )?;
    Ok(bytes)
}
fn archive(path: &Path) -> Result<zip::ZipArchive<std::fs::File>> {
    let archive = zip::ZipArchive::new(std::fs::File::open(path)?)
        .context("Damaged or encrypted ebook archive")?;
    Ok(archive)
}
fn entry(archive: &mut zip::ZipArchive<std::fs::File>, name: &str, limit: u64) -> Result<Vec<u8>> {
    let file = archive
        .by_name(name)
        .with_context(|| format!("Missing ebook resource: {name}"))?;
    reader_limits::preview_budget(file.size() <= limit, "Ebook resource exceeds reading limit")?;
    ensure!(
        file.unix_mode().is_none_or(|v| v & 0o170000 != 0o120000),
        "Ebook resources cannot be symbolic links"
    );
    bounded(file, limit)
}
fn xml_encoding(bytes: &[u8]) -> &'static encoding_rs::Encoding {
    if let Some((encoding, _)) = encoding_rs::Encoding::for_bom(bytes) {
        encoding
    } else if bytes.starts_with(&[0, b'<', 0, b'?']) {
        encoding_rs::UTF_16BE
    } else if bytes.starts_with(&[b'<', 0, b'?', 0]) {
        encoding_rs::UTF_16LE
    } else {
        if !bytes.starts_with(b"<?xml") {
            return encoding_rs::UTF_8;
        }
        let end = bytes
            .windows(2)
            .position(|pair| pair == b"?>")
            .map_or(bytes.len(), |index| index + 2);
        let mut probe = EncodingProbe::default();
        for byte in &bytes[..end] {
            probe.push(*byte);
        }
        probe.encoding.unwrap_or(encoding_rs::UTF_8)
    }
}
fn decode(bytes: &[u8]) -> Result<String> {
    let encoding = xml_encoding(bytes);
    let (text, _, errors) = encoding.decode(bytes);
    ensure!(!errors, "Ebook text encoding is invalid");
    let mut text = text.into_owned();
    while let Some(start) = text.find("<!DOCTYPE") {
        let end = text[start..].find('>').context("Truncated ebook doctype")? + start + 1;
        ensure!(
            !text[start..end].contains('['),
            "Ebook entity declarations are unsupported"
        );
        text.replace_range(start..end, "");
    }
    for (name, value) in [
        ("nbsp", 160),
        ("mdash", 8212),
        ("ndash", 8211),
        ("ldquo", 8220),
        ("rdquo", 8221),
        ("lsquo", 8216),
        ("rsquo", 8217),
        ("hellip", 8230),
        ("copy", 169),
    ] {
        text = text.replace(&format!("&{name};"), &format!("&#{value};"));
    }
    Ok(text)
}
fn xml(text: &str) -> Result<Document<'_>> {
    ensure!(
        !text.contains("<!DOCTYPE"),
        "Ebook XML document types and external entities are unsupported"
    );
    let parsed = Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: reader_limits::BOOK_XML_NODES,
            ..Default::default()
        },
    );
    match parsed {
        Err(roxmltree::Error::NodesLimitReached) => {
            Err(reader_limits::PreviewBudgetExceeded("Chapter XML working budget exceeded").into())
        }
        result => result.context("Invalid ebook XML"),
    }
}
fn named<'a, 'input>(node: Node<'a, 'input>, name: &str) -> impl Iterator<Item = Node<'a, 'input>> {
    let name = name.to_owned();
    node.descendants()
        .filter(move |n| n.is_element() && n.tag_name().name() == name)
}
fn text(node: Node<'_, '_>) -> String {
    node.descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned()
}
fn attribute<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}
/// Resolve only archive-local paths. Relative .. is allowed only while staying inside the archive.
pub fn resolve(base: &str, href: &str) -> Option<String> {
    if href.starts_with("//") || href.contains(':') || href.contains('\\') || href.contains('\0') {
        return None;
    }
    let (path, fragment) = href
        .split_once('#')
        .map(|(p, f)| (p, Some(f)))
        .unwrap_or((href, None));
    let path = path.split('?').next().unwrap_or(path);
    let mut decoded = Vec::new();
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    let path = String::from_utf8(decoded).ok()?;
    if path.contains([':', '\\', '\0']) {
        return None;
    }
    let mut components = if path.starts_with('/') {
        Vec::new()
    } else {
        base.split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    if path.is_empty() { /* Keep the current document for fragment-only links. */
    } else {
        if !path.starts_with('/') {
            components.pop();
        }
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    components.pop()?;
                }
                _ => components.push(part.to_owned()),
            }
        }
    }
    let mut result = components.join("/");
    if let Some(fragment) = fragment {
        result.push('#');
        result.push_str(fragment);
    }
    Some(result)
}
pub fn read(path: &Path) -> Result<Book> {
    read_cancellable(path, &crate::work::Cancellation::default())
}
pub fn read_cancellable(path: &Path, cancel: &crate::work::Cancellation) -> Result<Book> {
    match read_structured(path, cancel) {
        Err(error) if error.is::<reader_limits::PreviewBudgetExceeded>() => {
            source_book(path, cancel)
        }
        result => result,
    }
}
fn read_structured(path: &Path, cancel: &crate::work::Cancellation) -> Result<Book> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; reader_limits::IO_BUFFER_BYTES];
    loop {
        cancel.check()?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let fingerprint = format!("ebook:{:x}", hash.finalize());
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut book = if name.ends_with(".epub") {
        epub(path)?
    } else if name.ends_with(".cbz") {
        comic(path)?
    } else {
        let bytes = if name.ends_with(".zip") {
            let mut package = archive(path)?;
            let entries = package
                .file_names()
                .filter(|v| v.to_ascii_lowercase().ends_with(".fb2"))
                .map(str::to_owned)
                .collect::<Vec<_>>();
            ensure!(entries.len() == 1, "FB2 ZIP must contain exactly one book");
            entry(&mut package, &entries[0], TEXT_LIMIT)?
        } else {
            bounded(std::fs::File::open(path)?, TEXT_LIMIT)?
        };
        fictionbook(path, &decode(&bytes)?)?
    };
    book.fingerprint = fingerprint;
    if book.title.is_empty() {
        book.title = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
    }
    ensure!(
        !book.chapters.is_empty(),
        "Ebook requires chapters or comic pages within the preview count limit"
    );
    Ok(book)
}
fn empty(path: &Path, format: Format) -> Result<Book> {
    Ok(Book {
        path: std::path::absolute(path)?,
        fingerprint: String::new(),
        format,
        title: String::new(),
        author: String::new(),
        chapters: Vec::new(),
        toc: Vec::new(),
        cover: None,
        binaries: HashMap::new(),
        source_pages: false,
    })
}
fn epub(path: &Path) -> Result<Book> {
    let mut package = archive(path)?;
    if package.file_names().any(|v| v == "META-INF/encryption.xml") {
        let value = decode(&entry(
            &mut package,
            "META-INF/encryption.xml",
            CHAPTER_LIMIT,
        )?)?;
        let document = xml(&value)?;
        for method in named(document.root(), "EncryptionMethod") {
            let algorithm = method.attribute("Algorithm").unwrap_or_default();
            ensure!(
                matches!(
                    algorithm,
                    "http://www.idpf.org/2008/embedding" | "http://ns.adobe.com/pdf/enc#RC"
                ),
                "DRM-encrypted EPUBs are unsupported"
            );
        }
    }
    let container = decode(&entry(
        &mut package,
        "META-INF/container.xml",
        CHAPTER_LIMIT,
    )?)?;
    let document = xml(&container)?;
    let root = named(document.root(), "rootfile")
        .find_map(|n| n.attribute("full-path"))
        .context("EPUB has no package document")?;
    let root = resolve("", root).context("EPUB package path is outside the archive")?;
    let opf = decode(&entry(&mut package, &root, CHAPTER_LIMIT)?)?;
    let document = xml(&opf)?;
    let mut book = empty(path, Format::Epub)?;
    book.title = named(document.root(), "title")
        .next()
        .map(text)
        .unwrap_or_default();
    book.author = named(document.root(), "creator")
        .map(text)
        .collect::<Vec<_>>()
        .join(", ");
    let mut items = HashMap::new();
    let mut nav = None;
    let mut ncx = None;
    for node in named(document.root(), "item") {
        let id = node
            .attribute("id")
            .context("EPUB manifest item lacks identity")?;
        let href = resolve(
            &root,
            node.attribute("href")
                .context("EPUB manifest item lacks resource")?,
        )
        .context("EPUB resource path escapes the archive")?;
        let media = node.attribute("media-type").unwrap_or_default();
        let properties = node.attribute("properties").unwrap_or_default();
        if properties.split_whitespace().any(|v| v == "nav") {
            nav = Some(href.clone());
        }
        if properties.split_whitespace().any(|v| v == "cover-image") {
            book.cover = Some(href.clone());
        }
        if media == "application/x-dtbncx+xml" {
            ncx = Some(href.clone());
        }
        ensure!(
            items.insert(id, (href, media)).is_none(),
            "Duplicate EPUB manifest identity"
        );
    }
    for node in named(document.root(), "itemref") {
        let (href, media) = items
            .get(node.attribute("idref").unwrap_or_default())
            .context("EPUB spine references a missing item")?;
        ensure!(
            matches!(*media, "application/xhtml+xml" | "text/html"),
            "EPUB spine contains an unsupported chapter type"
        );
        book.chapters.push(Chapter {
            href: href.clone(),
            title: format!("Chapter {}", book.chapters.len() + 1),
            html: None,
        });
    }
    if let Some(nav) = nav {
        let html = decode(&entry(&mut package, &nav, CHAPTER_LIMIT)?)?;
        let doc = xml(&html)?;
        if let Some(toc) = named(doc.root(), "nav").find(|n| {
            attribute(*n, "type").is_some_and(|s| s.split_whitespace().any(|s| s == "toc"))
        }) {
            for link in named(toc, "a") {
                if let Some(href) = link.attribute("href").and_then(|v| resolve(&nav, v)) {
                    let depth = link
                        .ancestors()
                        .take_while(|n| *n != toc)
                        .filter(|n| n.is_element() && n.tag_name().name() == "ol")
                        .count()
                        .saturating_sub(1);
                    book.toc.push(TocEntry {
                        title: text(link),
                        href,
                        depth: depth,
                    });
                }
            }
        }
    }
    if book.toc.is_empty()
        && let Some(ncx) = ncx
    {
        let body = decode(&entry(&mut package, &ncx, CHAPTER_LIMIT)?)?;
        let doc = xml(&body)?;
        for point in named(doc.root(), "navPoint") {
            let label = named(point, "navLabel")
                .next()
                .map(text)
                .unwrap_or_default();
            if let Some(href) = point
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "content")
                .and_then(|n| n.attribute("src"))
                .and_then(|v| resolve(&ncx, v))
            {
                let depth = point
                    .ancestors()
                    .skip(1)
                    .filter(|n| n.is_element() && n.tag_name().name() == "navPoint")
                    .count();
                book.toc.push(TocEntry {
                    title: label,
                    href,
                    depth: depth,
                });
            }
        }
    }
    for chapter in &mut book.chapters {
        if let Some(label) = book
            .toc
            .iter()
            .find(|t| t.href.split('#').next() == Some(chapter.href.as_str()))
        {
            chapter.title.clone_from(&label.title);
        }
    }
    if book.toc.is_empty() {
        book.toc = book
            .chapters
            .iter()
            .map(|c| TocEntry {
                title: c.title.clone(),
                href: c.href.clone(),
                depth: 0,
            })
            .collect();
    }
    Ok(book)
}
fn comic(path: &Path) -> Result<Book> {
    let mut package = archive(path)?;
    let mut book = empty(path, Format::Comic)?;
    let mut images = package
        .file_names()
        .filter(|name| {
            matches!(
                name.rsplit('.')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"
            )
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    images.sort_by_cached_key(|name| natural_key(name));
    for href in images {
        ensure!(
            resolve("", &href).as_deref() == Some(href.as_str()),
            "Comic page path escapes archive"
        );
        book.chapters.push(Chapter {
            title: format!("Page {}", book.chapters.len() + 1),
            href,
            html: None,
        });
    }
    if package.file_names().any(|s| s == "ComicInfo.xml") {
        let metadata = decode(&entry(&mut package, "ComicInfo.xml", CHAPTER_LIMIT)?)?;
        let doc = xml(&metadata)?;
        book.title = named(doc.root(), "Title")
            .next()
            .map(text)
            .unwrap_or_default();
        book.author = named(doc.root(), "Writer")
            .map(text)
            .collect::<Vec<_>>()
            .join(", ");
    }
    book.toc = book
        .chapters
        .iter()
        .map(|c| TocEntry {
            title: c.title.clone(),
            href: c.href.clone(),
            depth: 0,
        })
        .collect();
    book.cover = book.chapters.first().map(|c| c.href.clone());
    Ok(book)
}
fn natural_key(value: &str) -> String {
    let mut out = String::new();
    let mut digits = String::new();
    for c in value.to_lowercase().chars().chain(std::iter::once('\0')) {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            if !digits.is_empty() {
                out.push_str(&format!("{:0>20}", digits));
                digits.clear();
            }
            if c != '\0' {
                out.push(c);
            }
        }
    }
    out
}
fn fictionbook(path: &Path, body: &str) -> Result<Book> {
    let doc = xml(body)?;
    ensure!(
        doc.root_element().tag_name().name() == "FictionBook",
        "Invalid FB2 root"
    );
    let mut book = empty(path, Format::FictionBook)?;
    let title_info = named(doc.root(), "title-info")
        .next()
        .context("FB2 has no book metadata")?;
    book.title = named(title_info, "book-title")
        .next()
        .map(text)
        .unwrap_or_default();
    book.author = named(title_info, "author")
        .map(|a| {
            a.children()
                .filter(|n| {
                    n.is_element()
                        && matches!(
                            n.tag_name().name(),
                            "first-name" | "middle-name" | "last-name"
                        )
                })
                .map(text)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(", ");
    use base64::{Engine, engine::general_purpose::STANDARD};
    let mut total = 0;
    for binary in named(doc.root(), "binary") {
        let id = binary.attribute("id").context("FB2 image lacks identity")?;
        let encoded = text(binary).split_whitespace().collect::<String>();
        let bytes = STANDARD
            .decode(encoded)
            .context("Invalid FB2 embedded image")?;
        total += bytes.len();
        reader_limits::preview_budget(
            total as u64 <= TEXT_LIMIT,
            "FB2 images exceed reading limit",
        )?;
        book.binaries.insert(id.into(), bytes);
    }
    for body in named(doc.root(), "body") {
        let sections = body
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "section")
            .collect::<Vec<_>>();
        for section in sections {
            let title = section
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "title")
                .map(text)
                .unwrap_or_else(|| format!("Chapter {}", book.chapters.len() + 1));
            let href = format!("chapter-{}", book.chapters.len());
            let mut html = String::new();
            for node in section.children() {
                serialize_fb2(node, &mut html, 0)?;
            }
            reader_limits::preview_budget(
                html.len() as u64 <= CHAPTER_LIMIT,
                "FB2 chapter exceeds reading limit",
            )?;
            book.chapters.push(Chapter {
                href: href.clone(),
                title: title.clone(),
                html: Some(format!("<body>{html}</body>")),
            });
            book.toc.push(TocEntry {
                title,
                href,
                depth: 0,
            });
        }
    }
    Ok(book)
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn serialize_fb2(node: Node<'_, '_>, out: &mut String, depth: usize) -> Result<()> {
    reader_limits::preview_budget(
        depth <= reader_limits::BOOK_FB2_DEPTH,
        "FB2 nesting exceeds reading limit",
    )?;
    if node.is_text() {
        out.push_str(&escape(node.text().unwrap_or_default()));
        return Ok(());
    }
    if !node.is_element() {
        return Ok(());
    }
    let tag = match node.tag_name().name() {
        "title" => "h2",
        "subtitle" => "h3",
        "emphasis" => "em",
        "strong" => "strong",
        "strikethrough" => "s",
        "code" => "code",
        "p" | "v" => "p",
        "cite" | "epigraph" | "poem" => "blockquote",
        "a" => "a",
        "image" => "img",
        "empty-line" => "br",
        _ => "div",
    };
    out.push_str(&format!("<{tag}"));
    if let Some(id) = node.attribute("id") {
        out.push_str(&format!(" id=\"{}\"", escape(id)));
    }
    if matches!(tag, "img" | "a")
        && let Some(href) = attribute(node, "href")
    {
        out.push_str(&format!(
            " {}=\"{}\"",
            if tag == "img" { "src" } else { "href" },
            escape(href)
        ));
    }
    out.push('>');
    for child in node.children() {
        serialize_fb2(child, out, depth + 1)?;
    }
    out.push_str(&format!("</{tag}>"));
    reader_limits::preview_budget(
        out.len() as u64 <= CHAPTER_LIMIT,
        "FB2 chapter exceeds reading limit",
    )?;
    Ok(())
}
pub fn chapter(book: &Book, index: usize) -> Result<ChapterContent> {
    chapter_cancellable(book, index, &crate::work::Cancellation::default())
}
pub fn chapter_cancellable(
    book: &Book,
    index: usize,
    cancel: &crate::work::Cancellation,
) -> Result<ChapterContent> {
    cancel.check()?;
    if book.source_pages {
        return source_chapter(book, index, cancel);
    }
    match chapter_structured(book, index, cancel) {
        Err(error) if error.is::<reader_limits::PreviewBudgetExceeded>() => {
            source_chapter(book, index, cancel)
        }
        result => result,
    }
}
fn chapter_structured(
    book: &Book,
    index: usize,
    cancel: &crate::work::Cancellation,
) -> Result<ChapterContent> {
    let chapter = book.chapters.get(index).context("Unknown ebook chapter")?;
    if book.format == Format::Comic {
        let png = archive_image(&book.path, &chapter.href, cancel)?;
        return Ok(ChapterContent {
            html: String::new(),
            images: HashMap::new(),
            anchors: HashMap::new(),
            comic: Some(png),
            page: None,
        });
    }
    let raw = if let Some(html) = &chapter.html {
        html.clone()
    } else {
        decode(&entry(
            &mut archive(&book.path)?,
            &chapter.href,
            CHAPTER_LIMIT,
        )?)?
    };
    let doc = xml(&raw)?;
    let body = named(doc.root(), "body")
        .next()
        .unwrap_or(doc.root_element());
    let mut output = ChapterContent {
        html: String::new(),
        images: HashMap::new(),
        anchors: HashMap::new(),
        comic: None,
        page: None,
    };
    sanitize(body, &mut output, 0)?;
    let refs = output.images.keys().cloned().collect::<Vec<_>>();
    let mut decoded_total = 0;
    let mut preview_total = 0;
    for reference in refs {
        cancel.check()?;
        let preview = if book.format == Format::FictionBook {
            book.binaries
                .get(reference.trim_start_matches('#'))
                .map(|bytes| image_png(bytes, &reference))
                .transpose()?
        } else if let Some(path) = resolve(&chapter.href, &reference) {
            let path = path.split('#').next().unwrap_or(&path);
            Some(archive_image(&book.path, path, cancel)?)
        } else {
            None
        };
        if let Some(png) = preview {
            use image::ImageDecoder;
            let decoder =
                image::ImageReader::with_format(Cursor::new(&png), image::ImageFormat::Png)
                    .into_decoder()?;
            decoded_total += decoder.total_bytes();
            drop(decoder);
            preview_total += png.len();
            reader_limits::preview_budget(
                decoded_total <= reader_limits::BOOK_CHAPTER_DECODED_BYTES
                    && preview_total as u64 <= TEXT_LIMIT,
                "Chapter illustrations exceed decoded memory limit",
            )?;
            output.images.insert(reference, png);
        }
    }
    Ok(output)
}
fn sanitize(node: Node<'_, '_>, output: &mut ChapterContent, depth: usize) -> Result<()> {
    reader_limits::preview_budget(
        depth <= reader_limits::BOOK_HTML_DEPTH && output.html.len() as u64 <= CHAPTER_LIMIT,
        "Chapter rendering exceeds reading limit",
    )?;
    if node.is_text() {
        output
            .html
            .push_str(&escape(node.text().unwrap_or_default()));
        return Ok(());
    }
    if !node.is_element() {
        return Ok(());
    }
    let tag = node.tag_name().name();
    if matches!(
        tag,
        "script"
            | "style"
            | "iframe"
            | "object"
            | "embed"
            | "form"
            | "input"
            | "head"
            | "link"
            | "audio"
            | "video"
    ) {
        return Ok(());
    }
    let tag = if matches!(
        tag,
        "p" | "div"
            | "span"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "em"
            | "i"
            | "strong"
            | "b"
            | "blockquote"
            | "ul"
            | "ol"
            | "li"
            | "table"
            | "thead"
            | "tbody"
            | "tr"
            | "td"
            | "th"
            | "pre"
            | "code"
            | "a"
            | "img"
            | "br"
            | "hr"
            | "sup"
            | "sub"
            | "s"
    ) {
        tag
    } else if tag == "image" {
        "img"
    } else {
        "div"
    };
    let start = output.html.len();
    output.html.push_str(&format!("<{tag}"));
    if tag == "a"
        && let Some(href) = attribute(node, "href")
    {
        output.html.push_str(&format!(" href=\"{}\"", escape(href)));
    }
    if tag == "img"
        && let Some(src) = attribute(node, "src").or_else(|| attribute(node, "href"))
    {
        reader_limits::preview_budget(
            output.images.len() < reader_limits::BOOK_CHAPTER_IMAGES,
            "Chapter image working budget exceeded",
        )?;
        output.images.insert(src.into(), Vec::new());
        output.html.push_str(&format!(" src=\"{}\"", escape(src)));
        if let Some(alt) = node.attribute("alt") {
            output.html.push_str(&format!(" alt=\"{}\"", escape(alt)));
        }
    }
    output.html.push('>');
    for child in node.children() {
        sanitize(child, output, depth + 1)?;
    }
    output.html.push_str(&format!("</{tag}>"));
    if let Some(id) = node.attribute("id") {
        output.anchors.insert(id.into(), start..output.html.len());
    }
    Ok(())
}
fn image_png(bytes: &[u8], _name: &str) -> Result<Vec<u8>> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(bytes)?;
    crate::inline_images::thumbnail_png(file.path(), reader_limits::BOOK_PREVIEW_EDGE)
}
fn archive_image(path: &Path, member: &str, cancel: &crate::work::Cancellation) -> Result<Vec<u8>> {
    let mut archive = archive(path)?;
    let entry = archive.by_name(member)?;
    let mut file = tempfile::NamedTempFile::new()?;
    cancel.copy(entry, file.as_file_mut())?;
    crate::inline_images::thumbnail_png_cancellable(
        file.path(),
        reader_limits::BOOK_PREVIEW_EDGE,
        cancel,
    )
}
fn source_book(path: &Path, cancel: &crate::work::Cancellation) -> Result<Book> {
    let zipped = !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("fb2"));
    let fiction = path.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(".fb2.zip")
    }) || !zipped;
    let mut book = empty(
        path,
        if fiction {
            Format::FictionBook
        } else {
            Format::Epub
        },
    )?;
    book.title = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    book.fingerprint = format!("ebook-source:{}", std::path::absolute(path)?.display());
    book.source_pages = true;
    if zipped {
        let mut package = archive(path)?;
        for index in 0..package.len() {
            cancel.check()?;
            let entry = package.by_index(index)?;
            let name = entry.name();
            if !entry.is_dir() {
                book.chapters.push(Chapter {
                    href: name.to_owned(),
                    title: name.to_owned(),
                    html: None,
                });
            }
        }
    } else {
        book.chapters.push(Chapter {
            href: String::new(),
            title: book.title.clone(),
            html: None,
        });
    }
    ensure!(
        !book.chapters.is_empty(),
        "Ebook contains no readable chapter source"
    );
    book.toc = book
        .chapters
        .iter()
        .map(|c| TocEntry {
            href: c.href.clone(),
            title: c.title.clone(),
            depth: 0,
        })
        .collect();
    Ok(book)
}
fn source_chapter(
    book: &Book,
    index: usize,
    cancel: &crate::work::Cancellation,
) -> Result<ChapterContent> {
    let chapter = book.chapters.get(index).context("Unknown ebook chapter")?;
    let page = if book
        .path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("fb2"))
    {
        transcode_source(std::fs::File::open(&book.path)?, &book.path, cancel)?
    } else {
        if book.source_pages
            && !matches!(
                chapter
                    .href
                    .rsplit('.')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .as_str(),
                "htm" | "html" | "xhtml" | "xml" | "opf" | "ncx" | "fb2"
            )
        {
            return Ok(ChapterContent {
                html: String::new(),
                images: HashMap::new(),
                anchors: HashMap::new(),
                comic: None,
                page: Some(crate::paged::zip_member_bytes(
                    &book.path,
                    &chapter.href,
                    cancel,
                )?),
            });
        }
        let mut package = archive(&book.path)?;
        let mut entry = package.by_name(&chapter.href)?;
        let mut prefix = Vec::new();
        (&mut entry).take(256).read_to_end(&mut prefix)?;
        let complete_declaration =
            !prefix.starts_with(b"<?xml") || prefix.windows(2).any(|pair| pair == b"?>");
        if complete_declaration && xml_encoding(&prefix) == encoding_rs::UTF_8 {
            drop(entry);
            drop(package);
            crate::paged::zip_member(&book.path, &chapter.href, cancel)?
        } else {
            transcode_source(
                std::io::Cursor::new(prefix).chain(entry),
                &book.path,
                cancel,
            )?
        }
    };
    Ok(ChapterContent {
        html: String::new(),
        images: HashMap::new(),
        anchors: HashMap::new(),
        comic: None,
        page: Some(page),
    })
}

fn transcode_source(
    mut input: impl Read,
    path: &Path,
    cancel: &crate::work::Cancellation,
) -> Result<crate::paged::TextPage> {
    use std::io::{BufReader, BufWriter, Write};
    let mut prefix = Vec::new();
    (&mut input).take(8).read_to_end(&mut prefix)?;
    let declaration = prefix.starts_with(b"<?xml");
    let mut encoding = xml_encoding(&prefix);
    let mut input = BufReader::with_capacity(
        reader_limits::IO_BUFFER_BYTES,
        std::io::Cursor::new(prefix).chain(input),
    );
    let mut file = tempfile::NamedTempFile::new()?;
    let mut writer = BufWriter::with_capacity(reader_limits::IO_BUFFER_BYTES, file.as_file_mut());
    if declaration {
        // XML declarations are ASCII, even for legacy single-byte encodings.
        // Stream arbitrary declaration whitespace without retaining a prefix cap.
        let mut probe = EncodingProbe::default();
        let mut previous = 0;
        let mut position = 0;
        loop {
            if position % reader_limits::IO_BUFFER_BYTES == 0 {
                cancel.check()?;
            }
            let mut byte = [0];
            input
                .read_exact(&mut byte)
                .context("Truncated XML declaration")?;
            probe.push(byte[0]);
            writer.write_all(&byte)?;
            position += 1;
            if previous == b'?' && byte[0] == b'>' {
                break;
            }
            previous = byte[0];
        }
        if let Some(detected) = probe.encoding {
            encoding = detected;
        }
    }
    if encoding == encoding_rs::UTF_8
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("fb2"))
    {
        writer.flush()?;
        drop(writer);
        return crate::paged::read_page(path, 0, false);
    }
    let mut decoder = encoding.new_decoder_with_bom_removal();
    let mut buffer = vec![0; reader_limits::IO_BUFFER_BYTES];
    let mut output = vec![0; reader_limits::IO_BUFFER_BYTES];
    loop {
        cancel.check()?;
        let count = input.read(&mut buffer)?;
        let mut consumed = 0;
        loop {
            let (result, read, written, errors) =
                decoder.decode_to_utf8(&buffer[consumed..count], &mut output, count == 0);
            ensure!(!errors, "Ebook text encoding is invalid");
            consumed += read;
            writer.write_all(&output[..written])?;
            if result == encoding_rs::CoderResult::InputEmpty {
                break;
            }
            cancel.check()?;
        }
        if count == 0 {
            break;
        }
    }
    writer.flush()?;
    drop(writer);
    crate::paged::generated(path, file)
}
#[derive(Default)]
struct EncodingProbe {
    state: usize,
    matched: usize,
    quote: u8,
    label: Vec<u8>,
    encoding: Option<&'static encoding_rs::Encoding>,
}
impl EncodingProbe {
    fn push(&mut self, byte: u8) {
        match self.state {
            0 => {
                let key = b"encoding";
                self.matched = if byte == key[self.matched] {
                    self.matched + 1
                } else {
                    usize::from(byte == key[0])
                };
                if self.matched == key.len() {
                    self.state = 1;
                    self.matched = 0;
                }
            }
            1 if byte.is_ascii_whitespace() => {}
            1 if byte == b'=' => self.state = 2,
            2 if byte.is_ascii_whitespace() => {}
            2 if matches!(byte, b'\'' | b'"') => {
                self.quote = byte;
                self.state = 3;
            }
            3 if byte == self.quote => {
                self.encoding = encoding_rs::Encoding::for_label(&self.label);
                self.state = 4;
            }
            3 => {
                if self.label.len() < reader_limits::IO_BUFFER_BYTES {
                    self.label.push(byte);
                }
            }
            4 => {}
            _ => self.state = 0,
        }
    }
}

/// A memory-only missing-resource fallback; never resolve unknown illustration URLs.
pub fn missing_image() -> Vec<u8> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").expect("Embedded fallback image")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn zip(path: &Path, entries: Vec<(&str, Vec<u8>)>) {
        let mut archive = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, bytes) in entries {
            archive
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            archive.write_all(&bytes).unwrap();
        }
        archive.finish().unwrap();
    }
    fn png() -> Vec<u8> {
        let image = image::DynamicImage::new_rgb8(8, 8);
        let mut output = Cursor::new(Vec::new());
        image
            .write_to(&mut output, image::ImageFormat::Png)
            .unwrap();
        output.into_inner()
    }
    #[test]
    fn epub_spine_navigation_images_and_script_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        let container =
            r#"<container><rootfiles><rootfile full-path="OPS/book.opf"/></rootfiles></container>"#;
        let opf = r#"<package xmlns:dc="urn:dc"><metadata><dc:title>Reader Test</dc:title><dc:creator>Author</dc:creator></metadata><manifest><item id="one" href="Text/one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="Text/two.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="toc.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="cover" href="Images/pixel.png" media-type="image/png" properties="cover-image"/></manifest><spine><itemref idref="two"/><itemref idref="one"/></spine></package>"#;
        let nav = r#"<html xmlns:epub="urn:epub"><body><nav epub:type="toc"><ol><li><a href="Text/two.xhtml#start">First chapter</a><ol><li><a href="Text/one.xhtml">Second chapter</a></li></ol></li></ol></nav></body></html>"#;
        let body = r#"<!DOCTYPE html><html><head><script>bad()</script></head><body><h1 id="start">First&nbsp;chapter</h1><p>Reading text.</p><img src="../Images/pixel.png"/><img src="https://example.com/tracker.png"/><script>bad()</script><iframe src="file:///private/data"/><a href="one.xhtml">Continue</a></body></html>"#;
        zip(
            &path,
            vec![
                ("mimetype", b"application/epub+zip".to_vec()),
                ("META-INF/container.xml", container.as_bytes().to_vec()),
                ("OPS/book.opf", opf.as_bytes().to_vec()),
                ("OPS/toc.xhtml", nav.as_bytes().to_vec()),
                (
                    "OPS/Text/one.xhtml",
                    b"<html><body><p>Second.</p></body></html>".to_vec(),
                ),
                ("OPS/Text/two.xhtml", body.as_bytes().to_vec()),
                ("OPS/Images/pixel.png", png()),
            ],
        );
        let before = std::fs::read(&path).unwrap();
        let book = read(&path).unwrap();
        assert_eq!(book.title, "Reader Test");
        assert_eq!(book.author, "Author");
        assert_eq!(book.chapters[0].href, "OPS/Text/two.xhtml");
        assert_eq!(book.toc[1].depth, 1);
        let content = chapter(&book, 0).unwrap();
        assert!(content.html.contains("Reading text."));
        assert!(!content.html.contains("bad()") && !content.html.contains("iframe"));
        assert!(!content.images["../Images/pixel.png"].is_empty());
        assert!(content.images["https://example.com/tracker.png"].is_empty());
        assert!(content.anchors.contains_key("start"));
        assert_eq!(before, std::fs::read(path).unwrap());
    }
    #[test]
    fn fictionbook_author_chapters_and_embedded_illustrations() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.fb2");
        let body = format!(
            r##"<?xml version="1.0" encoding="utf-8"?><FictionBook xmlns:xlink="urn:xlink"><description><title-info><book-title>中文书</book-title><author><first-name>Reader</first-name><last-name>Author</last-name></author></title-info></description><body><section id="first"><title><p>第一章</p></title><p>Hello <emphasis>reader</emphasis>.</p><image xlink:href="#picture"/></section><section><title><p>第二章</p></title><p>Next</p></section></body><binary id="picture" content-type="image/png">{}</binary></FictionBook>"##,
            STANDARD.encode(png())
        );
        std::fs::write(&path, &body).unwrap();
        let book = read(&path).unwrap();
        assert_eq!(book.title, "中文书");
        assert_eq!(book.author, "Reader Author");
        assert_eq!(book.chapters.len(), 2);
        let content = chapter(&book, 0).unwrap();
        assert!(content.html.contains("<em>reader</em>"));
        assert!(!content.images["#picture"].is_empty());
        let zipped = dir.path().join("book.fb2.zip");
        zip(&zipped, vec![("book.fb2", body.into_bytes())]);
        assert_eq!(read(&zipped).unwrap().chapters.len(), 2);
    }
    #[test]
    fn comic_pages_use_natural_order_and_local_images() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("comic.cbz");
        zip(
            &path,
            vec![
                ("page10.png", png()),
                ("page2.png", png()),
                ("page1.png", png()),
                (
                    "ComicInfo.xml",
                    b"<ComicInfo><Title>Comic</Title><Writer>Artist</Writer></ComicInfo>".to_vec(),
                ),
            ],
        );
        let book = read(&path).unwrap();
        assert_eq!(book.title, "Comic");
        assert_eq!(
            book.chapters
                .iter()
                .map(|c| c.href.as_str())
                .collect::<Vec<_>>(),
            ["page1.png", "page2.png", "page10.png"]
        );
        assert!(chapter(&book, 0).unwrap().comic.is_some());
    }
    #[test]
    fn resource_paths_stay_inside_the_book_and_xml_entities_are_rejected() {
        assert_eq!(
            resolve("OPS/Text/one.xhtml", "../Images/cover%20photo.png"),
            Some("OPS/Images/cover photo.png".into())
        );
        assert_eq!(
            resolve("OPS/one.xhtml", "#note"),
            Some("OPS/one.xhtml#note".into())
        );
        for href in [
            "../../../secret",
            "file:///etc/passwd",
            "https://example.com/a",
            "//example.com/a",
            "%2e%2e/%2e%2e/%2e%2e/secret",
            r"..\secret",
        ] {
            assert!(resolve("OPS/Text/one.xhtml", href).is_none());
        }
        assert!(
            xml("<!DOCTYPE book [<!ENTITY a SYSTEM 'file:///secret'>]><book>&a;</book>").is_err()
        );
        assert!(decode(b"<!DOCTYPE book [<!ENTITY a SYSTEM 'file:///secret'>]><book/>").is_err());
        assert!(image_png(&missing_image(), "missing.png").is_ok());
    }
}
