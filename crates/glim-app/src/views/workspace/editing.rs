use super::Workspace;
use glim_services::workspace::FolderSnapshot;
use gpui_kit::*;

pub(super) enum DiscardAction {
    CloseTab(usize),
    CloseTabs(Vec<super::header::Target>),
    CloseWindow,
    Folder(FolderSnapshot),
    Quit,
}
pub(super) struct CloseRequest {
    action: DiscardAction,
    discarded: Vec<EntityId>,
}
impl Workspace {
    pub(crate) fn can_begin_quit(&self) -> bool {
        self.confirm_task.is_none() && self.close_request.is_none() && !self.picking
    }
    pub(crate) fn quit_is_approved(&self) -> bool {
        self.quit_approval == Some(self.edit_revision)
    }
    pub(crate) fn reset_quit_approval(&mut self) {
        self.quit_approval = None;
        self.quit_discards.clear();
    }
    pub(crate) fn commit_quit(&mut self, cx: &App) {
        for reader in &self.tabs {
            if reader
                .editor()
                .is_some_and(|editor| self.quit_discards.contains(&editor.entity_id()))
                && let Some(key) = &reader.recovery_key
            {
                crate::app::recovery::remove(key, cx);
            }
        }
    }
    fn discard_recovery(&mut self, cx: &App) {
        for reader in &self.tabs {
            if let Some(key) = &reader.recovery_key {
                crate::app::recovery::remove(key, cx);
            }
        }
        self.recovery_keys.clear();
    }
    fn cancel_close_request(&mut self, cx: &mut Context<Self>) {
        if self
            .close_request
            .take()
            .is_some_and(|request| matches!(request.action, DiscardAction::Quit))
        {
            cx.defer(crate::app::cancel_quit);
        }
    }
    pub fn has_unsaved(&self) -> bool {
        self.tabs.iter().any(|r| r.is_dirty())
    }
    pub fn is_saving(&self) -> bool {
        self.saving.is_some()
            || self.editing_locked
            || self.tabs.iter().any(|r| r.is_persisting_book())
    }

    pub(super) fn cancel_save(&mut self, cx: &mut Context<Self>) {
        self.save_cancel = None;
        if let Some(id) = self.saving {
            self.save_queue.retain(|queued| *queued != id);
            if let Some((_, pending)) = self.autosave.get_mut(&id) {
                *pending = None;
            }
        }
        cx.notify();
    }
    pub(crate) fn open_document_paths(&self) -> Vec<std::path::PathBuf> {
        self.tabs
            .iter()
            .filter(|reader| reader.draft_directory.is_none())
            .map(|reader| reader.path.clone())
            .collect()
    }
    pub(crate) fn report_recovery_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        cx.notify();
    }
    pub(crate) fn restore_draft(
        &mut self,
        draft: glim_services::drafts::Draft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut reader = crate::views::reader::Reader::untitled(draft.directory, window, cx);
        reader.recovery_key = Some(draft.id);
        reader.title = draft.title.into();
        reader.set_word_wrap(self.word_wrap, window, cx);
        reader.restore_draft_text(draft.text, window, cx);
        glim_services::telemetry::track("draft_recovered", []);
        self.edit_revision = self.edit_revision.wrapping_add(1);
        self.tabs.push(reader);
        self.active = Some(self.tabs.len() - 1);
        self.page = super::Page::Document;
        self.observe_autosave(window, cx);
        self.tabs.last().unwrap().focus_source(window, cx);
        self.tab_scroll.scroll_to_item(self.tabs.len() - 1);
        cx.notify();
    }
    fn checkpoint_editor(&self, id: EntityId, cx: &App) {
        if let Some(reader) = self.tabs.iter().find(|reader| {
            reader
                .editor()
                .is_some_and(|editor| editor.entity_id() == id)
        }) && let (Some(key), Some(editor)) = (&reader.recovery_key, reader.editor())
        {
            crate::app::recovery::checkpoint(
                glim_services::drafts::Draft {
                    id: key.clone(),
                    title: if reader.draft_directory.is_some() {
                        reader.title.to_string()
                    } else {
                        reader
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    },
                    directory: reader
                        .draft_directory
                        .clone()
                        .or_else(|| reader.path.parent().map(ToOwned::to_owned))
                        .unwrap_or_default(),
                    text: editor.read(cx).value().to_string(),
                },
                cx,
            );
        }
    }
    fn finish_recovery(&mut self, id: EntityId, cx: &App) {
        if let Some(reader) = self.tabs.iter_mut().find(|reader| {
            reader
                .editor()
                .is_some_and(|editor| editor.entity_id() == id)
        }) && reader.draft_directory.is_none()
            && !reader.is_dirty()
            && let Some(key) = &reader.recovery_key
        {
            crate::app::recovery::remove(key, cx);
        }
    }

    pub(super) fn save_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(reader) = self.active_reader() {
            glim_services::telemetry::file_event(
                "file_save_requested",
                &reader.path,
                [(
                    "untitled",
                    glim_services::telemetry::flag(reader.draft_directory.is_some()),
                )],
            );
        }
        if let Some(editor) = self.active_reader().and_then(|r| r.editor()) {
            self.save_editor(editor.entity_id(), window, cx);
        }
    }

    pub(super) fn observe_autosave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let keys = self
            .tabs
            .iter()
            .filter_map(|reader| Some((reader.editor()?.entity_id(), reader.recovery_key.clone()?)))
            .collect::<std::collections::HashMap<_, _>>();
        for (id, key) in &self.recovery_keys {
            if keys.get(id) != Some(key) {
                crate::app::recovery::remove(key, cx);
            }
        }
        self.recovery_keys = keys;
        let editors = self
            .tabs
            .iter()
            .filter_map(|r| r.editor())
            .collect::<Vec<_>>();
        self.telemetry_edited
            .retain(|id| editors.iter().any(|editor| editor.entity_id() == *id));
        self.autosave
            .retain(|id, _| editors.iter().any(|e| e.entity_id() == *id));
        for editor in editors {
            let id = editor.entity_id();
            self.autosave.entry(id).or_insert_with(|| {
                let subscription =
                    cx.subscribe_in(&editor, window, move |view, _, event, window, cx| {
                        if matches!(event, gpui_kit::component::input::InputEvent::Change) {
                            if view.telemetry_edited.insert(id)
                                && let Some(reader) = view.tabs.iter().find(|reader| {
                                    reader
                                        .editor()
                                        .is_some_and(|editor| editor.entity_id() == id)
                                })
                            {
                                glim_services::telemetry::file_event(
                                    "editing_started",
                                    &reader.path,
                                    [(
                                        "untitled",
                                        glim_services::telemetry::flag(
                                            reader.draft_directory.is_some(),
                                        ),
                                    )],
                                );
                            }
                            view.edit_revision = view.edit_revision.wrapping_add(1);
                            if let Some(request) = &mut view.close_request {
                                request.discarded.retain(|discarded| *discarded != id);
                            }
                            view.checkpoint_editor(id, cx);
                            if view.tabs.iter().any(|reader| {
                                reader.draft_directory.is_some()
                                    && reader
                                        .editor()
                                        .is_some_and(|editor| editor.entity_id() == id)
                            }) {
                                return;
                            }
                            let task = cx.spawn_in(window, async move |view, cx| {
                                cx.background_executor()
                                    .timer(std::time::Duration::from_millis(800))
                                    .await;
                                let _ = view.update_in(cx, |view, window, cx| {
                                    view.save_editor(id, window, cx);
                                });
                            });
                            if let Some((_, pending)) = view.autosave.get_mut(&id) {
                                *pending = Some(task);
                            }
                        }
                    });
                (subscription, None)
            });
        }
    }

    pub(super) fn save_editor(
        &mut self,
        id: EntityId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editing_locked {
            return;
        }
        if self.saving.is_some() {
            if !self.save_queue.contains(&id) {
                self.save_queue.push_back(id);
            }
            return;
        }
        let Some(reader) = self.tabs.iter().find(|r| {
            (r.is_dirty() || r.draft_directory.is_some())
                && r.editor().is_some_and(|e| e.entity_id() == id)
        }) else {
            return;
        };
        if reader.draft_directory.is_some() {
            self.save_draft(id, window, cx);
            return;
        }
        let editor = reader.editor().unwrap();
        let path = reader.path.clone();
        let expected = reader.snapshot.clone();
        let page = reader.page_save_request(cx);
        let text = editor.read(cx).value().to_string();
        self.saving = Some(id);
        self.error = None;
        // A pre-save read must not arrive later and replace the just-saved buffer.
        self.refresh_task = None;
        let (cancel, guard) = glim_services::work::Cancellation::task();
        self.save_cancel = Some(guard);
        let work = cx.background_executor().spawn(async move {
            let _scope = glim_services::diagnostics::FileScope::enter(&path, "save");
            log::info!("save begin file={}", path.display());
            let started = std::time::Instant::now();
            let result = if let Some(page) = page {
                glim_services::paged::save_page_cancellable(&page, &text, &cancel).map(Some)
            } else {
                glim_services::files::save_document_cancellable(&path, &expected, &text, &cancel)
                    .map(|()| None)
            };
            glim_services::telemetry::result("file_save", Some(&path), started, &result);
            match &result {
                Ok(_) => log::info!("save complete file={}", path.display()),
                Err(error) => log::error!("save failed file={} error={error:#}", path.display()),
            }
            (text, result)
        });
        self.save_task = Some(cx.spawn_in(window, async move |view, cx| {
            let (text, result) = work.await;
            let _ = view.update_in(cx, |v, window, cx| {
                let cancelled = v.save_cancel.take().is_none();
                v.saving = None;
                match result {
                    Ok(page) => {
                        if let Some(reader) = v
                            .tabs
                            .iter_mut()
                            .find(|r| r.editor().is_some_and(|s| s.entity_id() == id))
                        {
                            if let Some(page) = page {
                                reader.page_saved(text, page, cx);
                            } else {
                                reader.mark_saved(text, cx);
                            }
                        }
                        v.finish_recovery(id, cx);
                        v.observe_autosave(window, cx);
                        if v.tabs.iter().any(|reader| {
                            reader.is_dirty()
                                && reader
                                    .editor()
                                    .is_some_and(|editor| editor.entity_id() == id)
                        }) && !v.save_queue.contains(&id)
                        {
                            v.save_queue.push_back(id);
                        }
                        v.refresh(window, cx);
                    }
                    Err(error) => {
                        v.cancel_close_request(cx);
                        if let Some(reader) = v
                            .tabs
                            .iter()
                            .find(|r| r.editor().is_some_and(|e| e.entity_id() == id))
                        {
                            reader.finish_page_save();
                        }
                        if !cancelled {
                            v.error = Some(format!("Save failed: {error:#}").into());
                        }
                    }
                }
                while v.saving.is_none() {
                    let Some(next) = v.save_queue.pop_front() else {
                        break;
                    };
                    v.save_editor(next, window, cx);
                }
                v.continue_close(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn save_draft(&mut self, id: EntityId, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking || crate::app::updates::busy(cx) {
            self.cancel_close_request(cx);
            return;
        }
        let Some(reader) = self.tabs.iter().find(|reader| {
            reader
                .editor()
                .is_some_and(|editor| editor.entity_id() == id)
        }) else {
            return;
        };
        let directory = reader.draft_directory.clone().unwrap();
        let name = if std::path::Path::new(reader.title.as_ref())
            .extension()
            .is_some()
        {
            reader.title.to_string()
        } else {
            format!("{}.txt", reader.title)
        };
        glim_services::telemetry::track("save_dialog_opened", []);
        let selection = cx.prompt_for_new_path(&directory, Some(&name));
        self.picking = true;
        self.saving = Some(id);
        self.refresh_task = None;
        let (cancel, guard) = glim_services::work::Cancellation::task();
        self.save_cancel = Some(guard);
        self.save_task = Some(cx.spawn_in(window, async move |view, cx| {
            let mut selection = selection;
            let result = loop {
                let selected = selection.await;
                match selected {
                    Ok(Ok(Some(path))) => {
                        if !glim_services::files::is_text_save_path(&path) {
                            glim_services::telemetry::file_event("save_format_rejected", &path, []);
                            let response = view.update_in(cx, |_, window, cx| window.prompt(
                                PromptLevel::Info,
                                &glim_services::locale::tr("Choose a text filename"),
                                Some(&glim_services::locale::tr("This note contains plain text. Use .txt, .md or a source-code extension; changing its name does not create a Word document, PDF or image.")),
                                &[glim_services::locale::tr("Choose Another Name").as_str(), glim_services::locale::tr("Cancel").as_str()], cx)).ok();
                            if let Some(response) = response && response.await == Ok(0) {
                                let proposed = path.with_extension("txt");
                                let next = view.update(cx, |_, cx| cx.prompt_for_new_path(
                                    proposed.parent().unwrap_or(std::path::Path::new("/")), proposed.file_name().and_then(|name| name.to_str())));
                                if let Ok(next) = next { selection = next; continue; }
                            }
                            break None;
                        }
                        let open = cx.update(|_, cx| crate::app::open_document_paths(cx)).unwrap_or_default();
                        let preparation = cancel.clone();
                        let destination = cx.background_executor().spawn(async move {
                            let result = glim_services::files::prepare_draft_destination(&path, &open, &preparation);
                            if let Err(error) = &result { glim_services::telemetry::operation_failed("prepare_save_destination", Some(&path), error); }
                            result
                        }).await;
                        let destination = match destination { Ok(destination) => destination, Err(error) => break Some(Err(error)) };
                        let text = view.update(cx, |view, cx| {
                            view.save_cancel.as_ref()?;
                            view.tabs.iter().find_map(|reader| reader.editor().filter(|editor| editor.entity_id() == id))
                                .map(|editor| editor.read(cx).value().to_string())
                        }).ok().flatten();
                        if let Some(text) = text {
                            let result = cx.background_executor().spawn(async move {
                                let started = std::time::Instant::now();
                                let result = glim_services::files::save_new_document(&destination, &text, &cancel);
                                glim_services::telemetry::result("draft_save", None, started, &result);
                                let path = result?;
                                Ok::<_, anyhow::Error>((path, text))
                            }).await;
                            break Some(result);
                        }
                        break None;
                    }
                    Ok(Ok(None)) => { glim_services::telemetry::track("save_dialog_cancelled", []); break None; },
                    other => break Some(Err(anyhow::anyhow!("Cannot choose save location: {other:?}"))),
                }
            };
            let _ = view.update_in(cx, |view, window, cx| {
                view.picking = false;
                view.saving = None;
                view.save_cancel = None;
                match result {
                    Some(Ok((path, text))) => {
                        if let Some(reader) = view.tabs.iter_mut().find(|reader| {
                            reader
                                .editor()
                                .is_some_and(|editor| editor.entity_id() == id)
                        }) {
                            let old = super::header::Target::from_reader(reader);
                            reader.bind_saved_draft(path, text, window, cx);
                            reader.set_word_wrap(view.word_wrap, window, cx);
                            let new = super::header::Target::from_reader(reader);
                            if let Some(CloseRequest {
                                action: DiscardAction::CloseTabs(targets),
                                ..
                            }) = &mut view.close_request
                            {
                                for target in targets {
                                    if *target == old {
                                        *target = new.clone();
                                    }
                                }
                            }
                            if view.active_reader().is_some_and(|reader| {
                                reader
                                    .editor()
                                    .is_some_and(|editor| editor.entity_id() == id)
                            }) {
                                view.reveal_active_file(cx);
                                view.record_history();
                            }
                        }
                        view.finish_recovery(id, cx);
                        view.observe_autosave(window, cx);
                        if view.tabs.iter().any(|reader| reader.is_dirty() && reader.editor().is_some_and(|editor| editor.entity_id() == id))
                            && !view.save_queue.contains(&id) { view.save_queue.push_back(id); }
                        view.error = None;
                        if view.root.is_none() {
                            view.watch_file_parent(window, cx);
                        }
                        view.refresh(window, cx);
                    }
                    other => {
                        view.cancel_close_request(cx);
                        // Cancel a batch of draft-save dialogs as a unit; ordinary
                        // file autosaves may continue without opening more panels.
                        let drafts = view.tabs.iter().filter(|reader| reader.draft_directory.is_some())
                            .filter_map(|reader| reader.editor().map(|editor| editor.entity_id()))
                            .collect::<Vec<_>>();
                        view.save_queue.retain(|queued| *queued != id && !drafts.contains(queued));
                        if let Some(Err(error)) = other {
                            view.error = Some(
                                format!("{}: {error:#}", glim_services::locale::tr("Save failed"))
                                    .into(),
                            );
                        }
                    }
                }
                while view.saving.is_none() {
                    let Some(next) = view.save_queue.pop_front() else {
                        break;
                    };
                    view.save_editor(next, window, cx);
                }
                view.continue_close(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn confirm_discard(
        &mut self,
        action: DiscardAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_saving()
            || matches!(action, DiscardAction::Quit)
                && crate::app::any_other_saving(cx.entity_id(), cx)
        {
            self.error = Some(
                "A file operation is in progress. Please wait before closing or changing folders."
                    .into(),
            );
            if matches!(action, DiscardAction::Quit) {
                cx.defer(crate::app::cancel_quit);
            }
            cx.notify();
            return;
        }
        if self.confirm_task.is_some() || self.close_request.is_some() {
            return;
        }
        let action = match action {
            DiscardAction::CloseTab(index) => {
                let Some(reader) = self.tabs.get(index) else {
                    return;
                };
                DiscardAction::CloseTabs(vec![super::header::Target::from_reader(reader)])
            }
            other => other,
        };
        self.close_request = Some(CloseRequest {
            action,
            discarded: Vec::new(),
        });
        self.continue_close(window, cx);
    }

    fn continue_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving.is_some() || self.confirm_task.is_some() {
            return;
        }
        let Some(request) = &self.close_request else {
            return;
        };
        let next = self.tabs.iter().find_map(|reader| {
            let editor = reader.editor()?;
            let id = editor.entity_id();
            let selected = match &request.action {
                DiscardAction::CloseTabs(targets) => {
                    targets.contains(&super::header::Target::from_reader(reader))
                }
                _ => true,
            };
            (selected && reader.is_dirty() && !request.discarded.contains(&id))
                .then(|| (id, reader.title.clone()))
        });
        let Some((id, title)) = next else {
            let request = self.close_request.take().unwrap();
            if matches!(request.action, DiscardAction::Quit) {
                self.quit_approval = Some(self.edit_revision);
                self.quit_discards = request.discarded;
                cx.defer(crate::app::continue_quit);
                return;
            }
            self.apply_discard_action(request.action, window, cx);
            return;
        };
        let prompt = glim_services::locale::tr("Save changes before closing?");
        let response = window.prompt(
            PromptLevel::Warning,
            &prompt,
            Some(&title),
            &[
                glim_services::locale::tr("Save").as_str(),
                glim_services::locale::tr("Don't Save").as_str(),
                glim_services::locale::tr("Cancel").as_str(),
            ],
            cx,
        );
        self.confirm_task = Some(cx.spawn_in(window, async move |view, cx| {
            let response = response.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.confirm_task = None;
                glim_services::telemetry::track(
                    "close_decision",
                    [(
                        "decision",
                        glim_services::telemetry::text(match response {
                            Ok(0) => "save",
                            Ok(1) => "discard",
                            _ => "cancel",
                        }),
                    )],
                );
                match response {
                    Ok(0) => {
                        view.save_editor(id, window, cx);
                        if view.saving.is_none() {
                            view.continue_close(window, cx);
                        }
                    }
                    Ok(1) => {
                        if let Some(request) = &mut view.close_request {
                            request.discarded.push(id);
                        }
                        view.continue_close(window, cx);
                    }
                    _ => view.cancel_close_request(cx),
                }
                cx.notify();
            });
        }));
    }

    fn apply_discard_action(
        &mut self,
        action: DiscardAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            DiscardAction::CloseTab(index) => self.close_tab_unchecked(index, window, cx),
            DiscardAction::CloseTabs(targets) => self.close_tabs_unchecked(targets, window, cx),
            DiscardAction::CloseWindow => {
                self.discard_recovery(cx);
                window.remove_window();
            }
            DiscardAction::Folder(folder) => self.replace_folder(folder, window, cx),
            DiscardAction::Quit => cx.defer(crate::app::continue_quit),
        }
    }
    pub fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(DiscardAction::Quit, window, cx);
    }
    pub(super) fn request_close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if crate::app::updates::intercept_last_window_close(cx) {
            return;
        }
        if self.has_unsaved() || self.is_saving() {
            self.confirm_discard(DiscardAction::CloseWindow, window, cx);
        } else {
            window.remove_window();
        }
    }
    pub(super) fn receive_folder(
        &mut self,
        folder: FolderSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.has_unsaved() || self.is_saving() {
            self.confirm_discard(DiscardAction::Folder(folder), window, cx);
        } else {
            self.replace_folder(folder, window, cx);
        }
    }
    fn replace_folder(
        &mut self,
        folder: FolderSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.discard_recovery(cx);
        self.tabs.clear();
        self.autosave.clear();
        self.save_queue.clear();
        self.active = None;
        self.history.clear();
        self.history_cursor = None;
        self.install_folder(folder, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::Workspace;
    use gpui_kit::{
        AppContext, Bounds, Point, TestAppContext, WindowBounds, WindowOptions, px, size,
        test::TestWindowExt,
    };

    #[gpui_kit::test]
    fn editing_save_refresh_and_close_keep_the_draft_safe(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "original\n").unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.bind_keys([gpui_kit::KeyBinding::new(
                "secondary-s",
                crate::app::actions::SaveFile,
                None,
            )]);
        });
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1000.), px(700.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| Workspace::new(Some(path.clone()), window, cx)),
            )
            .unwrap()
        });
        cx.run_until_parked();
        let editor = cx.update(|cx| view.read(cx).active_reader().unwrap().editor().unwrap());
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click(("input", editor.entity_id()), cx);
            window.press("secondary-a", cx);
            window.input("edited\n", cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert!(cx.update(|cx| view.read(cx).has_unsaved()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original\n");
        // Adding folders must preserve the open editor and its draft.
        let other = tempfile::tempdir().unwrap();
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| {
                for root in [dir.path(), other.path(), dir.path()] {
                    v.add_folder(
                        glim_services::workspace::open_folder(root).unwrap(),
                        window,
                        cx,
                    );
                }
                assert_eq!(
                    v.roots,
                    [
                        dir.path().canonicalize().unwrap(),
                        other.path().canonicalize().unwrap(),
                        dir.path().canonicalize().unwrap(),
                    ]
                );
                assert_eq!(
                    v.active_reader().unwrap().editor().unwrap().entity_id(),
                    editor.entity_id()
                );
                assert!(v.has_unsaved());
            });
        })
        .unwrap();
        assert!(cx.update(|cx| {
            view.read(cx)
                .autosave
                .get(&editor.entity_id())
                .is_some_and(|(_, task)| task.is_some())
        }));
        // A debounce callback targets the edited document even after leaving its tab.
        cx.update_window(handle, |_, _, cx| {
            view.update(cx, |v, cx| {
                v.active = None;
                cx.notify();
            });
        })
        .unwrap();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(800));
        cx.run_until_parked();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "edited\n");
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| v.activate_tab(0, window, cx));
            window.click(("input", editor.entity_id()), cx);
            window.press("secondary-a", cx);
            window.input("manually saved\n", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| window.press("secondary-s", cx))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "manually saved\n");
        assert!(!cx.update(|cx| view.read(cx).has_unsaved()));
        // Undo survives Save, and is itself an unsaved edit.
        cx.update_window(handle, |_, window, cx| window.press("secondary-z", cx))
            .unwrap();
        cx.run_until_parked();
        assert!(cx.update(|cx| view.read(cx).has_unsaved()));
        std::fs::write(&path, "external\n").unwrap();
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| v.refresh(window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        assert_ne!(
            cx.update(|cx| editor.read(cx).value().to_string()),
            "external\n"
        );
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| v.save_active(window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external\n");
        assert!(cx.update(|cx| view.read(cx).has_unsaved()));
        assert!(cx.update(|cx| view.read(cx).error.is_some()));
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |v, cx| v.close_tab(0, window, cx))
        })
        .unwrap();
        cx.run_until_parked();
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| view.read(cx).tabs.len()), 1);
        assert!(cx.update(|cx| view.read(cx).has_unsaved()));
    }
}
