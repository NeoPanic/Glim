mod books;
mod boundary;
mod csv;
mod diagrams;
mod diff;
mod json;
mod lexical;
mod markup;
mod media;
mod native;

mod native_bridge;
mod paged;
mod pictures;
mod source;
mod xmind;
use diff::DiffReader;
use glim_core::{DiffDocument, Document, DocumentKind, language_for_path};
use gpui_kit::*;
use markup::MarkupReader;
use media::{GifReader, ImageReader, UnavailableReader};
use source::SourceReader;
use std::{path::PathBuf, sync::Arc};

/// Keep native text editing actions without the toolkit's unused LSP entries.
pub(super) fn editor_menu(
    menu: gpui_kit::component::native_menu::NativeMenu,
    editable: bool,
    copyable: bool,
) -> gpui_kit::component::native_menu::NativeMenu {
    use gpui_kit::component::input::{Copy, Cut, Paste, SelectAll};
    menu.menu_with_disabled(
        glim_services::locale::tr("Cut"),
        !(editable && copyable),
        Box::new(Cut),
    )
    .menu_with_disabled(glim_services::locale::tr("Copy"), !copyable, Box::new(Copy))
    .menu_with_disabled(
        glim_services::locale::tr("Paste"),
        !editable,
        Box::new(Paste),
    )
    .separator()
    .menu(glim_services::locale::tr("Select All"), Box::new(SelectAll))
}

pub(super) fn preview_loading(cx: &App) -> AnyElement {
    use gpui_kit::component::ActiveTheme;
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(glim_services::locale::tr("Loading preview…"))
        .into_any_element()
}

pub type OpenLink = Arc<dyn Fn(PathBuf, Option<String>, &mut Window, &mut App) + Send + Sync>;

/// Tab identity is separate from the concrete renderer and its persistent state.
/// No image/error tab allocates an unused editor. JSON shares the code editor's
/// syntax-aware folding; Markdown owns preview state as well as a source editor.
enum Renderer {
    Source(SourceReader),
    Paged {
        view: Entity<paged::PagedReader>,
        html: Option<(Entity<native::NativeReader>, bool)>,
        editor: Option<Entity<gpui_kit::component::input::EditorState>>,
        dirty: std::rc::Rc<std::cell::Cell<bool>>,
        saving: std::rc::Rc<std::cell::Cell<bool>>,
    },
    Markup(Box<MarkupReader>),
    Diff(DiffReader),
    Image(ImageReader),
    Gif(Entity<GifReader>),
    XMind(Entity<xmind::XMindReader>),
    Book(
        Entity<books::BookReader>,
        Arc<std::sync::atomic::AtomicBool>,
    ),
    Native(Entity<native::NativeReader>),
    Unavailable(UnavailableReader),
}
pub struct Reader {
    pub repository_root: Option<PathBuf>,
    pub snapshot: String,
    pub title: SharedString,
    pub path: PathBuf,
    pub diff: Option<glim_core::GitChange>,
    pub historical: bool,
    pub commit_id: Option<String>,
    pub commit_file: Option<PathBuf>,
    json: Option<(Entity<json::JsonTree>, bool)>,
    csv: Option<(Entity<csv::CsvTable>, bool)>,
    renderer: Renderer,
}
impl Reader {
    pub fn new(document: Document, window: &mut Window, cx: &mut App) -> Self {
        let html = document
            .path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("html") || s.eq_ignore_ascii_case("htm"));
        let renderer = if document.kind == DocumentKind::Markdown
            || html
            || glim_services::text_formats::supports(&document.path)
        {
            Renderer::Markup(Box::new(MarkupReader::new(
                &document.path,
                &document.text,
                window,
                cx,
            )))
        } else {
            Renderer::Source(SourceReader::new(
                &document.text,
                language_for_path(&document.path),
                false,
                Vec::new(),
                ScrollHandle::new(),
                window,
                cx,
            ))
        };
        let json = (language_for_path(&document.path) == "json"
            && !glim_services::text_formats::supports(&document.path))
        .then(|| {
            let format = document
                .path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("json")
                .to_ascii_lowercase();
            let format = if format == "code-workspace" {
                "jsonc".to_owned()
            } else {
                format
            };
            (cx.new(|_| json::JsonTree::new(format)), false)
        });
        let csv = document
            .path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| {
                matches!(
                    s.to_ascii_lowercase().as_str(),
                    "csv" | "tsv" | "gpx" | "kml"
                )
            })
            .then(|| (cx.new(|_| csv::CsvTable::new()), false));
        let mut reader = Self {
            title: document.path.display().to_string().into(),
            path: document.path,
            snapshot: document.text,
            repository_root: None,
            diff: None,
            historical: false,
            commit_id: None,
            commit_file: None,
            json,
            csv,
            renderer,
        };
        reader.restore_csv_preview(cx);
        reader
    }
    pub fn from_native(path: PathBuf, folder: bool, shuffle: bool, cx: &mut App) -> Self {
        Self::from_native_playback(path, folder, shuffle, true, cx)
    }
    fn from_native_playback(
        path: PathBuf,
        folder: bool,
        shuffle: bool,
        autoplay: bool,
        cx: &mut App,
    ) -> Self {
        Self {
            repository_root: None,
            snapshot: "native-preview".into(),
            title: path.display().to_string().into(),
            path: path.clone(),
            diff: None,
            historical: false,
            commit_id: None,
            commit_file: None,
            json: None,
            csv: None,
            renderer: Renderer::Native(cx.new(|cx| {
                if autoplay {
                    native::NativeReader::new(path, folder, shuffle, cx)
                } else {
                    native::NativeReader::new_paused(path, folder, shuffle, cx)
                }
            })),
        }
    }

    pub fn from_native_paused(path: PathBuf, folder: bool, shuffle: bool, cx: &mut App) -> Self {
        Self::from_native_playback(path, folder, shuffle, false, cx)
    }
    pub(crate) fn session_tab(&self, cx: &App) -> glim_core::update::SessionTab {
        use glim_core::update::SessionTab;
        if let (Some(root), Some(commit)) = (&self.repository_root, &self.commit_id) {
            return SessionTab::Commit {
                root: root.clone(),
                commit: commit.clone(),
                file: self.commit_file.clone(),
            };
        }
        if let (Some(root), Some(change)) = (&self.repository_root, &self.diff) {
            return SessionTab::Diff {
                root: root.clone(),
                change: change.clone(),
                side_by_side: self.side_by_side(),
            };
        }
        if let Renderer::Native(native) = &self.renderer
            && let Some(shuffle) = native.read(cx).playlist_shuffle()
        {
            return SessionTab::Playlist {
                path: self.path.clone(),
                shuffle,
            };
        }
        SessionTab::File {
            path: self.path.clone(),
            preview: self.toggle_label() == "Source",
        }
    }

    pub fn from_page(
        page: glim_services::paged::TextPage,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let path = page.path.clone();
        let snapshot = page.fingerprint();
        let view = cx.new(|cx| paged::PagedReader::new(page, window, cx));
        let editor = view
            .read(cx)
            .source_editor
            .as_ref()
            .map(|source| source.state.clone());
        let dirty = view
            .read(cx)
            .source_editor
            .as_ref()
            .map(|source| source.dirty.clone())
            .unwrap_or_default();
        let saving = view.read(cx).saving.clone();
        let csv = path
            .extension()
            .and_then(|s| s.to_str())
            .filter(|s| {
                matches!(
                    s.to_ascii_lowercase().as_str(),
                    "csv" | "tsv" | "gpx" | "kml"
                )
            })
            .map(|ext| {
                let delimiter = if ext.eq_ignore_ascii_case("tsv") {
                    b'\t'
                } else {
                    b','
                };
                (
                    cx.new(|_| csv::CsvTable::from_file(path.clone(), delimiter)),
                    false,
                )
            });
        let html = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm"))
            .then(|| {
                let active = cx
                    .try_global::<crate::app::ReaderPreferences>()
                    .is_none_or(|prefs| prefs.html_preview);
                (
                    cx.new(|cx| native::NativeReader::html(path.clone(), None, window, cx)),
                    active,
                )
            });
        let mut reader = Self {
            title: path.display().to_string().into(),
            path,
            snapshot,
            repository_root: None,
            diff: None,
            historical: false,
            commit_id: None,
            commit_file: None,
            json: None,
            csv,
            renderer: Renderer::Paged {
                html,
                view,
                editor,
                dirty,
                saving,
            },
        };
        reader.restore_csv_preview(cx);
        reader
    }
    pub fn from_commit(
        path: PathBuf,
        text: String,
        commit_file: Option<PathBuf>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let (content, spans) = glim_core::commit_content(&text);
        let source = SourceReader::new(
            &content,
            "plain",
            true,
            spans,
            ScrollHandle::new(),
            window,
            cx,
        );
        let split = glim_core::split_diff::split_commit_patch(&text)
            .map(|patch| cx.new(|cx| crate::views::split_diff::SplitDiff::new(patch, window, cx)));
        let side_by_side = split.is_some()
            && cx
                .try_global::<crate::app::ReaderPreferences>()
                .and_then(|p| p.diff_side_by_side)
                .unwrap_or(commit_file.is_some());
        let root = path.parent().map(ToOwned::to_owned);
        let commit_id = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("Commit "))
            .map(str::to_owned);
        Self {
            title: commit_file
                .as_ref()
                .map_or_else(
                    || path.display().to_string(),
                    |file| {
                        format!(
                            "{} · {}",
                            file.display(),
                            path.file_name().unwrap_or_default().to_string_lossy()
                        )
                    },
                )
                .into(),
            path,
            snapshot: text,
            repository_root: root,
            diff: None,
            historical: true,
            commit_id,
            commit_file,
            json: None,
            csv: None,
            renderer: Renderer::Diff(DiffReader {
                source,
                split,
                side_by_side,
                notice: None,
            }),
        }
    }
    #[cfg(test)]
    pub fn from_bytes(
        document: glim_services::binary::BytePreview,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let source = SourceReader::new(
            &document.text,
            "plain",
            true,
            Vec::new(),
            ScrollHandle::new(),
            window,
            cx,
        );
        Self {
            title: document.path.display().to_string().into(),
            path: document.path,
            snapshot: document.text,
            repository_root: None,
            diff: None,
            historical: false,
            commit_id: None,
            commit_file: None,
            json: None,
            csv: None,
            renderer: Renderer::Source(source),
        }
    }
    pub fn from_diff(
        document: DiffDocument,
        root: &std::path::Path,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let renderer = Renderer::Diff(DiffReader::new(&document, window, cx));
        Self {
            title: format!(
                "{} · {}",
                document.change.path.display(),
                document.change.scope.label()
            )
            .into(),
            path: root.join(&document.change.path),
            snapshot: document.patch,
            repository_root: Some(root.to_path_buf()),
            diff: Some(document.change),
            historical: false,
            commit_id: None,
            commit_file: None,
            json: None,
            csv: None,
            renderer,
        }
    }
    pub fn from_image(
        document: glim_services::media::ImageDocument,
        _: &mut Window,
        cx: &mut App,
    ) -> Self {
        let renderer = if document.gif {
            Renderer::Gif(cx.new(|cx| GifReader::from_file(document.path.clone(), cx)))
        } else {
            let pages = if document.pages.is_empty() {
                vec![document.png]
            } else {
                document.pages
            };
            let images = pages
                .into_iter()
                .map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes)))
                .collect();
            Renderer::Image(ImageReader {
                pages: images,
                page: cx.new(|_| 0_usize),
            })
        };
        Self {
            title: document.path.display().to_string().into(),
            path: document.path,
            snapshot: document.fingerprint,
            repository_root: None,
            diff: None,
            historical: false,
            commit_id: None,
            commit_file: None,
            json: None,
            csv: None,
            renderer,
        }
    }
    pub fn from_book(
        document: glim_services::books::Book,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let path = document.path.clone();
        let fingerprint = document.fingerprint.clone();
        let view = cx.new(|cx| books::BookReader::new(document, window, cx));
        let writing = view.read(cx).writing.clone();
        Self {
            title: path.display().to_string().into(),
            path,
            snapshot: fingerprint,
            repository_root: None,
            diff: None,
            historical: false,
            commit_id: None,
            commit_file: None,
            json: None,
            csv: None,
            renderer: Renderer::Book(view, writing),
        }
    }
    pub fn is_persisting_book(&self) -> bool {
        matches!(&self.renderer, Renderer::Book(_, writing) if writing.load(std::sync::atomic::Ordering::SeqCst))
    }
    pub(crate) fn take_book_write(&mut self, cx: &mut App) -> Option<Task<()>> {
        if let Renderer::Book(view, _) = &self.renderer {
            view.update(cx, |view, _| view.take_write())
        } else {
            None
        }
    }
    pub fn from_xmind(document: glim_services::xmind::MindMap, cx: &mut App) -> Self {
        Self {
            title: document.path.display().to_string().into(),
            path: document.path.clone(),
            snapshot: document.fingerprint.clone(),
            repository_root: None,
            diff: None,
            historical: false,
            commit_id: None,
            commit_file: None,
            json: None,
            csv: None,
            renderer: Renderer::XMind(cx.new(|_| xmind::XMindReader::new(document))),
        }
    }
    pub fn unavailable(path: PathBuf, reason: String, _: &mut Window, _: &mut App) -> Self {
        let renderer = Renderer::Unavailable(UnavailableReader::new(&path, reason));
        Self {
            title: path.display().to_string().into(),
            path,
            snapshot: String::new(),
            repository_root: None,
            diff: None,
            historical: false,
            commit_id: None,
            commit_file: None,
            json: None,
            csv: None,
            renderer,
        }
    }
    pub fn hide_native(&self, cx: &mut App) {
        if let Renderer::Paged {
            html: Some((view, _)),
            ..
        } = &self.renderer
        {
            view.read(cx).hide();
        }
        if let Renderer::Markup(view) = &self.renderer {
            view.set_images_active(false, cx);
        }
        if let Renderer::Gif(view) = &self.renderer {
            view.update(cx, |view, cx| view.set_active(false, cx));
        }
        if let Renderer::Native(view) = &self.renderer {
            view.read(cx).hide();
        }
    }

    pub fn reveal_native(&self, cx: &mut App) -> bool {
        if let Renderer::Paged {
            html: Some((view, true)),
            ..
        } = &self.renderer
        {
            view.update(cx, |view, cx| view.open(cx));
            return true;
        }
        if let Renderer::Gif(view) = &self.renderer {
            view.update(cx, |view, cx| view.set_active(true, cx));
        }
        if let Renderer::Native(view) = &self.renderer {
            view.update(cx, |v, cx| v.open(cx));
            return true;
        }
        false
    }

    pub fn is_unavailable(&self) -> bool {
        matches!(self.renderer, Renderer::Unavailable(_))
    }
    fn source(&self) -> Option<&SourceReader> {
        match &self.renderer {
            Renderer::Source(s) => Some(s),
            Renderer::Markup(m) => Some(&m.source),
            Renderer::Diff(d) => Some(&d.source),
            _ => None,
        }
    }
    pub fn editor(&self) -> Option<Entity<gpui_kit::component::input::EditorState>> {
        if self.diff.is_some() {
            return None;
        }
        match &self.renderer {
            Renderer::Source(s) if !s.is_readonly() => Some(s.state.clone()),
            Renderer::Markup(m) => Some(m.source.state.clone()),
            Renderer::Paged { editor, .. } => editor.clone(),
            _ => None,
        }
    }
    pub fn set_editing_locked(&mut self, locked: bool, cx: &mut App) {
        match &mut self.renderer {
            Renderer::Source(s) => s.set_locked(locked, cx),
            Renderer::Markup(m) => m.source.set_locked(locked, cx),
            Renderer::Paged { view, .. } => view.update(cx, |page, cx| {
                if let Some(source) = &mut page.source_editor {
                    source.set_locked(locked, cx);
                }
            }),
            _ => {}
        }
    }
    pub fn is_dirty(&self) -> bool {
        match &self.renderer {
            Renderer::Source(s) => s.is_dirty(),
            Renderer::Markup(m) => m.source.is_dirty(),
            Renderer::Paged { dirty, .. } => dirty.get(),
            _ => false,
        }
    }
    pub fn native_image_stamp(&self, cx: &App) -> Option<(u64, Option<std::time::SystemTime>)> {
        match &self.renderer {
            Renderer::Native(view) => view.read(cx).image_stamp(),
            _ => None,
        }
    }
    pub fn page_refresh_request(&self, cx: &App) -> Option<glim_services::paged::PageRefresh> {
        if let Renderer::Paged { view, .. } = &self.renderer {
            view.read(cx).refresh_request()
        } else {
            None
        }
    }
    pub fn page_save_request(&self, cx: &mut App) -> Option<glim_services::paged::PageEdit> {
        if let Renderer::Paged { view, .. } = &self.renderer {
            view.update(cx, |page, _| page.save_request())
        } else {
            None
        }
    }
    pub fn page_saved(
        &mut self,
        text: String,
        saved: glim_services::paged::PageSaved,
        cx: &mut App,
    ) {
        if let Renderer::Paged { view, .. } = &self.renderer {
            self.snapshot = format!("paged:{}:{:?}", saved.total, saved.modified);
            view.update(cx, |page, cx| page.saved(text, saved, cx));
            if let Renderer::Paged {
                html: Some((browser, _)),
                ..
            } = &self.renderer
            {
                browser.update(cx, |browser, cx| browser.reload_html_file(cx));
            }
        }
    }
    pub fn finish_page_save(&self) {
        if let Renderer::Paged { saving, .. } = &self.renderer {
            saving.set(false);
        }
    }
    pub fn mark_saved(&mut self, text: String, cx: &App) {
        self.snapshot = text.clone();
        match &mut self.renderer {
            Renderer::Source(s) => s.mark_saved(text, cx),
            Renderer::Markup(m) => m.source.mark_saved(text, cx),
            _ => {}
        }
    }
    pub fn inherit_view(&mut self, old: &Self, cx: &mut App) {
        if let Some((_, visible)) = &old.csv {
            self.set_preview(*visible, cx);
        } else if old.json.as_ref().is_some_and(|(_, visible)| *visible) {
            self.set_preview(true, cx);
        }
        if let Renderer::Paged {
            html: Some((_, visible)),
            ..
        } = &old.renderer
        {
            self.set_preview(*visible, cx);
        }
        match (&mut self.renderer, &old.renderer) {
            (Renderer::Source(s), Renderer::Source(o)) => s.inherit(o, cx),
            (Renderer::Markup(s), Renderer::Markup(o)) => {
                s.source.inherit(&o.source, cx);
                s.preview = o.preview;
                s.scroll.set_offset(o.scroll.offset());
            }
            (Renderer::Diff(s), Renderer::Diff(o)) => {
                s.source.inherit(&o.source, cx);
                s.side_by_side = o.side_by_side && s.split.is_some();
                if let (Some(new), Some(old)) = (&s.split, &o.split) {
                    let offset = old.read(cx).offset(cx);
                    new.update(cx, |v, cx| v.restore_offset(offset, cx));
                }
            }
            _ => {}
        }
    }
    pub fn can_word_wrap(&self) -> bool {
        self.source().is_some()
    }

    pub fn set_word_wrap(&self, enabled: bool, window: &mut Window, cx: &mut App) {
        if let Some(source) = self.source() {
            source
                .state
                .update(cx, |s, cx| s.set_soft_wrap(enabled, window, cx));
        }
        if let Renderer::Diff(diff) = &self.renderer
            && let Some(split) = &diff.split
        {
            split.update(cx, |s, cx| s.set_word_wrap(enabled, window, cx));
        }
    }

    pub fn horizontal_offsets(&self, cx: &App) -> Option<Vec<Pixels>> {
        if self.csv.as_ref().is_some_and(|(_, visible)| *visible)
            || self.json.as_ref().is_some_and(|(_, visible)| *visible)
        {
            return Some(Vec::new());
        }
        match &self.renderer {
            Renderer::Paged { .. } => None,
            Renderer::Markup(m) if m.preview && m.html => None,
            Renderer::Markup(m) if m.preview => Some(vec![m.scroll.offset().x]),
            Renderer::Diff(d) if d.side_by_side => {
                d.split.as_ref().map(|s| s.read(cx).horizontal_offsets(cx))
            }
            _ => Some(
                self.source()
                    .map(|s| vec![s.state.read(cx).scroll_offset().x])
                    .unwrap_or_default(),
            ),
        }
    }

    pub fn focus_source(&self, window: &mut Window, cx: &mut App) {
        if let Some(source) = self.source() {
            source.state.update(cx, |s, cx| s.focus(window, cx));
        }
    }
    pub fn history_summary(&self) -> Option<(String, String)> {
        if !self.historical {
            return None;
        }
        let header = self.snapshot.split("\ndiff --git ").next().unwrap_or("");
        let subject = header
            .lines()
            .find_map(|line| {
                line.strip_prefix("    ")
                    .filter(|line| !line.trim().is_empty())
            })
            .unwrap_or("Commit changes");
        let author = header
            .lines()
            .find_map(|line| line.strip_prefix("Author:"))
            .map(|author| author.trim().split(" <").next().unwrap_or(author))
            .unwrap_or("");
        let id = self.commit_id.as_deref().unwrap_or("");
        Some((
            subject.to_owned(),
            format!("{} · {}", author, &id[..8.min(id.len())]),
        ))
    }

    pub fn is_csv(&self) -> bool {
        self.csv.is_some()
            && self.path.extension().is_some_and(|extension| {
                extension.eq_ignore_ascii_case("csv") || extension.eq_ignore_ascii_case("tsv")
            })
    }
    fn geo_format(&self) -> Option<&'static str> {
        let extension = self.path.extension()?.to_str()?;
        if extension.eq_ignore_ascii_case("gpx") {
            Some("GPX")
        } else if extension.eq_ignore_ascii_case("kml") {
            Some("KML")
        } else {
            None
        }
    }
    pub fn is_json(&self) -> bool {
        self.json.is_some()
    }
    fn restore_csv_preview(&mut self, cx: &mut App) {
        if self.csv.is_some()
            && cx.try_global::<crate::app::ReaderPreferences>().map_or(
                self.geo_format().is_some(),
                |prefs| {
                    if self.geo_format().is_some() {
                        prefs.markdown_preview
                    } else {
                        prefs.csv_preview
                    }
                },
            )
        {
            self.set_preview(true, cx);
        }
    }
    pub fn preview_preference(&self) -> Option<&'static str> {
        if matches!(&self.renderer, Renderer::Paged { html: Some(_), .. }) {
            return Some("html");
        }
        if self.is_csv() {
            return Some("csv");
        }
        if self.geo_format().is_some() {
            return Some("markdown");
        }
        match &self.renderer {
            Renderer::Markup(m) => Some(if m.html { "html" } else { "markdown" }),
            _ => None,
        }
    }
    pub fn can_preview(&self) -> bool {
        self.csv.is_some()
            || self.json.is_some()
            || matches!(
                self.renderer,
                Renderer::Markup(_) | Renderer::Paged { html: Some(_), .. }
            )
    }
    pub fn set_preview(&mut self, preview: bool, cx: &mut App) {
        if !preview && glim_services::diagnostics::failure(&self.path).is_some() {
            glim_services::diagnostics::retry(&self.path);
        }
        if preview
            && self.is_dirty()
            && let Renderer::Paged { view, .. } = &self.renderer
        {
            view.update(cx, |page, cx| page.request_save_before_preview(cx));
            return;
        }
        if let Renderer::Paged {
            html: Some((browser, visible)),
            ..
        } = &mut self.renderer
        {
            *visible = preview;
            if preview {
                browser.update(cx, |browser, cx| browser.open(cx));
            } else {
                browser.read(cx).hide();
            }
        }
        if self.csv.is_some() {
            let geo = self.geo_format();
            let text = if preview {
                self.source().map(|s| s.state.read(cx).value().to_string())
            } else {
                None
            };
            if let Some((table, visible)) = &mut self.csv {
                *visible = preview;
                if let Some(text) = text {
                    table.update(cx, |view, cx| {
                        if let Some(format) = geo {
                            view.load_geo(text, format, cx);
                            return;
                        }
                        view.load(
                            text,
                            if self
                                .path
                                .extension()
                                .is_some_and(|e| e.eq_ignore_ascii_case("tsv"))
                            {
                                b'\t'
                            } else {
                                b','
                            },
                            cx,
                        )
                    });
                } else if preview {
                    table.update(cx, |table, cx| table.reload(cx));
                }
            }
        }
        if self.json.is_some() {
            let text = if preview {
                self.source().map(|s| s.state.read(cx).value().to_string())
            } else {
                None
            };
            if let Some((tree, visible)) = &mut self.json {
                *visible = preview;
                if let Some(text) = text {
                    tree.update(cx, |v, cx| v.load(text, cx));
                }
            }
        }
        if let Renderer::Markup(m) = &mut self.renderer {
            if preview {
                m.sync_preview(cx);
            }
            m.preview = preview;
            if !preview {
                m.set_images_active(false, cx);
            }
        }
    }
    pub fn toggle_label(&self) -> &'static str {
        if self.csv.as_ref().is_some_and(|(_, visible)| *visible)
            || self.json.as_ref().is_some_and(|(_, visible)| *visible)
            || matches!(
                &self.renderer,
                Renderer::Paged {
                    html: Some((_, true)),
                    ..
                }
            )
            || matches!(&self.renderer, Renderer::Markup(m) if m.preview)
        {
            "Source"
        } else {
            "Preview"
        }
    }
    pub fn split_available(&self) -> bool {
        matches!(&self.renderer, Renderer::Diff(d) if d.split.is_some())
    }
    pub fn side_by_side(&self) -> bool {
        matches!(&self.renderer, Renderer::Diff(d) if d.side_by_side)
    }
    pub fn set_side_by_side(&mut self, enabled: bool) {
        if let Renderer::Diff(d) = &mut self.renderer {
            d.side_by_side = enabled && d.split.is_some();
        }
    }
    pub fn reveal_anchor(&mut self, anchor: &str, cx: &mut App) {
        if let Renderer::Paged {
            html: Some((browser, _)),
            ..
        } = &self.renderer
        {
            browser.update(cx, |browser, _| browser.reveal_html_anchor(anchor));
            self.set_preview(true, cx);
            return;
        }
        if let Renderer::Markup(m) = &mut self.renderer {
            m.sync_preview(cx);
            m.reveal(anchor, cx);
        } else if let Some(source) = self.source()
            && let Some(line) = anchor
                .strip_prefix('L')
                .and_then(|s| s.split('-').next())
                .and_then(|s| s.parse::<usize>().ok())
        {
            source.state.update(cx, |s, cx| {
                let height = s.line_height().unwrap_or(px(20.));
                s.set_scroll_offset(point(px(0.), -height * line.saturating_sub(1) as f32), cx);
            });
        }
    }
    pub fn render(&mut self, open: OpenLink, cx: &mut App) -> AnyElement {
        if let Some(error) = glim_services::diagnostics::failure(&self.path) {
            self.hide_native(cx);
            return boundary::failed(&self.path, error, cx);
        }
        let _scope = glim_services::diagnostics::FileScope::enter(&self.path, "render");
        let content = self.render_content(open, cx);
        boundary::FileBoundary {
            path: self.path.clone(),
            content,
        }
        .into_any_element()
    }
    fn render_content(&mut self, open: OpenLink, cx: &mut App) -> AnyElement {
        if let Some((table, true)) = &self.csv {
            return table.clone().into_any_element();
        }
        if let Some((tree, true)) = &self.json {
            return tree.clone().into_any_element();
        }
        match &mut self.renderer {
            Renderer::Source(s) => s.render(cx),
            Renderer::Markup(m) => m.render(open, cx),
            Renderer::Diff(d) => d.render(cx),
            Renderer::Image(i) => i.render(cx),
            Renderer::Gif(view) => {
                view.update(cx, |view, cx| view.set_active(true, cx));
                view.clone().into_any_element()
            }
            Renderer::XMind(v) => v.clone().into_any_element(),
            Renderer::Book(v, _) => v.clone().into_any_element(),
            Renderer::Native(n) => n.clone().into_any_element(),
            Renderer::Paged {
                html: Some((browser, true)),
                ..
            } => {
                browser.update(cx, |browser, cx| {
                    browser.set_html_open(open);
                    browser.open(cx);
                });
                browser.clone().into_any_element()
            }
            Renderer::Paged { view, .. } => view.clone().into_any_element(),
            Renderer::Unavailable(e) => e.render(cx),
        }
    }
}

#[cfg(test)]
mod editing_tests {
    use super::{Reader, Renderer};
    use glim_core::{DiffDocument, DiffScope, Document, DocumentKind, GitChange};
    use gpui_kit::{
        App, AppContext, Bounds, Context, Entity, IntoElement, Point, Render, TestAppContext,
        Window, WindowBounds, WindowOptions, px, size, test::TestWindowExt,
    };

    struct Fixture(Reader);
    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.0.render(std::sync::Arc::new(|_, _, _, _| {}), cx)
        }
    }
    fn open(
        cx: &mut TestAppContext,
        build: impl FnOnce(&mut Window, &mut App) -> Reader,
    ) -> (gpui_kit::AnyWindowHandle, Entity<Fixture>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(900.), px(600.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| Fixture(build(window, cx))),
            )
            .unwrap()
        })
    }
    #[gpui_kit::test]
    fn source_right_click_does_not_reborrow_editor(cx: &mut TestAppContext) {
        for (name, kind) in [
            ("test.rs", DocumentKind::Text),
            ("test.md", DocumentKind::Markdown),
            ("test.html", DocumentKind::Text),
        ] {
            let (handle, view) = open(cx, |window, cx| {
                let mut reader = Reader::new(
                    Document {
                        path: std::env::temp_dir().join(name),
                        kind,
                        text: "Sample text\n".into(),
                    },
                    window,
                    cx,
                );
                reader.set_preview(false, cx);
                reader
            });
            let editor = cx.update(|cx| view.read(cx).0.editor().unwrap());
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                window.right_click(("input", editor.entity_id()), cx);
            })
            .unwrap();
            cx.run_until_parked();
        }
    }

    #[gpui_kit::test]
    fn markdown_preview_uses_unsaved_edits(cx: &mut TestAppContext) {
        let (handle, view) = open(cx, |window, cx| {
            let mut reader = Reader::new(
                Document {
                    path: std::env::temp_dir().join("test-note.md"),
                    kind: DocumentKind::Markdown,
                    text: "# Original\n".into(),
                },
                window,
                cx,
            );
            reader.set_preview(false, cx);
            reader
        });
        let editor = cx.update(|cx| view.read(cx).0.editor().unwrap());
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click(("input", editor.entity_id()), cx);
            window.press("secondary-a", cx);
            window.input("# Edited heading\n", cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert!(cx.update(|cx| view.read(cx).0.is_dirty()));
        cx.update(|cx| view.update(cx, |v, cx| v.0.set_preview(true, cx)));
        cx.run_until_parked();
        cx.update(|cx| {
            let Renderer::Markup(markdown) = &view.read(cx).0.renderer else {
                unreachable!()
            };
            assert_eq!(
                markdown.source.state.read(cx).value().as_ref(),
                "# Edited heading\n"
            );
            assert!(
                markdown
                    .state
                    .read(cx)
                    .rendered_text()
                    .as_str()
                    .contains("Edited heading")
            );
            assert_eq!(view.read(cx).0.snapshot, "# Original\n");
        });
    }
    #[gpui_kit::test]
    fn byte_preview_rejects_typing_and_has_no_save_buffer(cx: &mut TestAppContext) {
        let (handle, view) = open(cx, |window, cx| {
            Reader::from_bytes(
                glim_services::binary::BytePreview {
                    path: std::env::temp_dir().join(".DS_Store"),
                    text: "00000000  00 ff 41".into(),
                },
                window,
                cx,
            )
        });
        let editor = cx.update(|cx| view.read(cx).0.source().unwrap().state.clone());
        let original = cx.update(|cx| editor.read(cx).value());
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click(("input", editor.entity_id()), cx);
            window.press("secondary-a", cx);
            window.input("must not overwrite binary bytes", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(editor.read(cx).value(), original);
            assert!(view.read(cx).0.editor().is_none());
            assert!(!view.read(cx).0.is_dirty());
        });
    }

    #[gpui_kit::test]
    fn diff_rejects_typing_and_has_no_save_buffer(cx: &mut TestAppContext) {
        let (handle, view) = open(cx, |window, cx| {
            let mut reader = Reader::from_diff(
                DiffDocument {
                    change: GitChange {
                        path: "text.txt".into(),
                        original_path: None,
                        scope: DiffScope::Worktree,
                        status: 'M',
                    },
                    patch: "@@ -1 +1 @@\n-old\n+new\n".into(),
                },
                std::path::Path::new("/tmp"),
                window,
                cx,
            );
            reader.set_side_by_side(false);
            reader
        });
        let editor = cx.update(|cx| view.read(cx).0.source().unwrap().state.clone());
        let original = cx.update(|cx| editor.read(cx).value());
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.right_click(("input", editor.entity_id()), cx);
            window.click(("input", editor.entity_id()), cx);
            window.press("secondary-a", cx);
            window.input("must not replace the diff", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(editor.read(cx).value(), original);
            assert!(view.read(cx).0.editor().is_none());
            assert!(!view.read(cx).0.is_dirty());
        });
    }
}
