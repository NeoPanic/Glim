use super::{Workspace, loading::Content};
use glim_services::watch::WorkspaceWatch;
use gpui_kit::*;
use std::time::Duration;

impl Workspace {
    pub(super) fn start_watching(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.watch_task = Some(cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                if view
                    .update_in(cx, |view, window, cx| {
                        if !view.loading
                            && view.refresh_task.is_none()
                            && view
                                .watch
                                .as_ref()
                                .is_some_and(WorkspaceWatch::take_changed)
                        {
                            view.refresh_files(window, cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
    pub(super) fn watch_file_parent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parents = self
            .tabs
            .iter()
            .filter(|r| r.draft_directory.is_none())
            .filter_map(|r| r.path.parent().map(ToOwned::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        let work = cx.background_executor().spawn(async move {
            let mut parents = parents.into_iter();
            let parent = parents
                .next()
                .ok_or_else(|| anyhow::anyhow!("No open files"))?;
            let mut watch = WorkspaceWatch::new(&parent)?;
            for parent in parents {
                watch.add_path(&parent)?;
            }
            Ok::<_, anyhow::Error>(watch)
        });
        self.watch_setup = Some(cx.spawn_in(window, async move |view, cx| {
            let result = work.await;
            let _ = view.update_in(cx, |view, window, cx| {
                if let Ok(watch) = result {
                    view.watch = Some(watch);
                    view.start_watching(window, cx);
                }
            });
        }));
    }
    pub(super) fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_files(window, cx);
    }

    fn refresh_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.refresh_task.is_some() || self.watch_scan_task.is_some() || self.is_saving() {
            return;
        }
        for reader in &mut self.tabs {
            reader.refresh_dependencies(cx);
        }
        let targets = self
            .tabs
            .iter()
            .filter(|r| {
                r.draft_directory.is_none()
                    && (r.snapshot != "native-preview" || r.native_image_stamp(cx).is_some())
            })
            .map(|r| {
                (
                    r.path.clone(),
                    r.native_image_stamp(cx),
                    r.page_refresh_request(cx),
                )
            })
            .collect::<Vec<_>>();
        if let Some(explorer) = &self.explorer {
            explorer.update(cx, |v, cx| v.refresh(cx));
        }
        let (cancel, cancel_on_drop) = glim_services::work::Cancellation::task();
        let read = cx.background_executor().spawn(async move {
            targets
                .into_iter()
                .filter_map(|(path, image_stamp, page_refresh)| {
                    if let Some(expected) = image_stamp
                        && std::fs::metadata(&path)
                            .is_ok_and(|m| (m.len(), m.modified().ok()) == expected)
                    {
                        return None;
                    }
                    let content = if let Some(request) = page_refresh {
                        match request.read(&cancel) {
                            Ok(None) => return None,
                            Ok(Some(page)) => Ok(Content::Page(page)),
                            Err(error) => Err(error),
                        }
                    } else {
                        super::loading::read_content_cancellable(&path, &cancel)
                    };
                    Some((
                        path.clone(),
                        content.unwrap_or_else(|e| Content::Unavailable(path, format!("{e:#}"))),
                    ))
                })
                .collect::<Vec<_>>()
        });
        self.refresh_task = Some(cx.spawn_in(window, async move |view, cx| {
            let _cancel_on_drop = cancel_on_drop;
            let contents = read.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.refresh_task = None;
                for (path, content) in contents {
                    let Some(index) = view.tabs.iter().position(|r| r.path == path) else {
                        continue;
                    };
                    // Never replace a draft, even if this refresh started before typing.
                    if view.tabs[index].is_dirty() || view.is_saving() {
                        continue;
                    }
                    let unchanged = match &content {
                        Content::File(d) => view.tabs[index].snapshot == d.text,
                        Content::Image(d) => view.tabs[index].snapshot == d.fingerprint,
                        Content::Opml(d) => view.tabs[index].snapshot == d.fingerprint,
                        Content::XMind(d) => view.tabs[index].snapshot == d.fingerprint,
                        Content::Book(d) => view.tabs[index].snapshot == d.fingerprint,
                        Content::Page(d) => view.tabs[index].snapshot == d.fingerprint(),
                        Content::Native(..) | Content::Sqlite(..) => false,
                        Content::Unavailable(_, _) => false,
                    };
                    if unchanged && !view.tabs[index].is_unavailable() {
                        continue;
                    }
                    let mut reader = Self::make_reader(content, window, cx);
                    reader.inherit_view(&view.tabs[index], cx);
                    reader.set_word_wrap(view.word_wrap, window, cx);
                    reader.set_editing_locked(view.editing_locked, cx);
                    view.tabs[index] = reader;
                }
                view.observe_autosave(window, cx);
                cx.notify();
            });
        }));
    }
}
