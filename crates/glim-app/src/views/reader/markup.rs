use super::{OpenLink, diagrams::DiagramPreview, pictures::Pictures, source::SourceReader};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    base::{TextView, TextViewState},
    component::{ActiveTheme, scroll::Scrollbar},
    *,
};
use std::{cell::Cell, collections::HashMap, ops::Range, path::Path, rc::Rc};

pub(super) struct MarkupReader {
    pub source: SourceReader,
    pub preview: bool,
    pub html: bool,
    browser: Option<Entity<super::native::NativeReader>>,
    pub(super) state: Entity<TextViewState>,
    pub scroll: ScrollHandle,
    base: url::Url,
    headings: HashMap<String, Range<usize>>,
    pending_anchor: Option<String>,
    preview_text: String,
    format_path: Option<std::path::PathBuf>,
    _subscription: Subscription,
    reveal_pending: Rc<Cell<bool>>,
    pictures: Entity<Pictures>,
    _picture_subscription: Subscription,
    diagrams: Option<Entity<DiagramPreview>>,
    _diagram_subscription: Option<Subscription>,
}
impl MarkupReader {
    pub fn new(path: &Path, text: &str, window: &mut Window, cx: &mut App) -> Self {
        let scroll = ScrollHandle::new();
        let html = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("html") || s.eq_ignore_ascii_case("htm"));
        let preview = cx
            .try_global::<crate::app::ReaderPreferences>()
            .is_none_or(|preferences| {
                if html {
                    preferences.html_preview
                } else {
                    preferences.markdown_preview
                }
            });
        let format_path = glim_services::text_formats::supports(path).then(|| path.to_path_buf());
        let projected = format_path
            .as_ref()
            .map(|path| glim_services::text_formats::markdown(path, text));
        let preview_content = projected.as_deref().unwrap_or(text);
        let normalized =
            (!html).then(|| glim_services::markdown_preview::normalize_tables(preview_content));
        let preview_content = normalized.as_deref().unwrap_or(preview_content);
        let state = cx.new(|cx| {
            if html {
                TextViewState::html("", cx)
            } else {
                TextViewState::markdown(preview_content, cx)
            }
        });
        let base = url::Url::from_file_path(path).expect("absolute document path");
        let pictures = cx.new(|cx| {
            Pictures::new(
                if html {
                    String::new()
                } else {
                    preview_content.to_owned()
                },
                html,
                base.clone(),
                cx,
            )
        });
        let picture_subscription = cx.observe(&pictures, |_, cx| cx.refresh_windows());
        let reveal_pending = Rc::new(Cell::new(false));
        let pending = reveal_pending.clone();
        let subscription = cx.observe(&state, move |_, cx| {
            if pending.get() {
                cx.refresh_windows();
            }
        });
        let diagrams =
            (!html).then(|| cx.new(|cx| DiagramPreview::new(preview_content.into(), cx)));
        let diagram_subscription = diagrams
            .as_ref()
            .map(|diagrams| cx.observe(diagrams, |_, cx| cx.refresh_windows()));
        let browser = html.then(|| {
            cx.new(|cx| {
                super::native::NativeReader::html(
                    path.to_path_buf(),
                    Some(text.to_owned()),
                    preview,
                    window,
                    cx,
                )
            })
        });
        Self {
            browser,
            pictures,
            _picture_subscription: picture_subscription,
            diagrams,
            _diagram_subscription: diagram_subscription,
            _subscription: subscription,
            reveal_pending,
            source: SourceReader::new(
                text,
                glim_core::language_for_path(path),
                false,
                Vec::new(),
                scroll.clone(),
                window,
                cx,
            ),
            state,
            scroll,
            preview,
            html,
            base,
            headings: headings(preview_content),
            pending_anchor: None,
            preview_text: text.to_owned(),
            format_path,
        }
    }
    pub fn sync_preview(&mut self, cx: &mut App) {
        let text = self.source.state.read(cx).value().to_string();
        if text == self.preview_text {
            return;
        }
        self.preview_text.clone_from(&text);
        if let Some(browser) = &self.browser {
            browser.update(cx, |browser, cx| browser.update_html(text, cx));
            return;
        }
        let text = self
            .format_path
            .as_ref()
            .map(|path| glim_services::text_formats::markdown(path, &text))
            .unwrap_or(text);
        let text = if self.html {
            text
        } else {
            glim_services::markdown_preview::normalize_tables(&text).into_owned()
        };
        self.headings = headings(&text);
        self.state.update(cx, |s, cx| s.set_text(&text, cx));
        self.pictures.update(cx, |pictures, cx| {
            pictures.load(text.clone(), self.html, cx)
        });
        if let Some(diagrams) = &self.diagrams {
            diagrams.update(cx, |diagrams, cx| diagrams.load(text, cx));
        }
    }
    pub fn reveal(&mut self, anchor: &str, cx: &mut App) {
        if let Some(browser) = &self.browser {
            browser.update(cx, |browser, _| browser.reveal_html_anchor(anchor));
            return;
        }
        self.pending_anchor = Some(anchor.to_owned());
        self.reveal_pending.set(true);
    }
    pub fn set_images_active(&self, active: bool, cx: &mut App) {
        if let Some(browser) = &self.browser {
            if active {
                browser.update(cx, |browser, cx| browser.open(cx));
            } else {
                browser.read(cx).hide();
            }
        }
        self.pictures
            .update(cx, |pictures, cx| pictures.set_active(active, cx));
    }
    pub fn render(&mut self, open: OpenLink, cx: &mut App) -> AnyElement {
        self.set_images_active(self.preview, cx);
        if !self.preview {
            return self.source.render(cx);
        }
        if let Some(browser) = &self.browser {
            browser.update(cx, |browser, _| browser.set_html_open(open));
            return browser.clone().into_any_element();
        }
        if self
            .pending_anchor
            .as_ref()
            .is_some_and(|a| !self.headings.contains_key(a))
        {
            self.pending_anchor = None;
            self.reveal_pending.set(false);
        }
        if let Some(anchor) = &self.pending_anchor {
            let range = self.headings.get(anchor).and_then(|r| {
                self.state
                    .read(cx)
                    .rendered_text()
                    .range_for_source(r.clone())
            });
            if let Some(range) = range {
                self.state.update(cx, |s, cx| {
                    let _ = s.reveal_range(range, cx);
                });
                self.pending_anchor = None;
                self.reveal_pending.set(false);
            }
        }
        self.source
            .minimap
            .update(cx, |map, cx| map.set_preview(true, cx));
        let image_base = self.base.clone();
        let pictures = self.pictures.read(cx).sources.clone();
        let animations = self.pictures.read(cx).animations.clone();
        let stills = self.pictures.read(cx).stills.clone();
        let dark = cx.theme().is_dark();
        let link_base = self.base.clone();
        let scroll = self.scroll.clone();
        let minimap = self.source.minimap.clone();
        // gpui-base 0.7.1 gives an unclamped document state `h_full`, even
        // when its outer viewport owns scrolling. An unbounded line cap uses
        // its natural-height layout instead, without limiting document lines.
        // This keeps bottom images (including asynchronously loaded charts)
        // inside the extent measured by our ScrollHandle.
        let text_view = TextView::new(&self.state)
            .text_size(px(14.))
            .scrollable(false)
            .max_lines(usize::MAX)
            .w_full()
            .flex_shrink_0();
        let text_view = if let Some(diagrams) = &self.diagrams {
            text_view.markdown_extensions(super::diagrams::with_alerts(
                diagrams.read(cx).extensions.clone(),
                self.base.clone(),
                open.clone(),
            ))
        } else {
            text_view
        };
        let diagram_controls = self
            .diagrams
            .as_ref()
            .and_then(|view| DiagramPreview::controls(view, cx));
        let body = div()
            .relative()
            .size_full()
            .overflow_hidden()
            .child(
                div()
                    .id(("markdown-scroll", self.state.entity_id()))
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .on_scroll_wheel(move |_, _, cx| minimap.update(cx, |_, cx| cx.notify()))
                    .px_6()
                    .py_4()
                    .pr(px(124.))
                    .child(
                        text_view
                            .image_source(move |uri| {
                                let selected = pictures
                                    .get(uri.as_ref())
                                    .and_then(|p| {
                                        if dark {
                                            p.dark.as_deref()
                                        } else {
                                            p.light.as_deref()
                                        }
                                    })
                                    .unwrap_or(uri.as_ref());
                                match image_base.join(selected) {
                                    Ok(url)
                                        if matches!(url.scheme(), "file" | "http" | "https")
                                            && url
                                                .path()
                                                .to_ascii_lowercase()
                                                .ends_with(".gif") =>
                                    {
                                        // Stay pending until background parsing has registered
                                        // the player; never start GPUI's whole-GIF loader.
                                        let animation = animations.get(url.as_str()).cloned();
                                        ImageSource::from(move |_: &mut Window, cx: &mut App| {
                                            animation
                                                .as_ref()
                                                .and_then(|view| view.read(cx).image())
                                        })
                                    }
                                    Ok(url)
                                        if matches!(url.scheme(), "file" | "http" | "https") =>
                                    {
                                        let image = stills.get(url.as_str()).cloned();
                                        ImageSource::from(move |_: &mut Window, cx: &mut App| {
                                            image.as_ref().and_then(|image| image.read(cx).image())
                                        })
                                    }
                                    Ok(url) => SharedUri::from(url.to_string()).into(),
                                    Err(_) => SharedUri::from(selected.to_owned()).into(),
                                }
                            })
                            .on_link_click(move |href, _, window, cx| {
                                follow_link(&link_base, &open, href, window, cx);
                            })
                            .on_reveal(move |line, _, _| {
                                let viewport = scroll.bounds();
                                let mut offset = scroll.offset();
                                if line.bottom() > viewport.bottom() {
                                    offset.y -= line.bottom() - viewport.bottom();
                                } else if line.top() < viewport.top() {
                                    offset.y += viewport.top() - line.top();
                                }
                                scroll.set_offset(offset);
                            }),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .w(px(110.))
                    .child(self.source.minimap.clone()),
            )
            .child(Scrollbar::vertical(&self.scroll));
        gpui_kit::component::v_flex()
            .size_full()
            .min_h_0()
            .when_some(diagram_controls, |view, controls| view.child(controls))
            .child(div().flex_1().min_h_0().w_full().child(body))
            .into_any_element()
    }
}
pub(super) fn follow_link(
    base: &url::Url,
    open: &OpenLink,
    href: &str,
    window: &mut Window,
    cx: &mut App,
) {
    if let Ok(url) = base.join(href) {
        match url.scheme() {
            "file" => {
                if let Ok(path) = url.to_file_path() {
                    open(path, url.fragment().map(decode_fragment), window, cx);
                }
            }
            "http" | "https" | "mailto" => cx.open_url(url.as_str()),
            _ => {}
        }
    }
}
fn decode_fragment(value: &str) -> String {
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < value.len() {
        if value.as_bytes()[i] == b'%'
            && let Some(code) = value.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(code, 16)
        {
            bytes.push(byte);
            i += 3;
        } else {
            bytes.push(value.as_bytes()[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
fn headings(text: &str) -> HashMap<String, Range<usize>> {
    let mut out = HashMap::new();
    let mut counts = HashMap::<String, usize>::new();
    let mut offset = 0;
    let mut fence = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = !fence;
        }
        if !fence && trimmed.starts_with('#') {
            let title = trimmed
                .trim_start_matches('#')
                .trim()
                .trim_end_matches('#')
                .trim();
            let slug = title
                .to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric() || c.is_whitespace() || matches!(c, '-' | '_'))
                .map(|c| if c.is_whitespace() { '-' } else { c })
                .collect::<String>();
            let count = counts.entry(slug.clone()).or_default();
            let key = if *count == 0 {
                slug
            } else {
                format!("{slug}-{count}")
            };
            *count += 1;
            out.insert(key, offset..offset + line.trim_end().len());
        }
        offset += line.len();
    }
    out
}
#[cfg(test)]
mod tests {
    use super::{decode_fragment, headings};
    #[test]
    fn anchors_handle_unicode_duplicate_titles_and_fences() {
        let text = "# 中文 标题\n# 中文 标题\n```\n# ignore\n```\n";
        let map = headings(text);
        assert_eq!(&text[map["中文-标题"].clone()], "# 中文 标题");
        assert!(map.contains_key("中文-标题-1"));
        assert!(!map.contains_key("ignore"));
        assert_eq!(decode_fragment("%E4%B8%AD%E6%96%87"), "中文");
    }
}
