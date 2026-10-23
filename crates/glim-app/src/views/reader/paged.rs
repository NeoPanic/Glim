use glim_services::paged::{PageSource, TextPage};
use gpui_kit::{
    component::{ActiveTheme, Disableable, button::Button, h_flex, v_flex},
    prelude::FluentBuilder,
    *,
};
use std::ops::Range;

type PreviewAction = std::rc::Rc<dyn Fn(&mut App)>;

/// Virtual rows contain slices into one buffer, not one allocation per line.
/// Paging replaces the buffer, so browsing an unbounded file stays bounded.
pub(super) struct PagedReader {
    pub source_editor: Option<super::source::SourceReader>,
    pub controls_bottom: Pixels,
    pub return_to_preview: Option<PreviewAction>,
    pub saving: std::rc::Rc<std::cell::Cell<bool>>,
    page: TextPage,
    _source_owner: Option<glim_services::work::CancelOnDrop>,
    report_source: Option<PageSource>,
    rows: Vec<Range<usize>>,
    widest_row: Option<usize>,
    previous: Vec<u64>,
    task: Option<Task<()>>,
    error: Option<String>,
    scroll: UniformListScrollHandle,
}
impl PagedReader {
    pub fn new(page: TextPage, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let source_editor = page.source.is_editable().then(|| {
            super::source::SourceReader::new(
                &page.text,
                "plain",
                false,
                ScrollHandle::new(),
                window,
                cx,
            )
        });
        Self::with_editor(page, source_editor)
    }
    pub fn readonly(page: TextPage) -> Self {
        Self::with_editor(page, None)
    }
    pub fn text_only(page: TextPage) -> Self {
        let mut view = Self::readonly(page);
        view.report_source = None;
        view
    }
    fn with_editor(mut page: TextPage, source_editor: Option<super::source::SourceReader>) -> Self {
        let rows = rows(&page.text);
        let widest_row = widest_row(&page.text, &rows);
        Self {
            _source_owner: page.owner.take(),
            source_editor,
            controls_bottom: px(16.),
            return_to_preview: None,
            saving: std::rc::Rc::new(std::cell::Cell::new(false)),
            report_source: page.source.is_generated().then(|| page.source.clone()),
            widest_row,
            page,
            rows,
            previous: Vec::new(),
            task: None,
            error: None,
            scroll: UniformListScrollHandle::new(),
        }
    }
    pub fn refresh_request(&self) -> Option<glim_services::paged::PageRefresh> {
        self.source_editor.as_ref()?;
        Some(glim_services::paged::PageRefresh {
            source: self.page.source.clone(),
            start: self.page.start,
            total: self.page.total,
            modified: self.page.modified,
            identity: self.page.identity,
        })
    }
    pub fn request_save_before_preview(&mut self, cx: &mut Context<Self>) {
        self.error = Some(glim_services::locale::tr(
            "Save this page before navigating.",
        ));
        cx.notify();
    }
    pub fn save_request(&mut self) -> Option<glim_services::paged::PageEdit> {
        self.source_editor.as_ref()?;
        self.saving.set(true);
        Some(glim_services::paged::PageEdit {
            path: self.page.path.clone(),
            start: self.page.start,
            end: self.page.end,
            total: self.page.total,
            modified: self.page.modified,
            identity: self.page.identity,
            expected: self.page.text.clone(),
        })
    }
    pub fn saved(
        &mut self,
        text: String,
        saved: glim_services::paged::PageSaved,
        cx: &mut Context<Self>,
    ) {
        self.saving.set(false);
        if let Some(editor) = &mut self.source_editor {
            editor.mark_saved(text.clone(), cx);
        }
        self.page.text = text;
        self.page.end = saved.end;
        self.page.total = saved.total;
        self.page.modified = saved.modified;
        self.page.identity = saved.identity;
        cx.notify();
    }
    fn toggle_raw(&mut self, cx: &mut Context<Self>) {
        glim_services::telemetry::track("paged_mode_changed", []);
        if self.task.is_some() {
            return;
        }
        let Some(report) = self.report_source.clone() else {
            return;
        };
        let source = if self.page.source.is_generated() {
            report.raw()
        } else {
            report
        };
        let telemetry_started = std::time::Instant::now();
        let work = cx.background_executor().spawn(async move {
            let page = source.read(0, false)?;
            let rows = rows(&page.text);
            let widest = widest_row(&page.text, &rows);
            Ok::<_, anyhow::Error>((page, rows, widest))
        });
        self.task = Some(cx.spawn(async move |view, cx| {
            let result = work.await;
            glim_services::telemetry::result("paged_load", None, telemetry_started, &result);
            let _ = view.update(cx, |view, cx| {
                view.task = None;
                match result {
                    Ok((page, rows, widest)) => {
                        view.page = page;
                        view.rows = rows;
                        view.widest_row = widest;
                        view.previous.clear();
                        view.error = None;
                        view.scroll = UniformListScrollHandle::new();
                    }
                    Err(error) => {
                        glim_services::telemetry::operation_failed("paged_load", None, &error);
                        view.error = Some(format!("{error:#}"));
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn load(
        &mut self,
        start: u64,
        all: bool,
        back: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        glim_services::telemetry::reader_feature(
            "paged",
            if all {
                "expand_page"
            } else if back {
                "previous_page"
            } else {
                "next_page"
            },
            [(
                "binary",
                glim_services::telemetry::flag(self.page.source.binary),
            )],
        );
        if self.task.is_some() || self.saving.get() {
            return;
        }
        if self
            .source_editor
            .as_ref()
            .is_some_and(|editor| editor.is_dirty())
        {
            self.error = Some(glim_services::locale::tr(
                "Save this page before navigating.",
            ));
            cx.notify();
            return;
        }
        let source = self.page.source.clone();
        let stamp = (self.page.total, self.page.modified, self.page.identity);
        let telemetry_started = std::time::Instant::now();
        let (cancel, guard) = glim_services::work::Cancellation::task();
        self.task = Some(cx.spawn_in(window, async move |view, cx| {
            let _guard = guard;
            let result = cx
                .background_executor()
                .spawn(async move {
                    let page = source.read_cancellable(start, all, &cancel)?;
                    anyhow::ensure!(
                        page.source.is_generated()
                            || (page.total, page.modified, page.identity) == stamp,
                        "File changed on disk. Reopen or refresh before paging."
                    );
                    let rows = rows(&page.text);
                    let widest = widest_row(&page.text, &rows);
                    Ok::<_, anyhow::Error>((page, rows, widest))
                })
                .await;
            glim_services::telemetry::result("paged_load", None, telemetry_started, &result);
            let _ = view.update_in(cx, |v, window, cx| {
                v.task = None;
                match result {
                    Ok((page, rows, widest)) => {
                        if back {
                            v.previous.pop();
                        } else if page.start != v.page.start {
                            v.previous.push(v.page.start);
                        }
                        if let Some(editor) = &mut v.source_editor {
                            editor.state.update(cx, |state, cx| {
                                state.set_value(page.text.clone(), window, cx)
                            });
                            editor.mark_saved(page.text.clone(), cx);
                        }
                        v.widest_row = widest;
                        v.page = page;
                        v.rows = rows;
                        v.error = None;
                        v.scroll = UniformListScrollHandle::new();
                    }
                    Err(e) => {
                        glim_services::telemetry::operation_failed("paged_load", None, &e);
                        v.error = Some(format!("{e:#}"));
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
fn rows(text: &str) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let mut start = 0;
    // Cap individual layout runs, including minified multi-megabyte JSON lines.
    for (offset, ch) in text.char_indices() {
        if ch == '\n' || offset - start >= glim_services::limits::TEXT_LAYOUT_RUN_BYTES {
            result.push(start..offset);
            start = if ch == '\n' { offset + 1 } else { offset };
        }
    }
    if start < text.len() {
        result.push(start..text.len());
    }
    result
}
// Select once per loaded buffer, outside render. The list measures this row
// with the active font and padding, and clamps its extent to the viewport.
fn widest_row(text: &str, rows: &[Range<usize>]) -> Option<usize> {
    rows.iter()
        .enumerate()
        .max_by_key(|(_, range)| text[(*range).clone()].chars().count())
        .map(|(index, _)| index)
}
impl Render for PagedReader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.task.is_some() || self.saving.get();
        let previous = self.previous.last().copied();
        let next = self.page.end;
        v_flex()
            .relative()
            .size_full()
            .min_h_0()
            .children(self.error.iter().map(|e| {
                div()
                    .px_3()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(e.clone())
            }))
            .child(if let Some(editor) = &mut self.source_editor {
                editor.render(cx)
            } else {
                uniform_list(
                    "large-text",
                    self.rows.len()
                        + ((f32::from(self.controls_bottom) + 64.) / 22.).ceil() as usize,
                    cx.processor(|v, range: Range<usize>, _, cx| {
                        range
                            .map(|i| {
                                if i >= v.rows.len() {
                                    return div().h(px(22.));
                                }
                                div()
                                    .h(px(22.))
                                    .px_3()
                                    .text_sm()
                                    .font_family("Menlo")
                                    .whitespace_nowrap()
                                    .text_color(cx.theme().foreground)
                                    .child(v.page.text[v.rows[i].clone()].to_owned())
                            })
                            .collect()
                    }),
                )
                .with_width_from_item(self.widest_row)
                .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                .track_scroll(&self.scroll)
                .flex_1()
                .min_h_0()
                .w_full()
                .into_any_element()
            })
            .child(super::controls::floating_at(
                "paged-controls",
                h_flex()
                    .px_0()
                    .py_0()
                    .gap_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().min_w_0().truncate().child(format!(
                        "{} · bytes {}–{} / {}{}",
                        glim_services::locale::tr(if self.source_editor.is_some() {
                            "Editable page"
                        } else {
                            "Read-only preview"
                        }),
                        self.page.start,
                        self.page.end,
                        self.page.total,
                        if self.page.total_known { "" } else { "+" }
                    )))
                    .when_some(self.return_to_preview.clone(), |bar, action| {
                        bar.child(
                            Button::new("page-return-preview")
                                .label(glim_services::locale::tr("Preview"))
                                .on_click(move |_, _, cx| action(cx)),
                        )
                    })
                    .when(self.source_editor.is_none(), |bar| {
                        bar.child(
                            Button::new("copy-reader-page")
                                .label(glim_services::locale::tr("Copy page"))
                                .disabled(busy)
                                .on_click(cx.listener(|view, _, _, cx| {
                                    glim_services::telemetry::reader_feature(
                                        "paged",
                                        "copy_page",
                                        [],
                                    );
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        view.page.text.clone(),
                                    ));
                                })),
                        )
                    })
                    .when(self.report_source.is_some(), |bar| {
                        bar.child(
                            Button::new("page-raw")
                                .label(glim_services::locale::tr(
                                    if self.page.source.is_generated() {
                                        "File bytes"
                                    } else {
                                        "Report"
                                    },
                                ))
                                .disabled(busy)
                                .on_click(cx.listener(|view, _, _, cx| view.toggle_raw(cx))),
                        )
                    })
                    .child(
                        Button::new("page-back")
                            .label(glim_services::locale::tr("Previous"))
                            .disabled(busy || previous.is_none())
                            .on_click(cx.listener(move |v, _, w, cx| {
                                if let Some(offset) = previous {
                                    v.load(offset, false, true, w, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("page-next")
                            .label(glim_services::locale::tr("Next page"))
                            .disabled(busy || !self.page.more)
                            .on_click(
                                cx.listener(move |v, _, w, cx| v.load(next, false, false, w, cx)),
                            ),
                    )
                    .child(
                        Button::new("page-all")
                            .label(glim_services::locale::tr("Expand this page"))
                            .disabled(
                                busy || self.page.source.binary
                                    || (self.page.start == 0 && !self.page.more),
                            )
                            .on_click(
                                cx.listener(|v, _, w, cx| v.load(v.page.start, true, false, w, cx)),
                            ),
                    ),
                self.controls_bottom,
                cx,
            ))
    }
}
