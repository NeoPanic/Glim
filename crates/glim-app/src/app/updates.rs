//! Application-wide updater. Native callbacks never decide whether drafts may be discarded.
use glim_core::update::{UpdateSession, UpdateSnapshot};
use gpui_kit::{component::TitleBar, *};
use std::{path::PathBuf, time::Duration};

pub(crate) struct UpdateService(pub Entity<UpdateState>);
impl Global for UpdateService {}
#[derive(Default)]
struct UpdateInstallLock(bool);
impl Global for UpdateInstallLock {}

pub(crate) struct UpdateState {
    pub snapshot: UpdateSnapshot,
    pub message: Option<String>,
    pub preparing: bool,
    pub exiting: bool,
    pub backup: Option<PathBuf>,
    #[cfg(feature = "macos-updater")]
    native: Option<super::updater_bridge::NativeUpdater>,
    _poll: Option<Task<()>>,
    preparation: Option<Task<()>>,
    exit_task: Option<Task<()>>,
    startup: Option<Task<()>>,
    exit_started: Option<std::time::Instant>,
    pending_check: bool,
    manual_check: bool,
    maintenance: Option<Task<()>>,
    exit_generation: u64,
}
impl UpdateState {
    fn command(&self, command: &str) {
        #[cfg(feature = "macos-updater")]
        if let Some(native) = &self.native {
            native.command(command);
        }

        #[cfg(not(feature = "macos-updater"))]
        let _ = command;
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        #[cfg(feature = "macos-updater")]
        if let Some(native) = &self.native
            && let Ok(snapshot) = native.snapshot()
            && self.snapshot != snapshot
        {
            let cancelled = snapshot.phase == "cancelled" && self.snapshot.phase != "cancelled";
            self.snapshot = snapshot;
            if self.manual_check
                && matches!(
                    self.snapshot.phase.as_str(),
                    "available" | "ready" | "current" | "error" | "cancelled"
                )
            {
                self.manual_check = false;
                if self.snapshot.has_update() {
                    cx.defer(open_updates_window);
                }
            }
            if self.snapshot.needs_quit_approval {
                self.command("quit-request-received");
                cx.defer(super::request_quit);
            }
            if cancelled {
                self.clear_checkpoint(None, cx);
            }
            if self.exiting && !self.preparing && !self.snapshot.exit_requested {
                self.exiting = false;
                set_locked(false, cx);
            }
            cx.notify();
            cx.refresh_windows();
            cx.defer(super::menus::update);
        }

        if self.exiting
            && !self.preparing
            && self
                .exit_started
                .is_some_and(|t| t.elapsed() > Duration::from_secs(15))
        {
            self.command("approval-denied");
            self.exiting = false;
            self.exit_started = None;
            self.message = Some("Update is waiting for the application to quit. Close system prompts and try again.".into());
            set_locked(false, cx);
            cx.notify();
        }
        #[cfg(not(feature = "macos-updater"))]
        let _ = cx;
    }
    fn clear_checkpoint(&mut self, exit_task: Option<Task<()>>, cx: &mut Context<Self>) {
        let previous = self.maintenance.take();
        self.maintenance = Some(cx.spawn(async move |state, cx| {
            if let Some(task) = exit_task {
                task.await;
            }
            if let Some(previous) = previous {
                previous.await;
            }
            let result = cx
                .background_executor()
                .spawn(async { glim_services::updates::clear_session() })
                .await;
            if let Err(error) = result {
                let _ = state.update(cx, |state, cx| {
                    state.message =
                        Some(format!("Cannot clear cancelled update session: {error:#}"));
                    cx.notify();
                });
            }
        }));
    }
    fn prepare(&mut self, cx: &mut Context<Self>) {
        if !self.snapshot.enabled || self.preparing || self.backup.is_some() {
            return;
        }
        self.preparing = true;
        self.message = None;
        let path = self.snapshot.bundle_path.clone();
        let work = cx
            .background_executor()
            .spawn(async move { glim_services::updates::prepare_backup(&path) });
        self.preparation = Some(cx.spawn(async move |state, cx| {
            let result = work.await;
            let _ = state.update(cx, |state, cx| {
                state.preparing = false;
                match result {
                    Ok(path) => {
                        state.backup = Some(path);
                        state.command("start");
                        if state.pending_check {
                            state.pending_check = false;
                            state.command("check");
                        }
                        state.refresh(cx);
                    }
                    Err(error) => {
                        state.manual_check = false;
                        state.pending_check = false;
                        state.message = Some(format!("Cannot prepare update backup: {error:#}"));
                    }
                }
                cx.notify();
                cx.refresh_windows();
                cx.defer(super::menus::update);
            });
        }));
        cx.notify();
    }
    fn prepare_exit(&mut self, relaunch: bool, cx: &mut Context<Self>) {
        if self.exiting || self.preparing {
            return;
        }
        if self.backup.is_none() {
            self.message = Some("A verified recovery backup is required before updating.".into());
            cx.notify();
            return;
        }
        if relaunch && !self.snapshot.can_install && !self.snapshot.needs_install_approval {
            return;
        }
        let windows = super::OpenWorkspaces::windows(cx);
        for (view, _) in &windows {
            if let Some(view) = view.upgrade()
                && let Some(reason) = view.read(cx).update_activity(cx).blocker()
            {
                self.message = Some(glim_services::locale::tr(reason));
                self.command("approval-denied");
                cx.notify();
                return;
            }
        }
        self.command("validate-exit");
        self.refresh(cx);
        if !self.snapshot.exit_permitted {
            self.message = Some(glim_services::locale::tr(
                "Quit other running copies of Glim before updating.",
            ));
            cx.notify();
            return;
        }
        let workspaces = windows
            .iter()
            .filter_map(|(view, _)| view.upgrade())
            .map(|view| view.read(cx).update_session(cx))
            .collect();
        let session = UpdateSession {
            schema: 1,
            source_build: self.snapshot.current_build.clone(),
            target_build: self.snapshot.build.clone(),
            workspaces,
        };
        self.exiting = true;
        self.exit_generation += 1;
        let generation = self.exit_generation;
        self.preparing = true;
        self.message = None;
        set_locked(true, cx);
        // Existing preference writes finish before we checkpoint and terminate.
        let mut writes = Vec::new();
        if let Some(task) = self.maintenance.take() {
            writes.push(task);
        }
        for (view, _) in &windows {
            if let Some(view) = view.upgrade() {
                writes.extend(view.update(cx, |view, cx| view.take_book_writes(cx)));
            }
            if let Some(view) = view.upgrade()
                && let Some(task) = view.update(cx, |view, _| view.take_preference_write())
            {
                writes.push(task);
            }
        }
        if let Some(task) = cx.global_mut::<super::ExplorerSaveTask>().0.take() {
            writes.push(task);
        }
        if let Some(task) = cx.global_mut::<super::WorkspaceSaveTask>().0.take() {
            writes.push(task);
        }
        if let Some(task) = cx.global_mut::<super::AppearanceSaveTask>().0.take() {
            writes.push(task);
        }
        if let Some(task) = cx.global_mut::<super::LanguageSaveTask>().0.take() {
            writes.push(task);
        }
        if let Some(task) = cx
            .global_mut::<super::ReaderPreferences>()
            .preview_save_task
            .take()
        {
            writes.push(task);
        }
        let backup = self.backup.clone().expect("Backup checked above");
        self.exit_task = Some(cx.spawn(async move |state, cx| {
            for task in writes {
                task.await;
            }
            let result = cx
                .background_executor()
                .spawn(async move {
                    glim_services::updates::verify_backup(&backup)?;
                    glim_services::updates::save_session(&session)
                })
                .await;
            let _ = state.update(cx, |state, cx| {
                if state.exit_generation != generation {
                    return;
                }
                state.preparing = false;
                state.exit_started = Some(std::time::Instant::now());
                if let Err(error) = result {
                    state.message = Some(format!("Cannot prepare update installation: {error:#}"));
                    state.backup = None;
                    state.exiting = false;
                    set_locked(false, cx);
                } else if relaunch {
                    state.command("install-approved");
                    state.refresh(cx);

                    if !state.snapshot.exit_requested {
                        state.exiting = false;
                        set_locked(false, cx);
                    }
                } else {
                    state.command("quit-approved");
                    state.refresh(cx);

                    if state.snapshot.exit_requested {
                        cx.quit();
                    } else {
                        state.exiting = false;
                        set_locked(false, cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(UpdateInstallLock::default());
    #[cfg(feature = "macos-updater")]
    let native = super::updater_bridge::NativeUpdater::new();
    #[cfg(feature = "macos-updater")]
    let snapshot = native.snapshot().unwrap_or_default();

    #[cfg(not(feature = "macos-updater"))]
    let snapshot = UpdateSnapshot {
        current_version: env!("CARGO_PKG_VERSION").into(),
        mode: glim_core::update::UpdateMode::Manual,
        phase: "disabled".into(),
        message:
            "This build does not include automatic updates. Download the latest release on GitHub."
                .into(),
        ..Default::default()
    };
    let state = cx.new(|_| UpdateState {
        snapshot,
        message: None,
        preparing: false,
        exiting: false,
        backup: None,
        #[cfg(feature = "macos-updater")]
        native: Some(native),
        _poll: None,
        preparation: None,
        exit_task: None,
        startup: None,
        exit_started: None,
        pending_check: false,
        manual_check: false,
        maintenance: None,
        exit_generation: 0,
    });
    cx.set_global(UpdateService(state.clone()));
    state.update(cx, |state, cx| {
        if state.snapshot.enabled {
            state._poll = Some(cx.spawn(async move |state, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(300))
                        .await;
                    if state.update(cx, |state, cx| state.refresh(cx)).is_err() {
                        break;
                    }
                }
            }));
            state.prepare(cx);
        }
    });
}
pub(crate) fn state(cx: &App) -> Option<Entity<UpdateState>> {
    cx.try_global::<UpdateService>().map(|v| v.0.clone())
}
pub(crate) fn busy(cx: &App) -> bool {
    cx.try_global::<UpdateInstallLock>().is_some_and(|v| v.0)
}
/// A Finder open arriving during preparation cancels the exit, not the file open.
pub(crate) fn interrupt_for_files(cx: &mut App) {
    if !busy(cx) {
        return;
    }
    if let Some(state) = state(cx) {
        state.update(cx, |state, cx| {
            state.command(if state.snapshot.exit_requested {
                "withdraw-approval"
            } else {
                "approval-denied"
            });
            state.exit_generation += 1;
            state.exiting = false;
            state.preparing = false;
            state.exit_started = None;
            let pending = state.exit_task.take();
            state.clear_checkpoint(pending, cx);
            state.message =
                Some("New files were opened. Review them before restarting to update.".into());
            set_locked(false, cx);
            cx.notify();
        });
    }
}
#[derive(Default)]
struct UpdatesWindow(Option<AnyWindowHandle>);
impl Global for UpdatesWindow {}

pub(crate) fn show(_: &mut Window, cx: &mut App) {
    // Open after the originating window/menu callback releases its borrow.
    cx.defer(open_updates_window);
}
fn open_updates_window(cx: &mut App) {
    if let Some(handle) = cx.try_global::<UpdatesWindow>().and_then(|state| state.0)
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        cx.activate(true);
        return;
    }
    let Some(state) = state(cx) else { return };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(740.), px(640.)),
            cx,
        ))),
        window_min_size: Some(size(px(640.), px(320.))),
        ..TitleBar::window_options()
    };
    match gpui_kit::open_window(options, cx, |window, cx| {
        window.on_window_should_close(cx, |_, cx| !intercept_last_window_close(cx));
        cx.new(|cx| crate::views::updates::Updates::new(state, window, cx))
    }) {
        Ok((handle, _)) => {
            cx.set_global(UpdatesWindow(Some(handle)));
            cx.activate(true);
        }
        Err(error) => log::error!("Cannot open Software Update: {error:#}"),
    }
}
pub(crate) fn show_from_menu(cx: &mut App) {
    cx.defer(|cx| {
        if state(cx).is_some_and(|s| {
            let s = s.read(cx);
            !s.snapshot.enabled || s.snapshot.has_update()
        }) {
            open_updates_window(cx);
        } else {
            command("check", cx);
        }
    });
}
pub(crate) fn check_label(cx: &App) -> &'static str {
    let Some(state) = state(cx) else {
        return "Check…";
    };
    let state = state.read(cx);
    if state.manual_check || state.snapshot.phase == "checking" {
        return "Checking for updates…";
    }
    if state.message.is_some() || state.snapshot.phase == "error" {
        return "Update could not complete";
    }
    match state.snapshot.phase.as_str() {
        "current" => "You’re up to date",
        "available" | "ready" => "An update is available",
        _ => "Check…",
    }
}
pub(crate) fn checking(cx: &App) -> bool {
    state(cx).is_some_and(|s| s.read(cx).manual_check || s.read(cx).snapshot.phase == "checking")
}
pub(crate) fn command(command: &str, cx: &mut App) {
    let Some(state) = state(cx) else { return };
    state.update(cx, |state, cx| {
        if state.exiting {
            return;
        }
        if command == "check" {
            if state.manual_check || state.snapshot.phase == "checking" {
                return;
            }
            state.manual_check = true;
        }
        state.message = None;
        if state.backup.is_none() {
            state.pending_check |= command == "check";
            state.prepare(cx);
        } else {
            state.command(command);
            state.refresh(cx);
        }
        cx.notify();
        cx.refresh_windows();
        cx.defer(super::menus::update);
    });
}
pub(crate) fn install(cx: &mut App) {
    if let Some(state) = state(cx) {
        state.update(cx, |s, cx| s.prepare_exit(true, cx));
    }
}
pub(crate) fn save_all(cx: &mut App) {
    cx.defer(save_all_after_dispatch);
}
fn save_all_after_dispatch(cx: &mut App) {
    if busy(cx) {
        return;
    }
    for (view, window) in super::OpenWorkspaces::windows(cx) {
        let _ = window.update(cx, |_, window, cx| {
            let _ = view.update(cx, |v, cx| v.save_all_for_update(window, cx));
        });
    }
}
pub(crate) fn has_pending_install(cx: &App) -> bool {
    state(cx).is_some_and(|s| {
        let s = s.read(cx);

        s.snapshot.enabled
            && (s.snapshot.install_on_quit
                || s.snapshot.phase == "downloading"
                || s.snapshot.phase == "extracting"
                || s.snapshot.phase == "installing")
    })
}
/// Schedule outside a workspace entity update to avoid reentrant borrows.
pub(crate) fn intercept_last_window_close(cx: &mut App) -> bool {
    if busy(cx) {
        return true;
    }
    if has_pending_install(cx) && cx.windows().len() == 1 {
        cx.defer(|cx| {
            prepare_quit(cx);
        });
        return true;
    }
    false
}
pub(crate) fn prepare_quit(cx: &mut App) -> bool {
    if let Some(state) = state(cx) {
        state.update(cx, |s, cx| s.refresh(cx));
    }
    if !has_pending_install(cx) {
        return false;
    }
    if let Some(state) = state(cx) {
        state.update(cx, |s, cx| s.prepare_exit(false, cx));
        if !state.read(cx).exiting {
            show_from_menu_without_check(cx);
        }
    }
    true
}
fn show_from_menu_without_check(cx: &mut App) {
    cx.defer(|cx| {
        if cx.windows().is_empty() {
            super::open_workspace(None, cx);
        }
        show_panel_after_dispatch(cx);
    });
}
fn show_panel_after_dispatch(cx: &mut App) {
    if let Some(window) = cx.active_window().or_else(|| cx.windows().last().copied()) {
        let _ = window.update(cx, |_, window, cx| show(window, cx));
    }
}
fn set_locked(locked: bool, cx: &mut App) {
    cx.set_global(UpdateInstallLock(locked));
    for (view, _) in super::OpenWorkspaces::windows(cx) {
        if let Some(view) = view.upgrade() {
            view.update(cx, |v, cx| v.set_git_editing_locked(locked, cx));
        }
    }
}

pub(crate) fn start_application(path: Option<PathBuf>, urls: Vec<String>, cx: &mut App) {
    let Some(state) = state(cx) else { return };
    let build = state.read(cx).snapshot.current_build.clone();
    let work = cx.background_executor().spawn(async move {
        if build.is_empty() {
            Ok(None)
        } else {
            glim_services::updates::take_session(&build)
        }
    });
    state.update(cx, |state, cx| {
        state.startup = Some(cx.spawn(async move |state, cx| {
            let result = work.await;
            cx.update(|cx| {
                let mut restore_error = false;
                let restored = match result {
                    Ok(Some(session)) if !session.workspaces.is_empty() => {
                        for workspace in session.workspaces {
                            super::open_workspace_with_session(None, Some(workspace), cx);
                        }
                        true
                    }
                    Ok(_) => false,
                    Err(error) => {
                        let _ = state.update(cx, |state, cx| {
                            state.message =
                                Some(format!("Cannot restore update session: {error:#}"));
                            cx.notify();
                        });
                        restore_error = true;
                        false
                    }
                };
                if !urls.is_empty() {
                    super::open_system_files(urls, cx);
                } else if let Some(path) = path {
                    super::open_workspace(Some(path), cx);
                } else if !restored && cx.windows().is_empty() {
                    super::open_workspace(None, cx);
                }
                if restore_error {
                    show_from_menu_without_check(cx);
                }
            });
        }));
    });
}

#[cfg(test)]
mod tests {
    use super::{UpdateInstallLock, UpdateState, busy};
    use crate::views::workspace::Workspace;
    use glim_core::update::UpdateSnapshot;
    use gpui_kit::{AppContext, TestAppContext, WindowOptions, test::TestWindowExt};

    #[gpui_kit::test]
    fn install_gate_keeps_unsaved_editor_and_never_enters_exit(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("draft.txt");
        std::fs::write(&path, "on disk").unwrap();
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| Workspace::new(Some(path.clone()), window, cx))
            })
            .unwrap()
        });
        cx.run_until_parked();
        let editor = cx.update(|cx| view.read(cx).update_session(cx));
        assert_eq!(editor.tabs.len(), 1);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            // The source editor is the only textbox in the reader; use its
            // existing text selection via the focused reader action below.
            view.update(cx, |view, cx| view.focus_active_editor(window, cx));
            window.press("secondary-a", cx);
            window.input("unsaved draft", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update(|cx| {
            cx.set_global(super::super::OpenWorkspaces(vec![(
                view.downgrade(),
                handle,
            )]));
            cx.set_global(UpdateInstallLock(false));
            let state = cx.new(|_| UpdateState {
                snapshot: UpdateSnapshot {
                    enabled: true,
                    can_install: true,
                    phase: "ready".into(),
                    current_build: "12".into(),
                    build: "13".into(),
                    ..Default::default()
                },
                message: None,
                preparing: false,
                exiting: false,
                backup: Some(dir.path().join("backup.zip")),
                #[cfg(feature = "macos-updater")]
                native: None,
                _poll: None,
                preparation: None,
                exit_task: None,
                startup: None,
                exit_started: None,
                pending_check: false,
                manual_check: false,
                maintenance: None,
                exit_generation: 0,
            });
            assert!(view.read(cx).has_unsaved());
            state.update(cx, |s, cx| s.prepare_exit(true, cx));
            assert!(!state.read(cx).exiting);
            assert!(state.read(cx).exit_task.is_none());
            assert!(state.read(cx).message.is_some());
            assert!(view.read(cx).has_unsaved());
            assert!(!busy(cx));
        });
        assert_eq!(std::fs::read_to_string(path).unwrap(), "on disk");
    }
}
