//! Local draw.io XML decoding and basic SVG/raster rendering; no scripts or remote resources.
use crate::{limits, media::ImageDocument, work::Cancellation};
use anyhow::{Context, Result, ensure};
use base64::Engine;
use roxmltree::Node;
use std::{
    collections::HashMap,
    io::Read,
    path::Path,
    sync::{Arc, OnceLock},
};

type Point = (f64, f64);
#[derive(Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}
struct Cell<'a> {
    node: Node<'a, 'a>,
    geometry: Node<'a, 'a>,
    style: HashMap<&'a str, &'a str>,
    rect: Rect,
}
pub fn supports(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("drawio"))
}
fn number(node: Node<'_, '_>, name: &str, default: f64) -> f64 {
    node.attribute(name)
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite())
        .unwrap_or(default)
}
fn escape(text: &str) -> String {
    quick_xml::escape::escape(text).into_owned()
}
fn percent_decode(text: &[u8]) -> Result<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if text[index] == b'%' {
            ensure!(index + 2 < text.len(), "Invalid draw.io URL encoding");
            let value = std::str::from_utf8(&text[index + 1..index + 3])?;
            bytes.push(u8::from_str_radix(value, 16)?);
            index += 3;
        } else {
            bytes.push(text[index]);
            index += 1;
        }
    }
    Ok(String::from_utf8(bytes)?)
}
fn decode_page(node: Node<'_, '_>, cancel: &Cancellation) -> Result<String> {
    let text = node.text().unwrap_or("").trim();
    if text.starts_with('<') {
        return Ok(text.to_string());
    }
    let compressed = base64::engine::general_purpose::STANDARD.decode(
        text.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>(),
    )?;
    let decoder = flate2::read::DeflateDecoder::new(compressed.as_slice());
    let mut data = Vec::new();
    crate::work::Reader {
        inner: decoder,
        cancel: cancel.clone(),
    }
    .take(limits::DRAWIO_XML_BYTES as u64 + 1)
    .read_to_end(&mut data)?;
    ensure!(
        data.len() <= limits::DRAWIO_XML_BYTES,
        "Draw.io page exceeds preview XML budget"
    );
    percent_decode(&data)
}
fn label(cell: &Cell<'_>) -> String {
    let value = cell
        .node
        .attribute("value")
        .or_else(|| cell.node.parent().and_then(|p| p.attribute("label")))
        .unwrap_or("");
    if cell.style.get("html") != Some(&"1") {
        return value.to_string();
    }
    // Keep labels as plain SVG text: never execute or embed their HTML.
    let mut text = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    for ch in value.chars() {
        if ch == '<' {
            in_tag = true;
            tag.clear();
        } else if ch == '>' && in_tag {
            in_tag = false;
            let name = tag.trim().to_ascii_lowercase();
            if name.starts_with("br") || name == "/div" || name == "/p" {
                text.push('\n');
            }
        } else if in_tag {
            tag.push(ch);
        } else {
            text.push(ch);
        }
    }
    let text = text.replace("&nbsp;", " ");
    quick_xml::escape::unescape(&text)
        .map(|s| s.trim_end().to_string())
        .unwrap_or(text)
}
fn style<'a>(cell: &'a Cell<'_>, key: &str, default: &'a str) -> &'a str {
    cell.style.get(key).copied().unwrap_or(default)
}
fn numeric(cell: &Cell<'_>, key: &str, default: f64) -> f64 {
    cell.style
        .get(key)
        .and_then(|s| s.parse().ok())
        .filter(|n: &f64| n.is_finite())
        .unwrap_or(default)
}
fn offset(node: Node<'_, '_>, cells: &[Cell<'_>], ids: &HashMap<&str, usize>) -> Result<Point> {
    let mut parent = node.attribute("parent");
    let (mut x, mut y) = (0., 0.);
    for _ in 0..limits::DRAWIO_NESTING {
        let Some(cell) = parent
            .and_then(|id| ids.get(id))
            .map(|index| &cells[*index])
        else {
            return Ok((x, y));
        };
        x += cell.rect.x;
        y += cell.rect.y;
        parent = cell.node.attribute("parent");
    }
    anyhow::bail!("Cyclic or deeply nested draw.io groups")
}
fn text_svg(cell: &Cell<'_>, rect: Rect, value: &str, edge: bool) -> String {
    let font = numeric(cell, "fontSize", 12.).max(1.);
    let align = style(cell, "align", "center");
    let (x, anchor) = match align {
        "left" => (rect.x + 4., "start"),
        "right" => (rect.x + rect.w - 4., "end"),
        _ => (rect.x + rect.w / 2., "middle"),
    };
    let lines = value.lines().collect::<Vec<_>>();
    let step = font * 1.25;
    let y = if edge {
        rect.y - 5. - step * (lines.len().saturating_sub(1)) as f64
    } else {
        match style(cell, "verticalAlign", "middle") {
            "top" => rect.y + font + 4.,
            "bottom" => rect.y + rect.h - 4. - step * (lines.len().saturating_sub(1)) as f64,
            _ => {
                rect.y + rect.h / 2. + font * 0.35
                    - step * (lines.len().saturating_sub(1)) as f64 / 2.
            }
        }
    };
    let color = escape(style(cell, "fontColor", "#000000"));
    let weight = if numeric(cell, "fontStyle", 0.) as u32 & 1 != 0 {
        "bold"
    } else {
        "normal"
    };
    format!(
        "<text fill=\"{color}\" font-family=\"sans-serif\" font-size=\"{font}\" font-weight=\"{weight}\" text-anchor=\"{anchor}\">{}</text>",
        lines
            .iter()
            .enumerate()
            .map(|(i, line)| format!(
                "<tspan x=\"{x}\" y=\"{}\">{}</tspan>",
                y + i as f64 * step,
                escape(line)
            ))
            .collect::<String>()
    )
}
fn point(node: Node<'_, '_>, base: Point) -> Point {
    (
        base.0 + number(node, "x", 0.),
        base.1 + number(node, "y", 0.),
    )
}
fn svg(model: Node<'_, '_>, cancel: &Cancellation) -> Result<String> {
    let mut cells = Vec::new();
    for node in model.descendants().filter(|n| n.has_tag_name("mxCell")) {
        cancel.check()?;
        let Some(geometry) = node.children().find(|n| n.has_tag_name("mxGeometry")) else {
            continue;
        };
        let styles = node
            .attribute("style")
            .unwrap_or("")
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.split_once('=').unwrap_or((s, "1")))
            .collect();
        cells.push(Cell {
            node,
            geometry,
            style: styles,
            rect: Rect {
                x: number(geometry, "x", 0.),
                y: number(geometry, "y", 0.),
                w: number(geometry, "width", 0.).max(0.),
                h: number(geometry, "height", 0.).max(0.),
            },
        });
    }
    let ids = cells
        .iter()
        .enumerate()
        .filter_map(|(index, c)| c.node.attribute("id").map(|id| (id, index)))
        .collect::<HashMap<_, _>>();
    let mut rects = Vec::new();
    for cell in &cells {
        let (mut x, mut y) = offset(cell.node, &cells, &ids)?;
        if cell.geometry.attribute("relative") == Some("1")
            && cell.node.attribute("vertex") == Some("1")
        {
            if let Some(parent) = cell
                .node
                .attribute("parent")
                .and_then(|id| ids.get(id))
                .map(|i| &cells[*i])
            {
                x += cell.rect.x * parent.rect.w;
                y += cell.rect.y * parent.rect.h;
            }
        } else {
            x += cell.rect.x;
            y += cell.rect.y;
        }
        rects.push(Rect { x, y, ..cell.rect });
    }
    let (mut minx, mut miny, mut maxx, mut maxy) = (0_f64, 0_f64, 1_f64, 1_f64);
    let mut body = String::new();
    for (index, cell) in cells.iter().enumerate() {
        cancel.check()?;
        if cell.node.attribute("visible") == Some("0") {
            continue;
        }
        let rect = rects[index];
        let stroke = escape(style(cell, "strokeColor", "#000000"));
        let fill = escape(style(cell, "fillColor", "#ffffff"));
        let width = numeric(cell, "strokeWidth", 1.).max(0.);
        let dash = if style(cell, "dashed", "0") == "1" {
            "stroke-dasharray=\"6 4\""
        } else {
            ""
        };
        if cell.node.attribute("edge") == Some("1") {
            let base = offset(cell.node, &cells, &ids)?;
            let explicit = |name: &str| {
                cell.geometry
                    .children()
                    .find(|n| n.has_tag_name("mxPoint") && n.attribute("as") == Some(name))
                    .map(|n| point(n, base))
            };
            let terminal = |name: &str, prefix: &str| -> Option<Point> {
                let r = cell
                    .node
                    .attribute(name)
                    .and_then(|id| ids.get(id))
                    .map(|i| rects[*i])?;
                let x = numeric(cell, &format!("{prefix}X"), 0.5);
                let y = numeric(cell, &format!("{prefix}Y"), 0.5);
                Some((r.x + r.w * x, r.y + r.h * y))
            };
            let source = terminal("source", "exit")
                .or_else(|| explicit("sourcePoint"))
                .unwrap_or(base);
            let target = terminal("target", "entry")
                .or_else(|| explicit("targetPoint"))
                .unwrap_or(source);
            let mut points = vec![source];
            for array in cell
                .geometry
                .children()
                .filter(|n| n.has_tag_name("Array") && n.attribute("as") == Some("points"))
            {
                for n in array.children().filter(|n| n.has_tag_name("mxPoint")) {
                    points.push(point(n, base));
                }
            }
            if points.len() == 1
                && style(cell, "edgeStyle", "") == "orthogonalEdgeStyle"
                && (source.0 - target.0).abs() > 1.
                && (source.1 - target.1).abs() > 1.
            {
                points.push(((source.0 + target.0) / 2., source.1));
                points.push(((source.0 + target.0) / 2., target.1));
            }
            points.push(target);
            for &(x, y) in &points {
                minx = minx.min(x);
                miny = miny.min(y);
                maxx = maxx.max(x);
                maxy = maxy.max(y);
            }
            body.push_str(&format!("<polyline points=\"{}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"{width}\" {dash}/>",points.iter().map(|(x,y)|format!("{x},{y}")).collect::<Vec<_>>().join(" ")));
            for (arrow, tip, previous) in [
                (
                    style(cell, "endArrow", "classic"),
                    *points.last().unwrap(),
                    points[points.len() - 2],
                ),
                (style(cell, "startArrow", "none"), source, points[1]),
            ] {
                if arrow == "none" {
                    continue;
                }
                let angle = (tip.1 - previous.1).atan2(tip.0 - previous.0);
                let size = 8.;
                if arrow == "oval" {
                    body.push_str(&format!(
                        "<circle cx=\"{}\" cy=\"{}\" r=\"3\" fill=\"{stroke}\"/>",
                        tip.0, tip.1
                    ));
                    continue;
                }
                let p1 = (
                    tip.0 - size * (angle - 0.45).cos(),
                    tip.1 - size * (angle - 0.45).sin(),
                );
                let p2 = (
                    tip.0 - size * (angle + 0.45).cos(),
                    tip.1 - size * (angle + 0.45).sin(),
                );
                body.push_str(&format!(
                    "<path d=\"M {},{} L {},{} L {},{}\" stroke=\"{stroke}\" fill=\"{}\"/>",
                    p1.0,
                    p1.1,
                    tip.0,
                    tip.1,
                    p2.0,
                    p2.1,
                    if arrow == "open" { "none" } else { &stroke }
                ));
            }
            let mid = points[points.len() / 2];
            let location = Rect {
                x: (source.0 + target.0) / 2.,
                y: if points.len() == 2 {
                    (source.1 + target.1) / 2.
                } else {
                    mid.1
                },
                w: 0.,
                h: 0.,
            };
            body.push_str(&text_svg(cell, location, &label(cell), true));
        } else if cell.node.attribute("vertex") == Some("1") {
            minx = minx.min(rect.x);
            miny = miny.min(rect.y);
            maxx = maxx.max(rect.x + rect.w);
            maxy = maxy.max(rect.y + rect.h);
            let shape = style(cell, "shape", "");
            let mut label_rect = rect;
            let drawing = if shape == "umlLifeline" {
                label_rect.h = 40.;
                format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"40\"/><path d=\"M {} {} V {}\" fill=\"none\" stroke-dasharray=\"6 4\"/>",
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.x + rect.w / 2.,
                    rect.y + 40.,
                    rect.y + rect.h
                )
            } else if shape == "ellipse" || cell.style.contains_key("ellipse") {
                format!(
                    "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"/>",
                    rect.x + rect.w / 2.,
                    rect.y + rect.h / 2.,
                    rect.w / 2.,
                    rect.h / 2.
                )
            } else if shape == "rhombus" || cell.style.contains_key("rhombus") {
                format!(
                    "<polygon points=\"{},{} {},{} {},{} {},{}\"/>",
                    rect.x + rect.w / 2.,
                    rect.y,
                    rect.x + rect.w,
                    rect.y + rect.h / 2.,
                    rect.x + rect.w / 2.,
                    rect.y + rect.h,
                    rect.x,
                    rect.y + rect.h / 2.
                )
            } else if cell.style.contains_key("text") || shape == "text" || shape == "group" {
                String::new()
            } else {
                format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\"/>",
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    if style(cell, "rounded", "0") == "1" {
                        8.
                    } else {
                        0.
                    }
                )
            };
            body.push_str(&format!("<g stroke=\"{stroke}\" fill=\"{fill}\" stroke-width=\"{width}\" {dash}>{drawing}</g>"));
            body.push_str(&text_svg(cell, label_rect, &label(cell), false));
        }
    }
    let width = (maxx - minx + 64.).max(1.);
    let height = (maxy - miny + 64.).max(1.);
    ensure!(
        width.is_finite() && height.is_finite(),
        "Invalid draw.io bounds"
    );
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"{} {} {width} {height}\"><rect x=\"{}\" y=\"{}\" width=\"{width}\" height=\"{height}\" fill=\"white\"/>{body}</svg>",
        minx - 32.,
        miny - 32.,
        minx - 32.,
        miny - 32.
    ))
}
fn raster(svg: &str, remaining: u64) -> Result<(Vec<u8>, u64)> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    });
    let options = resvg::usvg::Options {
        fontdb: fonts.clone(),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(svg, &options)?;
    let size = tree.size();
    let scale = (limits::IMAGE_PREVIEW_EDGE as f32 / size.width().max(size.height())).min(1.);
    let width = (size.width() * scale).ceil().max(1.) as u32;
    let height = (size.height() * scale).ceil().max(1.) as u32;
    let decoded = u64::from(width) * u64::from(height) * 4;
    ensure!(
        decoded <= remaining,
        "Draw.io pages exceed decoded image budget"
    );
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(width, height).context("Cannot allocate draw.io preview")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok((pixmap.encode_png()?, decoded))
}
pub fn read(path: &Path, cancel: &Cancellation) -> Result<ImageDocument> {
    let mut source = String::new();
    crate::work::Reader {
        inner: std::fs::File::open(path)?,
        cancel: cancel.clone(),
    }
    .take(limits::DRAWIO_XML_BYTES as u64 + 1)
    .read_to_string(&mut source)?;
    ensure!(
        source.len() <= limits::DRAWIO_XML_BYTES,
        "Draw.io document exceeds preview XML budget"
    );
    let document = roxmltree::Document::parse(&source)?;
    let root = document.root_element();
    let mut pages = Vec::new();
    let mut memory = 0_u64;
    let mut decoded_memory = 0_u64;
    let models = if root.has_tag_name("mxGraphModel") {
        vec![root]
    } else {
        root.children()
            .filter(|n| n.has_tag_name("diagram"))
            .collect()
    };
    ensure!(!models.is_empty(), "No draw.io diagrams found");
    for page in models {
        cancel.check()?;
        let generated;
        let parsed;
        let model = if page.has_tag_name("mxGraphModel") {
            page
        } else if let Some(model) = page.children().find(|n| n.has_tag_name("mxGraphModel")) {
            model
        } else {
            generated = decode_page(page, cancel)?;
            parsed = roxmltree::Document::parse(&generated)?;
            parsed.root_element()
        };
        let output = svg(model, cancel)?;
        cancel.check()?;
        let (png, decoded) = raster(
            &output,
            limits::IMAGE_DECODED_BYTES.saturating_sub(decoded_memory),
        )?;
        decoded_memory += decoded;
        cancel.check()?;
        memory += png.len() as u64;
        ensure!(
            memory <= limits::DRAWIO_PREVIEW_BYTES,
            "Draw.io pages exceed preview memory budget"
        );
        pages.push(png);
    }
    let stamp = std::fs::metadata(path)?;
    Ok(ImageDocument {
        path: path.to_path_buf(),
        png: Vec::new(),
        pages,
        gif: false,
        fingerprint: format!("drawio:{}:{:?}", stamp.len(), stamp.modified().ok()),
    })
}
