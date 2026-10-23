mod editing;
mod file_operations;
mod gestures;
mod header;
mod loading;
mod reader_actions;
mod render;
mod session;
mod settings;
mod tabs;
mod toolbar;
mod updates;
mod usage;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use std::path::PathBuf;

use super::{explorer::Explorer, reader::Reader};

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
    watch_scan_task: Option<Task<()>>,
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
    subscriptions: Vec<Subscription>,
    tabs: Vec<Reader>,
    active: Option<usize>,
    tab_scroll: ScrollHandle,
    error: Option<SharedString>,
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
    editing_locked: bool,
    usage: Option<usage::Segment>,
    scroll_usage: Option<usage::ScrollUsage>,
    scroll_usage_task: Option<Task<()>>,
    usage_task: Option<Task<()>>,
    usage_activation: Option<Subscription>,
    telemetry_edited: std::collections::HashSet<EntityId>,
    autosave: std::collections::HashMap<EntityId, (Subscription, Option<Task<()>>)>,
    save_queue: std::collections::VecDeque<EntityId>,
    confirm_task: Option<Task<()>>,
    close_request: Option<editing::CloseRequest>,
    recovery_keys: std::collections::HashMap<EntityId, String>,
    edit_revision: u64,
    quit_approval: Option<u64>,
    quit_discards: Vec<EntityId>,
    context_menu_task: Option<Task<()>>,
}

impl Workspace {
    pub(crate) fn set_editing_locked(&mut self, locked: bool, cx: &mut Context<Self>) {
        self.editing_locked = locked;
        if locked {
            self.refresh_task = None;
        }
        for reader in &mut self.tabs {
            reader.set_editing_locked(locked, cx);
        }
        cx.notify();
    }
    pub(crate) fn unlock_and_refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_editing_locked(false, cx);
        self.refresh(window, cx);
    }

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
        glim_services::telemetry::track("settings_opened", []);
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
        cx.notify();
    }

    pub fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.on_release(|view, _| {
            view.flush_usage();
            glim_services::telemetry::track(
                "window_closed",
                [
                    (
                        "open_tabs",
                        glim_services::telemetry::number(view.tabs.len() as u64),
                    ),
                    (
                        "workspace_folders",
                        glim_services::telemetry::number(view.roots.len() as u64),
                    ),
                ],
            );
        })
        .detach();
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
            word_wrap: cx
                .try_global::<crate::app::ReaderPreferences>()
                .is_some_and(|p| p.word_wrap),
            preference_task: None,
            watch_scan_task: None,
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
            subscriptions: Vec::new(),
            tabs: Vec::new(),
            active: None,
            tab_scroll: ScrollHandle::new(),
            error: None,
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
            editing_locked: crate::app::workspace_editors_locked(cx),
            usage: None,
            scroll_usage: None,
            scroll_usage_task: None,
            usage_task: None,
            usage_activation: None,
            telemetry_edited: Default::default(),
            autosave: Default::default(),
            save_queue: Default::default(),
            confirm_task: None,
            close_request: None,
            recovery_keys: Default::default(),
            edit_revision: 0,
            quit_approval: None,
            quit_discards: Vec::new(),
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

        workspace.usage_activation =
            Some(cx.observe_window_activation(window, |view, window, cx| {
                view.update_usage(window, cx);
                cx.notify();
            }));
        workspace.start_usage_timer(window, cx);
        if let Some(path) = path {
            workspace.open_path(path, window, cx);
        }
        workspace
    }
}

impl Workspace {
    fn show_home(&mut self, cx: &mut Context<Self>) {
        self.page = Page::Start;
        if let Some(explorer) = &self.explorer {
            explorer.update(cx, |view, cx| view.clear_clicked_selection(cx));
        }
        cx.notify();
    }
}
