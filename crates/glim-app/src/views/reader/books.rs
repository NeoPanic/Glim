use crate::app::actions::{
    BookContents, BookNextChapter, BookNextPage, BookPreviousChapter, BookPreviousPage,
};
use glim_services::limits as reader_limits;
use glim_services::{
    books::{
        self, Book, ChapterContent, Format,
        preferences::{Bookmark, ReadingState},
    },
    locale::tr,
};
use gpui_kit::{
    base::{InteractiveElementExt as _, TextView, TextViewState, TextViewStyle},
    component::{
        ActiveTheme, Disableable, Selectable, Sizable,
        button::{Button, ButtonVariants},
        h_flex,
        scroll::Scrollbar,
        v_flex,
    },
    prelude::FluentBuilder,
    *,
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

fn book_label(text: &str) -> String {
    let mut label: String = text
        .chars()
        .take(reader_limits::BOOK_STATE_LABEL_CHARS)
        .collect();
    if label.len() < text.len() {
        label.push('…');
    }
    label
}

pub(super) struct BookReader {
    book: Arc<Book>,
    state: ReadingState,
    text: Option<Entity<TextViewState>>,
    paged: Option<Entity<super::paged::PagedReader>>,
    images: Arc<HashMap<String, Arc<Image>>>,
    missing: Arc<Image>,
    comic: Option<Arc<Image>>,
    comic_ratio: f32,
    anchors: HashMap<String, std::ops::Range<usize>>,
    restore: Option<f32>,
    anchor: Option<String>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    toc_scroll: UniformListScrollHandle,
    contents: bool,
    bookmarks: bool,
    loading: bool,
    error: Option<String>,
    task: Option<Task<()>>,
    write_task: Option<Task<()>>,
    writes: Arc<Mutex<Option<ReadingState>>>,
    pub writing: Arc<AtomicBool>,
    _subscription: Option<Subscription>,
}
impl BookReader {
    pub fn new(book: Book, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let book = Arc::new(book);
        let initial = book.clone();
        let (cancel, guard) = glim_services::work::Cancellation::task();
        let work = cx.background_executor().spawn(async move {
            let mut state = books::preferences::read(&initial.fingerprint).unwrap_or_default();
            state.chapter = state.chapter.min(initial.chapters.len() - 1);
            let content = books::chapter_cancellable(&initial, state.chapter, &cancel)?;
            Ok::<_, anyhow::Error>((state, content))
        });
        let task = cx.spawn(async move |view, cx| {
            let _guard = guard;
            let result = work.await;
            let _ = view.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok((state, content)) => {
                        view.state = state;
                        view.install(content, cx);
                    }
                    Err(error) => view.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        });
        Self {
            book,
            state: ReadingState::default(),
            text: None,
            paged: None,
            images: Arc::new(HashMap::new()),
            missing: Arc::new(Image::from_bytes(ImageFormat::Png, books::missing_image())),
            comic: None,
            comic_ratio: 1.,
            anchors: HashMap::new(),
            restore: None,
            anchor: None,
            focus,
            scroll: ScrollHandle::new(),
            toc_scroll: UniformListScrollHandle::new(),
            contents: true,
            bookmarks: false,
            loading: true,
            error: None,
            task: Some(task),
            write_task: None,
            writes: Arc::new(Mutex::new(None)),
            writing: Arc::new(AtomicBool::new(false)),
            _subscription: None,
        }
    }
    fn install(&mut self, mut content: ChapterContent, cx: &mut Context<Self>) {
        self.paged = content
            .page
            .take()
            .map(|page| cx.new(|_| super::paged::PagedReader::readonly(page)));
        self.images = Arc::new(
            content
                .images
                .into_iter()
                .filter(|(_, data)| !data.is_empty())
                .map(|(name, data)| (name, Arc::new(Image::from_bytes(ImageFormat::Png, data))))
                .collect(),
        );
        self.comic_ratio = content
            .comic
            .as_ref()
            .and_then(|data| {
                let width = u32::from_be_bytes(data.get(16..20)?.try_into().ok()?);
                let height = u32::from_be_bytes(data.get(20..24)?.try_into().ok()?);
                (width > 0 && height > 0).then_some(width as f32 / height as f32)
            })
            .unwrap_or(1.);
        self.comic = content
            .comic
            .map(|data| Arc::new(Image::from_bytes(ImageFormat::Png, data)));
        self.anchors = content.anchors;
        let text = cx.new(|cx| TextViewState::html(&content.html, cx));
        self._subscription = Some(cx.observe(&text, |_, _, cx| cx.notify()));
        self.text = Some(text);
        self.scroll = ScrollHandle::new();
        self.restore = Some(self.state.fraction);
        self.loading = false;
        self.error = None;
    }
    fn go(&mut self, index: usize, fraction: f32, anchor: Option<String>, cx: &mut Context<Self>) {
        if index >= self.book.chapters.len() {
            return;
        }
        self.state.chapter = index;
        self.state.fraction = fraction.clamp(0., 1.);
        self.anchor = anchor;
        self.loading = true;
        self.error = None;
        self.save(cx);
        let book = self.book.clone();
        let (cancel, guard) = glim_services::work::Cancellation::task();
        let work = cx
            .background_executor()
            .spawn(async move { books::chapter_cancellable(&book, index, &cancel) });
        self.task = Some(cx.spawn(async move |view, cx| {
            let _guard = guard;
            let result = work.await;
            let _ = view.update(cx, |view, cx| {
                view.task = None;
                view.loading = false;
                match result {
                    Ok(content) => view.install(content, cx),
                    Err(error) => view.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn follow(&mut self, href: &str, cx: &mut Context<Self>) {
        if let Ok(url) = url::Url::parse(href)
            && matches!(url.scheme(), "https" | "http" | "mailto")
        {
            cx.open_url(href);
            return;
        }
        let base = &self.book.chapters[self.state.chapter].href;
        if self.book.format == Format::FictionBook && href.starts_with('#') {
            self.anchor = Some(href.trim_start_matches('#').into());
            cx.notify();
            return;
        }
        let direct = href
            .split('#')
            .next()
            .is_some_and(|path| self.book.chapters.iter().any(|c| c.href == path));
        if let Some(target) = if direct {
            Some(href.to_owned())
        } else {
            books::resolve(base, href)
        } {
            let (path, anchor) = target
                .split_once('#')
                .map(|(p, a)| (p, Some(a.to_owned())))
                .unwrap_or((target.as_str(), None));
            if let Some(index) = self.book.chapters.iter().position(|c| c.href == path) {
                if index == self.state.chapter {
                    self.anchor = anchor;
                    cx.notify();
                } else {
                    self.go(index, 0., anchor, cx);
                }
            }
        }
    }
    fn position(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.restore.is_some() {
            return;
        }
        let maximum = f32::from(self.scroll.max_offset().y);
        let fraction = if maximum > 0. {
            (-f32::from(self.scroll.offset().y) / maximum).clamp(0., 1.)
        } else {
            0.
        };
        if (self.state.fraction - fraction).abs() > 0.001 {
            self.state.fraction = fraction;
            self.save(cx);
            cx.notify();
        }
    }
    fn page(&mut self, next: bool, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        if self.book.format == Format::Comic {
            self.chapter(next, cx);
            return;
        }
        let offset = self.scroll.offset();
        let maximum = self.scroll.max_offset().y;
        let height = self.scroll.bounds().size.height;
        if next && -offset.y >= maximum - px(2.) {
            self.chapter(true, cx);
        } else if !next && offset.y >= -px(2.) {
            self.chapter(false, cx);
        } else {
            self.scroll.set_offset(point(
                px(0.),
                (offset.y + height * if next { -0.9 } else { 0.9 }).clamp(-maximum, px(0.)),
            ));
            self.position(cx);
            cx.notify();
        }
    }
    fn chapter(&mut self, next: bool, cx: &mut Context<Self>) {
        let index = if next {
            self.state.chapter + 1
        } else {
            self.state.chapter.saturating_sub(1)
        };
        if index != self.state.chapter {
            self.go(index, 0., None, cx);
        }
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        *self.writes.lock().unwrap() = Some(self.state.clone());
        if self.writing.swap(true, Ordering::SeqCst) {
            return;
        }
        let writes = self.writes.clone();
        let writing = self.writing.clone();
        let fingerprint = self.book.fingerprint.clone();
        self.write_task = Some(cx.spawn(async move |view, cx| {
            loop {
                let state = {
                    let mut pending = writes.lock().unwrap();
                    if let Some(state) = pending.take() {
                        state
                    } else {
                        writing.store(false, Ordering::SeqCst);
                        break;
                    }
                };
                let key = fingerprint.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { books::preferences::save(&key, &state) })
                    .await;
                if let Err(error) = result {
                    let _ = view.update(cx, |view, cx| {
                        view.error = Some(format!("Cannot save reading position: {error:#}"));
                        cx.notify();
                    });
                }
            }
        }));
    }
    fn bookmark(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.state.bookmarks.iter().position(|b| {
            b.chapter == self.state.chapter && (b.fraction - self.state.fraction).abs() < 0.02
        }) {
            self.state.bookmarks.remove(index);
        } else {
            self.state.bookmarks.push(Bookmark {
                chapter: self.state.chapter,
                fraction: self.state.fraction,
                label: self.book.chapters[self.state.chapter]
                    .title
                    .chars()
                    .take(reader_limits::BOOK_STATE_LABEL_CHARS)
                    .collect(),
            });
        }
        self.save(cx);
        cx.notify();
    }
    pub fn take_write(&mut self) -> Option<Task<()>> {
        self.write_task.take()
    }
}
impl Drop for BookReader {
    fn drop(&mut self) {
        if let Some(task) = self.write_task.take() {
            task.detach();
        }
    }
}
impl Render for BookReader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let font = self.state.font_size;
        let paper = if self.state.paper {
            cx.theme()
                .background
                .blend(
                    cx.theme()
                        .yellow
                        .opacity(if cx.theme().is_dark() { 0.06 } else { 0.12 }),
                )
        } else {
            cx.theme().background
        };
        let marked = self.state.bookmarks.iter().any(|b| {
            b.chapter == self.state.chapter && (b.fraction - self.state.fraction).abs() < 0.02
        });
        let mut content = div().into_any_element();
        if self.loading {
            content = div()
                .p_8()
                .text_color(cx.theme().muted_foreground)
                .child(tr("Loading chapter…"))
                .into_any_element();
        } else if let Some(error) = &self.error {
            content = div()
                .p_8()
                .whitespace_normal()
                .child(error.clone())
                .into_any_element();
        } else if let Some(image) = &self.comic {
            content = img(image.clone())
                .w_full()
                .h(px((self.state.page_width - 64.) / self.comic_ratio))
                .aspect_ratio(self.comic_ratio)
                .object_fit(ObjectFit::Contain)
                .into_any_element();
        } else if let Some(text) = &self.text {
            let scroll_reveal = self.scroll.clone();
            let images = self.images.clone();
            let view = cx.entity().downgrade();
            let fallback = self.missing.clone();
            content = TextView::new(text)
                .scrollable(false)
                .selectable(true)
                .w_full()
                .font_family(if self.state.serif {
                    "Georgia".into()
                } else {
                    cx.theme().font_family.clone()
                })
                .text_size(px(font))
                .line_height(relative(self.state.line_height))
                .style(
                    TextViewStyle::default()
                        .with_foreground(cx.theme().foreground)
                        .with_muted_foreground(cx.theme().muted_foreground)
                        .with_link(cx.theme().primary)
                        .with_selection(cx.theme().selection)
                        .with_border(cx.theme().border)
                        .with_code_background(cx.theme().muted)
                        .with_paragraph_gap(rems(1.2)),
                )
                .image_source(move |uri| {
                    images
                        .get(uri.as_ref())
                        .cloned()
                        .unwrap_or_else(|| fallback.clone())
                        .into()
                })
                .on_reveal(move |line, _, _| {
                    let bounds = scroll_reveal.bounds();
                    let mut offset = scroll_reveal.offset();
                    if line.bottom() > bounds.bottom() {
                        offset.y -= line.bottom() - bounds.bottom();
                    } else if line.top() < bounds.top() {
                        offset.y += bounds.top() - line.top();
                    }
                    scroll_reveal.set_offset(offset);
                })
                .on_link_click(move |href, _, _, cx| {
                    let _ = view.update(cx, |view, cx| view.follow(href, cx));
                })
                .into_any_element();
        }
        let view = cx.entity().downgrade();
        let scroll = self.scroll.clone();
        let progress = if self.book.format == Format::Comic {
            (self.state.chapter + 1) as f32 / self.book.chapters.len() as f32
        } else {
            (self.state.chapter as f32 + self.state.fraction) / self.book.chapters.len() as f32
        };
        v_flex()
            .size_full()
            .min_h_0()
            .track_focus(&self.focus)
            .key_context("Ebook")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| window.focus(&view.focus, cx)),
            )
            .on_action(cx.listener(|view, _: &BookNextPage, _, cx| view.page(true, cx)))
            .on_action(cx.listener(|view, _: &BookPreviousPage, _, cx| view.page(false, cx)))
            .on_action(cx.listener(|view, _: &BookNextChapter, _, cx| view.chapter(true, cx)))
            .on_action(cx.listener(|view, _: &BookPreviousChapter, _, cx| view.chapter(false, cx)))
            .on_action(cx.listener(|view, _: &BookContents, _, cx| {
                view.contents = !view.contents;
                cx.notify();
            }))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .p_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("book-contents")
                            .ghost()
                            .small()
                            .selected(self.contents)
                            .label(tr("Contents"))
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.contents = !v.contents;
                                v.bookmarks = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        v_flex().id("book-metadata")
                            .on_double_click(cx.listener(|view, _, window, cx| {
                                super::source::show_text_dialog(tr("Book metadata"), format!("{}\n\n{}", view.book.title, view.book.author), window, cx);
                            }))
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(book_label(&self.book.title)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(format!(
                                        "{} · {}",
                                        self.book.format.label(),
                                        book_label(&self.book.author)
                                    )),
                            ),
                    )
                    .child(
                        Button::new("book-font-smaller")
                            .ghost()
                            .small()
                            .label("A−")
                            .disabled(font <= 14.)
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.state.font_size = (v.state.font_size - 2.).max(14.);
                                v.save(cx);
                                cx.notify();
                            })),
                    )
                    .child(div().text_xs().child(format!("{font:.0}")))
                    .child(
                        Button::new("book-font-larger")
                            .ghost()
                            .small()
                            .label("A+")
                            .disabled(font >= 32.)
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.state.font_size = (v.state.font_size + 2.).min(32.);
                                v.save(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("book-font")
                            .ghost()
                            .small()
                            .label(tr(if self.state.serif {
                                "Serif"
                            } else {
                                "Sans serif"
                            }))
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.state.serif = !v.state.serif;
                                v.save(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("book-spacing")
                            .ghost()
                            .small()
                            .label(format!("{} {:.1}", tr("Spacing"), self.state.line_height))
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.state.line_height = if v.state.line_height < 1.6 {
                                    1.7
                                } else if v.state.line_height < 1.9 {
                                    2.0
                                } else {
                                    1.5
                                };
                                v.save(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("book-width")
                            .ghost()
                            .small()
                            .label(tr("Page width"))
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.state.page_width = if v.state.page_width < 650. {
                                    720.
                                } else if v.state.page_width < 800. {
                                    880.
                                } else {
                                    560.
                                };
                                v.save(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("book-paper")
                            .ghost()
                            .small()
                            .selected(self.state.paper)
                            .label(tr("Paper"))
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.state.paper = !v.state.paper;
                                v.save(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("book-bookmark")
                            .ghost()
                            .small()
                            .selected(marked)
                            .label(tr(if marked {
                                "Remove bookmark"
                            } else {
                                "Add bookmark"
                            }))
                            .on_click(cx.listener(|v, _, _, cx| v.bookmark(cx))),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .when(self.contents, |row| {
                        row.child(
                            v_flex()
                                .w(px(224.))
                                .h_full()
                                .flex_shrink_0()
                                .border_r_1()
                                .border_color(cx.theme().border)
                                .child(
                                    h_flex()
                                        .p_2()
                                        .gap_2()
                                        .child(
                                            Button::new("book-toc-tab")
                                                .ghost()
                                                .small()
                                                .selected(!self.bookmarks)
                                                .label(tr("Contents"))
                                                .on_click(cx.listener(|v, _, _, cx| {
                                                    v.bookmarks = false;
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            Button::new("book-marks-tab")
                                                .ghost()
                                                .small()
                                                .selected(self.bookmarks)
                                                .label(tr("Bookmarks"))
                                                .on_click(cx.listener(|v, _, _, cx| {
                                                    v.bookmarks = true;
                                                    cx.notify();
                                                })),
                                        ),
                                )
                                .child(
                                    uniform_list(
                                        "book-toc",
                                        if self.bookmarks {
                                            self.state.bookmarks.len()
                                        } else {
                                            self.book.toc.len()
                                        },
                                        cx.processor(|v, range: std::ops::Range<usize>, _, cx| {
                                            range
                                                .map(|i| {
                                                    let (title, href, depth, bookmark) =
                                                        if v.bookmarks {
                                                            let b = &v.state.bookmarks[i];
                                                            (
                                                                b.label.clone(),
                                                                String::new(),
                                                                0,
                                                                Some((b.chapter, b.fraction)),
                                                            )
                                                        } else {
                                                            let t = &v.book.toc[i];
                                                            (
                                                                t.title.clone(),
                                                                t.href.clone(),
                                                                t.depth,
                                                                None,
                                                            )
                                                        };
                                                    let label = book_label(&title);
                                                    let shortened = label != title;
                                                    let full = title.clone();
                                                    let entry = Button::new(("book-toc-item", i))
                                                        .ghost()
                                                        .label(label.clone())
                                                        .tooltip(label)
                                                        .flex_1().min_w_0()
                                                        .h(px(36.))
                                                        .justify_start()
                                                        .pl(px(12. + depth.min(reader_limits::BOOK_TOC_DEPTH) as f32 * 12.))
                                                        .on_click(cx.listener(
                                                            move |v, _, _, cx| {
                                                                if let Some((chapter, fraction)) =
                                                                    bookmark
                                                                {
                                                                    v.go(
                                                                        chapter, fraction, None, cx,
                                                                    );
                                                                } else {
                                                                    v.follow(&href, cx);
                                                                }
                                                            },
                                                        ))
;
                                                    h_flex().w_full().min_w_0().child(entry)
                                                        .when(shortened, |row| row.child(Button::new(("book-full-label", i)).ghost().small().label("…")
                                                            .tooltip(tr("Full text")).on_click(cx.listener(move |_, _, window, cx| {
                                                                super::source::show_text_dialog(tr("Full text"), full.clone(), window, cx);
                                                            }))))
                                                        .into_any_element()
                                                })
                                                .collect()
                                        }),
                                    )
                                    .flex_1()
                                    .min_h_0()
                                    .track_scroll(&self.toc_scroll),
                                ),
                        )
                    })
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .bg(cx.theme().muted)
                            .child(
                                div()
                                    .id("ebook-scroll")
                                    .size_full()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.scroll)
                                    .child(
                                        v_flex().w_full().items_center().py_6().child(
                                            div()
                                                .w_full()
                                                .max_w(px(self.state.page_width))
                                                .min_h(px(300.))
                                                .px_8()
                                                .py_7()
                                                .bg(paper)
                                                .rounded_md()
                                                .shadow_sm()
                                                .child(content),
                                        ),
                                    )
                                    .child(
                                        canvas(
                                            |_, _, _| (),
                                            move |_, _, window, cx| {
                                                let view = view.clone();
                                                let scroll = scroll.clone();
                                                window.defer(cx, move |_, cx| {
                                                    let _ = view.update(cx, |v, cx| {
                                                        let ready = v.comic.is_some()
                                                            || v.text.as_ref().is_some_and(|t| {
                                                                !t.read(cx)
                                                                    .rendered_text()
                                                                    .is_empty()
                                                            });
                                                        if ready
                                                            && let Some(fraction) = v.restore.take()
                                                        {
                                                            scroll.set_offset(point(
                                                                px(0.),
                                                                -scroll.max_offset().y * fraction,
                                                            ));
                                                            cx.notify();
                                                        }
                                                        if ready
                                                            && let Some(anchor) = v.anchor.take()
                                                            && let (Some(text), Some(source)) =
                                                                (&v.text, v.anchors.get(&anchor))
                                                            && let Some(range) = text
                                                                .read(cx)
                                                                .rendered_text()
                                                                .range_for_source(source.clone())
                                                        {
                                                            text.update(cx, |text, cx| {
                                                                let _ =
                                                                    text.reveal_range(range, cx);
                                                            });
                                                        }
                                                        v.position(cx);
                                                    });
                                                });
                                            },
                                        )
                                        .absolute()
                                        .size_full(),
                                    ),
                            )
                            .when(self.paged.is_none(), |view| {
                                view.child(Scrollbar::vertical(&self.scroll))
                            })
                            .when_some(
                                self.paged
                                    .clone()
                                    .filter(|_| !self.loading && self.error.is_none()),
                                |view, page| {
                                    view.child(
                                        div()
                                            .absolute()
                                            .size_full()
                                            .bg(cx.theme().background)
                                            .child(page),
                                    )
                                },
                            ),
                    ),
            )
            .child(
                h_flex()
                    .p_2()
                    .gap_3()
                    .items_center()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("book-previous")
                            .ghost()
                            .small()
                            .label(tr("Previous page"))
                            .on_click(cx.listener(|v, _, _, cx| v.page(false, cx))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} / {} · {} · {:.0}%",
                                self.state.chapter + 1,
                                self.book.chapters.len(),
                                self.book.chapters[self.state.chapter].title,
                                progress * 100.
                            )),
                    )
                    .child(
                        Button::new("book-next")
                            .ghost()
                            .small()
                            .label(tr("Next page"))
                            .on_click(cx.listener(|v, _, _, cx| v.page(true, cx))),
                    ),
            )
    }
}
