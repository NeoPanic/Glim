use super::Workspace;
use gpui_kit::*;
use std::path::{Path, PathBuf};

impl Workspace {
    pub(super) fn choose_new_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let directory = self
            .active_reader()
            .and_then(|reader| {
                reader
                    .draft_directory
                    .clone()
                    .or_else(|| reader.path.parent().map(Path::to_path_buf))
            })
            .or_else(|| self.root.clone())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));
        self.choose_new_file_in(directory, window, cx);
    }

    pub(super) fn choose_new_file_in(
        &mut self,
        directory: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.picking
            || self.close_request.is_some()
            || crate::app::updates::busy(cx)
            || crate::app::workspace_editors_locked(cx)
        {
            return;
        }
        glim_services::telemetry::track("draft_created", []);
        let reader = crate::views::reader::Reader::untitled(directory, window, cx);
        reader.set_word_wrap(self.word_wrap, window, cx);
        self.load_task = None;
        self.loading = false;
        self.loading_preview = false;
        self.page = super::Page::Document;
        self.error = None;
        self.tabs.push(reader);
        self.active = Some(self.tabs.len() - 1);
        self.tab_scroll.scroll_to_item(self.tabs.len() - 1);
        self.observe_autosave(window, cx);
        self.tabs.last().unwrap().focus_source(window, cx);
        cx.notify();
    }

    pub(super) fn confirm_trash(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.confirm_task.is_some() || crate::app::workspace_editors_locked(cx) {
            return;
        }
        let title = glim_services::locale::tr("Move to Trash?");
        let detail = format!(
            "{}\n\n{}",
            path.display(),
            glim_services::locale::tr(
                "This item and its contents will be moved to the macOS Trash. You can restore it from Finder."
            )
        );
        let response = window.prompt(
            PromptLevel::Warning,
            &title,
            Some(&detail),
            &[
                glim_services::locale::tr("Cancel").as_str(),
                glim_services::locale::tr("Move to Trash").as_str(),
            ],
            cx,
        );
        self.confirm_task = Some(cx.spawn_in(window, async move |view, cx| {
            let accepted = response.await == Ok(1);
            let _ = view.update_in(cx, |view, window, cx| {
                view.confirm_task = None;
                if accepted {
                    view.trash(path, window, cx);
                }
            });
        }));
    }

    fn trash(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // Share the cross-window mutation lock to prevent autosave racing
        // the move. Recheck after the confirmation dialog.
        if self.has_unsaved() || self.is_saving() {
            self.error = Some(glim_services::locale::tr("Save your changes and wait for active operations before moving an item to Trash.").into());
            cx.notify();
            return;
        }
        if let Err(error) = crate::app::lock_workspace_editors(cx.entity().entity_id(), cx) {
            self.error = Some(error.to_string().into());
            cx.notify();
            return;
        }
        self.set_editing_locked(true, cx);
        let target = path.clone();
        let work = cx.background_executor().spawn(async move {
            let started = std::time::Instant::now();
            let result = glim_services::workspace::trash_entry(&target);
            glim_services::telemetry::result("move_to_trash", Some(&target), started, &result);
            result
        });
        self.confirm_task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = work.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.confirm_task = None;
                view.set_editing_locked(false, cx);
                crate::app::unlock_workspace_editors(cx.entity().entity_id(), cx);
                match result {
                    Ok(()) => {
                        view.did_trash(&path, window, cx);
                        crate::app::finish_trash_in_other_windows(
                            cx.entity().entity_id(),
                            &path,
                            cx,
                        );
                    }
                    Err(error) => {
                        view.error = Some(
                            format!(
                                "{}: {error:#}",
                                glim_services::locale::tr("Could not move item to Trash")
                            )
                            .into(),
                        );
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(crate) fn did_trash(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_task = None;
        self.load_task = None;
        self.incoming_file_task = None;
        self.loading = false;
        self.incoming_files.retain(|p| !p.starts_with(path));
        self.search_task = None;
        self.results.retain(|p| !p.starts_with(path));
        let targets = self
            .tabs
            .iter()
            .filter(|reader| reader.path.starts_with(path))
            .map(super::header::Target::from_reader)
            .collect();
        self.close_tabs_unchecked(targets, window, cx);
        let roots = self
            .explorer
            .as_ref()
            .map(|e| e.read(cx).root_ids_under(path))
            .unwrap_or_default();
        for id in roots {
            self.remove_folder(id, window, cx);
        }
        self.refresh(window, cx);
        cx.notify();
    }
}
