//! Shared content-level floating controls with a transparent bottom hover zone.
use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};
use std::time::Duration;

#[derive(Default)]
struct HoverState {
    hovered: bool,
    dragging: bool,
    expanded: bool,
    task: Option<Task<()>>,
}
impl HoverState {
    fn schedule(&mut self, show: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.task = None;
        if self.dragging || self.expanded == show {
            return;
        }
        self.task = Some(cx.spawn_in(window, async move |state, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(if show { 250 } else { 700 }))
                .await;
            let _ = state.update_in(cx, |state, _, cx| {
                state.task = None;
                if !state.dragging && state.hovered == show {
                    state.expanded = show;
                    cx.notify();
                }
            });
        }));
    }
}
#[derive(IntoElement)]
struct FloatingControls {
    id: &'static str,
    controls: AnyElement,
    bottom: Pixels,
    persistent: bool,
}
impl RenderOnce for FloatingControls {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id, cx, |_, _| HoverState::default());
        let expanded = self.persistent || state.read(cx).expanded;
        div()
            .id((ElementId::from(self.id), "hover-zone"))
            .absolute()
            .left(px(16.))
            .right(px(16.))
            .bottom(self.bottom)
            .min_h(px(72.))
            .flex()
            .items_end()
            .justify_center()
            .on_hover(window.listener_for(&state, |state, hovered, window, cx| {
                state.hovered = *hovered;
                state.schedule(*hovered, window, cx);
            }))
            .on_mouse_move(window.listener_for(
                &state,
                |state, event: &MouseMoveEvent, window, cx| {
                    state.dragging = event.pressed_button.is_some();
                    if state.dragging {
                        state.task = None;
                    } else if state.hovered && !state.expanded && state.task.is_none() {
                        state.schedule(true, window, cx);
                    }
                },
            ))
            .on_mouse_up_out(
                MouseButton::Left,
                window.listener_for(&state, |state, _, window, cx| {
                    state.dragging = false;
                    if !state.hovered {
                        state.schedule(false, window, cx);
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                window.listener_for(&state, |state, _, _, _| {
                    state.dragging = false;
                }),
            )
            .child(
                div()
                    .id(self.id)
                    .when(!expanded, |view| view.invisible())
                    .max_w(relative(1.))
                    .min_w_0()
                    .px_3()
                    .py_2()
                    .rounded(px(24.))
                    .border_1()
                    .border_color(cx.theme().border.opacity(0.7))
                    .bg(cx.theme().background.opacity(0.94))
                    .shadow_md()
                    .overflow_x_scroll()
                    .occlude()
                    .child(self.controls),
            )
    }
}
pub(super) fn floating(id: &'static str, controls: impl IntoElement, cx: &App) -> impl IntoElement {
    floating_at(id, controls, px(16.), cx)
}
pub(super) fn floating_at(
    id: &'static str,
    controls: impl IntoElement,
    bottom: Pixels,
    _cx: &App,
) -> impl IntoElement {
    FloatingControls {
        id,
        controls: controls.into_any_element(),
        bottom,
        persistent: false,
    }
}

/// Frequent canvas operations stay discoverable without a hidden hover target.
pub(super) fn persistent(id: &'static str, controls: impl IntoElement) -> impl IntoElement {
    FloatingControls {
        id,
        controls: controls.into_any_element(),
        bottom: px(16.),
        persistent: true,
    }
}
