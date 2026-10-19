//! Offline diagram preparation. All parsing, child processes and rasterization
//! run on the caller's background executor; document text is never a script.
use crate::limits as reader_limits;
use anyhow::{Context, Result, ensure};
use markdown::{ParseOptions, mdast::Node};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Accept blank lines between explicit, matching pipe-table rows in preview only.
/// AST block ranges keep code, HTML, math and frontmatter examples untouched.
pub fn normalize_tables(text: &str) -> std::borrow::Cow<'_, str> {
    use std::{borrow::Cow, ops::Range};
    // The repair only removes blank lines between explicit pipe rows. Ordinary
    // tables need no AST pass; false-positive candidates still use the parser
    // below so fenced code, HTML and frontmatter remain protected.
    let mut previous_pipe = false;
    let mut gap = false;
    let candidate = text.lines().any(|line| {
        let line = line.trim();
        if line.is_empty() {
            gap = true;
            return false;
        }
        let pipe = line.starts_with('|') && line.ends_with('|');
        let candidate = previous_pipe && gap && pipe;
        previous_pipe = pipe;
        gap = false;
        candidate
    });
    if !candidate {
        return Cow::Borrowed(text);
    }
    let mut options = ParseOptions::gfm();
    options.constructs.math_flow = true;
    options.constructs.math_text = true;
    options.constructs.frontmatter = true;
    let Ok(Node::Root(root)) = markdown::to_mdast(text, &options) else {
        return Cow::Borrowed(text);
    };
    let mut blocks = root
        .children
        .iter()
        .filter_map(|node| {
            if matches!(node, Node::Paragraph(_) | Node::Table(_)) {
                node.position().map(|p| p.start.offset..p.end.offset)
            } else {
                None
            }
        })
        .peekable();
    let mut offset = 0;
    let lines = text
        .split_inclusive('\n')
        .map(|line| {
            let start = offset;
            offset += line.len();
            let content = line.trim_end_matches(['\r', '\n']);
            let first = start + content.len() - content.trim_start().len();
            while blocks.peek().is_some_and(|block| block.end <= first) {
                blocks.next();
            }
            let eligible = blocks.peek().is_some_and(|block| block.contains(&first));
            (start..offset, content, eligible)
        })
        .collect::<Vec<_>>();
    let next_row = |index: usize| (index..lines.len()).find(|&i| !lines[i].1.trim().is_empty());
    let mut removed = Vec::<Range<usize>>::new();
    let mut index = 0;
    while index < lines.len() {
        let (_, header, eligible) = &lines[index];
        let Some(columns) = eligible.then(|| pipe_cells(header)).flatten() else {
            index += 1;
            continue;
        };
        let Some(separator) = next_row(index + 1) else {
            break;
        };
        if !lines[separator].2 || !table_separator(lines[separator].1, columns.len()) {
            index += 1;
            continue;
        }
        if separator > index + 1 {
            removed.push(lines[index + 1].0.start..lines[separator].0.start);
        }
        let mut previous = separator;
        while let Some(row) = next_row(previous + 1) {
            if !lines[row].2
                || pipe_cells(lines[row].1).is_none_or(|cells| cells.len() != columns.len())
            {
                break;
            }
            // A second explicit header starts another table, rather than becoming data.
            if row > previous + 1
                && next_row(row + 1).is_some_and(|next| {
                    lines[next].2 && table_separator(lines[next].1, columns.len())
                })
            {
                break;
            }
            if row > previous + 1 {
                removed.push(lines[previous + 1].0.start..lines[row].0.start);
            }
            previous = row;
        }
        index = previous + 1;
    }
    if removed.is_empty() {
        return Cow::Borrowed(text);
    }
    let mut output = String::with_capacity(text.len());
    let mut start = 0;
    for range in removed {
        output.push_str(&text[start..range.start]);
        start = range.end;
    }
    output.push_str(&text[start..]);
    Cow::Owned(output)
}

fn pipe_cells(line: &str) -> Option<Vec<&str>> {
    // Explicit outer pipes avoid reinterpreting ordinary prose containing a pipe.
    if line.starts_with('\t') || line.starts_with("    ") {
        return None;
    }
    let row = line.trim().strip_prefix('|')?.strip_suffix('|')?;
    let mut cells = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (offset, ch) in row.char_indices() {
        if ch == '|' && !escaped {
            cells.push(row[start..offset].trim());
            start = offset + 1;
        }
        escaped = ch == '\\' && !escaped;
    }
    cells.push(row[start..].trim());
    (cells.len() >= 2).then_some(cells)
}

fn table_separator(line: &str, columns: usize) -> bool {
    pipe_cells(line).is_some_and(|cells| {
        cells.len() == columns
            && cells.iter().all(|cell| {
                let dashes = cell.strip_prefix(':').unwrap_or(cell);
                let dashes = dashes.strip_suffix(':').unwrap_or(dashes);
                !dashes.is_empty() && dashes.bytes().all(|byte| byte == b'-')
            })
    })
}

pub const MAX_DIAGRAMS: usize = reader_limits::DIAGRAM_BLOCKS;
pub const MAX_SOURCE: usize = reader_limits::DIAGRAM_SOURCE_BYTES;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Diagram {
    pub kind: String,
    pub source: String,
}

pub fn fenced(kind: &str, source: &str) -> Option<Diagram> {
    let mut language = kind
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut header = source.split_whitespace();
    let first = header.next().unwrap_or_default();
    let second = header.next().unwrap_or_default();
    // Many notes omit the fence language; recognize only explicit engine
    // headers, leaving ordinary unlabelled code blocks untouched.
    if language.is_empty() || language == "flowchart" {
        if language.is_empty()
            && (first == "digraph"
                || (first == "strict" && matches!(second, "graph" | "digraph"))
                || (first == "graph"
                    && !matches!(second, "TB" | "TD" | "BT" | "LR" | "RL")
                    && source.contains('{')))
        {
            language = "dot".into();
        } else if matches!(
            first,
            "flowchart"
                | "graph"
                | "sequenceDiagram"
                | "classDiagram"
                | "stateDiagram"
                | "stateDiagram-v2"
                | "erDiagram"
                | "gantt"
                | "pie"
                | "mindmap"
                | "journey"
                | "gitGraph"
                | "timeline"
                | "quadrantChart"
                | "sankey-beta"
                | "xychart-beta"
                | "block-beta"
                | "packet-beta"
                | "architecture-beta"
                | "kanban"
                | "requirementDiagram"
                | "zenuml"
                | "radar-beta"
                | "treemap-beta"
                | "C4Context"
                | "C4Container"
                | "C4Component"
                | "C4Dynamic"
                | "C4Deployment"
        ) {
            language = "mermaid".into();
        } else if language.is_empty()
            && matches!(
                first,
                "@startuml"
                    | "@startmindmap"
                    | "@startwbs"
                    | "@startjson"
                    | "@startyaml"
                    | "@startgantt"
                    | "@startsalt"
                    | "@startdot"
                    | "@startchen"
            )
        {
            language = "plantuml".into();
        }
    }
    let kind = match language.as_str() {
        "d2" => "d2",
        "pikchr" => "pikchr",
        "vega" => "vega",
        "mermaid" | "mmd" => "mermaid",
        "zenuml" => "zenuml",
        "dot" | "graphviz" => "dot",
        "plantuml" | "puml" | "uml" => "plantuml",
        "flow" | "flowchart" | "flowchart.js" => "flowchart",
        "wavedrom" | "wavejson" => "wavedrom",
        "math" | "latex" | "tex" | "katex" => "math",
        _ => return None,
    };
    Some(Diagram {
        kind: kind.into(),
        source: source.into(),
    })
}

pub fn from_node(node: &Node) -> Option<Diagram> {
    match node {
        Node::Code(code) => fenced(code.lang.as_deref().unwrap_or_default(), &code.value),
        Node::Math(math) => Some(Diagram {
            kind: "math".into(),
            source: math.value.clone(),
        }),
        Node::InlineMath(math) => Some(Diagram {
            kind: "inline-math".into(),
            source: math.value.clone(),
        }),
        _ => None,
    }
}

#[derive(Clone)]
pub struct DiagramImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub baseline: f32,
}
// Compressed results are reusable within this process. Source, engine kind and
// raster edge form the key; eviction bounds cache memory independently of files.
type CachedDiagram = (Diagram, u32, Arc<DiagramImage>);
static IMAGE_CACHE: Mutex<VecDeque<CachedDiagram>> = Mutex::new(VecDeque::new());
fn cached_image(diagram: &Diagram, edge: u32) -> Option<Arc<DiagramImage>> {
    let mut cache = IMAGE_CACHE.lock().ok()?;
    let index = cache
        .iter()
        .position(|(key, size, _)| key == diagram && *size == edge)?;
    let entry = cache.remove(index)?;
    let image = entry.2.clone();
    cache.push_back(entry);
    Some(image)
}
fn remember_image(diagram: &Diagram, edge: u32, image: &DiagramImage) {
    let cost = |diagram: &Diagram, image: &DiagramImage| {
        diagram
            .source
            .len()
            .saturating_add(diagram.kind.len())
            .saturating_add(image.png.len())
            .saturating_add(std::mem::size_of::<CachedDiagram>())
    };
    let bytes = cost(diagram, image);
    if bytes as u64 > reader_limits::IO_CACHE_BYTES {
        return;
    }
    let Ok(mut cache) = IMAGE_CACHE.lock() else {
        return;
    };
    cache.retain(|(key, size, _)| key != diagram || *size != edge);
    let mut used: usize = cache.iter().map(|(key, _, image)| cost(key, image)).sum();
    while used.saturating_add(bytes) as u64 > reader_limits::IO_CACHE_BYTES {
        let Some((key, _, image)) = cache.pop_front() else {
            break;
        };
        used = used.saturating_sub(cost(&key, &image));
    }
    cache.push_back((diagram.clone(), edge, Arc::new(image.clone())));
}

/// Common LaTeX display delimiters are escaped punctuation to the GFM parser.
/// Recognize them using the original paragraph source, preserving source offsets.
pub fn from_node_with_source(node: &Node, source: Option<&str>) -> Option<Diagram> {
    if matches!(node, Node::Paragraph(_))
        && let Some(source) = source
        && let Some(body) = source.trim().strip_prefix("\\[")
        && let Some(body) = body.strip_suffix("\\]")
        && !body.trim().is_empty()
        && !body.contains("\\[")
        && !body.contains("\\]")
    {
        return Some(Diagram {
            kind: "math".into(),
            source: body.trim().into(),
        });
    }
    from_node(node)
}
pub type RenderedDiagrams = HashMap<Diagram, std::result::Result<DiagramImage, String>>;

/// Deduplicate repeated blocks and keep source ranges entirely in TextView.
pub fn collect(text: &str) -> Result<Vec<Diagram>> {
    // Any supported diagram must use a fence, math delimiter, or indented code.
    // This is only a conservative negative filter, never a diagram parser.
    if !text.contains("```")
        && !text.contains("~~~")
        && !text.contains('$')
        && !text.contains("\\[")
        && !text.contains("    ")
        && !text.contains('\t')
    {
        return Ok(Vec::new());
    }
    let mut options = ParseOptions::gfm();
    options.constructs.math_flow = true;
    options.constructs.math_text = true;
    options.constructs.frontmatter = true;
    let root = markdown::to_mdast(text, &options).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut pending = vec![&root];
    let mut seen = HashSet::new();
    let mut diagrams = Vec::new();
    while let Some(node) = pending.pop() {
        let source = node
            .position()
            .and_then(|position| text.get(position.start.offset..position.end.offset));
        if let Some(diagram) = from_node_with_source(node, source)
            && seen.insert(diagram.clone())
        {
            diagrams.push(diagram);
        }
        if let Some(children) = node.children() {
            pending.extend(children.iter().rev());
        }
    }
    Ok(diagrams)
}

/// Render a document in an isolated system WebView, with bundled engines and
/// no network or local-file access. Temporary sources are removed on return.
pub fn render(
    text: &str,
    cancelled: &AtomicBool,
    publish: impl FnMut(Diagram, std::result::Result<DiagramImage, String>),
) -> Result<()> {
    render_page(text, 0, cancelled, publish).map(|_| ())
}
pub fn render_page(
    text: &str,
    page: usize,
    cancelled: &AtomicBool,
    mut publish: impl FnMut(Diagram, std::result::Result<DiagramImage, String>),
) -> Result<usize> {
    let diagrams = collect(text)?;
    let total = diagrams.len();
    let mut requests = Vec::new();
    for (index, diagram) in diagrams.into_iter().enumerate() {
        if index / MAX_DIAGRAMS != page {
            publish(
                diagram,
                Err(crate::locale::tr(
                    "Use the diagram page controls to render this block; its complete source remains available.",
                )),
            );
            continue;
        }
        if diagram.source.len() > MAX_SOURCE {
            publish(
                diagram,
                Err(crate::locale::tr(
                    "Diagram source exceeds the active render budget; complete source is available in Source view.",
                )),
            );
        } else {
            requests.push(diagram);
        }
    }
    if requests.is_empty() || cancelled.load(Ordering::Relaxed) {
        return Ok(total);
    }
    let per_image = (reader_limits::DIAGRAM_AGGREGATE_ENCODED_BYTES as u64)
        .min(reader_limits::DIAGRAM_AGGREGATE_DECODED_BYTES)
        / requests.len().max(1) as u64;
    let edge = ((per_image.saturating_sub(reader_limits::IO_BUFFER_BYTES as u64) / 4) as f64)
        .sqrt()
        .floor()
        .max(1.) as u32;
    let edge = edge.min(reader_limits::DIAGRAM_PREVIEW_EDGE);
    let mut bytes_used = 0;
    let mut pixels_used = 0_u64;
    requests.retain(|diagram| {
        if cancelled.load(Ordering::Relaxed) {
            return false;
        }
        if let Some(image) = cached_image(diagram, edge) {
            bytes_used += image.png.len();
            pixels_used += u64::from(image.width) * u64::from(image.height) * 4;
            if bytes_used <= reader_limits::DIAGRAM_AGGREGATE_ENCODED_BYTES
                && pixels_used <= reader_limits::DIAGRAM_AGGREGATE_DECODED_BYTES
            {
                publish(diagram.clone(), Ok((*image).clone()));
                return false;
            }
            bytes_used -= image.png.len();
            pixels_used -= u64::from(image.width) * u64::from(image.height) * 4;
        }
        true
    });
    if requests.is_empty() || cancelled.load(Ordering::Relaxed) {
        return Ok(total);
    }
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("request.json");
    let output = dir.path().join("result.json");
    let request = requests
        .iter()
        .map(|d| serde_json::json!({ "kind": d.kind, "source": d.source }))
        .collect::<Vec<_>>();
    std::fs::write(&input, serde_json::to_vec(&request)?)?;
    let executable = std::env::current_exe()?;

    let executable = executable
        .parent()
        .and_then(|directory| directory.parent())
        .map(|contents| contents.join("Helpers/GlimRenderer.app/Contents/MacOS/glim-renderer"))
        .filter(|helper| helper.is_file())
        .unwrap_or(executable);
    let mut command = Command::new(executable);
    command
        .arg("--render-markdown")
        .arg(&input)
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child = command
        .spawn()
        .context("Cannot start offline diagram renderer")?;
    let started = Instant::now();
    let deadline = Duration::from_secs(
        reader_limits::DIAGRAM_STARTUP_SECONDS
            + requests.len() as u64 * reader_limits::DIAGRAM_BLOCK_SECONDS,
    );
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    let fonts = FONTS
        .get_or_init(|| {
            let mut fonts = resvg::usvg::fontdb::Database::new();
            fonts.load_system_fonts();
            Arc::new(fonts)
        })
        .clone();
    let mut completed = HashSet::new();
    let mut timed_out = false;
    loop {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
        };
        if cancelled.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(total);
        }
        if started.elapsed() > deadline {
            timed_out = true;
            let _ = child.kill();
            let _ = child.wait();
        }
        let finished = status.is_some() || timed_out;
        let missing_reason = if timed_out {
            crate::locale::tr("Diagram rendering timed out; this block is incomplete")
        } else if let Some(status) = status.filter(|status| !status.success()) {
            format!(
                "{} ({status})",
                crate::locale::tr("Offline renderer exited unexpectedly")
            )
        } else {
            crate::locale::tr("Offline renderer exited without a result for this block")
        };
        // Each block is published atomically once. Reading only new slots
        // avoids repeatedly serializing/parsing all preceding SVGs.
        let final_values = if finished {
            std::fs::metadata(&output)
                .ok()
                .filter(|m| m.len() <= reader_limits::DIAGRAM_IPC_BYTES)
                .and_then(|_| std::fs::read(&output).ok())
                .and_then(|bytes| serde_json::from_slice::<Vec<serde_json::Value>>(&bytes).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        for (index, diagram) in requests.iter().enumerate() {
            if completed.contains(&index) {
                continue;
            }
            let slot_path = output.with_extension(format!("{index}.json"));
            let slot = std::fs::metadata(&slot_path)
                .ok()
                .filter(|m| m.len() <= reader_limits::DIAGRAM_SLOT_BYTES)
                .and_then(|_| std::fs::read(slot_path).ok())
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
            let value = slot.as_ref().or_else(|| final_values.get(index));
            if !finished && value.is_none_or(|v| v.is_null()) {
                continue;
            }
            let result = if let Some(svg) =
                value.and_then(|v| v.get("svg")).and_then(|v| v.as_str())
            {
                let baseline = value
                    .and_then(|v| v.get("baseline"))
                    .and_then(|v| v.as_f64());
                rasterize(svg, fonts.clone(), baseline, edge)
                    .and_then(|image| {
                        bytes_used += image.png.len();
                        pixels_used += u64::from(image.width) * u64::from(image.height) * 4;
                        ensure!(
                            bytes_used <= reader_limits::DIAGRAM_AGGREGATE_ENCODED_BYTES,
                            "Diagram image budget exceeded"
                        );
                        ensure!(
                            pixels_used <= reader_limits::DIAGRAM_AGGREGATE_DECODED_BYTES,
                            "Decoded diagram image budget exceeded"
                        );
                        Ok(image)
                    })
                    .map_err(|e| format!("{}: {e}", crate::locale::tr("Cannot display diagram")))
            } else {
                Err(value
                    .and_then(|v| v.get("error"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(&missing_reason)
                    .to_owned())
            };
            if let Ok(image) = &result {
                remember_image(diagram, edge, image);
            }
            completed.insert(index);
            publish(diagram.clone(), result);
        }
        if finished {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(total)
}

fn rasterize(
    svg: &str,
    fonts: Arc<resvg::usvg::fontdb::Database>,
    baseline: Option<f64>,
    edge: u32,
) -> Result<DiagramImage> {
    ensure!(
        svg.len() <= reader_limits::DIAGRAM_SVG_BYTES,
        "SVG exceeds the preview size limit"
    );
    let options = resvg::usvg::Options {
        fontdb: fonts,
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(svg, &options)?;
    let size = tree.size();
    let scale = (edge as f32 / size.width().max(size.height()))
        .min(reader_limits::DIAGRAM_MAX_UPSCALE as f32);
    let width = (size.width() * scale).ceil().clamp(1.0, edge as f32) as u32;
    let height = (size.height() * scale).ceil().clamp(1.0, edge as f32) as u32;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(width, height).context("Invalid diagram dimensions")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(DiagramImage {
        png: pixmap.encode_png()?,
        width,
        height,
        baseline: baseline
            .filter(|value| value.is_finite())
            .map_or(size.height(), |value| value as f32)
            .clamp(0.0, size.height())
            * scale,
    })
}
