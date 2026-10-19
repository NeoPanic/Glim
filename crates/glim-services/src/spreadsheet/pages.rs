//! Disk-backed worksheet source positions and bounded, styled presentation pages.
use super::{attr, attributes};
use crate::{limits, work::Cancellation};
use anyhow::{Context, Result, ensure};
use quick_xml::{
    Reader, Writer,
    events::{BytesEnd, BytesStart, BytesText, Event},
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub(super) fn initialize(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE source_rows(sheet INTEGER,r INTEGER,start INTEGER,length INTEGER,PRIMARY KEY(sheet,r)) WITHOUT ROWID;
        CREATE TABLE source_cells(sheet INTEGER,r INTEGER,c INTEGER,start INTEGER,length INTEGER,PRIMARY KEY(sheet,r,c)) WITHOUT ROWID;
        CREATE TABLE source_parts(sheet INTEGER,kind TEXT,start INTEGER,length INTEGER,PRIMARY KEY(sheet,kind)) WITHOUT ROWID;
        CREATE TABLE source_merges(sheet INTEGER,r1 INTEGER,c1 INTEGER,r2 INTEGER,c2 INTEGER);
        CREATE INDEX source_merge_range ON source_merges(sheet,r1,r2);
        CREATE TABLE resources(name TEXT PRIMARY KEY,path TEXT);
        CREATE TABLE settings(name TEXT PRIMARY KEY,value TEXT);")?;
    Ok(())
}
pub(super) fn resources(
    archive: &mut zip::ZipArchive<File>,
    directory: &Path,
    db: &Connection,
    cancel: &Cancellation,
) -> Result<()> {
    for (name, file) in [
        ("styles", "xl/styles.xml"),
        ("theme", "xl/theme/theme1.xml"),
    ] {
        if let Ok(input) = archive.by_name(file) {
            let path = directory.join(format!("{name}.xml"));
            cancel.copy(input, File::create(&path)?)?;
            db.execute(
                "INSERT INTO resources VALUES (?1,?2)",
                params![name, path.to_str()],
            )?;
        }
    }
    Ok(())
}
pub(super) fn index_sheet(
    archive: &mut zip::ZipArchive<File>,
    member: &str,
    sheet: i64,
    directory: &Path,
    db: &Connection,
    cancel: &Cancellation,
) -> Result<PathBuf> {
    let path = directory.join(format!("sheet-{sheet}.xml"));
    cancel.copy(archive.by_name(member)?, File::create(&path)?)?;
    let input = crate::work::RecordReader::new(File::open(&path)?, limits::IO_RECORD_BYTES, cancel);
    let mut reader = Reader::from_reader(BufReader::with_capacity(limits::IO_BUFFER_BYTES, input));
    let mut buffer = Vec::new();
    let (mut row, mut column, mut max_row, mut max_column) = (0_u64, 0_u64, 0_u64, 0_u64);
    let (mut row_start, mut cell_start) = (0_u64, 0_u64);
    let mut part: Option<(String, u64)> = None;
    loop {
        cancel.check()?;
        reader.get_mut().get_mut().reset(limits::IO_RECORD_BYTES);
        let start = reader.buffer_position();
        let event = reader.read_event_into(&mut buffer)?;
        let end = reader.buffer_position();
        match &event {
            Event::Start(tag) | Event::Empty(tag) => {
                let empty = matches!(event, Event::Empty(_));
                let name = tag.local_name();
                let a = attributes(tag, reader.decoder())?;
                match name.as_ref() {
                    b"worksheet" => {
                        db.execute(
                            "INSERT INTO source_parts VALUES (?1,'root',?2,?3)",
                            params![sheet, start, end - start],
                        )?;
                    }
                    b"sheetFormatPr" | b"pageMargins" if !empty => {
                        part = Some((String::from_utf8_lossy(name.as_ref()).into_owned(), start));
                    }
                    b"sheetFormatPr" | b"pageMargins" => {
                        db.execute(
                            "INSERT INTO source_parts VALUES (?1,?2,?3,?4)",
                            params![
                                sheet,
                                String::from_utf8_lossy(name.as_ref()),
                                start,
                                end - start
                            ],
                        )?;
                    }
                    b"cols" if !empty => part = Some(("cols".into(), start)),
                    b"row" => {
                        row = attr(&a, "r").parse().unwrap_or(row.saturating_add(1));
                        column = 0;
                        row_start = start;
                        max_row = max_row.max(row);
                        if empty {
                            db.execute(
                                "INSERT INTO source_rows VALUES (?1,?2,?3,?4)",
                                params![sheet, row, start, end - start],
                            )?;
                        }
                    }
                    b"c" => {
                        if let Some((r, c)) = coordinate(attr(&a, "r")) {
                            row = r;
                            column = c;
                        } else {
                            column = column.saturating_add(1);
                        }
                        max_row = max_row.max(row);
                        max_column = max_column.max(column);
                        cell_start = start;
                        if empty {
                            db.execute(
                                "INSERT INTO source_cells VALUES (?1,?2,?3,?4,?5)",
                                params![sheet, row, column, start, end - start],
                            )?;
                        }
                    }
                    b"mergeCell" => {
                        let value = attr(&a, "ref");
                        let (first, last) = value.split_once(':').unwrap_or((value, value));
                        if let (Some((r1, c1)), Some((r2, c2))) =
                            (coordinate(first), coordinate(last))
                        {
                            db.execute(
                                "INSERT INTO source_merges VALUES (?1,?2,?3,?4,?5)",
                                params![sheet, r1, c1, r2, c2],
                            )?;
                            max_row = max_row.max(r2);
                            max_column = max_column.max(c2);
                        }
                    }
                    _ => {}
                }
            }
            Event::End(tag) => match tag.local_name().as_ref() {
                b"row" => {
                    db.execute(
                        "INSERT INTO source_rows VALUES (?1,?2,?3,?4)",
                        params![sheet, row, row_start, end - row_start],
                    )?;
                }
                b"c" => {
                    db.execute(
                        "INSERT INTO source_cells VALUES (?1,?2,?3,?4,?5)",
                        params![sheet, row, column, cell_start, end - cell_start],
                    )?;
                }
                b"cols" | b"sheetFormatPr" | b"pageMargins" => {
                    if let Some((kind, begin)) = part.take() {
                        db.execute(
                            "INSERT INTO source_parts VALUES (?1,?2,?3,?4)",
                            params![sheet, kind, begin, end - begin],
                        )?;
                    }
                }
                _ => {}
            },
            Event::DocType(_) => anyhow::bail!("Spreadsheet external entities are unsupported"),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    db.execute(
        "UPDATE sheets SET source=?1,max_row=?2,max_column=?3 WHERE id=?4",
        params![path.to_str(), max_row, max_column, sheet],
    )?;
    Ok(path)
}
fn coordinate(value: &str) -> Option<(u64, u64)> {
    let value = value.trim_start_matches('$');
    let mut column = 0_u64;
    let mut digits = 0;
    for byte in value.bytes() {
        if byte.is_ascii_alphabetic() {
            column = column
                .checked_mul(26)?
                .checked_add(u64::from(byte.to_ascii_uppercase() - b'A' + 1))?;
            digits += 1;
        } else {
            break;
        }
    }
    let row = value.get(digits..)?.trim_start_matches('$').parse().ok()?;
    (row > 0 && column > 0).then_some((row, column))
}
fn address(row: u64, column: u64) -> String {
    let mut n = column;
    let mut letters = Vec::new();
    while n > 0 {
        n -= 1;
        letters.push((b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    format!("{}{row}", letters.iter().rev().collect::<String>())
}
fn fragment(
    path: &Path,
    start: u64,
    length: u64,
    cancel: &Cancellation,
) -> Result<Reader<BufReader<crate::work::RecordReader<std::io::Take<File>>>>> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    Ok(Reader::from_reader(BufReader::with_capacity(
        limits::IO_BUFFER_BYTES,
        crate::work::RecordReader::new(file.take(length), limits::IO_RECORD_BYTES, cancel),
    )))
}
fn start_tag(
    path: &Path,
    start: u64,
    length: u64,
    cancel: &Cancellation,
) -> Result<BytesStart<'static>> {
    let mut reader = fragment(path, start, length, cancel)?;
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(tag) | Event::Empty(tag) => return Ok(tag.into_owned()),
            Event::Eof => anyhow::bail!("Missing worksheet tag"),
            _ => {}
        }
        buffer.clear();
    }
}
fn rewrite(
    tag: &BytesStart<'_>,
    decoder: quick_xml::encoding::Decoder,
    changed: &[(&str, String)],
    removed: &[&str],
) -> Result<BytesStart<'static>> {
    let name = String::from_utf8(tag.name().as_ref().to_vec())?;
    let mut output = BytesStart::new(name);
    for a in tag.attributes() {
        let a = a?;
        let key = std::str::from_utf8(a.key.as_ref())?;
        if removed.contains(&key) || changed.iter().any(|(k, _)| *k == key) {
            continue;
        }
        let value = a.decode_and_unescape_value(decoder)?;
        output.push_attribute((key, value.as_ref()));
    }
    for (key, value) in changed {
        output.push_attribute((*key, value.as_str()));
    }
    Ok(output)
}
fn part(db: &Connection, sheet: i64, kind: &str) -> Result<Option<(u64, u64)>> {
    Ok(db
        .query_row(
            "SELECT start,length FROM source_parts WHERE sheet=?1 AND kind=?2",
            params![sheet, kind],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}
fn copy_part(
    output: &mut impl Write,
    path: &Path,
    range: (u64, u64),
    cancel: &Cancellation,
) -> Result<()> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(range.0))?;
    cancel.copy(file.take(range.1), output)?;
    Ok(())
}
struct PageWriter<W> {
    inner: W,
    written: u64,
}
impl<W: Write> Write for PageWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.written.saturating_add(bytes.len() as u64) > limits::IO_CACHE_BYTES {
            return Err(std::io::Error::other(limits::PreviewBudgetExceeded(
                "Active worksheet preview exceeds working budget; use Cell data",
            )));
        }
        let count = self.inner.write(bytes)?;
        self.written += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
fn render_cell(
    output: &mut Writer<impl Write>,
    db: &Connection,
    path: &Path,
    start: u64,
    length: u64,
    position: (u64, u64),
    cancel: &Cancellation,
) -> Result<()> {
    let (row, column) = position;
    let mut reader = fragment(path, start, length, cancel)?;
    let mut buffer = Vec::new();
    let mut shared = false;
    let mut skip = 0;
    let mut string_id = String::new();
    let mut in_value = false;
    loop {
        cancel.check()?;
        reader.get_mut().get_mut().reset(limits::IO_RECORD_BYTES);
        let event = reader.read_event_into(&mut buffer)?;
        let empty_cell = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(tag) | Event::Empty(tag) if tag.local_name().as_ref() == b"c" => {
                let empty = empty_cell;
                let a = attributes(&tag, reader.decoder())?;
                shared = attr(&a, "t") == "s";
                let mut changed = vec![("r", address(row, column))];
                if shared {
                    changed.push(("t", "inlineStr".into()));
                }
                let tag = rewrite(&tag, reader.decoder(), &changed, &[])?;
                // Empty cells are detected from the original source marker.
                if empty {
                    output.write_event(Event::Empty(tag))?;
                } else {
                    output.write_event(Event::Start(tag))?;
                }
            }
            Event::Start(tag) if tag.local_name().as_ref() == b"f" => skip = 1,
            Event::Empty(tag) if tag.local_name().as_ref() == b"f" => {}
            Event::Start(tag) if skip > 0 => {
                let _ = tag;
                skip += 1;
            }
            Event::End(_) if skip > 0 => skip -= 1,
            Event::Start(tag) if shared && tag.local_name().as_ref() == b"v" => in_value = true,
            Event::End(tag) if shared && tag.local_name().as_ref() == b"v" => in_value = false,
            Event::End(tag) if tag.local_name().as_ref() == b"c" => {
                if shared && !string_id.is_empty() {
                    let value: String = db.query_row(
                        "SELECT value FROM strings WHERE id=?1",
                        [string_id.trim().parse::<i64>()?],
                        |r| r.get(0),
                    )?;
                    output.write_event(Event::Start(BytesStart::new("is")))?;
                    let mut text = BytesStart::new("t");
                    text.push_attribute(("xml:space", "preserve"));
                    output.write_event(Event::Start(text))?;
                    output.write_event(Event::Text(BytesText::new(&value)))?;
                    output.write_event(Event::End(BytesEnd::new("t")))?;
                    output.write_event(Event::End(BytesEnd::new("is")))?;
                }
                output.write_event(Event::End(tag))?;
            }
            Event::Text(_) | Event::CData(_) | Event::GeneralRef(_) if shared && in_value => {
                if let Some(text) = crate::xml::event_text(&event)? {
                    string_id.push_str(&text);
                }
            }
            Event::Eof => break,
            event if skip == 0 && !in_value => {
                output.write_event(event)?;
            }
            _ => {}
        }
        buffer.clear();
    }
    Ok(())
}

/// A presentation copy contains only one active row/column page. The source and
/// its complete disk-backed cell index remain unchanged; formulas use cached values.
pub fn render_page(
    cache: &Path,
    sheet: i64,
    row_start: u64,
    column_start: u64,
    destination: &Path,
    cancel: &Cancellation,
) -> Result<()> {
    let db = Connection::open_with_flags(cache, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.pragma_update(None, "cache_size", -limits::SQLITE_CACHE_KIB)?;
    let (source, name, rows, columns): (String, String, u64, u64) = db.query_row(
        "SELECT source,name,max_row,max_column FROM sheets WHERE id=?1",
        [sheet],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let source = Path::new(&source);
    let row_end = row_start
        .saturating_add(limits::SQLITE_PAGE_ROWS as u64)
        .min(rows.max(1));
    let column_end = column_start
        .saturating_add(limits::SQLITE_COLUMNS as u64)
        .min(columns.max(1));
    ensure!(
        row_start < row_end && column_start < column_end,
        "Worksheet page is outside its dimensions"
    );
    let mut cells: BTreeMap<(u64, u64), (u64, u64)> = BTreeMap::new();
    {
        let mut query=db.prepare("SELECT r,c,start,length FROM source_cells WHERE sheet=?1 AND r>?2 AND r<=?3 AND c>?4 AND c<=?5 ORDER BY r,c")?;
        let mut data = query.query(params![sheet, row_start, row_end, column_start, column_end])?;
        while let Some(r) = data.next()? {
            cancel.check()?;
            cells.insert((r.get(0)?, r.get(1)?), (r.get(2)?, r.get(3)?));
        }
    }
    let mut merges = Vec::new();
    {
        let mut query=db.prepare("SELECT r1,c1,r2,c2 FROM source_merges WHERE sheet=?1 AND r1<=?2 AND r2>?3 AND c1<=?4 AND c2>?5")?;
        let mut data = query.query(params![sheet, row_end, row_start, column_end, column_start])?;
        while let Some(r) = data.next()? {
            cancel.check()?;
            limits::preview_budget(
                merges.len() < limits::SQLITE_PAGE_ROWS.saturating_mul(limits::SQLITE_COLUMNS),
                "Worksheet merge layout exceeds active page budget; use Cell data",
            )?;
            let (r1, c1, r2, c2): (u64, u64, u64, u64) =
                (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
            let (top, left) = (r1.max(row_start + 1), c1.max(column_start + 1));
            if (top, left) != (r1, c1)
                && let Some(span) = db
                    .query_row(
                        "SELECT start,length FROM source_cells WHERE sheet=?1 AND r=?2 AND c=?3",
                        params![sheet, r1, c1],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?
            {
                cells.insert((top, left), span);
            }
            merges.push((top, left, r2.min(row_end), c2.min(column_end)));
        }
    }
    let active_bytes = cells.values().try_fold(0_u64, |sum, (_, size)| {
        sum.checked_add(*size)
            .context("Worksheet page size overflow")
    })?;
    limits::preview_budget(
        active_bytes <= limits::IO_CACHE_BYTES,
        "Worksheet cells exceed active preview budget; use Cell data for full values",
    )?;
    let mut xml = tempfile::NamedTempFile::new()?;
    {
        let mut output = Writer::new(BufWriter::with_capacity(
            limits::IO_BUFFER_BYTES,
            PageWriter {
                inner: xml.as_file_mut(),
                written: 0,
            },
        ));
        let (start, length) = part(&db, sheet, "root")?.context("Missing worksheet root")?;
        let root = start_tag(source, start, length, cancel)?;
        let root_name = String::from_utf8(root.name().as_ref().to_vec())?;
        let mut root = root;
        if !root
            .attributes()
            .any(|a| a.is_ok_and(|a| a.key.as_ref() == b"xmlns"))
        {
            root.push_attribute((
                "xmlns",
                "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
            ));
        }
        output.write_event(Event::Start(root))?;
        output.get_mut().write_all(
            format!(
                "<dimension ref=\"A1:{}\"/>",
                address(row_end - row_start, column_end - column_start)
            )
            .as_bytes(),
        )?;
        if let Some(range) = part(&db, sheet, "sheetFormatPr")? {
            copy_part(output.get_mut(), source, range, cancel)?;
        }
        if let Some((start, length)) = part(&db, sheet, "cols")? {
            let mut reader = fragment(source, start, length, cancel)?;
            let mut buffer = Vec::new();
            output.get_mut().write_all(b"<cols>")?;
            loop {
                reader.get_mut().get_mut().reset(limits::IO_RECORD_BYTES);
                let event = reader.read_event_into(&mut buffer)?;
                match event {
                    Event::Start(tag) | Event::Empty(tag)
                        if tag.local_name().as_ref() == b"col" =>
                    {
                        let a = attributes(&tag, reader.decoder())?;
                        let first = attr(&a, "min").parse::<u64>()?;
                        let last = attr(&a, "max").parse::<u64>()?;
                        if first <= column_end && last > column_start {
                            let tag = rewrite(
                                &tag,
                                reader.decoder(),
                                &[
                                    (
                                        "min",
                                        (first.max(column_start + 1) - column_start).to_string(),
                                    ),
                                    ("max", (last.min(column_end) - column_start).to_string()),
                                ],
                                &[],
                            )?;
                            output.write_event(Event::Empty(tag))?;
                        }
                    }
                    Event::Eof => break,
                    _ => {}
                }
                buffer.clear();
            }
            output.get_mut().write_all(b"</cols>")?;
        }
        output.get_mut().write_all(b"<sheetData>")?;
        for row in row_start + 1..=row_end {
            cancel.check()?;
            let range: Option<(u64, u64)> = db
                .query_row(
                    "SELECT start,length FROM source_rows WHERE sheet=?1 AND r=?2",
                    params![sheet, row],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let tag = if let Some((start, length)) = range {
                let tag = start_tag(source, start, length, cancel)?;
                rewrite(
                    &tag,
                    Reader::from_str("").decoder(),
                    &[("r", (row - row_start).to_string())],
                    &["spans"],
                )?
            } else {
                let mut tag = BytesStart::new("row");
                tag.push_attribute(("r", (row - row_start).to_string().as_str()));
                tag
            };
            let row_name = String::from_utf8(tag.name().as_ref().to_vec())?;
            output.write_event(Event::Start(tag))?;
            for (&(_, column), &(start, length)) in cells.range((row, 0)..=(row, u64::MAX)) {
                render_cell(
                    &mut output,
                    &db,
                    source,
                    start,
                    length,
                    (row - row_start, column - column_start),
                    cancel,
                )?;
            }
            output.write_event(Event::End(BytesEnd::new(row_name)))?;
        }
        output.get_mut().write_all(b"</sheetData>")?;
        if !merges.is_empty() {
            output
                .get_mut()
                .write_all(format!("<mergeCells count=\"{}\">", merges.len()).as_bytes())?;
            for (r1, c1, r2, c2) in merges {
                output.get_mut().write_all(
                    format!(
                        "<mergeCell ref=\"{}:{}\"/>",
                        address(r1 - row_start, c1 - column_start),
                        address(r2 - row_start, c2 - column_start)
                    )
                    .as_bytes(),
                )?;
            }
            output.get_mut().write_all(b"</mergeCells>")?;
        }
        if let Some(range) = part(&db, sheet, "pageMargins")? {
            copy_part(output.get_mut(), source, range, cancel)?;
        }
        output.write_event(Event::End(BytesEnd::new(root_name)))?;
        output.into_inner().flush()?;
    }
    limits::preview_budget(
        xml.as_file().metadata()?.len() <= limits::IO_CACHE_BYTES,
        "Active worksheet preview exceeds working budget; use Cell data",
    )?;
    let mut resources = Vec::new();
    let mut active_size = xml.as_file().metadata()?.len();
    for (name, target, kind) in [
        ("styles", "xl/styles.xml", "styles"),
        ("theme", "xl/theme/theme1.xml", "theme"),
    ] {
        if let Some(path) = db
            .query_row("SELECT path FROM resources WHERE name=?1", [name], |row| {
                row.get::<_, String>(0)
            })
            .optional()?
        {
            active_size = active_size.saturating_add(std::fs::metadata(&path)?.len());
            limits::preview_budget(
                active_size <= limits::IO_CACHE_BYTES,
                "Worksheet styling exceeds active preview budget; use Cell data",
            )?;
            resources.push((target, kind, path));
        }
    }
    let mut archive = zip::ZipWriter::new(File::create(destination)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    {
        let mut add = |name: &str, bytes: &[u8]| -> Result<()> {
            archive.start_file(name, options)?;
            archive.write_all(bytes)?;
            Ok(())
        };
        let mut types = String::from(
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
        );
        let mut relationships = String::from(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="sheet" Target="worksheets/sheet1.xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet"/>"#,
        );
        for (target, kind, _) in &resources {
            let content_type = if *kind == "styles" {
                "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"
            } else {
                "application/vnd.openxmlformats-officedocument.theme+xml"
            };
            types.push_str(&format!(
                r#"<Override PartName="/{target}" ContentType="{content_type}"/>"#
            ));
            relationships.push_str(&format!(r#"<Relationship Id="{kind}" Target="{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}"/>"#,target.trim_start_matches("xl/")));
        }
        types.push_str("</Types>");
        relationships.push_str("</Relationships>");
        add("[Content_Types].xml", types.as_bytes())?;
        add("_rels/.rels",br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="book" Target="xl/workbook.xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"/></Relationships>"#)?;
        let date: String = db
            .query_row(
                "SELECT value FROM settings WHERE name='date1904'",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or_else(|| "0".into());
        add("xl/workbook.xml",format!(r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><workbookPr date1904="{}"/><sheets><sheet name="{}" sheetId="1" r:id="sheet"/></sheets></workbook>"#,quick_xml::escape::escape(&date),quick_xml::escape::escape(&name)).as_bytes())?;
        add("xl/_rels/workbook.xml.rels", relationships.as_bytes())?;
    }
    archive.start_file("xl/worksheets/sheet1.xml", options)?;
    xml.as_file_mut().seek(SeekFrom::Start(0))?;
    cancel.copy(xml.as_file_mut(), &mut archive)?;
    for (target, _, path) in resources {
        archive.start_file(target, options)?;
        cancel.copy(File::open(path)?, &mut archive)?;
    }
    archive.finish()?;
    Ok(())
}
