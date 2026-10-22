use super::actions::{CloseWindow, CloseWorkspace};
use gpui_kit::{
    component::{ActiveTheme, TitleBar, h_flex, v_flex},
    *,
};

struct Shortcuts {
    focus: FocusHandle,
}
impl Render for Shortcuts {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = [
            ("Open Settings", "⌘,"),
            ("New window", "⌘⇧N"),
            ("Open file", "⌘O"),
            ("Open folder", "⌘⇧O"),
            ("Add workspace folder", "⌘⌥O"),
            ("Save file", "⌘S"),
            ("Refresh", "⌘R"),
            ("Close current tab; close window when empty", "⌘W"),
            ("Close workspace window", "⌘⇧W"),
            ("Quit Glim", "⌘Q"),
            ("Search workspace files", "⌘P"),
            ("Toggle sidebar", "⌘B"),
            ("Toggle preview / source", "⌘⇧V"),
            ("Toggle word wrap", "⌥Z"),
            ("Next tab", "Ctrl+Tab / ⌘⇧]"),
            ("Previous tab", "Ctrl+Shift+Tab / ⌘⇧["),
            ("Navigate back / forward", "⌥← / ⌥→"),
            ("Toggle theme", "⌘⌥T"),
            ("Keyboard shortcuts", "F1"),
            ("Copy / cut / paste / select all (editor)", "⌘C / X / V / A"),
            ("Copy selected text / image / table cell", "⌘C / Ctrl+C"),
            ("Copy preview text (native preview)", "⌘⇧C"),
            ("Zoom in / out (native preview)", "⌘+ / ⌘−"),
            ("Reset zoom / fit (native preview)", "⌘0"),
            ("Actual image size", "⌘1"),
            ("Previous / next preview page", "⌥PageUp / ⌥PageDown"),
            ("Open preview with another app", "⌘⇧E"),
            ("Recognize image text (native preview)", "⌘⇧I"),
            ("Undo (editor)", "⌘Z"),
            ("Redo (editor)", "⌘⇧Z"),
            ("Find in file (editor)", "⌘F"),
            ("Commit staged contents (commit input)", "⌘Enter"),
            ("Select / expand / open in file tree", "↑↓ / ←→ / Enter"),
        ];
        v_flex()
            .size_full()
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &CloseWindow, window, _| window.remove_window()))
            .on_action(cx.listener(|_, _: &CloseWorkspace, window, _| window.remove_window()))
            .child(TitleBar::new().child(glim_services::locale::tr("Keyboard Shortcuts — Glim")))
            .child(
                v_flex()
                    .id("shortcut-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .child(
                        v_flex()
                            .flex_shrink_0()
                            .w_full()
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded(px(8.))
                            .overflow_hidden()
                            .child(
                                h_flex()
                                    .flex_shrink_0()
                                    .w_full()
                                    .bg(cx.theme().muted)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .px_3()
                                            .py_2()
                                            .child(glim_services::locale::tr("Operation")),
                                    )
                                    .child(
                                        div()
                                            .w(px(200.))
                                            .flex_shrink_0()
                                            .px_3()
                                            .py_2()
                                            .border_l_1()
                                            .border_color(cx.theme().border)
                                            .child(glim_services::locale::tr("Shortcut")),
                                    ),
                            )
                            .children(entries.into_iter().map(|(description, mac)| {
                                h_flex()
                                    .flex_shrink_0()
                                    .w_full()
                                    .items_stretch()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .text_sm()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .px_3()
                                            .py_2()
                                            .whitespace_normal()
                                            .child(glim_services::locale::tr(description)),
                                    )
                                    .child(
                                        div()
                                            .w(px(200.))
                                            .flex_shrink_0()
                                            .px_3()
                                            .py_2()
                                            .border_l_1()
                                            .border_color(cx.theme().border)
                                            .child(mac),
                                    )
                            })),
                    ),
            )
    }
}
pub fn open(cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(680.), px(760.)),
            cx,
        ))),
        ..TitleBar::window_options()
    };
    if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            Shortcuts { focus }
        })
    }) {
        log::error!("Cannot open keyboard shortcuts: {error:#}");
    }
}
