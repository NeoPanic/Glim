//! Stream XLSX/XLSM cells into a disk-backed index; legacy XLS uses native Quick Look.
//! Stored formula results are displayed without evaluating workbook code.
mod pages;
use crate::{limits, work::Cancellation};
use anyhow::{Context, Result, ensure};
pub use pages::render_page;
use quick_xml::{Reader, events::Event};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

pub fn supports(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "xls" | "xlsx" | "xlsm"))
}

pub struct Spreadsheet {
    pub database: PathBuf,
    pub cache: PathBuf,
    pub information: String,
    _directory: tempfile::TempDir,
}
fn connection(path: &Path) -> Result<Connection> {
    let db = Connection::open(path)?;
    db.pragma_update(None, "cache_size", -limits::SQLITE_CACHE_KIB)?;
    db.pragma_update(None, "temp_store", "FILE")?;
    db.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;")?;
    Ok(db)
}
fn scan(
    archive: &mut zip::ZipArchive<File>,
    name: &str,
    cancel: &Cancellation,
    visit: impl FnMut(&Event<'_>, quick_xml::encoding::Decoder) -> Result<()>,
) -> Result<()> {
    let entry = archive
        .by_name(name)
        .with_context(|| format!("Missing worksheet resource: {name}"))?;
    scan_reader(entry, cancel, visit)
}
fn scan_reader(
    input: impl std::io::Read,
    cancel: &Cancellation,
    mut visit: impl FnMut(&Event<'_>, quick_xml::encoding::Decoder) -> Result<()>,
) -> Result<()> {
    let input = crate::work::RecordReader::new(input, limits::IO_RECORD_BYTES, cancel);
    let mut reader = Reader::from_reader(BufReader::with_capacity(limits::IO_BUFFER_BYTES, input));
    let mut buffer = Vec::new();
    loop {
        cancel.check()?;
        buffer.clear();
        reader.get_mut().get_mut().reset(limits::IO_RECORD_BYTES);
        let event = reader.read_event_into(&mut buffer)?;
        if matches!(event, Event::DocType(_)) {
            anyhow::bail!("Spreadsheet external entities are unsupported");
        }
        if matches!(event, Event::Eof) {
            break;
        }
        visit(&event, reader.decoder())?;
    }
    Ok(())
}
fn attributes(
    tag: &quick_xml::events::BytesStart<'_>,
    decoder: quick_xml::encoding::Decoder,
) -> Result<Vec<(String, String)>> {
    tag.attributes()
        .map(|attribute| {
            let attribute = attribute?;
            Ok((
                String::from_utf8_lossy(attribute.key.local_name().as_ref()).into_owned(),
                attribute.decode_and_unescape_value(decoder)?.into_owned(),
            ))
        })
        .collect()
}
fn attr<'a>(attributes: &'a [(String, String)], name: &str) -> &'a str {
    attributes
        .iter()
        .find(|(key, _)| key == name)
        .map_or("", |(_, value)| value)
}
fn append(value: &mut String, text: &str) -> Result<()> {
    limits::preview_budget(
        value.len().saturating_add(text.len()) <= limits::IO_RECORD_BYTES,
        "Active spreadsheet value exceeds structured preview memory; use the native document preview",
    )?;
    value.push_str(text);
    Ok(())
}
pub fn prepare(path: &Path, cancel: &Cancellation) -> Result<Option<Spreadsheet>> {
    match prepare_index(path, cancel) {
        Err(error) => {
            cancel.check()?;
            log::warn!("XLSX index unavailable; using native preview: {error:#}");
            Ok(None)
        }
        result => result,
    }
}
fn prepare_index(path: &Path, cancel: &Cancellation) -> Result<Option<Spreadsheet>> {
    if !path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("xlsx") || extension.eq_ignore_ascii_case("xlsm")
    }) {
        return Ok(None);
    }
    let before = std::fs::metadata(path)?;
    let mut archive = zip::ZipArchive::new(File::open(path)?)?;
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("worksheets.sqlite");
    let output = connection(&database)?;
    let cache_path = directory.path().join("strings.sqlite");
    let cache = connection(&cache_path)?;
    cache.execute_batch("CREATE TABLE strings (id INTEGER PRIMARY KEY, value TEXT); CREATE TABLE relationships(id TEXT PRIMARY KEY, target TEXT); CREATE TABLE sheets(id INTEGER PRIMARY KEY, name TEXT, reference TEXT, source TEXT, max_row INTEGER DEFAULT 0, max_column INTEGER DEFAULT 0);")?;
    pages::initialize(&cache)?;
    pages::resources(&mut archive, directory.path(), &cache, cancel)?;
    cache.execute_batch("BEGIN")?;
    if archive
        .file_names()
        .any(|name| name == "xl/sharedStrings.xml")
    {
        let (mut value, mut id, mut in_text) = (String::new(), 0_i64, false);
        let mut phonetic = 0_usize;
        scan(&mut archive, "xl/sharedStrings.xml", cancel, |event, _| {
            match event {
                Event::Start(tag) if tag.local_name().as_ref() == b"si" => value.clear(),
                Event::Start(tag) if tag.local_name().as_ref() == b"rPh" => phonetic += 1,
                Event::End(tag) if tag.local_name().as_ref() == b"rPh" => {
                    phonetic = phonetic.saturating_sub(1)
                }
                Event::Start(tag) if tag.local_name().as_ref() == b"t" => in_text = phonetic == 0,
                Event::End(tag) if tag.local_name().as_ref() == b"t" => in_text = false,
                event if in_text => {
                    if let Some(text) = crate::xml::event_text(event)? {
                        append(&mut value, &text)?;
                    }
                }
                Event::End(tag) if tag.local_name().as_ref() == b"si" => {
                    cache.execute("INSERT INTO strings VALUES (?1, ?2)", params![id, value])?;
                    id += 1;
                }
                Event::Empty(tag) if tag.local_name().as_ref() == b"si" => {
                    cache.execute("INSERT INTO strings VALUES (?1, '')", [id])?;
                    id += 1;
                }
                _ => {}
            }
            Ok(())
        })?;
    }
    scan(
        &mut archive,
        "xl/_rels/workbook.xml.rels",
        cancel,
        |event, decoder| {
            if let Event::Start(tag) | Event::Empty(tag) = event
                && tag.local_name().as_ref() == b"Relationship"
            {
                let a = attributes(tag, decoder)?;
                if attr(&a, "TargetMode") != "External" {
                    cache.execute(
                        "INSERT INTO relationships VALUES (?1, ?2)",
                        params![attr(&a, "Id"), attr(&a, "Target")],
                    )?;
                }
            }
            Ok(())
        },
    )?;
    scan(&mut archive, "xl/workbook.xml", cancel, |event, decoder| {
        if let Event::Start(tag) | Event::Empty(tag) = event
            && tag.local_name().as_ref() == b"workbookPr"
        {
            let a = attributes(tag, decoder)?;
            cache.execute(
                "INSERT OR REPLACE INTO settings VALUES ('date1904',?1)",
                [if matches!(attr(&a, "date1904"), "1" | "true") {
                    "1"
                } else {
                    "0"
                }],
            )?;
        }
        if let Event::Start(tag) | Event::Empty(tag) = event
            && tag.local_name().as_ref() == b"sheet"
        {
            let a = attributes(tag, decoder)?;
            cache.execute(
                "INSERT INTO sheets(name,reference) VALUES (?1, ?2)",
                params![attr(&a, "name"), attr(&a, "id")],
            )?;
        }
        Ok(())
    })?;
    cache.execute_batch("COMMIT")?;
    let mut count = 0;
    let mut last_sheet = 0_i64;
    cache.execute_batch("BEGIN")?;
    output.execute_batch("BEGIN")?;
    loop {
        cancel.check()?;
        let next: Option<(i64,String,String)> = cache.query_row(
            "SELECT s.id,s.name,r.target FROM sheets s JOIN relationships r ON r.id=s.reference WHERE s.id>?1 ORDER BY s.id LIMIT 1",
            [last_sheet], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))
        ).optional()?;
        let Some((id, name, target)) = next else {
            break;
        };
        last_sheet = id;
        let member =
            crate::books::resolve("xl/workbook.xml", &target).context("Invalid worksheet path")?;
        let source = pages::index_sheet(
            &mut archive,
            member.trim_start_matches('/'),
            id,
            directory.path(),
            &cache,
            cancel,
        )?;
        let table = format!("{id}: {name}");
        let quoted = format!("\"{}\"", table.replace('"', "\"\""));
        let fields = [
            crate::locale::tr("Cell"),
            crate::locale::tr("Value"),
            crate::locale::tr("Formula"),
        ]
        .into_iter()
        .map(|name| format!("\"{}\" TEXT", name.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",");
        output.execute_batch(&format!("CREATE TABLE {quoted} ({fields})"))?;
        let mut insert = output.prepare(&format!("INSERT INTO {quoted} VALUES (?1,?2,?3)"))?;
        let (mut cell, mut kind, mut value, mut formula, mut field) = (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        );
        let mut in_cell = false;
        let (mut row_number, mut column_number) = (0_u64, 0_u64);
        let mut phonetic = 0_usize;
        scan_reader(File::open(source)?, cancel, |event, decoder| {
            match event {
                Event::Start(tag) | Event::Empty(tag) if tag.local_name().as_ref() == b"row" => {
                    let a = attributes(tag, decoder)?;
                    row_number = attr(&a, "r")
                        .parse()
                        .unwrap_or_else(|_| row_number.saturating_add(1));
                    column_number = 0;
                }
                Event::Empty(tag) if tag.local_name().as_ref() == b"c" => {
                    let a = attributes(tag, decoder)?;
                    let address =
                        cell_address(attr(&a, "r"), row_number.max(1), &mut column_number);
                    insert.execute(params![address, "", ""])?;
                }
                Event::Start(tag) if tag.local_name().as_ref() == b"c" => {
                    let a = attributes(tag, decoder)?;
                    cell = cell_address(attr(&a, "r"), row_number.max(1), &mut column_number);
                    kind = attr(&a, "t").to_owned();
                    value.clear();
                    formula.clear();
                    field.clear();
                    in_cell = true;
                }
                Event::Start(tag) if tag.local_name().as_ref() == b"rPh" => phonetic += 1,
                Event::End(tag) if tag.local_name().as_ref() == b"rPh" => {
                    phonetic = phonetic.saturating_sub(1)
                }
                Event::Start(tag)
                    if in_cell
                        && phonetic == 0
                        && matches!(tag.local_name().as_ref(), b"v" | b"t" | b"f") =>
                {
                    field = String::from_utf8_lossy(tag.local_name().as_ref()).into_owned()
                }
                Event::End(tag) if matches!(tag.local_name().as_ref(), b"v" | b"t" | b"f") => {
                    field.clear()
                }
                event @ (Event::Text(_) | Event::CData(_) | Event::GeneralRef(_))
                    if in_cell && phonetic == 0 && !field.is_empty() =>
                {
                    let text = crate::xml::event_text(event)?.unwrap_or_default();
                    append(
                        if field == "f" {
                            &mut formula
                        } else {
                            &mut value
                        },
                        &text,
                    )?;
                }
                Event::End(tag) if tag.local_name().as_ref() == b"c" => {
                    if kind == "s" && !value.is_empty() {
                        value = cache.query_row(
                            "SELECT value FROM strings WHERE id=?1",
                            [value.parse::<i64>()?],
                            |row| row.get(0),
                        )?;
                    }
                    insert.execute(params![cell, value, formula])?;
                    in_cell = false;
                }
                _ => {}
            }
            Ok(())
        })?;
        count += 1;
    }
    ensure!(count > 0, "Spreadsheet contains no supported worksheets");
    output.execute_batch("COMMIT")?;
    cache.execute_batch("COMMIT")?;
    drop(output);
    let after = std::fs::metadata(path)?;
    ensure!(
        (before.len(), before.modified().ok()) == (after.len(), after.modified().ok()),
        "Spreadsheet changed; reopen it"
    );
    Ok(Some(Spreadsheet {
        database,
        cache: cache_path,
        information: crate::locale::tr(
            "Read-only worksheet cells · Saved formula results · Select a worksheet, page through cells, or double-click a value to read it in full.",
        ),
        _directory: directory,
    }))
}

fn cell_address(explicit: &str, row: u64, column: &mut u64) -> String {
    if !explicit.is_empty() {
        let index = explicit
            .bytes()
            .take_while(u8::is_ascii_alphabetic)
            .try_fold(0_u64, |value, byte| {
                value
                    .checked_mul(26)?
                    .checked_add(u64::from(byte.to_ascii_uppercase() - b'A' + 1))
            });
        *column = index
            .filter(|index| *index > 0)
            .unwrap_or_else(|| column.saturating_add(1));
        return explicit.to_owned();
    }
    let mut index = *column;
    *column = column.saturating_add(1);
    let mut letters = Vec::new();
    loop {
        letters.push(char::from(b'A' + (index % 26) as u8));
        index /= 26;
        if index == 0 {
            break;
        }
        index -= 1;
    }
    format!("{}{row}", letters.into_iter().rev().collect::<String>())
}
