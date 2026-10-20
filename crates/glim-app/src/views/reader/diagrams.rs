use super::OpenLink;
use glim_services::markdown_preview::{self, Diagram};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    base::{
        InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext,
        MarkdownPlugin, TextView, markdown_ast,
    },
    component::{ActiveTheme, h_flex},
    *,
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

#[derive(Clone)]
enum Preview {
    Image(Arc<Image>, u32, u32, f32),
    Error(String),
    Pending,
}

/// Alert rendering uses the same prepared diagram snapshot, image base and
/// link callback as the surrounding document. Replacing these render handles
/// does not change the parser revision or restart diagram work.
pub(super) fn with_alerts(
    extensions: MarkdownExtensions,
    base: url::Url,
    open: OpenLink,
) -> MarkdownExtensions {
    extensions.clone().plugin(AlertPlugin {
        nested: extensions,
        base,
        open,
    })
}

struct AlertPlugin {
    nested: MarkdownExtensions,
    base: url::Url,
    open: OpenLink,
}
struct AlertNode {
    kind: String,
    body: String,
}
impl MarkdownPlugin for AlertPlugin {
    fn name(&self) -> &str {
        "glim-alert"
    }
    fn is_block(&self) -> bool {
        true
    }
    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        if !matches!(node, markdown_ast::Node::Blockquote(_)) {
            return None;
        }
        let source = cx.node_source(node)?;
        let lines = source
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                trimmed
                    .strip_prefix('>')
                    .map(|line| line.strip_prefix(' ').unwrap_or(line))
                    .unwrap_or(line)
            })
            .collect::<Vec<_>>();
        let kind = match lines.first()?.trim() {
            "[!NOTE]" => "提示",
            "[!TIP]" => "建议",
            "[!IMPORTANT]" => "重要",
            "[!WARNING]" => "警告",
            "[!CAUTION]" => "注意",
            _ => return None,
        };
        let body = lines[1..].join("\n");
        Some(
            MarkdownNode::new(
                self.name().to_owned(),
                AlertNode {
                    kind: kind.into(),
                    body: body.clone(),
                },
            )
            .text(format!("{kind}\n{body}"))
            .markdown(source.to_owned()),
        )
    }
    fn render(&self, node: &MarkdownNode, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(alert) = node.data::<AlertNode>() else {
            return div().into_any_element();
        };
        let color = match alert.kind.as_str() {
            "警告" | "注意" => cx.theme().yellow,
            "建议" => cx.theme().green,
            "重要" => cx.theme().magenta,
            _ => cx.theme().blue,
        };
        let image_base = self.base.clone();
        let link_base = self.base.clone();
        let open = self.open.clone();
        div()
            .w_full()
            .my_2()
            .pl_3()
            .border_l_2()
            .border_color(color)
            .child(
                div()
                    .mb_2()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(alert.kind.clone()),
            )
            .child(
                TextView::markdown(
                    ("markdown-alert", node.source_range().map_or(0, |r| r.start)),
                    alert.body.clone(),
                )
                .scrollable(false)
                .w_full()
                .markdown_extensions(self.nested.clone())
                .image_source(move |uri| {
                    image_base
                        .join(uri.as_ref())
                        .ok()
                        .and_then(|url| url.to_file_path().ok())
                        .map(ImageSource::from)
                        .unwrap_or_else(|| uri.clone().into())
                })
                .on_link_click(move |href, _, window, cx| {
                    super::markup::follow_link(&link_base, &open, href, window, cx)
                }),
            )
            .into_any_element()
    }
}
type Snapshot = Arc<Mutex<PreviewStore>>;
static REVISION: AtomicU64 = AtomicU64::new(1);
struct PreviewEntry {
    preview: Preview,
    encoded: usize,
    decoded: u64,
    keys: usize,
    used: u64,
}
#[derive(Default)]
struct PreviewStore {
    entries: HashMap<Diagram, PreviewEntry>,
    queue: std::collections::VecDeque<Diagram>,
    pending: std::collections::HashSet<Diagram>,
    pending_bytes: usize,
    clock: u64,
    revision: u64,
    encoded: usize,
    decoded: u64,
    keys: usize,
}
impl PreviewStore {
    fn request(&mut self, diagram: &Diagram) -> (Preview, bool) {
        self.clock = self.clock.wrapping_add(1);
        if let Some(entry) = self.entries.get_mut(diagram) {
            entry.used = self.clock;
            return (entry.preview.clone(), false);
        }
        let cost = diagram
            .source
            .len()
            .saturating_add(diagram.kind.len())
            .saturating_add(std::mem::size_of::<Diagram>())
            .saturating_mul(2);
        let mut queued = false;
        if !self.pending.contains(diagram)
            && self.pending_bytes.saturating_add(cost) as u64
                <= glim_services::limits::IO_CACHE_BYTES
        {
            self.pending.insert(diagram.clone());
            self.queue.push_back(diagram.clone());
            self.pending_bytes += cost;
            queued = true;
        }
        (Preview::Pending, queued)
    }
    fn batch(&mut self) -> Vec<Diagram> {
        self.queue
            .drain(..self.queue.len().min(markdown_preview::MAX_DIAGRAMS))
            .collect()
    }
    fn complete(
        &mut self,
        diagram: Diagram,
        result: Result<markdown_preview::DiagramImage, String>,
    ) {
        if !self.pending.remove(&diagram) {
            return;
        }
        let cost = diagram
            .source
            .len()
            .saturating_add(diagram.kind.len())
            .saturating_add(std::mem::size_of::<Diagram>())
            .saturating_mul(2);
        self.pending_bytes = self.pending_bytes.saturating_sub(cost);
        let (preview, encoded, decoded, error_bytes) = match result {
            Ok(image) => {
                let encoded = image.png.len();
                let decoded = u64::from(image.width) * u64::from(image.height) * 4;
                (
                    Preview::Image(
                        Arc::new(Image::from_bytes(ImageFormat::Png, image.png)),
                        image.width,
                        image.height,
                        image.baseline,
                    ),
                    encoded,
                    decoded,
                    0,
                )
            }
            Err(error) => {
                let bytes = error.len();
                (Preview::Error(error), 0, 0, bytes)
            }
        };
        let keys = diagram
            .source
            .len()
            .saturating_add(diagram.kind.len())
            .saturating_add(error_bytes)
            .saturating_add(std::mem::size_of::<PreviewEntry>());
        while self.encoded.saturating_add(encoded)
            > glim_services::limits::DIAGRAM_AGGREGATE_ENCODED_BYTES
            || self.decoded.saturating_add(decoded)
                > glim_services::limits::DIAGRAM_AGGREGATE_DECODED_BYTES
            || self.keys.saturating_add(keys) as u64 > glim_services::limits::IO_CACHE_BYTES
        {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            let old = self.entries.remove(&oldest).unwrap();
            self.encoded -= old.encoded;
            self.decoded -= old.decoded;
            self.keys -= old.keys;
        }
        self.clock = self.clock.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
        self.encoded += encoded;
        self.decoded += decoded;
        self.keys += keys;
        self.entries.insert(
            diagram,
            PreviewEntry {
                preview,
                encoded,
                decoded,
                keys,
                used: self.clock,
            },
        );
    }
}

/// Parser nodes retain identities; visible blocks request mutable render resources.
pub(super) struct DiagramPreview {
    pub extensions: MarkdownExtensions,
    snapshot: Snapshot,
    task: Option<Task<()>>,
    cancelled: Arc<AtomicBool>,
    active: bool,
}
impl DiagramPreview {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let snapshot = Arc::new(Mutex::new(PreviewStore::default()));
        Self {
            extensions: extensions(snapshot.clone(), cx.entity().downgrade()),
            snapshot,
            task: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            active: false,
        }
    }
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.task = None;
        self.snapshot = Arc::new(Mutex::new(PreviewStore::default()));
        self.extensions = extensions(self.snapshot.clone(), cx.entity().downgrade());
        if self.active {
            self.start(cx);
        }
        cx.notify();
    }
    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        if active {
            self.ensure_work(cx);
        } else {
            self.cancelled.store(true, Ordering::Relaxed);
            self.task = None;
            let mut snapshot = self.snapshot.lock().unwrap();
            snapshot.pending.clear();
            snapshot.queue.clear();
            snapshot.pending_bytes = 0;
            snapshot.entries.clear();
            snapshot.encoded = 0;
            snapshot.decoded = 0;
            snapshot.keys = 0;
        }
    }
    fn ensure_work(&mut self, cx: &mut Context<Self>) {
        if self.active && self.task.is_none() && !self.snapshot.lock().unwrap().queue.is_empty() {
            self.start(cx);
        }
    }
    fn start(&mut self, cx: &mut Context<Self>) {
        self.cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = self.cancelled.clone();
        let snapshot = self.snapshot.clone();
        self.task = Some(cx.spawn(async move |view, cx| {
            while !cancelled.load(Ordering::Relaxed) {
                let batch = snapshot.lock().unwrap().batch();
                if batch.is_empty() {
                    let _ = view.update(cx, |view, cx| {
                        view.task = None;
                        view.ensure_work(cx);
                    });
                    return;
                }
                let before = snapshot.lock().unwrap().revision;
                let results = snapshot.clone();
                let cancel = cancelled.clone();
                let done = Arc::new(AtomicBool::new(false));
                let finished = done.clone();
                let work = cx.background_executor().spawn(async move {
                    let result =
                        markdown_preview::render_batch(&batch, &cancel, |diagram, image| {
                            if !cancel.load(Ordering::Relaxed) {
                                results.lock().unwrap().complete(diagram, image);
                            }
                        });
                    if let Err(error) = result {
                        for diagram in batch {
                            if cancel.load(Ordering::Relaxed) {
                                break;
                            }
                            results
                                .lock()
                                .unwrap()
                                .complete(diagram, Err(format!("{error:#}")));
                        }
                    }
                    finished.store(true, Ordering::Release);
                });
                let mut revision = before;
                while !done.load(Ordering::Acquire) {
                    if cancelled.load(Ordering::Relaxed) {
                        return;
                    }
                    let current = snapshot.lock().unwrap().revision;
                    if current != revision {
                        revision = current;
                        if view.update(cx, |_, cx| cx.notify()).is_err() {
                            return;
                        }
                    }
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(100))
                        .await;
                }
                work.await;
                if view.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        }));
    }
}
impl Drop for DiagramPreview {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

fn extensions(snapshot: Snapshot, owner: WeakEntity<DiagramPreview>) -> MarkdownExtensions {
    MarkdownExtensions::default()
        .frontmatter()
        .parser_revision(REVISION.fetch_add(1, Ordering::Relaxed))
        .plugin(DiagramPlugin {
            snapshot: snapshot.clone(),
            owner: owner.clone(),
            block: true,
        })
        .plugin(DiagramPlugin {
            snapshot,
            owner,
            block: false,
        })
}
struct DiagramPlugin {
    snapshot: Snapshot,
    owner: WeakEntity<DiagramPreview>,
    block: bool,
}
struct DiagramNode {
    diagram: Diagram,
    inline: bool,
}
impl DiagramPlugin {
    fn preview(&self, diagram: &Diagram, cx: &mut App) -> Preview {
        let (preview, queued) = self.snapshot.lock().unwrap().request(diagram);
        if queued {
            let owner = self.owner.clone();
            cx.defer(move |cx| {
                let _ = owner.update(cx, |view, cx| view.ensure_work(cx));
            });
        }
        preview
    }
}
impl MarkdownPlugin for DiagramPlugin {
    fn name(&self) -> &str {
        if self.block {
            "glim-diagram"
        } else {
            "glim-formula"
        }
    }
    fn is_block(&self) -> bool {
        self.block
    }
    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let inline = matches!(node, markdown_ast::Node::InlineMath(_));
        if inline == self.block {
            return None;
        }
        let diagram = markdown_preview::from_node_with_source(node, cx.node_source(node))?;
        let source = cx.node_source(node).unwrap_or(&diagram.source).to_owned();
        Some(
            MarkdownNode::new(
                self.name().to_owned(),
                DiagramNode {
                    diagram: diagram.clone(),
                    inline,
                },
            )
            .text(diagram.source)
            .markdown(source),
        )
    }
    fn render(&self, node: &MarkdownNode, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(data) = node.data::<DiagramNode>() else {
            return div().into_any_element();
        };
        let preview = self.preview(&data.diagram, cx);
        match &preview {
            Preview::Image(image, width, height, _) => {
                if data.inline {
                    img(image.clone())
                        .w(px(*width as f32 / 2.0))
                        .h(px(*height as f32 / 2.0))
                        .object_fit(ObjectFit::Contain)
                        .into_any_element()
                } else {
                    div()
                        .w_full()
                        .py_2()
                        .child(
                            div()
                                .w_full()
                                .max_w(px(*width as f32 / 2.0))
                                .aspect_ratio(*width as f32 / *height as f32)
                                .child(
                                    img(image.clone())
                                        .size_full()
                                        .object_fit(ObjectFit::Contain),
                                ),
                        )
                        .into_any_element()
                }
            }
            Preview::Error(_) if data.inline => div()
                .text_color(cx.theme().muted_foreground)
                .child(data.diagram.source.clone())
                .into_any_element(),
            Preview::Error(error) => {
                let mut end = data
                    .diagram
                    .source
                    .len()
                    .min(glim_services::limits::TEXT_LAYOUT_RUN_BYTES);
                while !data.diagram.source.is_char_boundary(end) {
                    end -= 1;
                }
                div()
                    .w_full()
                    .p_3()
                    .bg(cx.theme().muted)
                    .text_color(cx.theme().muted_foreground)
                    .child(div().text_sm().child(error.clone()))
                    .child(div().mt_2().child(data.diagram.source[..end].to_owned()))
                    .when(end < data.diagram.source.len(), |view| {
                        view.child(glim_services::locale::tr(
                            "Complete source is available in Source view.",
                        ))
                    })
                    .into_any_element()
            }
            Preview::Pending if data.inline => div()
                .text_color(cx.theme().muted_foreground)
                .child(data.diagram.source.clone())
                .into_any_element(),
            Preview::Pending => div()
                .py_2()
                .child(
                    h_flex()
                        .gap_2()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().muted.opacity(0.6))
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(div().text_color(cx.theme().muted_foreground).child("◷"))
                        .child(glim_services::locale::tr("Rendering diagram…")),
                )
                .into_any_element(),
        }
    }
    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        let data = node.data::<DiagramNode>()?;
        let preview = self.preview(&data.diagram, cx);
        if let Preview::Image(image, width, height, baseline) = &preview {
            let scale = context.font_size() / px(16.0);
            Some(
                InlineElement::new(
                    img(image.clone())
                        .w(px(*width as f32 / 2.0) * scale)
                        .h(px(*height as f32 / 2.0) * scale)
                        .object_fit(ObjectFit::Contain),
                )
                .with_baseline(px(*baseline / 2.0) * scale),
            )
        } else {
            Some(InlineElement::new(self.render(node, window, cx)))
        }
    }
}
