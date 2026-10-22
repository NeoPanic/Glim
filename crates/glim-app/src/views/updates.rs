use crate::app::actions::{CloseWindow, CloseWorkspace};
use crate::app::updates::{self, UpdateState};
use glim_core::update::UpdateMode;
use glim_services::locale::tr;
use gpui_kit::{
    base::TextView,
    component::{
        ActiveTheme, Disableable, Selectable, Sizable, TitleBar,
        button::{Button, ButtonVariants},
        h_flex,
        switch::Switch,
        v_flex,
    },
    prelude::FluentBuilder,
    *,
};

pub(crate) struct Updates {
    focus: FocusHandle,
    preferences_open: bool,
    state: Entity<UpdateState>,
    _subscription: Subscription,
}
impl Updates {
    fn content_height(state: &UpdateState, preferences_open: bool) -> f32 {
        if !state.snapshot.enabled {
            return 300.;
        }
        400. + if state.snapshot.notes.is_empty() {
            0.
        } else {
            260.
        } + if preferences_open { 48. } else { 0. }
            + if state.snapshot.phase == "downloading" {
                40.
            } else {
                0.
            }
    }

    pub fn new(state: Entity<UpdateState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let subscription = cx.observe_in(&state, window, |view, state, window, cx| {
            let height = Self::content_height(state.read(cx), view.preferences_open);
            window.resize(size(window.viewport_size().width, px(height)));
            cx.notify();
        });
        window.resize(size(
            window.viewport_size().width,
            px(Self::content_height(state.read(cx), false)),
        ));
        Self {
            focus,
            preferences_open: false,
            state,
            _subscription: subscription,
        }
    }
}
impl Render for Updates {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let snapshot = state.snapshot.clone();
        let backup = state.backup.clone();
        let busy = state.preparing || state.exiting;
        let message = state
            .message
            .clone()
            .unwrap_or_else(|| snapshot.message.clone());
        let label = if state.preparing {
            "Preparing update…"
        } else {
            match snapshot.phase.as_str() {
                "checking" => "Checking for updates…",
                "available" => "An update is available",
                "downloading" => "Downloading update…",
                "extracting" => "Verifying and preparing update…",
                "ready" => "Update ready to install",
                "installing" => "Installing update…",
                "current" => "You’re up to date",
                "error" => "Update could not complete",
                "cancelled" => "Update cancelled",
                "disabled" => "Keep Glim up to date",
                _ => "Software Update",
            }
        };
        let working = busy
            || matches!(
                snapshot.phase.as_str(),
                "checking" | "downloading" | "extracting" | "installing"
            );
        v_flex().size_full().track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &CloseWindow, window, cx| { if !updates::intercept_last_window_close(cx) { window.remove_window(); } }))
            .on_action(cx.listener(|_, _: &CloseWorkspace, window, cx| { if !updates::intercept_last_window_close(cx) { window.remove_window(); } }))
            .child(TitleBar::new().child(tr("Software Update")))
            .child(v_flex().id("software-update-panel").flex_1().min_h_0().overflow_y_scroll().gap_5().p_6()
            .child(h_flex().gap_4().items_center()
                .child(img("branding/glim.png").size(px(72.)).flex_shrink_0())
                .child(v_flex().flex_1().min_w_0().gap_2()
                    .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(if snapshot.phase == "available" { tr("A new version of Glim is available") } else { tr(label) }))
                    .child(div().text_sm().whitespace_normal().text_color(cx.theme().muted_foreground)
                        .child(if snapshot.has_update() {
                            format!("{} {} · {} {}", tr("Available version"), snapshot.version, tr("Current version"), snapshot.current_version)
                        } else { format!("Glim {}", snapshot.current_version) }))
                )
            )
            .when(!message.is_empty(), |v| v.child(div().text_sm().whitespace_normal().text_color(cx.theme().muted_foreground).child(tr(&message))))
            .when(!snapshot.notes.is_empty(), |v| v.child(v_flex().w_full().gap_3().p_5().rounded_lg().border_1().border_color(cx.theme().border).bg(cx.theme().background)
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(tr("What’s new")))
                .when(!snapshot.published.is_empty(), |v| v.child(div().text_xs().text_color(cx.theme().muted_foreground).child(snapshot.published.clone())))
                .child(div().id("update-release-notes").min_h(px(200.)).max_h(px(300.)).overflow_y_scroll().text_sm().whitespace_normal().child(TextView::markdown("update-release-notes-text", snapshot.notes.clone()).scrollable(false)
                    .on_link_click(|href, _, _, cx| { if url::Url::parse(href.as_ref()).is_ok_and(|url| matches!(url.scheme(), "https" | "http")) { cx.open_url(href); } })))))
            .when(snapshot.phase == "downloading" && snapshot.progress_known && snapshot.expected > 0, |v| {
                let progress = (snapshot.received as f32 / snapshot.expected as f32).clamp(0., 1.);
                v.child(v_flex().gap_2()
                    .child(div().w_full().h(px(5.)).rounded_full().bg(cx.theme().muted)
                        .child(div().h_full().w(relative(progress)).rounded_full().bg(cx.theme().primary)))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground)
                        .child(format!("{:.0}% · {:.1} / {:.1} MiB", progress * 100., snapshot.received as f64 / 1048576., snapshot.expected as f64 / 1048576.))))
            })
            .when(snapshot.enabled, |v| v.child(h_flex().gap_2().items_center()
                .child(Switch::new("update-auto-download").small().checked(snapshot.mode == UpdateMode::Automatic)
                    .disabled(working).accessibility_label(tr("Automatically download and install future updates"))
                    .on_click(|enabled, _, cx| updates::command(if *enabled { "automatic" } else { "notify" }, cx)))
                .child(div().text_sm().whitespace_normal().child(tr("Automatically download and install future updates")))))
            .child(h_flex().w_full().gap_3().pt_2()
                .when(snapshot.phase == "available", |v| v.child(Button::new("update-skip").h(px(36.)).flex_shrink_0().rounded_full().label(tr("Skip This Version")).disabled(busy).on_click(|_, window, cx| {
                    updates::command("skip", cx); window.remove_window();
                })))
                .child(div().flex_1())
                .when(snapshot.phase == "available", |v| v.child(Button::new("update-later").h(px(36.)).flex_shrink_0().rounded_full().label(tr("Remind Me Later")).disabled(busy).on_click(|_, window, cx| {
                    updates::command("later", cx); window.remove_window();
                })))
                .when(snapshot.enabled && !snapshot.can_download && !snapshot.can_install && !working, |v| v.child(Button::new("update-check").primary().h(px(36.)).flex_shrink_0().rounded_full().label(tr("Check for Updates…"))
                    .on_click(|_, _, cx| updates::command("check", cx))))
                .when(!snapshot.enabled, |v| v.child(Button::new("update-download-manual").primary().h(px(36.)).flex_shrink_0().rounded_full().label(tr("Download Latest Version")).on_click(|_, _, cx| cx.open_url("https://github.com/glimapp/glim/releases/latest"))))
                .when(snapshot.can_download, |v| v.child(Button::new("update-download").primary().h(px(36.)).flex_shrink_0().rounded_full().label(tr("Download Update")).disabled(busy)
                    .on_click(|_, _, cx| updates::command("download", cx))))
                .when(snapshot.can_install, |v| v
                    .child(Button::new("update-install").primary().h(px(36.)).flex_shrink_0().rounded_full().label(tr("Restart and Update")).disabled(busy).on_click(|_, _, cx| updates::install(cx)))
                    .child(Button::new("update-on-quit").h(px(36.)).flex_shrink_0().rounded_full().label(tr("Install on Quit")).disabled(busy).on_click(|_, _, cx| updates::command("install-on-quit", cx))))
                .when(snapshot.can_cancel && snapshot.phase != "available", |v| v.child(Button::new("update-cancel").h(px(36.)).flex_shrink_0().rounded_full().label(tr("Cancel This Update")).disabled(busy).on_click(|_, _, cx| updates::command("cancel", cx))))

            )
            .when(snapshot.install_on_quit, |v| v.child(div().text_sm().child(tr("This update will install when Glim quits. Cancel this update to keep the current version."))))
            .when(snapshot.enabled, |v| v.child(v_flex().gap_3().pt_4().border_t_1().border_color(cx.theme().border)
                .child(h_flex().justify_between().items_center()
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).child(tr("Update preferences")))
                    .child(Button::new("update-preferences").ghost().disabled(!snapshot.enabled || busy).h(px(36.)).flex_shrink_0().rounded_full().label(tr(if self.preferences_open { "Done" } else { "Change…" }))
                        .on_click(cx.listener(|this, _, window, cx| { this.preferences_open = !this.preferences_open; window.resize(size(window.viewport_size().width, px(Self::content_height(this.state.read(cx), this.preferences_open)))); cx.notify(); }))))
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child(tr(if !snapshot.enabled {
                    "Automatic updates are unavailable in this build."
                } else { match snapshot.mode {
                    UpdateMode::Manual => "Only check manually",
                    UpdateMode::Notify => "Check daily and notify me",
                    UpdateMode::Automatic => "Download automatically and install on quit",
                }})))
                .when(self.preferences_open && snapshot.enabled, |v| v.child(h_flex().w_full().gap_2().children([
                    ("manual", UpdateMode::Manual, "Manual"),
                    ("notify", UpdateMode::Notify, "Notify"),
                    ("automatic", UpdateMode::Automatic, "Automatic"),
                ].map(|(id, mode, label)| Button::new(id).h(px(36.)).flex_shrink_0().rounded_full().label(tr(label)).selected(snapshot.mode == mode)
                    .disabled(busy).on_click(move |_, _, cx| updates::command(id, cx))))))
                .when(!snapshot.last_check.is_empty(), |v| v.child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!("{}: {}", tr("Last check"), snapshot.last_check))))
            )
            )
            .when(backup.is_some() || snapshot.enabled || snapshot.can_install, |v| v.child(h_flex().w_full().gap_2()
                .when(snapshot.can_install, |v| v.child(Button::new("update-save-all").ghost().small().h(px(36.)).flex_shrink_0().rounded_full().label(tr("Save All Files")).disabled(busy).on_click(|_, _, cx| updates::save_all(cx))))
                .when_some(backup, |v, path| v.child(Button::new("update-backup").ghost().small().h(px(36.)).flex_shrink_0().rounded_full().label(tr("Show Recovery Backup"))
                    .on_click(move |_, _, cx| cx.reveal_path(&path))))
                .child(div().flex_1())
                .when(snapshot.enabled, |v| v.child(Button::new("update-releases").ghost().small().h(px(36.)).flex_shrink_0().rounded_full().label(tr("GitHub Releases")).on_click(|_, _, cx| cx.open_url("https://github.com/glimapp/glim/releases/latest"))))
            ))
            )
    }
}
