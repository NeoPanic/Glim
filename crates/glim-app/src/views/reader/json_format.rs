use glim_services::locale::tr;
use gpui_kit::{
    component::{ActiveTheme, Disableable, Sizable, button::Button, h_flex, input::EditorState},
    *,
};

pub(super) struct JsonFormat {
    editor: Option<Entity<EditorState>>,
    path: std::path::PathBuf,
    graph: Option<Entity<super::xmind::XMindReader>>,
    visible: bool,
    focus: Option<FocusHandle>,
    pub(super) blocked: bool,
    graph_task: Option<Task<()>>,
    cancel: Option<glim_services::work::CancelOnDrop>,
    task: Option<Task<()>>,
    error: Option<String>,
}
impl JsonFormat {
    pub fn new(path: std::path::PathBuf, editor: Option<Entity<EditorState>>) -> Self {
        Self {
            editor,
            path,
            graph: None,
            visible: false,
            focus: None,
            blocked: false,
            graph_task: None,
            cancel: None,
            task: None,
            error: None,
        }
    }
    fn format(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.clone() else {
            return;
        };
        if self.task.is_some() || !editor.read(cx).context_menu_capabilities().is_editable() {
            return;
        }
        let original = editor.read(cx).value().to_string();
        let requested = original.clone();
        self.error = None;
        let work = cx
            .background_executor()
            .spawn(async move { glim_services::json::format_document(&requested) });
        self.task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = work.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.task = None;
                match result {
                    Ok(formatted) => {
                        // Do not overwrite typing or a new read-only state while work ran.
                        let Some(editor_entity) = view.editor.clone() else {
                            return;
                        };
                        let editor = editor_entity.read(cx);
                        if editor.value().as_ref() == original
                            && editor.context_menu_capabilities().is_editable()
                        {
                            if formatted != original {
                                editor_entity.update(cx, |editor, cx| {
                                    editor.replace_all(formatted, window, cx)
                                });
                            }
                        } else {
                            view.error = Some(tr("Content changed; click Format again."));
                        }
                    }
                    Err(error) => view.error = Some(format!("{}: {error}", tr("Invalid JSON"))),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
impl JsonFormat {
    fn toggle_graph(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible {
            self.visible = false;
            self.cancel = None;
            self.graph_task = None;
            if let Some(editor) = &self.editor {
                editor.update(cx, |editor, cx| editor.focus(window, cx));
            }
            cx.notify();
            return;
        }
        if self.blocked {
            return;
        }
        self.visible = true;
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle());
        window.focus(focus, cx);
        self.error = None;
        self.graph = None;
        let text = self
            .editor
            .as_ref()
            .map(|editor| editor.read(cx).value().to_string());
        let original = text.clone();
        let path = self.path.clone();
        let (cancel, guard) = glim_services::work::Cancellation::task();
        self.cancel = Some(guard);
        let work = cx
            .background_executor()
            .spawn(async move { glim_services::graph::read(&path, text, &cancel) });
        self.graph_task = Some(cx.spawn(async move |view, cx| {
            let result = work.await;
            let _ = view.update(cx, |view, cx| {
                view.graph_task = None;
                view.cancel = None;
                if !view.visible { return; }
                if let (Some(editor), Some(original)) = (&view.editor, original)
                    && editor.read(cx).value().as_ref() != original {
                        view.error = Some(tr("Content changed; reopen the graph."));
                        cx.notify();
                        return;
                }
                match result {
                    Ok(document) => view.graph = Some(cx.new(|_| super::xmind::XMindReader::embedded(document))),
                    Err(_) => view.error = Some(tr("Cannot visualize this content. Check syntax or use Source for larger files.")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
impl Render for JsonFormat {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let can_format = self
            .path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            && self
                .editor
                .as_ref()
                .is_some_and(|e| e.read(cx).context_menu_capabilities().is_editable());
        let mut buttons = h_flex().gap_2().items_center().child(
            Button::new("graph-mode")
                .small()
                .disabled(self.blocked && !self.visible)
                .label(tr(if self.visible {
                    "Back to document"
                } else {
                    "Interactive graph"
                }))
                .on_click(cx.listener(|view, _, window, cx| view.toggle_graph(window, cx))),
        );
        if !self.visible && can_format {
            buttons = buttons.child(
                Button::new("format-json")
                    .small()
                    .label(tr("Format"))
                    .disabled(self.task.is_some())
                    .on_click(cx.listener(|view, _, window, cx| view.format(window, cx))),
            );
        }
        if let Some(graph) = self.graph.clone().filter(|_| self.visible) {
            for (id, label, action) in [
                ("graph-out", "−", 0),
                ("graph-in", "+", 1),
                ("graph-reset", "Reset view", 2),
                ("graph-fold", "Collapse all", 3),
                ("graph-expand", "Expand all", 4),
                ("graph-copy", "Copy", 5),
            ] {
                let graph = graph.clone();
                buttons = buttons.child(Button::new(id).small().label(tr(label)).on_click(
                    move |_, _, cx| {
                        graph.update(cx, |graph, cx| match action {
                            0 => graph.change_zoom(-0.1, cx),
                            1 => graph.change_zoom(0.1, cx),
                            2 => graph.reset_view(cx),
                            3 => graph.fold_all(true, cx),
                            4 => graph.fold_all(false, cx),
                            _ => graph.copy_selected(cx),
                        });
                    },
                ));
            }
        }
        if self.blocked && !self.visible {
            buttons = buttons.child(
                div()
                    .text_sm()
                    .child(tr("Save changes before visualizing paged content.")),
            );
        }
        if let Some(error) = self.error.clone() {
            buttons = buttons.child(
                div()
                    .max_w(px(480.))
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error),
            );
        }
        let toolbar = super::controls::floating("structured-controls", buttons, cx);
        if !self.visible {
            return toolbar.into_any_element();
        }
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        div()
            .track_focus(&focus)
            .absolute()
            .inset_0()
            .bg(cx.theme().background)
            .child(match &self.graph {
                Some(graph) => graph.clone().into_any_element(),
                None if self.graph_task.is_some() => super::preview_loading(cx),
                None => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_6()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.error.clone().unwrap_or_default())
                    .into_any_element(),
            })
            .child(toolbar)
            .into_any_element()
    }
}
