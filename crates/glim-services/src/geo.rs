//! Cancellable coordinate pages; neither point count nor file size limits opening.
use crate::{csv::TablePage, limits, work::Cancellation};
use anyhow::Result;
use quick_xml::{Reader, events::Event};
use std::io::{BufReader, Read};

fn point(token: &str) -> Option<Vec<String>> {
    let mut fields = token.split(',');
    let longitude = fields.next()?.parse::<f64>().ok()?;
    let latitude = fields.next()?.parse::<f64>().ok()?;
    let altitude = fields.next().map(str::parse::<f64>).transpose().ok()?;
    if fields.next().is_some()
        || !longitude.is_finite()
        || !latitude.is_finite()
        || altitude.is_some_and(|value| !value.is_finite())
    {
        return None;
    }
    Some(vec![
        longitude.to_string(),
        latitude.to_string(),
        altitude.map(|value| value.to_string()).unwrap_or_default(),
    ])
}
pub fn read_page(input: impl Read, start: usize, cancel: &Cancellation) -> Result<TablePage> {
    let input = crate::work::RecordReader::new(input, limits::IO_RECORD_BYTES, cancel);
    let mut reader = Reader::from_reader(BufReader::with_capacity(limits::IO_BUFFER_BYTES, input));
    let mut buffer = Vec::new();
    let mut rows = Vec::new();
    let (mut seen, mut bytes) = (0, 0);
    let mut in_coordinates = false;
    let mut token = String::new();
    let mut pending: Option<Vec<String>> = None;
    let mut elevation = false;
    let mut emit = |row: Vec<String>| -> Result<bool> {
        if seen >= start {
            let length = row.iter().map(String::len).sum::<usize>();
            if !rows.is_empty()
                && (rows.len() == limits::GEO_POINTS || bytes + length > limits::CSV_INPUT_BYTES)
            {
                return Ok(true);
            }
            limits::preview_budget(
                length <= limits::CSV_INPUT_BYTES,
                "Coordinate record exceeds table working memory; complete XML remains in Source",
            )?;
            bytes += length;
            rows.push(row);
        }
        seen += 1;
        Ok(false)
    };
    loop {
        cancel.check()?;
        buffer.clear();
        reader.get_mut().get_mut().reset(limits::IO_RECORD_BYTES);
        let event = reader.read_event_into(&mut buffer)?;
        if (in_coordinates || elevation)
            && let Some(text) = crate::xml::event_text(&event)?
        {
            if in_coordinates {
                for (position, ch) in text.chars().enumerate() {
                    if position % limits::IO_BUFFER_BYTES == 0 {
                        cancel.check()?;
                    }
                    if ch.is_whitespace() {
                        if let Some(row) = point(&token)
                            && emit(row)?
                        {
                            return Ok(TablePage {
                                rows,
                                columns: 3,
                                more: true,
                            });
                        }
                        token.clear();
                    } else {
                        limits::preview_budget(
                            token.len() + ch.len_utf8() <= limits::CSV_INPUT_BYTES,
                            "Coordinate token exceeds table working memory; complete XML remains in Source",
                        )?;
                        token.push(ch);
                    }
                }
            } else if let Some(row) = &mut pending {
                limits::preview_budget(
                    row[2].len().saturating_add(text.len()) <= limits::CSV_INPUT_BYTES,
                    "Elevation exceeds table working memory; complete XML remains in Source",
                )?;
                row[2].push_str(&text);
            }
            continue;
        }
        match event {
            Event::Start(ref tag) | Event::Empty(ref tag) => {
                let empty = matches!(event, Event::Empty(_));
                match tag.local_name().as_ref() {
                    b"coordinates" => {
                        in_coordinates = !empty;
                        token.clear();
                    }
                    b"ele" => elevation = !empty,
                    b"wpt" | b"trkpt" | b"rtept" => {
                        let (mut longitude, mut latitude) = (None, None);
                        for attribute in tag.attributes() {
                            let attribute = attribute?;
                            let value = attribute.decode_and_unescape_value(reader.decoder())?;
                            let number = value
                                .parse::<f64>()
                                .ok()
                                .filter(|number| number.is_finite());
                            match attribute.key.local_name().as_ref() {
                                b"lon" => longitude = number,
                                b"lat" => latitude = number,
                                _ => {}
                            }
                        }
                        if let (Some(longitude), Some(latitude)) = (longitude, latitude) {
                            let row =
                                vec![longitude.to_string(), latitude.to_string(), String::new()];
                            if empty {
                                if emit(row)? {
                                    return Ok(TablePage {
                                        rows,
                                        columns: 3,
                                        more: true,
                                    });
                                }
                            } else {
                                pending = Some(row);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::End(tag) => match tag.local_name().as_ref() {
                b"coordinates" => {
                    if let Some(row) = point(&token)
                        && emit(row)?
                    {
                        return Ok(TablePage {
                            rows,
                            columns: 3,
                            more: true,
                        });
                    }
                    token.clear();
                    in_coordinates = false;
                }
                b"ele" => elevation = false,
                b"wpt" | b"trkpt" | b"rtept" => {
                    if let Some(row) = pending.take()
                        && emit(row)?
                    {
                        return Ok(TablePage {
                            rows,
                            columns: 3,
                            more: true,
                        });
                    }
                }
                _ => {}
            },
            Event::DocType(_) => anyhow::bail!("Geographic XML document types are unsupported"),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(TablePage {
        rows,
        columns: 3,
        more: false,
    })
}
