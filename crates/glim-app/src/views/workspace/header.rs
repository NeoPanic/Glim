use super::Workspace;
use glim_services::limits as reader_limits;
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Sizable, TitleBar,
        button::{Button, ButtonCustomVariant, ButtonVariants},
        h_flex,
        input::Input,
        v_flex,
    },
    prelude::FluentBuilder,
    *,
};
use std::path::PathBuf;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Target {
    pub path: PathBuf,
    pub diff: Option<glim_core::GitChange>,
    pub root: Option<PathBuf>,
    pub commit_id: Option<String>,
    pub commit_file: Option<PathBuf>,
}
impl Target {
    pub(super) fn from_reader(reader: &crate::views::reader::Reader) -> Self {
        Self {
            path: reader.path.clone(),
            diff: reader.diff.clone(),
            root: reader.repository_root.clone(),
            commit_id: reader.commit_id.clone(),
            commit_file: reader.commit_file.clone(),
        }
    }
}
impl Workspace {
    pub(super) fn record_history(&mut self) {
        if self.navigating {
            self.navigating = false;
            return;
        }
        let Some(r) = self.active_reader() else {
            return;
        };
        let target = Target::from_reader(r);
        if self.history_cursor.and_then(|i| self.history.get(i)) == Some(&target) {
            return;
        }
        self.history
            .truncate(self.history_cursor.map_or(0, |i| i + 1));
        self.history.push(target);
        self.history_cursor = Some(self.history.len() - 1);
    }
    pub(super) fn navigate(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cursor) = self.history_cursor else {
            return;
        };
        let next = if forward {
            cursor + 1
        } else {
            cursor.saturating_sub(1)
        };
        let Some(target) = self.history.get(next).cloned() else {
            return;
        };
        if next == cursor {
            return;
        }
        self.history_cursor = Some(next);
        self.navigating = true;
        if let (Some(root), Some(oid)) = (target.root.clone(), target.commit_id) {
            self.open_commit_file(root, oid, target.commit_file, window, cx);
        } else if let (Some(root), Some(change)) = (target.root, target.diff) {
            self.open_diff(root, change, window, cx);
        } else {
            self.open_path(target.path, window, cx);
        }
    }
    pub(super) fn search_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_offset = 0;
        self.load_search_page(window, cx);
    }
    fn load_search_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().to_string();
        self.results.clear();
        self.search_task = None;
        self.search_more = false;
        if query.trim().is_empty() {
            self.search_offset = 0;
            cx.notify();
            return;
        }
        let roots = self.roots.clone();
        let offset = self.search_offset;
        let (cancel, guard) = glim_services::work::Cancellation::task();
        self.search_task = Some(cx.spawn_in(window, async move |view, cx| {
            let _guard = guard;
            cx.background_executor()
                .timer(std::time::Duration::from_millis(150))
                .await;
            let result = cx
                .background_executor()
                .spawn(async move {
                    glim_services::workspace::search_page(&roots, &query, offset, &cancel)
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                match result {
                    Ok(page) => {
                        view.results = page.paths;
                        view.search_more = page.more;
                    }
                    Err(error) => view.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub(super) fn toggle_word_wrap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.word_wrap = !self.word_wrap;
        let enabled = self.word_wrap;
        cx.global_mut::<crate::app::ReaderPreferences>().word_wrap = enabled;
        for reader in &self.tabs {
            reader.set_word_wrap(enabled, window, cx);
        }
        let write = cx
            .background_executor()
            .spawn(async move { glim_services::preferences::save_word_wrap(enabled) });
        self.preference_task = Some(cx.spawn(async move |view, cx| {
            if let Err(error) = write.await {
                let _ = view.update(cx, |v, cx| {
                    v.error = Some(format!("Cannot remember word wrap: {error:#}").into());
                    cx.notify();
                });
            }
        }));
        cx.notify();
    }

    pub(super) fn set_reader_preview(
        &mut self,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.active else {
            return;
        };
        let preference = self.tabs[index].preview_preference();
        self.tabs[index].set_preview(preview, cx);
        if (self.tabs[index].toggle_label() == "Source") != preview {
            cx.notify();
            return;
        }
        if !preview {
            self.tabs[index].focus_source(window, cx);
        }
        if let Some(kind) = preference {
            let prefs = cx.global_mut::<crate::app::ReaderPreferences>();
            match kind {
                "html" => prefs.html_preview = preview,
                "csv" => prefs.csv_preview = preview,
                _ => prefs.markdown_preview = preview,
            }
            let previous = prefs.preview_save_task.take();
            let task = cx.spawn(async move |view, cx| {
                if let Some(previous) = previous {
                    previous.await;
                }
                let result = cx
                    .background_executor()
                    .spawn(
                        async move { glim_services::preferences::save_preview_mode(kind, preview) },
                    )
                    .await;
                if let Err(error) = result {
                    let _ = view.update(cx, |v, cx| {
                        v.error = Some(format!("Cannot remember reading mode: {error:#}").into());
                        cx.notify();
                    });
                }
            });
            cx.global_mut::<crate::app::ReaderPreferences>()
                .preview_save_task = Some(task);
        }
        cx.notify();
    }

    pub(super) fn set_diff_mode(&mut self, side_by_side: bool, cx: &mut Context<Self>) {
        let Some(index) = self.active else {
            return;
        };
        if side_by_side && !self.tabs[index].split_available() {
            return;
        }
        self.tabs[index].set_side_by_side(side_by_side);
        let prefs = cx.global_mut::<crate::app::ReaderPreferences>();
        prefs.diff_side_by_side = Some(side_by_side);
        let previous = prefs.preview_save_task.take();
        let task = cx.spawn(async move |view, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let result = cx
                .background_executor()
                .spawn(async move { glim_services::preferences::save_diff_mode(side_by_side) })
                .await;
            if let Err(error) = result {
                let _ = view.update(cx, |v, cx| {
                    v.error = Some(format!("Cannot remember diff mode: {error:#}").into());
                    cx.notify();
                });
            }
        });
        cx.global_mut::<crate::app::ReaderPreferences>()
            .preview_save_task = Some(task);
        for reader in &mut self.tabs {
            reader.set_side_by_side(side_by_side);
        }
        cx.refresh_windows();
        cx.notify();
    }

    pub(super) fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.file_fullscreen {
            let name = self
                .active_reader()
                .map(|reader| {
                    reader
                        .path
                        .file_name()
                        .unwrap_or(reader.path.as_os_str())
                        .to_string_lossy()
                        .into_owned()
                })
                .unwrap_or_default();
            return workspace_title_bar(
                h_flex()
                    .size_full()
                    .pl(px(94.))
                    .pr_3()
                    .gap_3()
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(name))
                    .child(self.workspace_toolbar(cx)),
                cx,
            );
        }
        let search = h_flex()
            .relative()
            .w(relative(0.34))
            .max_w(px(420.))
            .child(
                Input::new(&self.search)
                    .small()
                    .h(px(34.))
                    .px_3()
                    .text_size(px(14.))
                    .rounded(px(10.))
                    .bg(cx
                        .theme()
                        .background
                        .blend(cx.theme().foreground.opacity(0.025)))
                    .border_color(cx.theme().border.opacity(0.65))
                    .focus_bordered(true)
                    .w_full(),
            )
            .when(!self.results.is_empty() || self.search_offset > 0, |bar| {
                bar.child(
                    deferred(
                        v_flex()
                            .id("file-search-results")
                            .absolute()
                            .top(px(42.))
                            .right_0()
                            .w_full()
                            .max_h(px(280.))
                            .overflow_y_scroll()
                            .p_1()
                            .rounded(px(10.))
                            .shadow_lg()
                            .occlude()
                            .bg(cx.theme().background)
                            .border_1()
                            .border_color(cx.theme().border)
                            .on_mouse_down_out(cx.listener(|v, _, _, cx| {
                                v.results.clear();
                                v.search_offset = 0;
                                v.search_more = false;
                                v.search_task = None;
                                cx.notify();
                            }))
                            .children(self.results.iter().enumerate().map(|(i, path)| {
                                let target = path.clone();
                                let label = path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned();
                                let parent = path.parent().unwrap_or(path);
                                let root = self
                                    .roots
                                    .iter()
                                    .filter(|root| path.starts_with(root))
                                    .max_by_key(|root| root.components().count());
                                let location = root
                                    .and_then(|root| root.parent())
                                    .and_then(|base| parent.strip_prefix(base).ok())
                                    .unwrap_or(parent)
                                    .to_string_lossy()
                                    .into_owned();
                                Button::new(("search-result", i))
                                    .custom(
                                        ButtonCustomVariant::new(cx).hover(cx.theme().secondary),
                                    )
                                    .small()
                                    .w_full()
                                    .rounded(px(6.))
                                    .h(px(44.))
                                    .tooltip(path.to_string_lossy().into_owned())
                                    .accessibility_label(format!(
                                        "{} {}",
                                        glim_services::locale::tr("Open"),
                                        path.display()
                                    ))
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .min_w_0()
                                            .gap_2()
                                            .child(crate::views::file_icons::file_icon(path))
                                            .child(
                                                v_flex()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .gap_1()
                                                    .child(div().truncate().text_sm().child(label))
                                                    .child(
                                                        div()
                                                            .truncate()
                                                            .text_xs()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(location),
                                                    ),
                                            ),
                                    )
                                    .on_click(cx.listener(move |v, _, w, cx| {
                                        v.search.update(cx, |s, cx| s.set_value("", w, cx));
                                        v.results.clear();
                                        v.search_offset = 0;
                                        v.search_more = false;
                                        v.search_task = None;
                                        v.sidebar = super::Sidebar::Files;
                                        v.sidebar_visible = true;
                                        v.open_path(target.clone(), w, cx);
                                    }))
                            }))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .p_2()
                                    .child(
                                        Button::new("search-previous")
                                            .ghost()
                                            .small()
                                            .label(glim_services::locale::tr("Previous"))
                                            .disabled(self.search_offset == 0)
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.search_offset =
                                                    view.search_offset.saturating_sub(
                                                        reader_limits::WORKSPACE_SEARCH_RESULTS,
                                                    );
                                                view.load_search_page(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("search-next")
                                            .ghost()
                                            .small()
                                            .label(glim_services::locale::tr("Next page"))
                                            .disabled(!self.search_more)
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.search_offset =
                                                    view.search_offset.saturating_add(
                                                        reader_limits::WORKSPACE_SEARCH_RESULTS,
                                                    );
                                                view.load_search_page(window, cx);
                                            })),
                                    ),
                            ),
                    )
                    .with_priority(1),
                )
            });
        v_flex()
            .relative()
            .w_full()
            .flex_shrink_0()
            .on_key_down(cx.listener(|v, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    v.results.clear();
                    v.search_offset = 0;
                    v.search_more = false;
                    v.search_task = None;
                    cx.notify();
                }
            }))
            .child(workspace_title_bar(
                h_flex()
                    .relative()
                    .w_full()
                    .h_full()
                    .justify_center()
                    .child(
                        h_flex()
                            .absolute()
                            .left(px(94.))
                            .top_0()
                            .h_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .h(px(28.))
                                    .flex()
                                    .items_center()
                                    .text_size(px(16.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Glim"),
                            )
                            .child(div().w(px(1.)).h(px(16.)).mx_1().bg(cx.theme().border))
                            .child(self.navigation_toolbar(cx)),
                    )
                    .child(search)
                    .child(
                        h_flex()
                            .absolute()
                            .right(px(12.))
                            .top_0()
                            .h_full()
                            .gap_1()
                            .child(self.workspace_toolbar(cx))
                            .when(self.saving.is_some() && self.save_cancel.is_some(), |row| {
                                row.child(
                                    Button::new("cancel-file-save")
                                        .ghost()
                                        .small()
                                        .label(glim_services::locale::tr("Cancel save"))
                                        .on_click(
                                            cx.listener(|view, _, _, cx| view.cancel_save(cx)),
                                        ),
                                )
                            }),
                    ),
                cx,
            ))
            .into_any_element()
    }
}

fn workspace_title_bar(content: impl IntoElement, cx: &App) -> AnyElement {
    TitleBar::new()
        .bg(cx.theme().sidebar)
        .border_b_0()
        .h(px(48.))
        .pl_0()
        .pr_0()
        .child(content)
        .into_any_element()
}
