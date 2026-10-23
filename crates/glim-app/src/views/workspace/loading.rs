use super::Workspace;
use crate::views::{
    explorer::{Explorer, ExplorerEvent},
    reader::Reader,
};
use glim_core::Document;
use glim_services::limits as reader_limits;
use glim_services::{
    files::read_document,
    workspace::{FolderSnapshot, open_folder},
};
use gpui_kit::*;
use std::path::PathBuf;

enum Opened {
    Folder(FolderSnapshot),
    File(PathBuf, anyhow::Result<Box<Content>>),
}
pub(super) enum Content {
    File(Document),
    Page(glim_services::paged::TextPage),
    Image(glim_services::media::ImageDocument),
    XMind(glim_services::xmind::MindMap),
    Opml(glim_services::opml::Document),
    Book(glim_services::books::Book),
    Native(PathBuf, bool, bool),
    Sqlite(PathBuf),
    Unavailable(PathBuf, String),
}
impl Workspace {
    pub(super) fn csv_preview_for_path(path: &std::path::Path, cx: &App) -> bool {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("csv") || ext.eq_ignore_ascii_case("tsv"))
            && cx
                .try_global::<crate::app::ReaderPreferences>()
                .is_some_and(|prefs| prefs.csv_preview)
    }

    pub(crate) fn restore_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loading = true;
        self.loading_preview = false;
        let work = cx.background_executor().spawn(async {
            let paths = glim_services::preferences::workspace_folders()?;
            let expanded = glim_services::preferences::explorer_expansion(&paths);
            let folders = paths
                .into_iter()
                .map(|path| open_folder(&path))
                .collect::<Vec<_>>();
            Ok::<_, anyhow::Error>((folders, expanded))
        });
        self.load_task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = work.await;
            let _ = view.update_in(cx, |view, window, cx| {
                let mut errors = Vec::new();
                match result {
                    Ok((folders, expanded)) => {
                        for folder in folders {
                            match folder {
                                Ok(folder) => view.add_folder(folder, window, cx),
                                Err(error) => errors.push(format!("{error:#}")),
                            }
                        }
                        match expanded {
                            Ok(paths) => {
                                if let Some(explorer) = &view.explorer {
                                    explorer.update(cx, |explorer, cx| {
                                        explorer.restore_expanded(paths, cx)
                                    });
                                }
                            }
                            Err(error) => errors.push(format!("{error:#}")),
                        }
                    }
                    Err(error) => errors.push(format!("{error:#}")),
                }
                view.loading = false;
                if !view.roots.is_empty()
                    && cx.try_global::<crate::app::RecentWorkspaces>().is_some()
                {
                    crate::app::remember_workspace(view.recent_id.clone(), view.roots.clone(), cx);
                }
                view.load_incoming_file(window, cx);
                if !errors.is_empty() {
                    view.error =
                        Some(format!("Cannot restore workspace: {}", errors.join("; ")).into());
                }
                cx.notify();
            });
        }));
    }
    pub(crate) fn reveal_file_path(
        &mut self,
        path: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let index = self.tabs.iter().position(|r| r.path == path);
        if let Some(index) = index {
            self.activate_tab(index, window, cx);
            self.tabs[index].reveal_native(cx);
            window.activate_window();
            cx.activate(true);
            true
        } else {
            false
        }
    }
    pub(crate) fn open_system_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // All Finder file types share one queue. Extension-based routing here
        // would split ordinary text/images and native formats across windows.
        if self.reveal_file_path(&path, window, cx) {
            return true;
        }
        if !self.incoming_files.contains(&path) {
            self.incoming_files.push_back(path);
        }
        self.load_incoming_file(window, cx);
        window.activate_window();
        true
    }
    pub(super) fn load_incoming_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.incoming_file_task.is_some() || self.loading {
            return;
        }
        let Some(path) = self.incoming_files.front().cloned() else {
            return;
        };
        let (cancel, cancel_on_drop) = glim_services::work::Cancellation::task();
        let work = cx.background_executor().spawn(async move {
            let started = std::time::Instant::now();
            glim_services::telemetry::file_event("file_open_requested", &path, []);
            if path.is_dir() && !glim_services::preview::is_iwork(&path) {
                let result = open_folder(&path);
                glim_services::telemetry::result("workspace_open", None, started, &result);
                result.map(Opened::Folder)
            } else {
                let result = read_content_cancellable(&path, &cancel).map(Box::new);
                glim_services::telemetry::result("file_open", Some(&path), started, &result);
                Ok(Opened::File(path, result))
            }
        });
        self.incoming_file_task = Some(cx.spawn_in(window, async move |view, cx| {
            let _cancel_on_drop = cancel_on_drop;
            let result = work.await;
            let _ = view.update_in(cx, |view, window, cx| {
                let path = view.incoming_files.pop_front();
                view.incoming_file_task = None;
                if let Some(index) = view
                    .tabs
                    .iter()
                    .position(|r| Some(&r.path) == path.as_ref())
                {
                    view.activate_tab(index, window, cx);
                } else {
                    match result {
                        Ok(Opened::Folder(folder)) => view.receive_folder(folder, window, cx),
                        Ok(Opened::File(_, Ok(content))) => {
                            view.install_content(*content, window, cx)
                        }
                        Ok(Opened::File(path, Err(error))) => view.install_content(
                            Content::Unavailable(path, format!("{error:#}")),
                            window,
                            cx,
                        ),
                        Err(error) => view.error = Some(format!("{error:#}").into()),
                    }
                }
                view.load_incoming_file(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        glim_services::diagnostics::event(
            "INFO",
            &format!("open requested file={}", path.display()),
        );
        self.page = super::Page::Document;
        if crate::app::updates::busy(cx) {
            return;
        }
        self.results.clear();
        self.search_task = None;
        self.pending_anchor = None;
        if let Some(index) = self.tabs.iter().position(|t| t.path == path) {
            self.activate_tab(index, window, cx);
            self.tabs[index].reveal_native(cx);
            return;
        }
        if crate::app::reveal_existing_file(&path, Some(cx.entity_id()), cx) {
            return;
        }
        glim_services::telemetry::file_event("file_open_requested", &path, []);
        self.restoring_roots = None;
        self.loading = true;
        self.loading_preview = Self::csv_preview_for_path(&path, cx);
        self.error = None;
        let (cancel, cancel_on_drop) = glim_services::work::Cancellation::task();
        let read = cx.background_executor().spawn(async move {
            let started = std::time::Instant::now();
            if path.is_dir() && !glim_services::preview::is_iwork(&path) {
                let result = open_folder(&path);
                glim_services::telemetry::result("workspace_open", None, started, &result);
                result.map(Opened::Folder)
            } else {
                let result = read_content_cancellable(&path, &cancel).map(Box::new);
                glim_services::telemetry::result("file_open", Some(&path), started, &result);
                Ok(Opened::File(path, result))
            }
        });
        self.load_task = Some(cx.spawn_in(window, async move |view, cx| {
            let _cancel_on_drop = cancel_on_drop;
            let result = read.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.loading = false;
                view.load_incoming_file(window, cx);
                match result {
                    Ok(Opened::Folder(folder)) => {
                        view.receive_folder(folder, window, cx);
                    }
                    Ok(Opened::File(_, Ok(document))) => {
                        view.install_content(*document, window, cx)
                    }
                    Ok(Opened::File(path, Err(error))) => view.install_content(
                        Content::Unavailable(path, format!("{error:#}")),
                        window,
                        cx,
                    ),
                    Err(error) => view.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub(super) fn install_folder(
        &mut self,
        snapshot: FolderSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.subscriptions.clear();
        self.explorer = None;
        self.roots.clear();
        self.root = None;
        self.add_folder(snapshot, window, cx);
    }

    pub(super) fn add_folder(
        &mut self,
        snapshot: FolderSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if crate::app::updates::busy(cx) {
            return;
        }
        self.sidebar_visible = true;
        self.roots.push(snapshot.root.clone());
        glim_services::telemetry::track(
            "workspace_folder_added",
            [(
                "folder_count",
                glim_services::telemetry::number(self.roots.len() as u64),
            )],
        );
        self.root = self.roots.first().cloned();
        if !self.loading && cx.try_global::<crate::app::ReaderPreferences>().is_some() {
            crate::app::remember_workspace(self.recent_id.clone(), self.roots.clone(), cx);
        }
        self.refresh_task = None;
        self.error = None;
        self.search_task = None;
        self.watch_setup = None;
        self.watch_task = None;
        self.watch = None;
        self.watch_scan_task = None;
        if let Some(explorer) = &self.explorer {
            explorer.update(cx, |v, cx| {
                v.add_folder(snapshot.root, snapshot.entries, cx)
            });
        } else {
            let explorer = cx.new(|cx| Explorer::new(snapshot.root, snapshot.entries, cx));
            self.subscriptions.push(cx.subscribe_in(
                &explorer,
                window,
                |view, _, event, window, cx| match event {
                    ExplorerEvent::SelectionChanged => {
                        cx.notify();
                    }
                    ExplorerEvent::OpenFile(path) => view.open_path(path.clone(), window, cx),
                    ExplorerEvent::NewFile(path) => {
                        view.choose_new_file_in(path.clone(), window, cx)
                    }
                    ExplorerEvent::TrashEntry(path) => view.confirm_trash(path.clone(), window, cx),
                    ExplorerEvent::RemoveFolder(root_id) => {
                        view.remove_folder(*root_id, window, cx)
                    }
                    ExplorerEvent::PlayFolder(path, shuffle) => {
                        view.install_content(
                            Content::Native(path.clone(), true, *shuffle),
                            window,
                            cx,
                        );
                    }
                    ExplorerEvent::ExpansionChanged(roots, expanded) => {
                        crate::app::remember_explorer(roots.clone(), expanded.clone(), cx);
                    }
                    ExplorerEvent::Error(error) => {
                        view.error = Some(error.clone().into());
                        cx.notify();
                    }
                },
            ));
            self.explorer = Some(explorer);
        }
        if !self.loading
            && let Some(explorer) = &self.explorer
        {
            let (roots, expanded) = explorer.read(cx).expansion_state();
            crate::app::remember_explorer(roots, expanded, cx);
        }
        self.scan_workspace_roots(window, cx);
    }

    pub(super) fn remove_folder(
        &mut self,
        root_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if crate::app::updates::busy(cx) {
            return;
        }
        let Some(explorer) = self.explorer.clone() else {
            return;
        };
        let Some(index) = explorer.update(cx, |explorer, cx| explorer.remove_folder(root_id, cx))
        else {
            return;
        };
        self.roots.remove(index);
        self.root = self.roots.first().cloned();
        self.restoring_roots = None;
        self.results.clear();
        self.search_task = None;
        self.pending_anchor = None;
        if cx.try_global::<crate::app::RecentWorkspaces>().is_some() {
            crate::app::remember_workspace(self.recent_id.clone(), self.roots.clone(), cx);
        }
        let (roots, expanded) = explorer.read(cx).expansion_state();
        crate::app::remember_explorer(roots, expanded, cx);
        self.scan_workspace_roots(window, cx);
        cx.notify();
    }

    fn scan_workspace_roots(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_task = None;
        self.watch_scan_task = None;
        self.watch_setup = None;
        self.watch_task = None;
        self.watch = None;
        if self.roots.is_empty() {
            if !self.tabs.is_empty() {
                self.watch_file_parent(window, cx);
            }
            return;
        }
        let roots = self.roots.clone();
        let parents = self
            .tabs
            .iter()
            .filter_map(|r| r.path.parent().map(ToOwned::to_owned))
            .collect::<Vec<_>>();
        let work = cx.background_executor().spawn(async move {
            let root = &roots[0];
            let mut watch = glim_services::watch::WorkspaceWatch::new(root).ok();
            if let Some(watch) = &mut watch {
                for path in roots.iter().skip(1).chain(parents.iter()) {
                    let _ = watch.add_path(path);
                }
            }
            (roots, watch)
        });
        self.watch_scan_task = Some(cx.spawn_in(window, async move |view, cx| {
            let (roots, watch) = work.await;
            let _ = view.update_in(cx, |view, window, cx| {
                if view.roots != roots {
                    return;
                }
                view.watch_scan_task = None;
                view.watch = watch;
                view.start_watching(window, cx);
                cx.notify();
            });
        }));
    }
    pub(super) fn make_reader(content: Content, window: &mut Window, cx: &mut App) -> Reader {
        let path = match &content {
            Content::File(d) => d.path.clone(),
            Content::Page(d) => d.path.clone(),
            Content::Image(d) => d.path.clone(),
            Content::XMind(d) => d.path.clone(),
            Content::Opml(d) => d.path.clone(),
            Content::Book(d) => d.path.clone(),
            Content::Sqlite(path) | Content::Native(path, ..) | Content::Unavailable(path, _) => {
                path.clone()
            }
        };
        let _scope = glim_services::diagnostics::FileScope::enter(&path, "initialize");
        log::info!("reader initialize file={}", path.display());
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Self::make_reader_inner(content, window, cx)
        })) {
            Ok(reader) => reader,
            Err(_) => Reader::unavailable(
                path.clone(),
                glim_services::diagnostics::failure(&path)
                    .unwrap_or_else(|| "Reader initialization failed".into()),
                window,
                cx,
            ),
        }
    }
    fn make_reader_inner(content: Content, window: &mut Window, cx: &mut App) -> Reader {
        match content {
            Content::File(d) => Reader::new(d, window, cx),
            Content::Page(d) => Reader::from_page(d, window, cx),
            Content::Image(d) => Reader::from_image(d, window, cx),
            Content::XMind(d) => Reader::from_xmind(d, cx),
            Content::Opml(d) => Reader::from_opml(d, cx),
            Content::Book(d) => Reader::from_book(d, window, cx),
            Content::Sqlite(path) => Reader::from_sqlite(path, window, cx),
            Content::Native(path, folder, shuffle) => {
                Reader::from_native(path, folder, shuffle, cx)
            }
            Content::Unavailable(p, e) => Reader::unavailable(p, e, window, cx),
        }
    }
    pub(super) fn install_content(
        &mut self,
        content: Content,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut reader = Self::make_reader(content, window, cx);
        glim_services::telemetry::file_event(
            "reader_opened",
            &reader.path,
            [
                (
                    "reader_kind",
                    glim_services::telemetry::text(reader.telemetry_kind()),
                ),
                (
                    "unavailable",
                    glim_services::telemetry::flag(reader.is_unavailable()),
                ),
                (
                    "can_preview",
                    glim_services::telemetry::flag(reader.can_preview()),
                ),
                (
                    "can_edit",
                    glim_services::telemetry::flag(reader.editor().is_some()),
                ),
            ],
        );
        reader.set_editing_locked(self.editing_locked, cx);
        reader.set_word_wrap(self.word_wrap, window, cx);
        if let Some(i) = self.tabs.iter().position(|t| t.path == reader.path) {
            self.tabs[i] = reader;
            self.active = Some(i);
        } else {
            self.tabs.push(reader);
            self.active = Some(self.tabs.len() - 1);
            self.tab_scroll.scroll_to_item(self.tabs.len() - 1);
        }
        if let Some((path, anchor)) = self.pending_anchor.take()
            && let Some(i) = self.active
            && self.tabs[i].path == path
        {
            self.tabs[i].reveal_anchor(&anchor, cx);
        }
        self.reveal_active_file(cx);
        self.observe_autosave(window, cx);
        self.record_history();
        window.set_window_title("Glim");
        if self.root.is_none() {
            self.watch_file_parent(window, cx);
        }
    }
    pub(super) fn choose_add_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking || crate::app::updates::busy(cx) {
            return;
        }
        self.picking = true;
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some(glim_services::locale::tr("Add folders to workspace").into()),
        });
        self.picker_task = Some(cx.spawn_in(window, async move |view, cx| {
            let selection = selection.await;
            let paths = match selection {
                Ok(Ok(Some(paths))) => paths,
                Ok(Ok(None)) => {
                    let _ = view.update(cx, |v, cx| {
                        v.picking = false;
                        cx.notify();
                    });
                    return;
                }
                other => {
                    let _ = view.update(cx, |v, cx| {
                        v.picking = false;
                        v.error = Some(format!("Cannot choose folders: {other:?}").into());
                        cx.notify();
                    });
                    return;
                }
            };
            for path in paths {
                let folder = cx
                    .background_executor()
                    .spawn(async move { open_folder(&path) })
                    .await;
                let _ = view.update_in(cx, |v, window, cx| {
                    match folder {
                        Ok(folder) => v.add_folder(folder, window, cx),
                        Err(error) => v.error = Some(format!("{error:#}").into()),
                    }
                    cx.notify();
                });
            }
            let _ = view.update(cx, |v, cx| {
                v.picking = false;
                cx.notify();
            });
        }));
    }

    pub(super) fn choose_path(
        &mut self,
        folder: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.picking || crate::app::updates::busy(cx) {
            return;
        }
        self.picking = true;
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: !folder,
            directories: folder,
            multiple: false,
            prompt: Some(if folder { "Open folder" } else { "Open file" }.into()),
        });
        self.picker_task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = selection.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.picking = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            view.open_path(path, window, cx);
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(e)) => view.error = Some(e.to_string().into()),
                    Err(e) => view.error = Some(e.to_string().into()),
                }
                cx.notify();
            });
        }));
    }
}

pub(super) fn read_content(path: &std::path::Path) -> anyhow::Result<Content> {
    read_content_cancellable(path, &glim_services::work::Cancellation::default())
}
pub(super) fn read_content_cancellable(
    path: &std::path::Path,
    cancel: &glim_services::work::Cancellation,
) -> anyhow::Result<Content> {
    let _scope = glim_services::diagnostics::FileScope::enter(path, "read");
    log::info!("read begin file={}", path.display());
    let started = std::time::Instant::now();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        read_content_inner(path, cancel)
    }))
    .unwrap_or_else(|_| Err(anyhow::anyhow!("File reader panicked: {}", path.display())));
    match &result {
        Ok(_) => log::info!(
            "read complete file={} elapsed_ms={}",
            path.display(),
            started.elapsed().as_millis()
        ),
        Err(error) => log::error!(
            "read failed file={} elapsed_ms={} error={error:#}",
            path.display(),
            started.elapsed().as_millis()
        ),
    }
    result
}
fn read_content_inner(
    path: &std::path::Path,
    cancel: &glim_services::work::Cancellation,
) -> anyhow::Result<Content> {
    cancel.check()?;
    if glim_services::plist::is_binary(path) {
        return glim_services::plist::read(path, cancel).map(Content::Page);
    }
    if glim_services::opml::supports(path) {
        return match glim_services::opml::read(path, cancel) {
            Ok(document) => Ok(Content::Opml(document)),
            Err(_) => glim_services::paged::read_page(path, 0, false).map(Content::Page),
        };
    }
    if glim_services::drawio::supports(path) {
        return match glim_services::drawio::read(path, cancel) {
            Ok(document) => Ok(Content::Image(document)),
            Err(_) => glim_services::paged::read_page(path, 0, false).map(Content::Page),
        };
    }
    if glim_services::executables::supports(path) {
        return glim_services::executables::read_preview_cancellable(path, cancel)
            .map(Content::Page);
    }
    if glim_services::books::supports(path) {
        return glim_services::books::read_cancellable(path, cancel).map(Content::Book);
    }
    if glim_services::xmind::supports(path) {
        return match glim_services::xmind::read(path) {
            Ok(map) => Ok(Content::XMind(map)),
            Err(error) if error.is::<reader_limits::PreviewBudgetExceeded>() => {
                glim_services::xmind::read_source(path, cancel).map(Content::Page)
            }
            Err(error) => Err(error),
        };
    }
    if glim_services::vector_formats::supports(path) {
        return match glim_services::vector_formats::read_preview(path) {
            Ok(image) => Ok(Content::Image(image)),
            Err(error) if error.is::<reader_limits::PreviewBudgetExceeded>() => {
                glim_services::paged::read_page(path, 0, false)
                    .or_else(|_| glim_services::paged::read_binary_page(path, 0))
                    .map(Content::Page)
            }
            Err(error) => Err(error),
        };
    }
    if glim_services::formats::supports(path) {
        return glim_services::formats::read_preview_cancellable(path, cancel).map(Content::Page);
    }
    if glim_services::sqlite::supports(path) || glim_services::sqlite::has_header(path) {
        return Ok(Content::Sqlite(path.to_path_buf()));
    }
    if glim_services::preview::can_open(path) {
        return Ok(Content::Native(path.to_path_buf(), false, false));
    }
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("safetensors"))
    {
        return glim_services::safetensors::read_preview_cancellable(path, cancel)
            .map(Content::Page);
    }
    let result = if glim_services::media::supports(path) {
        glim_services::media::read_image(path).map(Content::Image)
    } else {
        (|| {
            let first = glim_services::paged::read_page(path, 0, false)?;
            if first.total > glim_services::files::MAX_DOCUMENT_BYTES
                || first
                    .text
                    .lines()
                    .any(|line| line.len() > reader_limits::TEXT_LINE_BYTES)
            {
                Ok(Content::Page(first))
            } else {
                read_document(path)
                    .map(|d| {
                        if d.text
                            .lines()
                            .any(|line| line.len() > reader_limits::TEXT_LINE_BYTES)
                            || d.text
                                .bytes()
                                .filter(|b| *b == b'\n')
                                .take(reader_limits::TEXT_EDITABLE_NEWLINES + 1)
                                .count()
                                > reader_limits::TEXT_EDITABLE_NEWLINES
                        {
                            Content::Page(first)
                        } else {
                            Content::File(d)
                        }
                    })
                    .or_else(|error| {
                        if error.is::<reader_limits::PreviewBudgetExceeded>() {
                            glim_services::paged::read_page(path, 0, false).map(Content::Page)
                        } else {
                            Err(error)
                        }
                    })
            }
        })()
    };
    if result.is_err()
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'))
    {
        glim_services::paged::read_binary_page(path, 0).map(Content::Page)
    } else {
        result
    }
}

#[cfg(test)]
mod format_tests {
    use super::{Content, read_content};
    #[test]
    fn dotfiles_are_routed_by_content_not_missing_extension() {
        let dir = tempfile::tempdir().unwrap();
        for name in [".env", ".npmrc", ".gitignore", ".config.local"] {
            let path = dir.path().join(name);
            std::fs::write(&path, "EXAMPLE=value\n").unwrap();
            assert!(matches!(read_content(&path).unwrap(), Content::File(_)));
        }
        let binary = dir.path().join(".DS_Store");
        std::fs::write(&binary, [0, 1, 2, 255]).unwrap();
        assert!(matches!(read_content(&binary).unwrap(), Content::Page(_)));
    }
}

#[cfg(test)]
mod incoming_file_tests {
    use super::Workspace;
    use gpui_kit::{AppContext, TestAppContext, WindowOptions};

    #[gpui_kit::test]
    fn incoming_files_queue_without_cancelling_or_replacing_existing_tabs(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("notes.txt");
        let second = directory.path().join("README.md");
        let image = directory.path().join("picture.gif");
        let unsupported = directory.path().join("unknown.custom-format");
        std::fs::write(&first, "ordinary text").unwrap();
        std::fs::write(&second, "# Markdown").unwrap();
        // The simulated GPUI window cannot host AppKit image controllers.
        // Keep the queue's media case on the GPUI GIF reader.
        std::fs::write(
            &image,
            [
                71, 73, 70, 56, 57, 97, 1, 0, 1, 0, 128, 0, 0, 0, 0, 0, 255, 255, 255, 44, 0, 0, 0,
                0, 1, 0, 1, 0, 0, 2, 2, 68, 1, 0, 59,
            ],
        )
        .unwrap();
        std::fs::write(&unsupported, [0, 255, 0, 255]).unwrap();
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| Workspace::new(Some(first.clone()), window, cx))
            })
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| {
                for path in [&first, &second, &image, &unsupported, &first] {
                    assert!(view.open_system_file(path.clone(), window, cx));
                }
            });
        })
        .unwrap();
        cx.run_until_parked();
        let editor = cx.update(|cx| {
            let view = view.read(cx);
            assert_eq!(view.tabs.len(), 4);
            assert_eq!(view.tabs[0].path, first);
            assert_eq!(view.tabs[1].path, second);
            assert_eq!(view.tabs[2].path, image);
            assert!(!view.tabs[2].is_unavailable());
            assert_eq!(view.tabs[3].path, unsupported);
            assert!(view.tabs[3].is_unavailable());
            assert!(view.incoming_files.is_empty());
            assert!(view.incoming_file_task.is_none());
            view.tabs[0].editor().unwrap().entity_id()
        });
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| {
                assert!(view.open_system_file(first.clone(), window, cx));
                assert_eq!(view.active, Some(0));
                assert_eq!(view.tabs[0].editor().unwrap().entity_id(), editor);
                assert!(view.incoming_file_task.is_none());
            });
        })
        .unwrap();
    }
}
