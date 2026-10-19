//! Conservative first-pass document projection. Original text stays editable.
use std::path::Path;
pub fn supports(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.to_ascii_lowercase().ends_with(".vega.json"))
        || matches!(
            extension(path).as_str(),
            "adoc" | "org" | "rst" | "srt" | "vtt" | "gpx" | "kml" | "d2" | "pikchr"
        )
}
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}
pub fn markdown(path: &Path, text: &str) -> String {
    let ext = extension(path);
    if matches!(ext.as_str(), "d2" | "pikchr")
        || path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_ascii_lowercase().ends_with(".vega.json"))
    {
        let kind = if ext == "json" { "vega" } else { &ext };
        let fence = "`".repeat(
            text.lines()
                .map(|line| line.chars().take_while(|c| *c == '`').count())
                .max()
                .unwrap_or(0)
                .max(2)
                + 1,
        );
        return format!("{fence}{kind}\n{text}\n{fence}\n");
    }
    let mut output = String::new();
    let lines: Vec<_> = text.lines().collect();
    let mut code = false;
    for (index, line) in lines.iter().enumerate() {
        if ext == "adoc" && matches!(*line, "----" | "....") {
            code = !code;
            output.push_str("```\n");
        } else if code {
            output.push_str(line);
            output.push('\n');
        } else if ext == "adoc" && line.starts_with("[source,") {
        } else if ext == "org" && line.starts_with("#+TITLE:") {
            output.push_str(&format!(
                "# {}\n",
                line.trim_start_matches("#+TITLE:").trim()
            ));
        } else if (ext == "adoc" && line.starts_with('='))
            || (ext == "org" && line.starts_with('*'))
        {
            let marker = if ext == "org" { '*' } else { '=' };
            let count = line.chars().take_while(|c| *c == marker).count();
            if line.as_bytes().get(count) == Some(&b' ') {
                output.push_str(&format!(
                    "{} {}\n",
                    "#".repeat(count.min(6)),
                    line[count..].trim()
                ));
            } else {
                output.push_str(line);
                output.push('\n');
            }
        } else if ext == "rst"
            && lines.get(index + 1).is_some_and(|next| {
                next.len() >= line.len()
                    && !line.is_empty()
                    && next.chars().all(|c| c == '=' || c == '-')
            })
        {
            output.push_str(&format!("## {line}\n"));
        } else if ext == "rst" && !line.is_empty() && line.chars().all(|c| c == '=' || c == '-') {
        } else if matches!(ext.as_str(), "srt" | "vtt") && line.contains("-->") {
            output.push_str(&format!("\n### `{line}`\n\n"));
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    if code {
        output.push_str("```\n");
    }
    if matches!(ext.as_str(), "gpx" | "kml") {
        // XML is shown literally, never as active HTML or a remote map.
        output = format!(
            "# {}\n\n{}\n\n{}\n\n```xml\n{text}\n```\n",
            ext.to_uppercase(),
            crate::locale::tr("Geographic data · Offline coordinate preview"),
            coordinates(text)
        );
    }
    output
}

fn coordinates(text: &str) -> String {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut output = format!(
        "| {} | {} | {} |\n| --- | --- | --- |\n",
        crate::locale::tr("Longitude"),
        crate::locale::tr("Latitude"),
        crate::locale::tr("Altitude")
    );
    let mut in_coordinates = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(tag) | Event::Empty(tag)) => {
                if tag.local_name().as_ref() == b"coordinates" {
                    in_coordinates = true;
                }
                if matches!(tag.local_name().as_ref(), b"wpt" | b"trkpt" | b"rtept") {
                    let mut lat = String::new();
                    let mut lon = String::new();
                    for attr in tag.attributes().flatten() {
                        let value = String::from_utf8_lossy(&attr.value).into_owned();
                        match attr.key.as_ref() {
                            b"lat" => lat = value,
                            b"lon" => lon = value,
                            _ => {}
                        }
                    }
                    if let (Ok(lat), Ok(lon)) = (lat.parse::<f64>(), lon.parse::<f64>())
                        && lat.is_finite()
                        && lon.is_finite()
                    {
                        output.push_str(&format!("| {lon} | {lat} | |\n"));
                    }
                }
            }
            Ok(Event::Text(value)) if in_coordinates => {
                if let Ok(value) = value.decode() {
                    for point in value.split_whitespace() {
                        let values: Vec<_> = point
                            .split(',')
                            .filter_map(|v| v.parse::<f64>().ok())
                            .collect();
                        if values.len() >= 2 && values.iter().all(|v| v.is_finite()) {
                            output.push_str(&format!(
                                "| {} | {} | {} |\n",
                                values[0],
                                values[1],
                                values.get(2).copied().unwrap_or(0.0)
                            ));
                        }
                    }
                }
            }
            Ok(Event::End(tag)) if tag.local_name().as_ref() == b"coordinates" => {
                in_coordinates = false
            }
            Ok(Event::Eof) => break,
            Err(error) => return format!("{}: {error}", crate::locale::tr("XML parsing failed")),
            _ => {}
        }
    }
    output
}
