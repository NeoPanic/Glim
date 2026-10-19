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
impl Workspace {
    pub fn has_unsaved(&self) -> bool {
        self.tabs.iter().any(|r| r.is_dirty())
    }
    pub fn is_saving(&self) -> bool {
        self.saving.is_some()
            || self.git_busy
            || self.git_edit_locked
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
    pub(super) fn save_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_reader().and_then(|r| r.editor()) {
            self.save_editor(editor.entity_id(), window, cx);
        }
    }

    pub(super) fn observe_autosave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editors = self
            .tabs
            .iter()
            .filter_map(|r| r.editor())
            .collect::<Vec<_>>();
        self.autosave
            .retain(|id, _| editors.iter().any(|e| e.entity_id() == *id));
        for editor in editors {
            let id = editor.entity_id();
            self.autosave.entry(id).or_insert_with(|| {
                let subscription =
                    cx.subscribe_in(&editor, window, move |view, _, event, window, cx| {
                        if matches!(event, gpui_kit::component::input::InputEvent::Change) {
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
        if self.git_edit_locked {
            return;
        }
        if self.saving.is_some() {
            if !self.save_queue.contains(&id) {
                self.save_queue.push_back(id);
            }
            return;
        }
        let Some(reader) = self
            .tabs
            .iter()
            .find(|r| r.is_dirty() && r.editor().is_some_and(|e| e.entity_id() == id))
        else {
            return;
        };
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
            let result = if let Some(page) = page {
                glim_services::paged::save_page_cancellable(&page, &text, &cancel).map(Some)
            } else {
                glim_services::files::save_document_cancellable(&path, &expected, &text, &cancel)
                    .map(|()| None)
            };
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
                        v.refresh(window, cx);
                    }
                    Err(error) => {
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
        if self.is_saving() {
            self.error = Some(
                "A save or Git operation is in progress. Please wait before closing or changing folders."
                    .into(),
            );
            cx.notify();
            return;
        }
        if self.confirm_task.is_some() {
            return;
        }
        let all = matches!(action, DiscardAction::Quit);
        let response = window.prompt(PromptLevel::Warning,
            &glim_services::locale::tr(if all { "Quit with unsaved changes?" } else { "Discard unsaved changes?" }),
            Some(&glim_services::locale::tr("Your edits have not been saved. Cancel and save the file first, or discard the edits to continue.")),
            &[glim_services::locale::tr("Cancel").as_str(), glim_services::locale::tr(if all { "Discard All and Quit" } else { "Discard Changes" }).as_str()], cx);
        self.confirm_task = Some(cx.spawn_in(window, async move |view, cx| {
            let discard = response.await == Ok(1);
            let _ = view.update_in(cx, |v, window, cx| {
                v.confirm_task = None;
                if discard {
                    v.apply_discard_action(action, window, cx);
                }
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
            DiscardAction::CloseWindow => window.remove_window(),
            DiscardAction::Folder(folder) => self.replace_folder(folder, window, cx),
            DiscardAction::Quit => {
                if self.is_saving() || crate::app::any_other_saving(cx.entity().entity_id(), cx) {
                    self.error =
                        Some("A window has an active save or Git operation. Please wait before quitting.".into());
                    cx.notify();
                } else {
                    cx.quit();
                }
            }
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
        self.tabs.clear();
        self.autosave.clear();
        self.save_queue.clear();
        self.active = None;
        self.history.clear();
        self.history_cursor = None;
        self.sidebar = super::Sidebar::Files;
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
