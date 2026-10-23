use glim_services::{
    locale::tr,
    xmind::{MindMap, Sheet},
};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Selectable,
        button::{Button, ButtonVariants},
        h_flex, v_flex,
    },
    prelude::FluentBuilder,
    *,
};
use std::{collections::HashSet, sync::Arc};

#[derive(Clone, Copy)]
struct Position {
    index: usize,
    x: f32,
    y: f32,
    depth: usize,
}
struct Layout {
    nodes: Vec<Position>,
    edges: Vec<(Position, Position)>,
    width: f32,
    height: f32,
}
fn layout(sheet: &Sheet, collapsed: &HashSet<usize>) -> Layout {
    fn place(
        index: usize,
        depth: usize,
        cursor: &mut f32,
        sheet: &Sheet,
        collapsed: &HashSet<usize>,
        result: &mut Layout,
    ) -> Position {
        let start = *cursor;
        let children = if collapsed.contains(&index) {
            &[][..]
        } else {
            &sheet.topics[index].children[..]
        };
        let mut placed = Vec::new();
        for &child in children {
            placed.push(place(child, depth + 1, cursor, sheet, collapsed, result));
        }
        let y = if let (Some(first), Some(last)) = (placed.first(), placed.last()) {
            (first.y + last.y) / 2.
        } else {
            *cursor += 128.;
            start
        };
        let position = Position {
            index,
            x: 32. + depth as f32 * 320.,
            y,
            depth,
        };
        result.width = result.width.max(position.x + 272.);
        result.nodes.push(position);
        for child in placed {
            result.edges.push((position, child));
        }
        position
    }
    let mut result = Layout {
        nodes: Vec::new(),
        edges: Vec::new(),
        width: 0.,
        height: 0.,
    };
    let mut cursor = 32.;
    for &root in &sheet.roots {
        place(root, 0, &mut cursor, sheet, collapsed, &mut result);
        cursor += 48.;
    }
    result.height = cursor + 32.;
    result
}
pub(super) struct XMindReader {
    document: MindMap,
    embedded: bool,
    pan: Option<(Point<Pixels>, Point<Pixels>)>,
    focus: Option<FocusHandle>,
    sheet: usize,
    selected: usize,
    collapsed: HashSet<usize>,
    layout: Arc<Layout>,
    zoom: f32,
    scroll: ScrollHandle,
}
impl XMindReader {
    pub fn new(document: MindMap) -> Self {
        let selected = document.sheets[0].roots[0];
        let mut view = Self {
            document,
            embedded: false,
            pan: None,
            focus: None,
            sheet: 0,
            selected,
            collapsed: HashSet::new(),
            layout: Arc::new(Layout {
                nodes: Vec::new(),
                edges: Vec::new(),
                width: 0.,
                height: 0.,
            }),
            zoom: 1.,
            scroll: ScrollHandle::new(),
        };
        view.default_expansion();
        view.rebuild();
        view.center_root();
        view
    }
    pub(super) fn embedded(document: MindMap) -> Self {
        let mut view = Self::new(document);
        view.embedded = true;
        view
    }
    pub(super) fn reset_view(&mut self, cx: &mut Context<Self>) {
        glim_services::telemetry::reader_feature("xmind", "reset_view", []);
        self.zoom = 1.;
        self.scroll.set_offset(point(px(0.), px(0.)));
        self.center_root();
        cx.notify();
    }
    pub(super) fn fold_all(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        glim_services::telemetry::reader_feature(
            "xmind",
            "fold_all",
            [("collapsed", glim_services::telemetry::flag(collapsed))],
        );
        self.collapsed = if collapsed {
            (0..self.document.sheets[self.sheet].topics.len()).collect()
        } else {
            HashSet::new()
        };
        self.rebuild();
        self.center_root();
        cx.notify();
    }
    pub(super) fn copy_selected(&self, cx: &mut App) {
        glim_services::telemetry::reader_feature("xmind", "copy_selected", []);
        let topic = &self.document.sheets[self.sheet].topics[self.selected];
        cx.write_to_clipboard(ClipboardItem::new_string(if topic.notes.is_empty() {
            topic.title.clone()
        } else {
            topic.notes.clone()
        }));
    }
    fn default_expansion(&mut self) {
        self.collapsed.clear();
        let sheet = &self.document.sheets[self.sheet];
        if sheet.topics.len() <= 80 {
            return;
        }
        let mut pending = sheet.roots.iter().map(|&i| (i, 0)).collect::<Vec<_>>();
        while let Some((index, depth)) = pending.pop() {
            if depth >= 2 {
                self.collapsed.insert(index);
            }
            pending.extend(sheet.topics[index].children.iter().map(|&i| (i, depth + 1)));
        }
    }
    fn rebuild(&mut self) {
        self.layout = Arc::new(layout(&self.document.sheets[self.sheet], &self.collapsed));
    }
    fn center_root(&self) {
        let root = self.document.sheets[self.sheet].roots[0];
        if let Some(position) = self.layout.nodes.iter().find(|p| p.index == root) {
            let height = f32::from(self.scroll.bounds().size.height).max(400.);
            self.scroll.set_offset(point(
                px(0.),
                px(-(position.y * self.zoom - height / 2. + 48. * self.zoom).max(0.)),
            ));
        }
    }
    pub(super) fn change_zoom(&mut self, delta: f32, cx: &mut Context<Self>) {
        glim_services::telemetry::reader_feature(
            "xmind",
            "zoom_button",
            [
                ("increase", glim_services::telemetry::flag(delta > 0.)),
                (
                    "zoom_percent",
                    glim_services::telemetry::number((self.zoom * 100.) as u64),
                ),
            ],
        );
        let viewport = self.scroll.bounds().size;
        self.zoom_at(
            self.zoom + delta,
            point(viewport.width / 2., viewport.height / 2.),
            cx,
        );
    }
    fn zoom_at(&mut self, zoom: f32, anchor: Point<Pixels>, cx: &mut Context<Self>) {
        if !zoom.is_finite() {
            return;
        }
        let old = self.zoom;
        self.zoom = zoom.clamp(0.1, 4.);
        let offset = self.scroll.offset();
        let ratio = self.zoom / old;
        self.scroll.set_offset(point(
            ((offset.x - anchor.x) * ratio + anchor.x).min(px(0.)),
            ((offset.y - anchor.y) * ratio + anchor.y).min(px(0.)),
        ));
        cx.notify();
    }
}
impl Render for XMindReader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let sheet = &self.document.sheets[self.sheet];
        let topic = &sheet.topics[self.selected];
        let zoom = self.zoom;
        let layout = self.layout.clone();
        let color = cx.theme().border;
        let viewport = self.scroll.bounds().size;
        let offset = self.scroll.offset();
        let visible = |p: &Position| {
            viewport.width == px(0.)
                || (p.x * zoom + f32::from(offset.x) < f32::from(viewport.width) + 240.
                    && (p.x + 240.) * zoom + f32::from(offset.x) > -240.
                    && p.y * zoom + f32::from(offset.y) < f32::from(viewport.height) + 100.
                    && (p.y + 96.) * zoom + f32::from(offset.y) > -100.)
        };
        v_flex()
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx)
            })
            .on_action(
                cx.listener(|view, _: &gpui_kit::component::input::Copy, _, cx| {
                    view.copy_selected(cx)
                }),
            )
            .relative()
            .size_full()
            .min_h_0()
            .gap_0()
            .child(
                h_flex()
                    .id("xmind-sheets")
                    .gap_2()
                    .p_2()
                    .w_full()
                    .overflow_x_scroll()
                    .children(
                        self.document
                            .sheets
                            .iter()
                            .enumerate()
                            .map(|(index, sheet)| {
                                Button::new(("xmind-sheet", index))
                                    .ghost()
                                    .selected(index == self.sheet)
                                    .label(if sheet.title.is_empty() {
                                        format!("{} {}", tr("Sheet"), index + 1)
                                    } else {
                                        if self.embedded {
                                            tr("Interactive graph")
                                        } else {
                                            sheet.title.clone()
                                        }
                                    })
                                    .on_click(cx.listener(move |v, _, _, cx| {
                                        glim_services::telemetry::reader_feature("xmind", "select_sheet", []);
                                                    v.sheet = index;
                                        v.selected = v.document.sheets[index].roots[0];
                                        v.default_expansion();
                                        v.rebuild();
                                        v.scroll = ScrollHandle::new();
                                        v.center_root();
                                        cx.notify();
                                    }))
                            }),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(
                        div()
                            .id("xmind-viewport")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_scroll()
                            .track_scroll(&self.scroll)
                            .on_pinch(cx.listener(|view, event: &PinchEvent, _, cx| {
                                let anchor = event.position - view.scroll.bounds().origin;
                                view.pan = None;
                                view.zoom_at(view.zoom * (1. + event.delta), anchor, cx);
                                cx.stop_propagation();
                            }))
                            .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|view, event: &MouseDownEvent, _, _| {
                                    view.pan = Some((event.position, view.scroll.offset()));
                                }),
                            )
                            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                                if event.pressed_button == Some(MouseButton::Left) {
                                    if let Some((origin, offset)) = view.pan {
                                        view.scroll.set_offset(offset + (event.position - origin));
                                        cx.notify();
                                    }
                                } else {
                                    view.pan = None;
                                }
                            }))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|view, _, _, _| view.pan = None),
                            )
                            .on_mouse_up_out(
                                MouseButton::Left,
                                cx.listener(|view, _, _, _| view.pan = None),
                            )
                            .child(
                                div()
                                    .relative()
                                    .flex_shrink_0()
                                    .w(px(self.layout.width * zoom))
                                    .h(px(self.layout.height * zoom))
                                    .child(
                                        canvas(
                                            |_, _, _| (),
                                            move |bounds, _, window, _| {
                                                for (from, to) in &layout.edges {
                                                    let a = bounds.origin
                                                        + point(
                                                            px((from.x + 240.) * zoom),
                                                            px((from.y + 48.) * zoom),
                                                        );
                                                    let b = bounds.origin
                                                        + point(
                                                            px(to.x * zoom),
                                                            px((to.y + 48.) * zoom),
                                                        );
                                                    let mut path = PathBuilder::stroke(px(1.5));
                                                    path.move_to(a);
                                                    path.cubic_bezier_to(
                                                        b,
                                                        point(a.x + px(40. * zoom), a.y),
                                                        point(b.x - px(40. * zoom), b.y),
                                                    );
                                                    if let Ok(path) = path.build() {
                                                        window.paint_path(path, color);
                                                    }
                                                }
                                            },
                                        )
                                        .absolute()
                                        .size_full(),
                                    )
                                    .children(self.layout.nodes.iter().filter(|p| visible(p)).map(
                                        |p| {
                                            let index = p.index;
                                            let topic = &sheet.topics[index];
                                            let root = p.depth == 0;
                                            v_flex()
                                                .id(("xmind-topic", index))
                                                .role(Role::Button)
                                                .aria_label(if topic.title.is_empty() {
                                                    tr("Untitled topic")
                                                } else {
                                                    topic.title.clone()
                                                })
                                                .absolute()
                                                .left(px(p.x * zoom))
                                                .top(px(p.y * zoom))
                                                .w(px(240. * zoom))
                                                .h(px(96. * zoom))
                                                .p(px(12. * zoom))
                                                .gap_1()
                                                .rounded_lg()
                                                .border_1()
                                                .border_color(if index == self.selected {
                                                    cx.theme().primary
                                                } else {
                                                    cx.theme().border
                                                })
                                                .bg(if root {
                                                    cx.theme().muted
                                                } else {
                                                    cx.theme().background
                                                })
                                                .text_color(cx.theme().foreground)
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |v, _, _, cx| {
                                                    glim_services::telemetry::reader_feature("xmind", "select_topic", []);
                                                    v.selected = index;
                                                    cx.notify();
                                                }))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_h_0()
                                                        .overflow_hidden()
                                                        .text_size(px(if self.embedded {
                                                            12. * zoom
                                                        } else {
                                                            14. * zoom
                                                        }))
                                                        .line_clamp(if self.embedded {
                                                            4
                                                        } else {
                                                            2
                                                        })
                                                        .child(if topic.title.is_empty() {
                                                            tr("Untitled topic")
                                                        } else {
                                                            topic.title.clone()
                                                        }),
                                                )
                                                .when(!topic.children.is_empty(), |node| {
                                                    node.child(
                                                        div()
                                                            .id(("xmind-fold", index))
                                                            .role(Role::Button)
                                                            .aria_label(format!(
                                                                "{}: {}",
                                                                tr(
                                                                    if self
                                                                        .collapsed
                                                                        .contains(&index)
                                                                    {
                                                                        "Expand topic"
                                                                    } else {
                                                                        "Collapse topic"
                                                                    }
                                                                ),
                                                                topic.title
                                                            ))
                                                            .h(px(20. * zoom))
                                                            .flex_shrink_0()
                                                            .text_size(px(12. * zoom))
                                                            .child(format!(
                                                                "{} {}",
                                                                if self.collapsed.contains(&index) {
                                                                    "+"
                                                                } else {
                                                                    "−"
                                                                },
                                                                topic.children.len()
                                                            ))
                                                            .on_click(cx.listener(
                                                                move |v, _, _, cx| {
                                                                    cx.stop_propagation();
                                                                    glim_services::telemetry::reader_feature("xmind", "toggle_topic", []);
                                                    if !v.collapsed.remove(&index) {
                                                                        v.collapsed.insert(index);
                                                                    }
                                                                    v.rebuild();
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    )
                                                })
                                        },
                                    )),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("xmind-details")
                            .w(px(260.))
                            .flex_shrink_0()
                            .h_full()
                            .overflow_y_scroll()
                            .border_l_1()
                            .border_color(cx.theme().border)
                            .p_4()
                            .gap_3()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr(if self.embedded {
                                        "Node details"
                                    } else {
                                        "Topic details"
                                    })),
                            )
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .whitespace_normal()
                                    .child(if topic.title.is_empty() {
                                        tr("Untitled topic")
                                    } else {
                                        topic.title.clone()
                                    }),
                            )
                            .when(!topic.labels.is_empty(), |v| {
                                v.child(
                                    div()
                                        .text_sm()
                                        .whitespace_normal()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(topic.labels.join(" · ")),
                                )
                            })
                            .when(!topic.notes.is_empty(), |v| {
                                v.child(
                                    div()
                                        .text_sm()
                                        .whitespace_normal()
                                        .child(topic.notes.clone()),
                                )
                            })
                            .when_some(topic.link.clone(), |v, link| {
                                let safe = url::Url::parse(&link)
                                    .is_ok_and(|u| matches!(u.scheme(), "https" | "http"));
                                v.child(div().text_sm().whitespace_normal().child(link.clone()))
                                    .when(safe, |v| {
                                        v.child(
                                            Button::new("xmind-open-link")
                                                .ghost()
                                                .label(tr("Open link"))
                                                .on_click(move |_, _, cx| cx.open_url(&link)),
                                        )
                                    })
                            })
                            .children(sheet.relationships.iter().filter_map(|&(a, b)| {
                                let other = if a == self.selected {
                                    b
                                } else if b == self.selected {
                                    a
                                } else {
                                    return None;
                                };
                                Some(div().text_sm().whitespace_normal().child(format!(
                                    "{}: {}",
                                    tr("Related topic"),
                                    sheet.topics[other].title
                                )))
                            })),
                    ),
            )
            .when(!self.embedded, |view| {
                view.child(super::controls::persistent(
                    "xmind-controls",
                    h_flex()
                        .gap_2()
                        .items_center()
                        .p_0()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr(if self.embedded {
                                    "Interactive graph"
                                } else {
                                    "Mind map · Read only"
                                })),
                        )
                        .child(
                            Button::new("xmind-collapse-all")
                                .label(tr("Collapse all"))
                                .on_click(cx.listener(|v, _, _, cx| {
                                    glim_services::telemetry::reader_feature("xmind", "collapse_all", []);
                                                    v.collapsed =
                                        (0..v.document.sheets[v.sheet].topics.len()).collect();
                                    v.rebuild();
                                    v.center_root();
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("xmind-expand-all")
                                .label(tr("Expand all"))
                                .on_click(cx.listener(|v, _, _, cx| {
                                    glim_services::telemetry::reader_feature("xmind", "expand_all", []);
                                                    v.collapsed.clear();
                                    v.rebuild();
                                    v.center_root();
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("xmind-zoom-out")
                                .label("−")
                                .disabled(zoom <= 0.1)
                                .on_click(cx.listener(|v, _, _, cx| v.change_zoom(-0.1, cx))),
                        )
                        .child(div().text_sm().child(format!("{:.0}%", zoom * 100.)))
                        .child(
                            Button::new("xmind-zoom-in")
                                .label("+")
                                .disabled(zoom >= 4.)
                                .on_click(cx.listener(|v, _, _, cx| v.change_zoom(0.1, cx))),
                        )
                        .child(Button::new("xmind-reset").label(tr("Reset view")).on_click(
                            cx.listener(|v, _, _, cx| {
                                v.zoom = 1.;
                                v.center_root();
                                cx.notify();
                            }),
                        )),
                ))
            })
    }
}
