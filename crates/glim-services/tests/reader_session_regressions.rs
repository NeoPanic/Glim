//! Generated, local-only fixtures for the new structured readers.
use anyhow::Result;
use glim_services::{graph, json, limits, opml, sqlite::browser, work::Cancellation};
use std::{fs, path::Path};

#[test]
fn sqlite_pages_wal_rows_and_columns_beyond_old_caps_without_source_writes() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("data.sqlite");
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT); WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<727) INSERT INTO records SELECT x, 'value-' || x FROM n;")?;
    let columns = (0..300)
        .map(|i| format!("c{i} TEXT"))
        .collect::<Vec<_>>()
        .join(",");
    connection.execute_batch(&format!(
        "CREATE TABLE wide({columns}); INSERT INTO wide(c299) VALUES ('last-column');"
    ))?;
    let original = fs::read(&path)?;
    let wal_path = root.path().join("data.sqlite-wal");
    let wal = fs::read(&wal_path)?;
    let cancel = Cancellation::default();
    let db = browser::open(&path, &cancel)?;
    let columns = db.columns("records", &cancel)?;
    let mut query = browser::Query {
        table: "records".into(),
        columns: columns.names,
        order: columns.order,
        ..Default::default()
    };
    assert_eq!(db.count(&query, &cancel)?, 727);
    assert_eq!(
        db.page(&query, &cancel)?.rows.len(),
        limits::SQLITE_PAGE_ROWS
    );
    query.page = 1;
    let page = db.page(&query, &cancel)?;
    assert_eq!(page.rows.len(), 727 - limits::SQLITE_PAGE_ROWS);
    assert!(!page.more);
    query.page = 0;
    query.filter = Some((1, "727".into()));
    assert_eq!(db.count(&query, &cancel)?, 1);
    assert_eq!(db.value(&query, 0, 1, 0, &cancel)?.0, "value-727");
    let columns = db.columns("wide", &cancel)?;
    assert_eq!(columns.names.len(), 300);
    let query = browser::Query {
        table: "wide".into(),
        columns: columns.names,
        order: columns.order,
        start_column: 299,
        ..Default::default()
    };
    assert_eq!(db.page(&query, &cancel)?.rows[0][0], "last-column");
    assert_eq!(fs::read(&path)?, original);
    assert_eq!(fs::read(&wal_path)?, wal);
    Ok(())
}

#[test]
fn formatting_preserves_duplicate_keys_and_numeric_spelling() -> Result<()> {
    let input = r#"{"x":1e+02,"x":-0.00,"text":" a \" b ","empty":[]}"#;
    let formatted = json::format_document(input)?;
    assert!(formatted.contains("1e+02"));
    assert!(formatted.contains("-0.00"));
    assert_eq!(formatted.matches("\"x\":").count(), 2);
    assert_eq!(json::format_document(&formatted)?, formatted);
    assert!(json::format_document("{invalid}").is_err());
    Ok(())
}

#[test]
fn graph_formats_keep_nested_data_and_reject_invalid_input() -> Result<()> {
    let cancel = Cancellation::default();
    for (ext, text) in [
        (
            "json",
            r#"{"items":[{"name":"Apple","details":{"type":"Pome"}}]}"#,
        ),
        (
            "yaml",
            "items:\n  - name: Apple\n    details:\n      type: Pome\n",
        ),
        ("csv", "name,name\nApple,Pome\n"),
        (
            "xml",
            "<items><item name=\"Apple\"><type>Pome</type></item></items>",
        ),
    ] {
        let document = graph::read(
            Path::new(&format!("fixture.{ext}")),
            Some(text.into()),
            &cancel,
        )?;
        assert!(
            document.sheets[0]
                .topics
                .iter()
                .any(|topic| topic.notes.contains("Apple")),
            "{ext}"
        );
        assert!(
            document.sheets[0]
                .topics
                .iter()
                .any(|topic| topic.notes.contains("Pome")),
            "{ext}"
        );
    }
    assert!(graph::read(Path::new("bad.json"), Some("{".into()), &cancel).is_err());
    Ok(())
}

#[test]
fn opml_retains_nested_outlines_and_only_exposes_web_links() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("feeds.opml");
    fs::write(
        &path,
        r#"<opml version="2.0"><head><title>Feeds</title></head><body><outline text="Group"><outline text="Safe" xmlUrl="https://example.invalid/feed"/><outline text="Unsafe" xmlUrl="file:///private/data" htmlUrl="javascript:alert(1)"/></outline></body></opml>"#,
    )?;
    let document = opml::read(&path, &Cancellation::default())?;
    assert_eq!(document.title, "Feeds");
    assert_eq!(document.roots.len(), 1);
    let group = &document.nodes[document.roots[0]];
    assert_eq!(group.children.len(), 2);
    assert!(document.nodes[group.children[0]].feed.is_some());
    assert!(document.nodes[group.children[1]].feed.is_none());
    assert!(document.nodes[group.children[1]].website.is_none());
    Ok(())
}

#[test]
fn drawio_decodes_compressed_pages_and_produces_raster_output() -> Result<()> {
    use base64::Engine;
    use std::io::Write;
    let root = tempfile::tempdir()?;
    let path = root.path().join("diagram.drawio");
    let model = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2" value="Box" vertex="1" parent="1"><mxGeometry x="10" y="20" width="120" height="60" as="geometry"/></mxCell></root></mxGraphModel>"#;
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(model.as_bytes())?;
    let compressed = base64::engine::general_purpose::STANDARD.encode(encoder.finish()?);
    fs::write(
        &path,
        format!("<mxfile><diagram>{model}</diagram><diagram>{compressed}</diagram></mxfile>"),
    )?;
    let document = glim_services::drawio::read(&path, &Cancellation::default())?;
    assert_eq!(document.pages.len(), 2);
    for page in document.pages {
        assert!(page.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
    Ok(())
}

#[test]
fn binary_plist_preview_decodes_without_changing_original() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("fixture.plist");
    fs::write(
        &path,
        r#"<?xml version="1.0"?><plist version="1.0"><dict><key>Name</key><string>Fixture</string></dict></plist>"#,
    )?;
    assert!(
        std::process::Command::new("/usr/bin/plutil")
            .args(["-convert", "binary1"])
            .arg(&path)
            .status()?
            .success()
    );
    let bytes = fs::read(&path)?;
    assert!(glim_services::plist::is_binary(&path));
    let _preview = glim_services::plist::read(&path, &Cancellation::default())?;
    assert_eq!(fs::read(&path)?, bytes);
    Ok(())
}

#[test]
fn scan_only_docx_uses_embedded_images_in_document_order() -> Result<()> {
    use std::io::Write;
    let root = tempfile::tempdir()?;
    let path = root.path().join("scanned.docx");
    let mut archive = zip::ZipWriter::new(fs::File::create(&path)?);
    let options = zip::write::SimpleFileOptions::default();
    archive.start_file("word/document.xml", options)?;
    archive.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><a:blip r:embed="second"/><a:blip r:embed="first"/></w:body></w:document>"#)?;
    archive.start_file("word/_rels/document.xml.rels", options)?;
    archive.write_all(br#"<Relationships><Relationship Id="first" Target="media/a.png"/><Relationship Id="second" Target="media/b.png"/></Relationships>"#)?;
    for (name, color) in [("a", [255, 0, 0, 255]), ("b", [0, 0, 255, 255])] {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba(color));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png)?;
        archive.start_file(format!("word/media/{name}.png"), options)?;
        archive.write_all(png.get_ref())?;
    }
    archive.finish()?;
    let document = glim_services::word_preview::prepare(&path, &Cancellation::default())?;
    assert!(document.image_pages);
    let first = image::open(document.html.parent().unwrap().join("scan-0.png"))?.into_rgba8();
    assert_eq!(first.get_pixel(0, 0).0, [0, 0, 255, 255]);
    Ok(())
}
