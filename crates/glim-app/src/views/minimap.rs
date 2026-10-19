use gpui_kit::{
    component::{ActiveTheme, input::EditorState},
    *,
};
use std::{cell::Cell, rc::Rc, sync::Arc};

/// A compact document overview. Text geometry is cached once, not parsed in render.
pub struct Minimap {
    source: Entity<EditorState>,
    lines: Arc<Vec<(u8, u8, i8)>>,
    preview_scroll: ScrollHandle,
    preview: bool,
    preview_list: Option<ListState>,
    dragging: bool,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    _subscription: Subscription,
}

impl Minimap {
    pub fn new(
        source: Entity<EditorState>,
        text: &str,
        is_diff: bool,
        preview_scroll: ScrollHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let lines = text
            .split('\n')
            .map(|line| {
                let indent = line
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .count()
                    .min(100);
                let width = line.chars().take(120).count().saturating_sub(indent);
                let change = if is_diff && line.starts_with('+') {
                    1
                } else if is_diff && line.starts_with('-') {
                    -1
                } else {
                    0
                };
                (indent as u8, width as u8, change)
            })
            .collect();
        let subscription = cx.observe(&source, |_, _, cx| cx.notify());
        Self {
            source,
            lines: Arc::new(lines),
            preview_scroll,
            preview: false,
            preview_list: None,
            dragging: false,
            bounds: Rc::new(Cell::new(Bounds::default())),
            _subscription: subscription,
        }
    }

    pub fn set_text(&mut self, text: &str, is_diff: bool, cx: &mut Context<Self>) {
        self.lines = Arc::new(
            text.split('\n')
                .map(|line| {
                    let indent = line
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .count()
                        .min(100);
                    let width = line.chars().take(120).count().saturating_sub(indent);
                    let change = if is_diff && line.starts_with('+') {
                        1
                    } else if is_diff && line.starts_with('-') {
                        -1
                    } else {
                        0
                    };
                    (indent as u8, width as u8, change)
                })
                .collect(),
        );
        cx.notify();
    }

    pub fn set_preview_list(&mut self, list: ListState, _cx: &mut Context<Self>) {
        self.preview_list = Some(list);
    }

    pub fn set_preview(&mut self, preview: bool, cx: &mut Context<Self>) {
        if self.preview != preview {
            self.preview = preview;
            cx.notify();
        }
    }

    fn seek(&self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let bounds = self.bounds.get();
        let total = self.lines.len().max(1) as f32;
        let rail = position.x >= bounds.origin.x + px(96.);
        let height = if rail {
            f32::from(bounds.size.height)
        } else {
            f32::from(bounds.size.height).min(total * 2.)
        }
        .max(1.);
        let fraction = (f32::from(position.y - bounds.origin.y) / height).clamp(0., 1.);
        if self.preview {
            let (max, visible) = if let Some(list) = &self.preview_list {
                (
                    list.max_offset_for_scrollbar().y,
                    list.viewport_bounds().size.height,
                )
            } else {
                (
                    self.preview_scroll.max_offset().y,
                    self.preview_scroll.bounds().size.height,
                )
            };
            let offset = ((max + visible) * fraction - visible / 2.)
                .max(px(0.))
                .min(max);
            if let Some(list) = &self.preview_list {
                list.set_offset_from_scrollbar(point(px(0.), -offset));
            } else {
                self.preview_scroll.set_offset(point(px(0.), -offset));
            }
            cx.notify();
        } else {
            self.source.update(cx, |state, cx| {
                let line_height = state.line_height().unwrap_or(px(20.));
                let visible = state.visible_row_range().map_or(1, |r| r.len()) as f32;
                let row = (fraction * total - visible / 2.).max(0.);
                state.set_scroll_offset(point(state.scroll_offset().x, -line_height * row), cx);
            });
        }
    }
}

impl Render for Minimap {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lines = self.lines.clone();
        let bounds_cell = self.bounds.clone();
        let source = self.source.clone();
        let scroll = self.preview_scroll.clone();
        let preview_list = self.preview_list.clone();
        let preview = self.preview;
        let foreground = cx.theme().muted_foreground.opacity(0.45);
        let added = cx.theme().green.opacity(0.6);
        let removed = cx.theme().red.opacity(0.6);
        let viewport = cx.theme().primary.opacity(0.12);
        div()
            .id("minimap")
            .w(px(110.))
            .h_full()
            .cursor_pointer()
            .bg(cx.theme().background)
            .border_l_1()
            .border_color(cx.theme().border)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, _, cx| {
                    view.dragging = true;
                    view.seek(event.position, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, _| view.dragging = false),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _, _, _| view.dragging = false),
            )
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                if view.dragging && event.pressed_button == Some(MouseButton::Left) {
                    view.seek(event.position, cx);
                }
            }))
            .child(
                canvas(
                    move |bounds, _, _| {
                        bounds_cell.set(bounds);
                    },
                    move |bounds, _, window, cx| {
                        let total = lines.len().max(1) as f32;
                        let height = f32::from(bounds.size.height).min(total * 2.);
                        let step = (total / (height / 2.).max(1.)).ceil() as usize;
                        for (index, &(indent, width, change)) in
                            lines.iter().enumerate().step_by(step.max(1))
                        {
                            if width == 0 {
                                continue;
                            }
                            let y = height * index as f32 / total;
                            let color = match change {
                                1 => added,
                                -1 => removed,
                                _ => foreground,
                            };
                            window.paint_quad(fill(
                                Bounds::new(
                                    bounds.origin + point(px(4. + indent as f32 * 0.7), px(y)),
                                    size(px((width as f32 * 0.7).min(86.)), px(1.)),
                                ),
                                color,
                            ));
                        }
                        let (start, length) = if preview {
                            let (visible, maximum, offset) = if let Some(list) = &preview_list {
                                (
                                    list.viewport_bounds().size.height,
                                    list.max_offset_for_scrollbar().y,
                                    list.scroll_px_offset_for_scrollbar().y,
                                )
                            } else {
                                (
                                    scroll.bounds().size.height,
                                    scroll.max_offset().y,
                                    scroll.offset().y,
                                )
                            };
                            let visible = f32::from(visible);
                            let full = f32::from(maximum) + visible;
                            if full > 0. {
                                (-f32::from(offset) / full, visible / full)
                            } else {
                                (0., 1.)
                            }
                        } else {
                            let range = source.read(cx).visible_row_range().unwrap_or(0..1);
                            (range.start as f32 / total, range.len() as f32 / total)
                        };
                        window.paint_quad(fill(
                            Bounds::new(
                                bounds.origin + point(px(0.), px(height * start.min(1.))),
                                size(px(96.), px((height * length.min(1.)).max(3.))),
                            ),
                            viewport,
                        ));
                        if !preview && length < 1. {
                            let rail_height = f32::from(bounds.size.height);
                            let thumb_height = (rail_height * length).max(24.).min(rail_height);
                            let progress = (start / (1. - length)).clamp(0., 1.);
                            window.paint_quad(fill(
                                Bounds::new(
                                    bounds.origin
                                        + point(
                                            px(99.),
                                            px((rail_height - thumb_height) * progress),
                                        ),
                                    size(px(8.), px(thumb_height)),
                                ),
                                foreground,
                            ));
                        }
                    },
                )
                .size_full(),
            )
    }
}
