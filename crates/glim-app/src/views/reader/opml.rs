use glim_services::{locale::tr, opml::Document};
use gpui_kit::{
    component::{ActiveTheme, Disableable, Sizable, button::Button, h_flex, v_flex},
    *,
};
use std::collections::HashSet;

pub(super) struct OpmlReader {
    document: Document,
    expanded: HashSet<usize>,
    rows: Vec<(usize, usize)>,
    scroll: UniformListScrollHandle,
}
impl OpmlReader {
    pub fn new(document: Document) -> Self {
        let expanded = document
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.children.is_empty())
            .map(|(i, _)| i)
            .collect();
        let mut reader = Self {
            document,
            expanded,
            rows: Vec::new(),
            scroll: UniformListScrollHandle::new(),
        };
        reader.rebuild();
        reader
    }
    fn rebuild(&mut self) {
        self.rows.clear();
        let mut pending = self
            .document
            .roots
            .iter()
            .rev()
            .map(|i| (*i, 0))
            .collect::<Vec<_>>();
        while let Some((index, depth)) = pending.pop() {
            self.rows.push((index, depth));
            if self.expanded.contains(&index) {
                pending.extend(
                    self.document.nodes[index]
                        .children
                        .iter()
                        .rev()
                        .map(|i| (*i, depth + 1)),
                );
            }
        }
    }
}
impl Render for OpmlReader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let feeds = self
            .document
            .nodes
            .iter()
            .filter(|n| n.feed.is_some())
            .count();
        v_flex()
            .size_full()
            .min_h_0()
            .child(
                h_flex()
                    .gap_3()
                    .p_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.document.title.clone()),
                    )
                    .child(format!("{}: {}", tr("Subscriptions"), feeds))
                    .child(
                        Button::new("opml-expand")
                            .small()
                            .label(tr("Expand all"))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.expanded.extend(0..view.document.nodes.len());
                                view.rebuild();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("opml-collapse")
                            .small()
                            .label(tr("Collapse all"))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.expanded.clear();
                                view.rebuild();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                uniform_list(
                    "opml-outlines",
                    self.rows.len(),
                    cx.processor(|view, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|row| {
                                let (index, depth) = view.rows[row];
                                let node = &view.document.nodes[index];
                                let feed = node.feed.clone();
                                let website = node.website.clone();
                                let group = !node.children.is_empty();
                                let marker = if !group {
                                    "·"
                                } else if view.expanded.contains(&index) {
                                    "▾"
                                } else {
                                    "▸"
                                };
                                h_flex()
                                    .h(px(54.))
                                    .w_full()
                                    .min_w_0()
                                    .gap_2()
                                    .px_3()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(div().w(px((depth.min(16) * 16) as f32)).flex_shrink_0())
                                    .child(
                                        Button::new(("opml-toggle", index))
                                            .small()
                                            .label(marker)
                                            .disabled(!group)
                                            .on_click(cx.listener(move |view, _, _, cx| {
                                                if !view.expanded.remove(&index) {
                                                    view.expanded.insert(index);
                                                }
                                                view.rebuild();
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .child(div().truncate().child(
                                                if node.title.is_empty() {
                                                    tr("Untitled")
                                                } else {
                                                    node.title.clone()
                                                },
                                            ))
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(node.description.clone()),
                                            ),
                                    )
                                    .children(website.map(|url| {
                                        Button::new(("opml-website", index))
                                            .small()
                                            .label(tr("Website"))
                                            .on_click(move |_, _, cx| cx.open_url(&url))
                                    }))
                                    .children(feed.map(|url| {
                                        let copy = url.clone();
                                        h_flex()
                                            .gap_1()
                                            .child(
                                                Button::new(("opml-feed", index))
                                                    .small()
                                                    .label(tr("RSS"))
                                                    .on_click(move |_, _, cx| cx.open_url(&url)),
                                            )
                                            .child(
                                                Button::new(("opml-copy", index))
                                                    .small()
                                                    .label(tr("Copy feed URL"))
                                                    .on_click(move |_, _, cx| {
                                                        cx.write_to_clipboard(
                                                            ClipboardItem::new_string(copy.clone()),
                                                        )
                                                    }),
                                            )
                                    }))
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .flex_1()
                .min_h_0(),
            )
    }
}
