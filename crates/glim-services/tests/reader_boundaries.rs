//! Regression coverage for complete access beyond retired total-size/count gates.
//! All fixtures are generated privately and are removed when the test finishes.
use anyhow::Result;
use glim_services::{
    books, formats, geo, limits, media, paged, safetensors, spreadsheet, work::Cancellation,
};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
};

fn entry(zip: &mut zip::ZipWriter<File>, name: &str, value: &str) -> Result<()> {
    zip.start_file(
        name,
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )?;
    zip.write_all(value.as_bytes())?;
    Ok(())
}
fn container(zip: &mut zip::ZipWriter<File>) -> Result<()> {
    entry(zip, "mimetype", "application/epub+zip")?;
    entry(
        zip,
        "META-INF/container.xml",
        r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
    )
}

#[test]
fn xlsx_disk_index_keeps_all_sheets_sparse_cells_and_long_strings() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("large.xlsx");
    let mut zip = zip::ZipWriter::new(File::create(&path)?);
    zip.start_file(
        "padding.bin",
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )?;
    let block = vec![0; limits::IO_BUFFER_BYTES];
    // A valid ZIP member takes the workbook beyond the retired 128 MiB gate.
    for _ in 0..(128 * 1024 * 1024 / block.len() + 1) {
        zip.write_all(&block)?;
    }
    entry(
        &mut zip,
        "_rels/.rels",
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="workbook" Target="xl/workbook.xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"/><Relationship Id="fixture" Target="padding.bin" Type="urn:glim:reader-boundary-fixture"/></Relationships>"#,
    )?;
    let mut content_types = String::from(
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="bin" ContentType="application/octet-stream"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>"#,
    );
    for index in 1..=131 {
        content_types.push_str(&format!(r#"<Override PartName="/xl/worksheets/s{index}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#));
    }
    content_types.push_str("</Types>");
    entry(&mut zip, "[Content_Types].xml", &content_types)?;
    let value = "界".repeat(5000) + " & tail";
    entry(
        &mut zip,
        "xl/sharedStrings.xml",
        &format!(
            r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>{}</t><rPh sb="0" eb="1"><t>phonetic</t></rPh></si></sst>"#,
            value.replace('&', "&amp;")
        ),
    )?;
    let mut sheets = String::new();
    let mut relationships = String::new();
    for index in 0..131 {
        sheets.push_str(&format!(
            r#"<sheet name="S{index}" sheetId="{}" r:id="r{}"/>"#,
            index + 1,
            index + 1
        ));
        relationships.push_str(&format!(r#"<Relationship Id="r{}" Target="worksheets/s{}.xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet"/>"#,index+1,index+1));
        let sheet = if index == 0 {
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row><c t="inlineStr"><is><t>first&#32;cell</t></is></c><c><v>7</v></c><c/></row><row r="5001"><c r="IW5001" t="s"><v>0</v></c><c r="IX5001"><f>IF(1&lt;2,2,3)</f><v>2</v></c></row></sheetData></worksheet>"#.to_owned()
        } else {
            format!(
                "<worksheet><sheetData><row><c><v>{index}</v></c></row></sheetData></worksheet>"
            )
        };
        entry(
            &mut zip,
            &format!("xl/worksheets/s{}.xml", index + 1),
            &sheet,
        )?;
    }
    relationships.push_str(r#"<Relationship Id="shared" Target="sharedStrings.xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings"/>"#);
    entry(
        &mut zip,
        "xl/workbook.xml",
        &format!(
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{sheets}</sheets></workbook>"#
        ),
    )?;
    entry(
        &mut zip,
        "xl/_rels/workbook.xml.rels",
        &format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{relationships}</Relationships>"#
        ),
    )?;
    zip.finish()?;
    assert!(fs::metadata(&path)?.len() > 128 * 1024 * 1024);
    let index = spreadsheet::prepare(&path, &Cancellation::default())?
        .expect("supported UTF-8 workbook must receive a complete cell index");
    let database = rusqlite::Connection::open(&index.database)?;
    assert_eq!(
        database.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        131
    );
    let name: String = database.query_row(
        "SELECT name FROM sqlite_master WHERE name LIKE '%: S0'",
        [],
        |row| row.get(0),
    )?;
    let mut statement = database.prepare(&format!("SELECT * FROM \"{name}\" ORDER BY rowid"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert_eq!(rows[0], ("A1".into(), "first cell".into(), String::new()));
    assert_eq!(rows[1].0, "B1");
    assert_eq!(rows[1].1, "7");
    assert_eq!(rows[2].0, "C1");
    assert_eq!(rows[3].0, "IW5001");
    assert_eq!(rows[3].1, value);
    assert_eq!(rows[4].2, "IF(1<2,2,3)");
    Ok(())
}

#[test]
fn epub_retains_a_large_catalog_and_full_metadata_labels() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("catalog.epub");
    let mut zip = zip::ZipWriter::new(File::create(&path)?);
    container(&mut zip)?;
    let title = "T".repeat(20_000);
    let (mut manifest, mut spine, mut nav) = (String::new(), String::new(), String::new());
    for index in 0..10_001 {
        manifest.push_str(&format!(
            r#"<item id="c{index}" href="c{index}.xhtml" media-type="application/xhtml+xml"/>"#
        ));
        spine.push_str(&format!(r#"<itemref idref="c{index}"/>"#));
        nav.push_str(&format!(
            r#"<li><a href="c{index}.xhtml">Chapter {index}</a></li>"#
        ));
        entry(
            &mut zip,
            &format!("OPS/c{index}.xhtml"),
            &format!(
                "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>{index}</p></body></html>"
            ),
        )?;
    }
    entry(
        &mut zip,
        "OPS/nav.xhtml",
        &format!(
            "<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><body><nav epub:type=\"toc\"><ol>{nav}</ol></nav></body></html>"
        ),
    )?;
    entry(
        &mut zip,
        "OPS/package.opf",
        &format!(
            "<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{title}</dc:title></metadata><manifest>{manifest}<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/></manifest><spine>{spine}</spine></package>"
        ),
    )?;
    zip.finish()?;
    let book = books::read(&path)?;
    assert_eq!(book.title, title);
    assert_eq!(book.chapters.len(), 10_001);
    assert_eq!(book.toc.len(), 10_001);
    let last = books::chapter(&book, 10_000)?;
    assert!(last.html.contains("10000"));
    assert!(last.page.is_none());
    Ok(())
}

#[test]
fn oversized_epub_chapter_keeps_complete_lazy_source_pages() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("chapter.epub");
    let mut zip = zip::ZipWriter::new(File::create(&path)?);
    container(&mut zip)?;
    entry(
        &mut zip,
        "OPS/package.opf",
        r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Large</dc:title></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#,
    )?;
    let chapter = format!(
        "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>{}THE_END</p></body></html>",
        "x".repeat(limits::BOOK_CHAPTER_BYTES as usize + 1)
    );
    entry(&mut zip, "OPS/chapter.xhtml", &chapter)?;
    zip.finish()?;
    let book = books::read(&path)?;
    let content = books::chapter(&book, 0)?;
    let first = content
        .page
        .expect("large chapter must have source continuation");
    assert!(!first.total_known);
    let last = first.source.read(chapter.len() as u64 - 128, false)?;
    assert!(last.total_known);
    assert_eq!(last.end, chapter.len() as u64);
    assert!(last.text.contains("THE_END"));
    Ok(())
}

#[test]
fn legacy_encoded_fb2_fallback_streams_without_losing_the_tail() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("legacy.FB2");
    let mut file = File::create(&path)?;
    file.write_all(b"<?xml version=\"1.0\" encoding = 'windows-1251'?><FictionBook><description><title-info><book-title>Large</book-title></title-info></description><body><section><p>")?;
    let block = vec![b'x'; limits::IO_BUFFER_BYTES];
    for _ in 0..(limits::BOOK_TEXT_BYTES / block.len() as u64 + 1) {
        file.write_all(&block)?;
    }
    file.write_all(&[0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2])?;
    file.write_all(b"</p></section></body></FictionBook>")?;
    drop(file);
    let book = books::read(&path)?;
    let content = books::chapter(&book, 0)?;
    let first = content.page.expect("large FB2 must retain source access");
    let last = first.source.read(first.total - 128, false)?;
    assert!(last.text.contains("Привет"));
    assert!(last.text.ends_with("</FictionBook>"));
    Ok(())
}

#[test]
fn coordinate_pages_preserve_continuation_cdata_and_character_references() -> Result<()> {
    let cancel = Cancellation::default();
    let points = "0,0,0 ".repeat(limits::GEO_POINTS + 1);
    let xml =
        format!("<kml><coordinates>{points}1&#46;5,2,3 <![CDATA[4,5,6]]></coordinates></kml>");
    let first = geo::read_page(xml.as_bytes(), 0, &cancel)?;
    assert_eq!(first.rows.len(), limits::GEO_POINTS);
    assert!(first.more);
    let next = geo::read_page(xml.as_bytes(), first.rows.len(), &cancel)?;
    assert_eq!(next.rows.len(), 3);
    assert!(!next.more);
    assert_eq!(next.rows[1], ["1.5", "2", "3"]);
    assert_eq!(next.rows[2], ["4", "5", "6"]);
    let gpx = "<gpx><wpt lat=\"39.9\" lon=\"116.4\"><ele><![CDATA[1]]>&#50;</ele></wpt></gpx>";
    assert_eq!(
        geo::read_page(gpx.as_bytes(), 0, &cancel)?.rows[0],
        ["116.4", "39.9", "12"]
    );
    let (stopped, guard) = Cancellation::task();
    drop(guard);
    assert!(geo::read_page(xml.as_bytes(), 0, &stopped).is_err());
    Ok(())
}

#[test]
fn numpy_parses_fields_after_the_old_header_prefix() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("header.npy");
    let mut header = format!(
        "{{{}'descr': '<f4', 'shape': (2,), 'fortran_order': False, }}",
        " ".repeat(limits::NUMPY_HEADER_BYTES + 8)
    );
    let padding = (64 - (12 + header.len() + 1) % 64) % 64;
    header.push_str(&" ".repeat(padding));
    header.push('\n');
    let mut file = File::create(&path)?;
    file.write_all(b"\x93NUMPY\x02\x00")?;
    file.write_all(&(header.len() as u32).to_le_bytes())?;
    file.write_all(header.as_bytes())?;
    file.write_all(&1.5_f32.to_le_bytes())?;
    file.write_all(&2.5_f32.to_le_bytes())?;
    drop(file);
    let report = formats::read_preview(&path)?;
    assert!(report.text.contains("[0] 1.5"));
    assert!(report.text.contains("[1] 2.5"));
    Ok(())
}

#[test]
fn a_large_tensor_record_exposes_the_full_header_without_weight_reads() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("metadata.safetensors");
    let mut file = File::create(&path)?;
    let block = vec![b'a'; limits::IO_BUFFER_BYTES];
    let blocks = limits::IO_RECORD_BYTES / block.len() + 1;
    let suffix = b"\":{\"dtype\":\"F32\",\"shape\":[0],\"data_offsets\":[0,0]}}";
    let length = 2 + blocks * block.len() + suffix.len();
    file.write_all(&(length as u64).to_le_bytes())?;
    file.write_all(b"{\"")?;
    for _ in 0..blocks {
        file.write_all(&block)?;
    }
    file.write_all(suffix)?;
    drop(file);
    let first = safetensors::read_preview(&path)?;
    assert!(first.text.starts_with("{\""));
    assert!(!first.total_known);
    let last = first.source.read(length as u64 - 128, false)?;
    assert_eq!(last.total, length as u64);
    assert!(last.total_known);
    assert!(last.text.ends_with("}}"));
    Ok(())
}

#[test]
fn editable_pages_save_atomically_and_cancel_without_mutating_the_file() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("large.txt");
    let block = b"entry\n".repeat(limits::IO_BUFFER_BYTES / 6);
    let mut file = File::create(&path)?;
    for _ in 0..(limits::TEXT_EDITABLE_BYTES / block.len() as u64 + 1) {
        file.write_all(&block)?;
    }
    drop(file);
    let page = paged::read_page(&path, limits::TEXT_PAGE_BYTES, false)?;
    assert!(page.total > limits::TEXT_EDITABLE_BYTES);
    assert!(page.source.is_editable());
    let changed = page.text.replacen("entry", "changed", 1);
    let edit = paged::PageEdit {
        path: path.clone(),
        start: page.start,
        end: page.end,
        total: page.total,
        modified: page.modified,
        identity: page.identity,
        expected: page.text.clone(),
    };
    let saved = paged::save_page(&edit, &changed)?;
    assert_eq!(saved.total, page.total + 2);
    let refresh = paged::PageRefresh {
        source: page.source.clone(),
        start: page.start,
        total: saved.total,
        modified: saved.modified,
        identity: saved.identity,
    };
    assert!(refresh.read(&Cancellation::default())?.is_none());
    assert!(paged::save_page(&edit, &changed).is_err());
    let current = paged::read_page(&path, 0, false)?;
    let edit = paged::PageEdit {
        path: path.clone(),
        start: current.start,
        end: current.end,
        total: current.total,
        modified: current.modified,
        identity: current.identity,
        expected: current.text,
    };
    let (cancel, guard) = Cancellation::task();
    drop(guard);
    assert!(paged::save_page_cancellable(&edit, "must not be written", &cancel).is_err());
    assert_eq!(fs::metadata(&path)?.len(), saved.total);
    let mut file = File::open(&path)?;
    file.seek(SeekFrom::Start(page.start))?;
    let mut sample = [0; 32];
    file.read_exact(&mut sample)?;
    assert!(String::from_utf8_lossy(&sample).contains("changed"));
    file.seek(SeekFrom::End(-6))?;
    let mut tail = [0; 6];
    file.read_exact(&mut tail)?;
    assert_eq!(&tail, b"entry\n");
    Ok(())
}

#[test]
fn wide_qoi_streams_to_a_preview_instead_of_rejecting_the_source_scanline() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("wide.qoi");
    let width = u32::try_from(limits::IMAGE_DECODED_BYTES / 8 + 1)?;
    let mut file = File::create(&path)?;
    file.write_all(b"qoif")?;
    file.write_all(&width.to_be_bytes())?;
    file.write_all(&1_u32.to_be_bytes())?;
    file.write_all(&[3, 0])?;
    let runs = usize::try_from(width / 62)?;
    file.write_all(&vec![0xfd; runs])?;
    let tail = width % 62;
    if tail > 0 {
        file.write_all(&[0xc0 | (tail as u8 - 1)])?;
    }
    file.write_all(&[0, 0, 0, 0, 0, 0, 0, 1])?;
    drop(file);
    let preview = media::read_image(&path)?;
    let image = image::load_from_memory(&preview.png)?;
    assert!(image.width() > 0 && image.width() <= limits::IMAGE_PREVIEW_EDGE);
    assert_eq!(image.height(), 1);
    assert_eq!(image.to_rgb8().get_pixel(0, 0).0, [0, 0, 0]);
    Ok(())
}

#[test]
fn executable_long_section_names_are_not_cut_at_the_library_cstring_window() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("long-name.elf");
    let name = "section_".to_owned() + &"n".repeat(6000);
    let names = format!("\0.shstrtab\0{name}\0").into_bytes();
    let mut header = vec![0; 256];
    header[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    header[16..18].copy_from_slice(&2_u16.to_le_bytes());
    header[18..20].copy_from_slice(&183_u16.to_le_bytes());
    header[20..24].copy_from_slice(&1_u32.to_le_bytes());
    header[40..48].copy_from_slice(&64_u64.to_le_bytes());
    header[52..54].copy_from_slice(&64_u16.to_le_bytes());
    header[54..56].copy_from_slice(&56_u16.to_le_bytes());
    header[58..60].copy_from_slice(&64_u16.to_le_bytes());
    header[60..62].copy_from_slice(&3_u16.to_le_bytes());
    header[62..64].copy_from_slice(&1_u16.to_le_bytes());
    header[128..132].copy_from_slice(&1_u32.to_le_bytes());
    header[132..136].copy_from_slice(&3_u32.to_le_bytes());
    header[152..160].copy_from_slice(&256_u64.to_le_bytes());
    header[160..168].copy_from_slice(&(names.len() as u64).to_le_bytes());
    header[176..184].copy_from_slice(&1_u64.to_le_bytes());
    header[192..196].copy_from_slice(&11_u32.to_le_bytes());
    header[196..200].copy_from_slice(&1_u32.to_le_bytes());
    header[216..224].copy_from_slice(&(256_u64 + names.len() as u64).to_le_bytes());
    header[240..248].copy_from_slice(&1_u64.to_le_bytes());
    let mut file = File::create(&path)?;
    file.write_all(&header)?;
    file.write_all(&names)?;
    file.set_len(2 * 1024 * 1024 * 1024)?;
    drop(file);
    let preview = glim_services::executables::read_preview(&path)?;
    assert!(preview.text.contains(&name));
    assert!(!preview.source.binary);
    assert_eq!(fs::metadata(&path)?.len(), 2 * 1024 * 1024 * 1024);
    Ok(())
}
