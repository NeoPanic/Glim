mod editing;
mod file_operations;
mod gestures;
mod git;
mod header;
mod loading;
mod reader_actions;
mod render;
mod session;
mod settings;
mod tabs;
mod toolbar;
mod updates;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use std::path::PathBuf;

use super::{changes::Changes, explorer::Explorer, reader::Reader};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sidebar {
    Files,
    Changes,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Document,
    Start,
    Settings,
}

const DEFAULT_SIDEBAR_RATIO: f32 = 0.20;
const SIDEBAR_COLLAPSE_WIDTH: f32 = 120.;

pub struct Workspace {
    path_hint: super::path_hint::PathHint,
    recent_id: String,
    restoring_roots: Option<Vec<PathBuf>>,
    focus: FocusHandle,
    page: Page,
    file_fullscreen: bool,
    file_fullscreen_entered_window: bool,
    file_fullscreen_seen_window: bool,
    sidebar_selection: Option<&'static str>,
    history: Vec<header::Target>,
    history_cursor: Option<usize>,
    navigating: bool,
    history_swipe: Option<gestures::HistorySwipe>,
    search: Entity<InputState>,
    sidebar_visible: bool,
    sidebar_ratio: f32,
    sidebar_window_width: f32,
    sidebar_layout: Entity<gpui_kit::component::resizable::ResizableState>,
    word_wrap: bool,
    preference_task: Option<Task<()>>,
    repository_task: Option<Task<()>>,
    results: Vec<PathBuf>,
    search_offset: usize,
    search_more: bool,
    recent_visible: usize,
    pending_anchor: Option<(PathBuf, String)>,
    search_task: Option<Task<()>>,
    _search_subscription: Subscription,
    _appearance_subscription: Subscription,
    watch: Option<glim_services::watch::WorkspaceWatch>,
    watch_task: Option<Task<()>>,
    watch_setup: Option<Task<()>>,
    refresh_task: Option<Task<()>>,
    root: Option<PathBuf>,
    roots: Vec<PathBuf>,
    explorer: Option<Entity<Explorer>>,
    changes: Option<Entity<Changes>>,
    subscriptions: Vec<Subscription>,
    sidebar: Sidebar,
    tabs: Vec<Reader>,
    active: Option<usize>,
    tab_scroll: ScrollHandle,
    error: Option<SharedString>,
    last_scan_warning: Option<String>,
    loading: bool,
    loading_preview: bool,
    load_task: Option<Task<()>>,
    incoming_files: std::collections::VecDeque<PathBuf>,
    incoming_file_task: Option<Task<()>>,
    picker_task: Option<Task<()>>,
    picking: bool,
    save_task: Option<Task<()>>,
    save_cancel: Option<glim_services::work::CancelOnDrop>,
    saving: Option<EntityId>,
    git_busy: bool,
    git_edit_locked: bool,
    autosave: std::collections::HashMap<EntityId, (Subscription, Option<Task<()>>)>,
    save_queue: std::collections::VecDeque<EntityId>,
    confirm_task: Option<Task<()>>,
    context_menu_task: Option<Task<()>>,
}

impl Workspace {
    pub(crate) fn recent_visible(&self) -> usize {
        self.recent_visible
    }
    pub(crate) fn show_more_recent(&mut self, cx: &mut Context<Self>) {
        self.recent_visible = self
            .recent_visible
            .saturating_add(glim_services::limits::WORKSPACE_RECENT_WORKSPACES);
        cx.notify();
    }
    pub(crate) fn folder_roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub(crate) fn matches_recent_workspace(
        &self,
        workspace: &glim_services::preferences::RecentWorkspace,
    ) -> bool {
        self.recent_id == workspace.id
            || (!workspace.folders.is_empty()
                && (self.roots == workspace.folders
                    || self.restoring_roots.as_ref() == Some(&workspace.folders)))
    }

    pub(crate) fn show_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Settings;
        self.results.clear();
        self.search_task = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn refresh_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| {
            search.set_placeholder(
                glim_services::locale::tr("Search files in workspace…"),
                window,
                cx,
            )
        });
        if let Some(changes) = &self.changes {
            changes.update(cx, |view, cx| view.refresh_language(window, cx));
        }
        cx.notify();
    }

    pub fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(glim_services::locale::tr("Search files in workspace…"))
        });
        let subscription = cx.subscribe_in(&search, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                view.search_files(window, cx);
            }
        });
        let appearance_subscription = window.observe_window_appearance(|window, cx| {
            crate::app::sync_system_appearance(window, cx);
        });
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            if crate::app::updates::intercept_last_window_close(cx) {
                return false;
            }
            weak.update(cx, |v, cx| {
                if v.has_unsaved() || v.is_saving() {
                    v.request_close_window(window, cx);
                    false
                } else {
                    true
                }
            })
            .unwrap_or(true)
        });
        let mut workspace = Self {
            path_hint: Default::default(),
            recent_id: format!(
                "{}-{:?}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
                cx.entity_id(),
            ),
            restoring_roots: None,
            history: Vec::new(),
            history_cursor: None,
            navigating: false,
            history_swipe: None,
            sidebar_visible: true,
            sidebar_ratio: DEFAULT_SIDEBAR_RATIO,
            sidebar_window_width: 0.,
            sidebar_layout: cx.new(|_| gpui_kit::component::resizable::ResizableState::default()),
            page: Page::Start,
            file_fullscreen: false,
            file_fullscreen_entered_window: false,
            file_fullscreen_seen_window: false,
            sidebar_selection: Some("start"),
            word_wrap: cx
                .try_global::<crate::app::ReaderPreferences>()
                .is_some_and(|p| p.word_wrap),
            preference_task: None,
            repository_task: None,
            search,
            results: Vec::new(),
            search_offset: 0,
            search_more: false,
            recent_visible: glim_services::limits::WORKSPACE_RECENT_WORKSPACES,
            pending_anchor: None,
            search_task: None,
            _search_subscription: subscription,
            _appearance_subscription: appearance_subscription,
            watch: None,
            watch_task: None,
            watch_setup: None,
            refresh_task: None,
            focus,
            root: None,
            roots: Vec::new(),
            explorer: None,
            changes: None,
            subscriptions: Vec::new(),
            sidebar: Sidebar::Files,
            tabs: Vec::new(),
            active: None,
            tab_scroll: ScrollHandle::new(),
            error: None,
            last_scan_warning: None,
            loading: false,
            loading_preview: false,
            load_task: None,
            incoming_files: std::collections::VecDeque::new(),
            incoming_file_task: None,
            picker_task: None,
            picking: false,
            save_task: None,
            save_cancel: None,
            saving: None,
            git_busy: false,
            git_edit_locked: crate::app::git_editors_locked(cx),
            autosave: Default::default(),
            save_queue: Default::default(),
            confirm_task: None,
            context_menu_task: None,
        };
        workspace
            .subscriptions
            .push(cx.observe_window_bounds(window, |view, window, cx| {
                if !view.file_fullscreen {
                    return;
                }
                if window.is_fullscreen() {
                    view.file_fullscreen_seen_window = true;
                } else if view.file_fullscreen_seen_window {
                    view.set_file_fullscreen(false, window);
                    view.sidebar_window_width = 0.;
                    cx.notify();
                }
            }));

        if let Some(path) = path {
            workspace.open_path(path, window, cx);
        }
        workspace
    }
}

impl Workspace {
    fn select_sidebar_navigation(&mut self, item: &'static str, cx: &mut Context<Self>) {
        self.sidebar_selection = Some(item);
        if let Some(explorer) = &self.explorer {
            explorer.update(cx, |view, cx| view.clear_clicked_selection(cx));
        }
        cx.notify();
    }
}
