mod about;
pub mod actions;
mod assets;
pub(crate) mod diagram_engine;
mod image_http;
pub(crate) mod languages;
pub(crate) mod media_prepare;
mod menus;
mod shortcuts;
pub(crate) mod spreadsheet_prepare;
pub(crate) mod telemetry;
#[cfg(feature = "macos-updater")]
mod updater_bridge;
pub(crate) mod updates;

use anyhow::{Result, bail};
use gpui_kit::*;

use crate::views::workspace::Workspace;
use actions::{
    About, AddFolder, CheckForUpdates, CloseWindow, CloseWorkspace, DefaultApplications,
    KeyboardShortcuts, LanguageChinese, LanguageEnglish, LanguageSystem, NavigateBack,
    NavigateForward, NewWindow, NextTab, OpenFile, OpenFolder, OpenSettings, PreviousTab, Quit,
    Refresh, SaveFile, SearchWorkspace, ToggleDarkMode, TogglePreview, ToggleSidebar,
    ToggleWordWrap,
};

pub struct ReaderPreferences {
    pub word_wrap: bool,
    pub html_preview: bool,
    pub markdown_preview: bool,
    pub csv_preview: bool,
    pub diff_side_by_side: Option<bool>,
    pub preview_save_task: Option<Task<()>>,
}
impl Global for ReaderPreferences {}

#[derive(Default)]
struct DefaultApplicationsTask(Option<Task<()>>);
impl Global for DefaultApplicationsTask {}

#[derive(Default)]
struct AppearanceSaveTask(Option<Task<()>>);
impl Global for AppearanceSaveTask {}

#[derive(Default)]
struct WorkspaceSaveTask(Option<Task<()>>);
impl Global for WorkspaceSaveTask {}

#[derive(Default)]
struct ExplorerSaveTask(Option<Task<()>>);
impl Global for ExplorerSaveTask {}

pub(crate) fn remember_explorer(
    roots: Vec<std::path::PathBuf>,
    expanded: Vec<std::path::PathBuf>,
    cx: &mut App,
) {
    if !cx.has_global::<ExplorerSaveTask>() {
        return;
    }
    let previous = cx.global_mut::<ExplorerSaveTask>().0.take();
    let task = cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            previous.await;
        }
        let result =
            cx.background_executor()
                .spawn(async move {
                    glim_services::preferences::save_explorer_expansion(&roots, &expanded)
                })
                .await;
        if let Err(error) = result {
            log::error!("Cannot remember file tree: {error:#}");
        }
    });
    cx.global_mut::<ExplorerSaveTask>().0 = Some(task);
}

#[derive(Default)]
pub(crate) struct RecentWorkspaces(pub Vec<glim_services::preferences::RecentWorkspace>);
impl Global for RecentWorkspaces {}

fn load_recent_workspaces(cx: &mut App) {
    let task = cx.spawn(async move |cx| {
        let result = cx
            .background_executor()
            .spawn(async { glim_services::preferences::recent_workspaces() })
            .await;
        match result {
            Ok(recent) => cx.update(|cx| {
                cx.global_mut::<RecentWorkspaces>().0 = recent;
                cx.refresh_windows();
            }),
            Err(error) => log::error!("Cannot load recent workspaces: {error:#}"),
        }
    });
    cx.global_mut::<WorkspaceSaveTask>().0 = Some(task);
}

pub(crate) fn remember_workspace(id: String, folders: Vec<std::path::PathBuf>, cx: &mut App) {
    let previous = cx.global_mut::<WorkspaceSaveTask>().0.take();
    let task = cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            previous.await;
        }
        let result = cx
            .background_executor()
            .spawn(async move {
                let recent = glim_services::preferences::remember_recent_workspace(
                    glim_services::preferences::RecentWorkspace {
                        id,
                        folders: folders.clone(),
                    },
                )?;
                glim_services::preferences::save_workspace_folders(&folders)?;
                anyhow::Ok(recent)
            })
            .await;
        match result {
            Ok(recent) => cx.update(|cx| {
                cx.global_mut::<RecentWorkspaces>().0 = recent;
                cx.refresh_windows();
            }),
            Err(error) => log::error!("Cannot remember workspace: {error:#}"),
        }
    });
    cx.global_mut::<WorkspaceSaveTask>().0 = Some(task);
}

pub(crate) struct AppearancePreference(pub glim_services::preferences::Appearance);
impl Global for AppearancePreference {}

pub(crate) fn sync_system_appearance(window: &mut Window, cx: &mut App) {
    if cx
        .try_global::<AppearancePreference>()
        .is_none_or(|preference| preference.0 == glim_services::preferences::Appearance::System)
    {
        gpui_kit::component::Theme::sync_system_appearance(Some(window), cx);
    }
}

pub(crate) fn recent_workspace_is_open(
    workspace: &glim_services::preferences::RecentWorkspace,
    exclude: EntityId,
    cx: &App,
) -> bool {
    OpenWorkspaces::windows(cx).iter().any(|(view, _)| {
        view.entity_id() != exclude
            && view
                .upgrade()
                .is_some_and(|view| view.read(cx).matches_recent_workspace(workspace))
    })
}

pub(crate) fn open_recent_workspace(
    workspace: glim_services::preferences::RecentWorkspace,
    cx: &mut App,
) {
    if updates::busy(cx) {
        return;
    }
    for (view, handle) in OpenWorkspaces::windows(cx) {
        let focused = handle
            .update(cx, |_, window, cx| {
                let matches = view
                    .upgrade()
                    .is_some_and(|view| view.read(cx).matches_recent_workspace(&workspace));
                if matches {
                    window.activate_window();
                    cx.activate(true);
                }
                matches
            })
            .unwrap_or(false);
        if focused {
            return;
        }
    }
    open_workspace_with_session(
        None,
        Some(glim_core::update::WorkspaceSession {
            roots: workspace.folders,
            tabs: Vec::new(),
            active: None,
        }),
        cx,
    );
}

pub(crate) fn toggle_dark_mode(cx: &mut App) {
    use glim_services::preferences::Appearance;
    let appearance = if gpui_kit::component::Theme::global(cx).is_dark() {
        Appearance::Light
    } else {
        Appearance::Dark
    };
    set_appearance(appearance, cx);
}

pub(crate) fn set_appearance(appearance: glim_services::preferences::Appearance, cx: &mut App) {
    if updates::busy(cx) {
        return;
    }
    cx.global_mut::<AppearancePreference>().0 = appearance;
    apply_appearance(appearance, cx);
    let previous = cx.global_mut::<AppearanceSaveTask>().0.take();
    let task = cx.spawn(async move |cx| {
        // Serialize writes so rapid toggles cannot restore an older preference.
        if let Some(previous) = previous {
            previous.await;
        }
        let result = cx
            .background_executor()
            .spawn(async move { glim_services::preferences::save_appearance(appearance) })
            .await;
        if let Err(error) = result {
            cx.update(|cx| {
                if let Some((_, handle)) = cx.global::<OpenWorkspaces>().0.last().cloned() {
                    let _ = handle.update(cx, |_, window, cx| {
                        drop(window.prompt(
                            PromptLevel::Warning,
                            &glim_services::locale::tr("Cannot remember appearance"),
                            Some(&format!("{error:#}")),
                            &[glim_services::locale::tr("OK").as_str()],
                            cx,
                        ));
                    });
                }
            });
        }
    });
    cx.global_mut::<AppearanceSaveTask>().0 = Some(task);
}

fn apply_appearance(appearance: glim_services::preferences::Appearance, cx: &mut App) {
    use glim_services::preferences::Appearance;
    use gpui_kit::component::{Theme, ThemeMode};
    match appearance {
        Appearance::System => {
            cx.set_window_appearance(None);
            Theme::sync_system_appearance(None, cx);
        }
        Appearance::Light => {
            cx.set_window_appearance(Some(WindowAppearance::Light));
            Theme::change(ThemeMode::Light, None, cx);
        }
        Appearance::Dark => {
            cx.set_window_appearance(Some(WindowAppearance::Dark));
            Theme::change(ThemeMode::Dark, None, cx);
        }
    }
}

fn open_default_applications(cx: &mut App) {
    if cx.global::<DefaultApplicationsTask>().0.is_some() {
        return;
    }
    let work = cx.background_executor().spawn(async {
        glim_services::preview::open_associations(std::sync::Arc::new(
            std::sync::atomic::AtomicBool::new(false),
        ))
    });
    let task = cx.spawn(async move |cx| {
        let result = work.await;
        cx.update(|cx| {
            cx.global_mut::<DefaultApplicationsTask>().0 = None;
            if let Err(error) = result
                && let Some((_, handle)) = cx.global::<OpenWorkspaces>().0.last().cloned()
            {
                let _ = handle.update(cx, |_, window, cx| {
                    drop(window.prompt(
                        PromptLevel::Warning,
                        &glim_services::locale::tr("Default applications"),
                        Some(&format!("{error:#}")),
                        &[glim_services::locale::tr("OK").as_str()],
                        cx,
                    ));
                });
            }
        });
    });
    cx.global_mut::<DefaultApplicationsTask>().0 = Some(task);
}

fn show_settings(cx: &mut App) {
    if updates::busy(cx) {
        return;
    }
    if !cx
        .global::<OpenWorkspaces>()
        .0
        .iter()
        .any(|(view, _)| view.upgrade().is_some())
    {
        open_workspace(None, cx);
    }
    let target = cx
        .global::<OpenWorkspaces>()
        .0
        .iter()
        .rev()
        .find(|(view, _)| view.upgrade().is_some())
        .cloned();
    if let Some((view, handle)) = target {
        let _ = handle.update(cx, |_, window, cx| {
            let _ = view.update(cx, |view, cx| view.show_settings(window, cx));
            window.activate_window();
        });
        cx.activate(true);
    }
}

fn show_about(cx: &mut App) {
    about::open(cx);
}

pub fn run() -> Result<()> {
    glim_services::diagnostics::initialize()?;
    let mut arguments = std::env::args_os().skip(1);
    let path = arguments.next();
    if arguments.next().is_some() {
        bail!("Usage: glim [file-or-folder]");
    }
    if path.as_deref() == Some(std::ffi::OsStr::new("--help")) {
        println!(
            "Usage: glim [file-or-folder]\n\nOpen a folder, Git repository, or UTF-8 file in the viewer and basic text editor."
        );
        return Ok(());
    }

    let language = glim_services::preferences::language().unwrap_or_else(|| "system".into());
    glim_services::locale::set(&language);
    let word_wrap = glim_services::preferences::word_wrap();
    let appearance = glim_services::preferences::appearance();
    let html_preview = glim_services::preferences::preview_mode("html");
    let markdown_preview = glim_services::preferences::preview_mode("markdown");
    let csv_preview = glim_services::preferences::preview_mode("csv");
    let diff_side_by_side = glim_services::preferences::diff_side_by_side();
    let application = gpui_kit::application().with_assets(assets::Assets);
    let image_client = image_http::ImageHttpClient::new(application.background_executor());
    let application = application.with_http_client(std::sync::Arc::new(image_client));
    let context = std::rc::Rc::new(std::cell::RefCell::new(None::<AsyncApp>));
    let pending = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let open_context = context.clone();
    let open_pending = pending.clone();
    application.on_open_urls(move |urls| {
        if let Some(app) = open_context.borrow().clone() {
            app.clone()
                .foreground_executor()
                .spawn(async move {
                    app.update(|cx| open_system_files(urls, cx));
                })
                .detach();
        } else {
            open_pending.borrow_mut().extend(urls);
        }
    });
    application.on_reopen(|cx| {
        if cx.windows().is_empty() && !updates::busy(cx) {
            open_workspace(None, cx);
        }
    });
    application.run(move |cx| {
        gpui_kit::init(cx);
        telemetry::initialize(cx);
        cx.set_quit_mode(QuitMode::Explicit);
        gpui_kit::component::set_locale(glim_services::locale::code());
        cx.set_global(LanguageSaveTask::default());
        cx.set_global(LanguagePreference(language));
        cx.set_global(AppearancePreference(appearance));
        apply_appearance(appearance, cx);
        cx.set_global(AppearanceSaveTask::default());
        cx.set_global(WorkspaceSaveTask::default());
        cx.set_global(ExplorerSaveTask::default());
        cx.on_app_quit(|cx| {
            log::info!("application quit requested");
            log::logger().flush();
            let telemetry = telemetry::shutdown(cx);
            let pending = cx.global_mut::<ExplorerSaveTask>().0.take();
            let workspace = cx.global_mut::<WorkspaceSaveTask>().0.take();
            async move {
                if let Some(telemetry) = telemetry {
                    telemetry.await;
                }
                if let Some(pending) = pending {
                    pending.await;
                }
                if let Some(workspace) = workspace {
                    workspace.await;
                }
            }
        })
        .detach();
        cx.set_global(RecentWorkspaces::default());
        load_recent_workspaces(cx);
        languages::init();
        cx.set_global(ReaderPreferences {
            word_wrap,
            html_preview,
            markdown_preview,
            csv_preview,
            diff_side_by_side,
            preview_save_task: None,
        });
        cx.set_global(OpenWorkspaces::default());
        cx.set_global(DefaultApplicationsTask::default());
        updates::init(cx);
        *context.borrow_mut() = Some(cx.to_async());
        gpui_kit::component::Theme::set_scrollbar_mode(
            gpui_kit::component::scroll::ScrollbarMode::Hover,
            cx,
        );
        crate::views::explorer::Explorer::init(cx);
        cx.bind_keys([
            KeyBinding::new("space", actions::BookNextPage, Some("Ebook")),
            KeyBinding::new("shift-space", actions::BookPreviousPage, Some("Ebook")),
            KeyBinding::new("pagedown", actions::BookNextPage, Some("Ebook")),
            KeyBinding::new("pageup", actions::BookPreviousPage, Some("Ebook")),
            KeyBinding::new("right", actions::BookNextChapter, Some("Ebook")),
            KeyBinding::new("left", actions::BookPreviousChapter, Some("Ebook")),
            KeyBinding::new("secondary-shift-t", actions::BookContents, Some("Ebook")),
        ]);
        cx.on_action(|_: &Quit, cx| request_quit(cx));
        cx.on_action(|_: &NewWindow, cx| {
            if !updates::busy(cx) {
                open_workspace(None, cx);
            }
        });
        cx.on_action(|_: &CheckForUpdates, cx| updates::show_from_menu(cx));
        cx.on_action(|_: &DefaultApplications, cx| open_default_applications(cx));
        cx.on_action(|_: &ToggleDarkMode, cx| toggle_dark_mode(cx));
        cx.on_action(|_: &About, cx| show_about(cx));
        cx.on_action(|_: &OpenSettings, cx| show_settings(cx));
        cx.on_action(|_: &LanguageSystem, cx| change_language("system", cx));
        cx.on_action(|_: &LanguageChinese, cx| change_language("zh-CN", cx));
        cx.on_action(|_: &LanguageEnglish, cx| change_language("en", cx));
        cx.on_action(|_: &KeyboardShortcuts, cx| shortcuts::open(cx));
        cx.bind_keys([
            KeyBinding::new("secondary-q", Quit, None),
            KeyBinding::new("secondary-,", OpenSettings, None),
            KeyBinding::new("secondary-shift-n", NewWindow, None),
            KeyBinding::new("secondary-o", OpenFile, None),
            KeyBinding::new("secondary-s", SaveFile, None),
            KeyBinding::new("secondary-shift-o", OpenFolder, None),
            KeyBinding::new("secondary-r", Refresh, None),
            KeyBinding::new("secondary-w", CloseWindow, None),
            KeyBinding::new("secondary-shift-w", CloseWorkspace, None),
            KeyBinding::new("secondary-alt-o", AddFolder, None),
            KeyBinding::new("secondary-p", SearchWorkspace, None),
            KeyBinding::new("secondary-b", ToggleSidebar, None),
            KeyBinding::new("secondary-shift-v", TogglePreview, None),
            KeyBinding::new("alt-z", ToggleWordWrap, None),
            KeyBinding::new("secondary-c", gpui_kit::component::input::Copy, None),
            KeyBinding::new("ctrl-c", gpui_kit::component::input::Copy, None),
            KeyBinding::new("ctrl-x", gpui_kit::component::input::Cut, Some("Input")),
            KeyBinding::new("ctrl-v", gpui_kit::component::input::Paste, Some("Input")),
            KeyBinding::new("secondary-shift-]", NextTab, None),
            KeyBinding::new("secondary-shift-[", PreviousTab, None),
            KeyBinding::new("ctrl-tab", NextTab, None),
            KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
            KeyBinding::new("alt-left", NavigateBack, None),
            KeyBinding::new("alt-right", NavigateForward, None),
            KeyBinding::new("secondary-alt-t", ToggleDarkMode, None),
            KeyBinding::new("f1", KeyboardShortcuts, None),
        ]);
        menus::update(cx);
        cx.on_window_closed(|cx, closed| {
            cx.global_mut::<OpenWorkspaces>()
                .0
                .retain(|(_, handle)| handle.window_id() != closed);
            cx.refresh_windows();
        })
        .detach();

        let requested = std::mem::take(&mut *pending.borrow_mut());
        updates::start_application(path.map(Into::into), requested, cx);
    });
    Ok(())
}

pub(crate) fn reveal_existing_file(
    path: &std::path::Path,
    exclude: Option<EntityId>,
    cx: &mut App,
) -> bool {
    let Some(windows) = cx.try_global::<OpenWorkspaces>().map(|v| v.0.clone()) else {
        return false;
    };
    windows
        .into_iter()
        .filter(|(view, _)| Some(view.entity_id()) != exclude)
        .any(|(view, handle)| {
            handle
                .update(cx, |_, window, cx| {
                    view.update(cx, |v, cx| v.reveal_file_path(path, window, cx))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        })
}

fn open_system_files(urls: Vec<String>, cx: &mut App) {
    updates::interrupt_for_files(cx);
    for value in urls {
        if let Ok(url) = url::Url::parse(&value)
            && url.scheme() == "file"
            && let Ok(path) = url.to_file_path()
        {
            if reveal_existing_file(&path, None, cx) {
                continue;
            }
            let windows = cx.global::<OpenWorkspaces>().0.clone();
            let reused = windows.iter().rev().any(|(view, handle)| {
                handle
                    .update(cx, |_, window, cx| {
                        view.update(cx, |v, cx| v.open_system_file(path.clone(), window, cx))
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
            });
            if reused {
                cx.activate(true);
            } else {
                open_workspace(Some(path), cx);
            }
        }
    }
}

fn open_workspace(path: Option<std::path::PathBuf>, cx: &mut App) {
    open_workspace_with_session(path, None, cx);
}
fn open_workspace_with_session(
    path: Option<std::path::PathBuf>,
    session: Option<glim_core::update::WorkspaceSession>,
    cx: &mut App,
) {
    // Use the usable screen rectangle directly. Maximized invokes macOS zoom,
    // which toggles an already full-size initial frame back to a smaller one.
    let bounds = cx
        .primary_display()
        .map(|display| display.visible_bounds())
        .unwrap_or_else(|| Bounds::centered(None, size(px(1100.), px(760.)), cx));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(800.), px(480.))),
        titlebar: Some(TitlebarOptions {
            // Center macOS traffic lights in the workspace's 48px title bar.
            traffic_light_position: Some(point(px(9.), px(17.))),
            ..gpui_kit::component::TitleBar::title_bar_options()
        }),
        ..gpui_kit::component::TitleBar::window_options()
    };
    match gpui_kit::open_window(options, cx, |window, cx| {
        window.set_draw_panic_handler(glim_services::diagnostics::take_render_failure);
        let restore =
            session.is_none() && path.is_none() && cx.global::<OpenWorkspaces>().0.is_empty();
        let workspace = cx.new(|cx| Workspace::new(path, window, cx));
        if let Some(session) = session {
            workspace.update(cx, |view, cx| {
                view.restore_update_session(session, window, cx)
            });
        } else if restore {
            workspace.update(cx, |view, cx| view.restore_workspace(window, cx));
        }
        let windows = &mut cx.global_mut::<OpenWorkspaces>().0;
        windows.retain(|(view, _)| view.upgrade().is_some());
        windows.push((workspace.downgrade(), window.window_handle()));
        workspace
    }) {
        Ok(_) => cx.activate(true),
        Err(error) => log::error!("Cannot open Glim window: {error:#}"),
    }
}

#[derive(Default)]
struct OpenWorkspaces(Vec<(WeakEntity<Workspace>, AnyWindowHandle)>);
impl Global for OpenWorkspaces {}
impl OpenWorkspaces {
    fn windows(cx: &App) -> Vec<(WeakEntity<Workspace>, AnyWindowHandle)> {
        cx.try_global::<Self>()
            .map(|v| v.0.clone())
            .unwrap_or_default()
    }
}

pub(crate) fn any_other_saving(exclude: EntityId, cx: &App) -> bool {
    cx.global::<OpenWorkspaces>().0.iter().any(|(v, _)| {
        v.upgrade()
            .is_some_and(|v| v.entity_id() != exclude && v.read(cx).is_saving())
    })
}
fn request_quit(cx: &mut App) {
    if updates::busy(cx) || updates::prepare_quit(cx) {
        return;
    }
    let windows = cx.global::<OpenWorkspaces>().0.clone();
    let target = windows
        .iter()
        .find(|(v, _)| v.upgrade().is_some_and(|v| v.read(cx).is_saving()))
        .or_else(|| {
            windows
                .iter()
                .find(|(v, _)| v.upgrade().is_some_and(|v| v.read(cx).has_unsaved()))
        });
    if let Some((view, handle)) = target {
        let _ = handle.update(cx, |_, window, cx| {
            let _ = view.update(cx, |v, cx| v.request_quit(window, cx));
        });
    } else {
        cx.quit();
    }
}

#[derive(Default)]
struct GitEditorLock(bool);
impl Global for GitEditorLock {}
pub(crate) fn git_editors_locked(cx: &App) -> bool {
    updates::busy(cx) || cx.try_global::<GitEditorLock>().is_some_and(|lock| lock.0)
}
pub(crate) fn lock_git_editors(exclude: EntityId, cx: &mut App) -> Result<()> {
    anyhow::ensure!(
        !git_editors_locked(cx),
        "Another Git operation is updating the working tree"
    );
    let windows = cx
        .try_global::<OpenWorkspaces>()
        .map(|w| w.0.clone())
        .unwrap_or_default();
    let others = windows
        .into_iter()
        .filter_map(|(v, _)| v.upgrade())
        .filter(|v| v.entity_id() != exclude)
        .collect::<Vec<_>>();
    anyhow::ensure!(
        others.iter().all(|v| !v.read(cx).has_unsaved()
            && !v.read(cx).is_saving()
            && !v.read(cx).has_running_git(cx)),
        "Another window has unsaved changes or an active operation. Save drafts and wait before updating the working tree."
    );
    cx.set_global(GitEditorLock(true));
    for view in others {
        view.update(cx, |v, cx| v.set_git_editing_locked(true, cx));
    }
    Ok(())
}
pub(crate) fn unlock_git_editors(exclude: EntityId, cx: &mut App) {
    cx.set_global(GitEditorLock(false));
    let windows = cx
        .try_global::<OpenWorkspaces>()
        .map(|w| w.0.clone())
        .unwrap_or_default();
    for (view, handle) in windows {
        if let Some(view) = view.upgrade()
            && view.entity_id() != exclude
        {
            let _ = handle.update(cx, |_, window, cx| {
                view.update(cx, |v, cx| v.unlock_git_and_refresh(window, cx));
            });
        }
    }
}

pub(crate) struct LanguagePreference(pub String);
impl Global for LanguagePreference {}

#[derive(Default)]
struct LanguageSaveTask(Option<Task<()>>);
impl Global for LanguageSaveTask {}
pub(crate) fn change_language(language: &'static str, cx: &mut App) {
    if updates::busy(cx) {
        return;
    }
    cx.global_mut::<LanguagePreference>().0 = language.to_owned();
    glim_services::locale::set(language);
    gpui_kit::component::set_locale(glim_services::locale::code());
    menus::update(cx);
    for (view, handle) in cx.global::<OpenWorkspaces>().0.clone() {
        if let Some(view) = view.upgrade() {
            let _ = handle.update(cx, |_, window, cx| {
                view.update(cx, |view, cx| view.refresh_language(window, cx))
            });
        }
    }
    cx.refresh_windows();
    let previous = cx.global_mut::<LanguageSaveTask>().0.take();
    let task = cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            previous.await;
        }
        let result = cx
            .background_executor()
            .spawn(async move { glim_services::preferences::save_language(language) })
            .await;
        if let Err(error) = result {
            log::error!("Cannot remember language: {error:#}");
        }
    });
    cx.global_mut::<LanguageSaveTask>().0 = Some(task);
}

/// Complete a filesystem mutation in every window displaying the same paths.
pub(crate) fn finish_trash_in_other_windows(
    exclude: EntityId,
    path: &std::path::Path,
    cx: &mut App,
) {
    for (view, handle) in OpenWorkspaces::windows(cx) {
        if let Some(view) = view.upgrade()
            && view.entity_id() != exclude
        {
            let _ = handle.update(cx, |_, window, cx| {
                view.update(cx, |view, cx| view.did_trash(path, window, cx));
            });
        }
    }
}
