use crate::app::actions::{CloseWindow, CloseWorkspace};
use crate::app::updates::{self, UpdateState};
use glim_core::update::UpdateMode;
use glim_services::locale::tr;
use gpui_kit::{
    base::TextView,
    component::{
        ActiveTheme, Disableable, Sizable, TitleBar,
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
    state: Entity<UpdateState>,
    _subscription: Subscription,
}
impl Updates {
    fn content_height(state: &UpdateState) -> f32 {
        270. + if state.snapshot.notes.is_empty() {
            0.
        } else {
            240.
        } + if state.snapshot.phase == "downloading" {
            40.
        } else {
            0.
        } + if state.snapshot.install_on_quit {
            40.
        } else {
            0.
        } + if state.message.as_ref().is_some_and(|s| !s.is_empty())
            || !state.snapshot.message.is_empty()
        {
            48.
        } else {
            0.
        }
    }

    pub fn new(state: Entity<UpdateState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let subscription = cx.observe_in(&state, window, |_, state, window, cx| {
            let height = Self::content_height(state.read(cx));
            window.resize(size(window.viewport_size().width, px(height)));
            cx.notify();
        });
        window.resize(size(
            window.viewport_size().width,
            px(Self::content_height(state.read(cx))),
        ));
        Self {
            focus,
            state,
            _subscription: subscription,
        }
    }
}
impl Render for Updates {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let snapshot = state.snapshot.clone();
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
            .child(v_flex().id("software-update-panel").flex_1().min_h_0().overflow_hidden().gap_4().p_5()
            .child(h_flex().gap_4().items_center()
                .child(img("branding/glim-welcome.png").size(px(72.)).flex_shrink_0())
                .child(v_flex().flex_1().min_w_0().gap_2()
                    .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(if snapshot.phase == "available" { tr("A new version of Glim is available") } else { tr(label) }))
                    .child(div().text_sm().whitespace_normal().text_color(cx.theme().muted_foreground)
                        .child(if snapshot.has_update() {
                            format!("{} {} · {} {}", tr("Available version"), snapshot.version, tr("Current version"), snapshot.current_version)
                        } else { format!("Glim {}", snapshot.current_version) }))
                )
            )
            .when(!message.is_empty(), |v| v.child(div().text_sm().whitespace_normal().text_color(cx.theme().muted_foreground).child(tr(&message))))
            .when(!snapshot.notes.is_empty(), |v| v.child(
                div().id("update-release-notes").w_full().h(px(240.)).min_h_0().rounded_lg().border_1().border_color(cx.theme().border).overflow_y_scroll().text_sm().whitespace_normal()
                    .child(TextView::markdown("update-release-notes-text", snapshot.notes.clone()).scrollable(false).p_3()
                        .on_link_click(|href, _, _, cx| { if url::Url::parse(href.as_ref()).is_ok_and(|url| matches!(url.scheme(), "https" | "http")) { cx.open_url(href); } }))
            ))
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
            )
    }
}
