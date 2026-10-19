use gpui_kit::{
    component::{
        ActiveTheme, WindowExt,
        button::{Button, ButtonVariants},
        v_flex,
    },
    prelude::FluentBuilder,
    *,
};
use std::path::PathBuf;

pub(super) struct NativeReader {
    path: PathBuf,
    folder: bool,
    shuffle: bool,
    autoplay: bool,
    task: Option<Task<()>>,
    // Drop the native controller before its prepared data (including SQLite snapshots).
    preview: Option<super::native_bridge::NativePreview>,
    prepared: Option<glim_services::preview::PreparedPreview>,
    error: Option<String>,
    obscured: bool,
    raw: Option<Entity<super::paged::PagedReader>>,
    loading_bytes: bool,
    html: Option<String>,
    html_file: bool,
    html_open: Option<super::OpenLink>,
    html_anchor: Option<String>,
    _html_navigation: Option<Task<()>>,
}
impl NativeReader {
    pub fn html(
        path: PathBuf,
        text: Option<String>,
        active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            path,
            folder: false,
            shuffle: false,
            autoplay: false,
            task: None,
            preview: None,
            prepared: None,
            error: None,
            obscured: false,
            raw: None,
            loading_bytes: false,
            html_file: text.is_none(),
            html: text,
            html_open: None,
            html_anchor: None,
            _html_navigation: None,
        };
        if active {
            view.load_html();
        }
        view._html_navigation = Some(cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                if view
                    .update_in(cx, |view, window, cx| {
                        let Some(preview) = &view.preview else {
                            return;
                        };
                        if let Some(anchor) = view.html_anchor.take() {
                            preview.reveal_anchor(&anchor);
                        }
                        if let Some(href) = preview.take_navigation()
                            && let Some(open) = view.html_open.clone()
                            && let Ok(base) = url::Url::from_file_path(&view.path)
                        {
                            // Let the child update finish before opening another reader.
                            window.defer(cx, move |window, cx| {
                                super::markup::follow_link(&base, &open, &href, window, cx)
                            });
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        view
    }
    fn load_html(&mut self) {
        let configuration = serde_json::json!({
            "files": [&self.path], "shuffle": false, "htmlContent": &self.html, "htmlFile": self.html_file,
            "language": glim_services::locale::code(), "localizations": glim_services::locale::translations(),
        });
        self.preview = Some(super::native_bridge::NativePreview::new(
            &serde_json::to_vec(&configuration).expect("HTML preview configuration"),
        ));
    }
    pub fn reload_html_file(&mut self, cx: &mut Context<Self>) {
        if self.html_file && self.preview.is_some() {
            self.load_html();
            cx.notify();
        }
    }
    pub fn update_html(&mut self, text: String, cx: &mut Context<Self>) {
        if self.html.as_deref() == Some(text.as_str()) {
            return;
        }
        self.html = Some(text);
        if self.preview.is_some() {
            self.load_html();
        }
        cx.notify();
    }
    pub fn set_html_open(&mut self, open: super::OpenLink) {
        self.html_open = Some(open);
    }
    pub fn reveal_html_anchor(&mut self, anchor: &str) {
        self.html_anchor = Some(anchor.to_owned());
    }

    fn open_bytes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task.is_some() || self.folder {
            return;
        }
        self.hide();
        self.loading_bytes = true;
        let path = self.path.clone();
        let work = cx
            .background_executor()
            .spawn(async move { glim_services::paged::read_binary_page(&path, 0) });
        self.task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = work.await;
            let _ = view.update_in(cx, |view, _, cx| {
                view.task = None;
                view.loading_bytes = false;
                match result {
                    Ok(page) => {
                        view.raw = Some(cx.new(|_| super::paged::PagedReader::readonly(page)))
                    }
                    Err(error) => view.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub fn image_stamp(&self) -> Option<(u64, Option<std::time::SystemTime>)> {
        self.prepared
            .as_ref()
            .and_then(|prepared| prepared.image_stamp)
    }
    pub fn new(path: PathBuf, folder: bool, shuffle: bool, cx: &mut Context<Self>) -> Self {
        Self::with_playback(path, folder, shuffle, true, cx)
    }
    pub fn new_paused(path: PathBuf, folder: bool, shuffle: bool, cx: &mut Context<Self>) -> Self {
        Self::with_playback(path, folder, shuffle, false, cx)
    }
    pub fn playlist_shuffle(&self) -> Option<bool> {
        self.folder.then_some(self.shuffle)
    }
    fn with_playback(
        path: PathBuf,
        folder: bool,
        shuffle: bool,
        autoplay: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            path,
            folder,
            shuffle,
            autoplay,
            task: None,
            preview: None,
            prepared: None,
            error: None,
            obscured: false,
            raw: None,
            loading_bytes: false,
            html: None,
            html_file: false,
            html_open: None,
            html_anchor: None,
            _html_navigation: None,
        };
        view.open(cx);
        view
    }
    pub(super) fn open(&mut self, cx: &mut Context<Self>) {
        if self.html.is_some() || self.html_file {
            if self.preview.is_none() {
                self.load_html();
                cx.notify();
            }
            return;
        }
        if self.task.is_some() || self.prepared.is_some() {
            return;
        }
        self.error = None;
        let path = self.path.clone();
        let folder = self.folder;
        let shuffle = self.shuffle;
        let autoplay = self.autoplay;
        let (cancel, guard) = glim_services::work::Cancellation::task();
        let work = cx.background_executor().spawn(async move {
            glim_services::preview::prepare_cancellable(&path, folder, shuffle, autoplay, &cancel)
        });
        self.task = Some(cx.spawn(async move |view, cx| {
            let _guard = guard;
            let result = work.await;
            let _ = view.update(cx, |v, cx| {
                v.task = None;
                match result {
                    Ok(prepared) => {
                        v.preview = Some(super::native_bridge::NativePreview::new(
                            &prepared.configuration,
                        ));
                        v.prepared = Some(prepared);
                    }
                    Err(e) => v.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub(super) fn hide(&self) {
        if let Some(preview) = &self.preview {
            preview.hide();
        }
    }
}
impl Render for NativeReader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Native views sit above GPUI's Metal surface. Hide them while a toolkit
        // menu/dialog is open so the overlay remains visible and clickable.
        let view = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let obscured = window.has_active_dialog(cx)
                || window.has_active_sheet(cx)
                || window
                    .context_stack()
                    .iter()
                    .any(|context| context.contains("PopupMenu"));
            let _ = view.update(cx, |view, cx| {
                if view.obscured != obscured {
                    view.obscured = obscured;
                    if obscured {
                        view.hide();
                    }
                    cx.notify();
                }
            });
        });
        if self.obscured {
            return div().size_full().into_any_element();
        }

        if self.loading_bytes {
            return super::preview_loading(cx);
        }
        if let Some(raw) = self.raw.clone() {
            return v_flex()
                .size_full()
                .min_h_0()
                .child(
                    Button::new("native-return-preview")
                        .ghost()
                        .label(glim_services::locale::tr("Preview"))
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.raw = None;
                            cx.notify();
                        })),
                )
                .child(div().flex_1().min_h_0().w_full().child(raw))
                .into_any_element();
        }
        let bytes_available = self.html.is_none()
            && !self.html_file
            && !self.folder
            && !glim_services::preview::is_iwork(&self.path);
        if self.preview.is_some() {
            let view = cx.entity();
            let surface = canvas(
                move |bounds, window, cx| {
                    if let Some(preview) = &view.read(cx).preview {
                        preview.layout(bounds, window);
                    }
                },
                |_, _, _, _| {},
            )
            .size_full();
            return v_flex()
                .size_full()
                .min_h_0()
                .child(div().flex_1().min_h_0().w_full().child(surface))
                .when(bytes_available, |view| {
                    let original = self.path.clone();
                    view.child(
                        gpui_kit::component::h_flex()
                            .px_2()
                            .gap_2()
                            .child(
                                Button::new("native-file-bytes")
                                    .ghost()
                                    .label(glim_services::locale::tr("File bytes"))
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.open_bytes(window, cx)
                                    })),
                            )
                            .when(
                                self.prepared
                                    .as_ref()
                                    .is_some_and(|prepared| prepared.is_spreadsheet()),
                                |row| {
                                    row.child(
                                        Button::new("spreadsheet-original")
                                            .ghost()
                                            .label(glim_services::locale::tr("Open in Default App"))
                                            .on_click(move |_, _, cx| {
                                                if let Ok(url) = url::Url::from_file_path(&original)
                                                {
                                                    cx.open_url(url.as_str());
                                                }
                                            }),
                                    )
                                },
                            ),
                    )
                })
                .into_any_element();
        }
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .p_6()
            .child(
                div().text_lg().child(
                    self.path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                ),
            )
            .when(self.task.is_some(), |view| {
                view.child(glim_services::locale::tr("Loading preview…"))
            })
            .when_some(self.error.clone(), |view, error| {
                view.child(div().text_sm().text_color(cx.theme().danger).child(error))
                    .child(
                        Button::new("retry-native-preview")
                            .primary()
                            .label(glim_services::locale::tr("Retry"))
                            .on_click(cx.listener(|v, _, _, cx| v.open(cx))),
                    )
            })
            .when(bytes_available && self.error.is_some(), |view| {
                view.child(
                    Button::new("failed-native-file-bytes")
                        .ghost()
                        .label(glim_services::locale::tr("File bytes"))
                        .on_click(cx.listener(|view, _, window, cx| view.open_bytes(window, cx))),
                )
            })
            .into_any_element()
    }
}
