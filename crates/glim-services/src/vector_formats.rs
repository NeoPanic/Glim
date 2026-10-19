//! Static projections for small mesh and bitmap font files. No document execution.
use crate::limits as reader_limits;
use crate::media::ImageDocument;
use anyhow::{Context, Result, ensure};
use std::{fmt::Write as _, io::Read, path::Path};
pub fn supports(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "obj" | "stl" | "bdf"))
}
pub fn read_preview(path: &Path) -> Result<ImageDocument> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(reader_limits::GEOMETRY_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    reader_limits::preview_budget(
        bytes.len() <= reader_limits::GEOMETRY_INPUT_BYTES,
        "Static geometry exceeds the preview input size limit",
    )?;
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let body = if ext == "bdf" {
        font(std::str::from_utf8(&bytes)?)?
    } else {
        mesh(&bytes, &ext)?
    };
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"900\" height=\"700\" viewBox=\"0 0 900 700\"><rect width=\"900\" height=\"700\" fill=\"#f5f7fb\"/>{body}</svg>"
    );
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default())?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(900, 700).context("Invalid preview size")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    Ok(ImageDocument {
        path: std::path::absolute(path)?,
        png: pixmap.encode_png()?,
        fingerprint: format!("vector:{:x}", hash.finish()),
        gif: false,
        pages: Vec::new(),
    })
}
fn mesh(bytes: &[u8], ext: &str) -> Result<String> {
    let mut vertices = Vec::<[f64; 3]>::new();
    let mut faces = Vec::<Vec<usize>>::new();
    let binary = ext == "stl" && bytes.len() >= 84 && {
        let count = u32::from_le_bytes(bytes[80..84].try_into()?);
        84_u64 + u64::from(count) * 50 == bytes.len() as u64
    };
    if binary {
        reader_limits::preview_budget(
            (bytes.len() - 84) / 50 <= reader_limits::GEOMETRY_FACES
                && (bytes.len() - 84) / 50 <= reader_limits::GEOMETRY_VERTICES / 3,
            "Mesh preview working budget exceeded",
        )?;
        for facet in bytes[84..].as_chunks::<50>().0 {
            let mut face = Vec::new();
            for data in facet[12..48].as_chunks::<12>().0 {
                face.push(vertices.len());
                vertices.push([
                    f32::from_le_bytes(data[..4].try_into()?) as f64,
                    f32::from_le_bytes(data[4..8].try_into()?) as f64,
                    f32::from_le_bytes(data[8..12].try_into()?) as f64,
                ]);
            }
            faces.push(face);
        }
    } else {
        for line in std::str::from_utf8(bytes)?.lines() {
            let mut words = line.split_whitespace();
            match words.next() {
                Some("v") if ext == "obj" => {
                    vertices.push(point(words)?);
                }
                Some("vertex") if ext == "stl" => {
                    vertices.push(point(words)?);
                    if vertices.len().is_multiple_of(3) {
                        let n = vertices.len();
                        faces.push(vec![n - 3, n - 2, n - 1]);
                    }
                }
                Some("f") if ext == "obj" => {
                    let face = words
                        .map(|word| {
                            let index: isize = word
                                .split('/')
                                .next()
                                .context("Invalid OBJ face")?
                                .parse()?;
                            let index = if index < 0 {
                                vertices.len() as isize + index
                            } else {
                                index - 1
                            };
                            ensure!(
                                index >= 0 && (index as usize) < vertices.len(),
                                "OBJ face index out of bounds"
                            );
                            Ok(index as usize)
                        })
                        .collect::<Result<Vec<_>>>()?;
                    ensure!(face.len() >= 3, "Invalid OBJ polygon");
                    reader_limits::preview_budget(
                        face.len() <= reader_limits::GEOMETRY_POLYGON_VERTICES,
                        "Polygon preview working budget exceeded",
                    )?;
                    faces.push(face);
                }
                _ => {}
            }
            reader_limits::preview_budget(
                vertices.len() <= reader_limits::GEOMETRY_VERTICES
                    && faces.len() <= reader_limits::GEOMETRY_FACES,
                "Mesh vertex/face count exceeds the preview limit",
            )?;
        }
    }
    ensure!(
        !vertices.is_empty() && !faces.is_empty(),
        "Mesh contains no faces"
    );
    reader_limits::preview_budget(
        vertices.len() <= reader_limits::GEOMETRY_VERTICES,
        "Mesh vertex count exceeds the preview limit",
    )?;
    ensure!(
        vertices.iter().flatten().all(|v| v.is_finite()),
        "Non-finite mesh coordinates"
    );
    let projected: Vec<_> = vertices
        .iter()
        .map(|v| (v[0] + v[2] * 0.4, -v[1] + v[2] * 0.25))
        .collect();
    let min_x = projected.iter().map(|v| v.0).fold(f64::INFINITY, f64::min);
    let min_y = projected.iter().map(|v| v.1).fold(f64::INFINITY, f64::min);
    let max_x = projected
        .iter()
        .map(|v| v.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_y = projected
        .iter()
        .map(|v| v.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let scale = (740.0 / (max_x - min_x).max(1e-12)).min(540.0 / (max_y - min_y).max(1e-12));
    let mut body = String::new();
    for face in faces {
        let mut points = String::new();
        for index in face {
            let (x, y) = projected[index];
            write!(
                points,
                "{},{} ",
                80.0 + (x - min_x) * scale,
                80.0 + (y - min_y) * scale
            )?;
        }
        write!(
            body,
            "<polygon points=\"{points}\" fill=\"#5288c855\" stroke=\"#235a9c\" stroke-width=\"2\"/>"
        )?;
        reader_limits::preview_budget(
            body.len() as u64 <= reader_limits::IO_CACHE_BYTES,
            "Mesh projection working buffer exceeded",
        )?;
    }
    Ok(body)
}
fn point<'a>(words: impl Iterator<Item = &'a str>) -> Result<[f64; 3]> {
    let values = words
        .take(3)
        .map(str::parse)
        .collect::<std::result::Result<Vec<f64>, _>>()?;
    ensure!(values.len() == 3, "Invalid mesh vertex");
    Ok([values[0], values[1], values[2]])
}
fn font(text: &str) -> Result<String> {
    ensure!(text.starts_with("STARTFONT"), "Invalid BDF header");
    let mut body = String::new();
    let mut bitmap = false;
    let mut row = 0;
    let mut glyph = 0;
    let mut width = 0;
    let mut pixel = 6;
    for line in text.lines() {
        if line.starts_with("BBX ") {
            width = line
                .split_whitespace()
                .nth(1)
                .context("Missing glyph width")?
                .parse::<usize>()?;
            reader_limits::preview_budget(
                width <= reader_limits::BDF_GLYPH_EDGE,
                "BDF glyph width exceeds the preview limit",
            )?;
            let height = line
                .split_whitespace()
                .nth(2)
                .context("Missing glyph height")?
                .parse::<usize>()?;
            reader_limits::preview_budget(
                height <= reader_limits::BDF_GLYPH_EDGE,
                "BDF glyph height exceeds the preview limit",
            )?;
            pixel = (100 / width.max(1)).min(70 / height.max(1)).min(6);
        } else if line == "BITMAP" {
            bitmap = true;
            row = 0;
        } else if line == "ENDCHAR" {
            bitmap = false;
            glyph += 1;
            reader_limits::preview_budget(
                glyph <= reader_limits::BDF_GLYPHS,
                "Glyph preview working budget exceeded",
            )?;
        } else if bitmap {
            reader_limits::preview_budget(
                row < reader_limits::BDF_GLYPH_EDGE,
                "BDF glyph height exceeds the preview limit",
            )?;
            reader_limits::preview_budget(
                line.len() <= reader_limits::BDF_GLYPH_EDGE / 4,
                "Glyph row preview working budget exceeded",
            )?;
            ensure!(
                line.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "Invalid BDF bitmap row"
            );
            let bit_width = line.len() * 4;
            ensure!(
                bit_width <= reader_limits::BDF_GLYPH_EDGE && bit_width >= width,
                "Invalid BDF bitmap row"
            );
            for col in 0..width {
                let digit = char::from(line.as_bytes()[col / 4])
                    .to_digit(16)
                    .context("Invalid BDF bitmap row")?;
                if (digit >> (3 - col % 4)) & 1 == 1 {
                    write!(
                        body,
                        "<rect x=\"{}\" y=\"{}\" width=\"{pixel}\" height=\"{pixel}\" fill=\"#235a9c\"/>",
                        20 + (glyph % 8) * 110 + col * pixel,
                        20 + (glyph / 8) * 80 + row * pixel
                    )?;
                }
            }
            reader_limits::preview_budget(
                body.len() as u64 <= reader_limits::IO_CACHE_BYTES,
                "Glyph projection working buffer exceeded",
            )?;
            row += 1;
        }
    }
    ensure!(glyph > 0, "BDF contains no glyphs");
    Ok(body)
}
