use super::reader_actions::{ToolbarAction, ToolbarItem, document_items};
use super::{Page, Workspace};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, Selectable, Sizable,
        button::{Button, ButtonCustomVariant, ButtonVariants},
        h_flex,
    },
    *,
};

impl Workspace {
    pub(super) fn set_file_fullscreen(&mut self, enabled: bool, window: &Window) {
        if enabled == self.file_fullscreen {
            return;
        }
        if enabled {
            self.file_fullscreen_seen_window = window.is_fullscreen();
            self.file_fullscreen_entered_window = !window.is_fullscreen();
            if self.file_fullscreen_entered_window {
                window.toggle_fullscreen();
            }
        } else {
            if self.file_fullscreen_entered_window && window.is_fullscreen() {
                window.toggle_fullscreen();
            }
            self.file_fullscreen_entered_window = false;
            self.file_fullscreen_seen_window = false;
        }
        self.file_fullscreen = enabled;
    }

    fn toolbar_button(&self, item: ToolbarItem, cx: &mut Context<Self>) -> Button {
        let action = item.action;
        let label = glim_services::locale::tr(item.label);
        let icon = match item.icon {
            Some(icon) => Icon::new(icon),
            None if matches!(action, ToolbarAction::FileFullscreen) => {
                Icon::empty().path(if self.file_fullscreen {
                    "navigation/file-fullscreen-exit.svg"
                } else {
                    "navigation/file-fullscreen.svg"
                })
            }
            None => Icon::empty().path("navigation/wrap.svg"),
        };
        Button::new(action.id())
            .custom(
                ButtonCustomVariant::new(cx)
                    .hover(cx.theme().foreground.opacity(0.05))
                    .active(cx.theme().foreground.opacity(0.09)),
            )
            .small()
            .w(px(28.))
            .h(px(28.))
            .flex_shrink_0()
            .rounded(px(7.))
            .icon(icon.size(px(16.)))
            .tooltip(label.clone())
            .accessibility_label(label)
            .selected(item.selected)
            .disabled(!item.enabled)
            .on_click(cx.listener(move |view, _, window, cx| {
                view.run_toolbar_action(action, window, cx);
            }))
    }

    pub(super) fn navigation_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut back = ToolbarItem::new(ToolbarAction::Back);
        back.enabled = self.history_cursor.is_some_and(|i| i > 0);
        let mut forward = ToolbarItem::new(ToolbarAction::Forward);
        forward.enabled = self
            .history_cursor
            .is_some_and(|i| i + 1 < self.history.len());
        h_flex()
            .h(px(28.))
            .items_center()
            .gap_1()
            .flex_shrink_0()
            .child(self.toolbar_button(ToolbarItem::new(ToolbarAction::Sidebar), cx))
            .child(self.toolbar_button(back, cx))
            .child(self.toolbar_button(forward, cx))
            .into_any_element()
    }

    pub(super) fn reader_actions(&self) -> Vec<ToolbarItem> {
        let mut document = if self.page == Page::Document {
            self.active_reader()
                .map(|r| document_items(r, self.word_wrap))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        if self.page == Page::Document && self.active_reader().is_some() {
            let mut fullscreen = ToolbarItem::new(ToolbarAction::FileFullscreen);
            fullscreen.selected = self.file_fullscreen;
            if self.file_fullscreen {
                fullscreen.label = "Exit File Fullscreen";
            }
            document.push(fullscreen);
        }
        document
    }

    pub(super) fn workspace_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let document = self.reader_actions();
        let has_update = crate::app::updates::state(cx)
            .is_some_and(|state| state.read(cx).snapshot.has_update());
        let border = cx.theme().border;
        let separator = || div().w(px(1.)).h(px(16.)).mx_1().bg(border);
        let has_document = !document.is_empty();
        let mut toolbar = h_flex().gap_1().flex_shrink_0();
        if !document.is_empty() {
            toolbar = toolbar.children(
                document
                    .into_iter()
                    .map(|item| self.toolbar_button(item, cx)),
            );
        }
        if has_update {
            if has_document {
                toolbar = toolbar.child(separator());
            }
            toolbar =
                toolbar.child(self.toolbar_button(ToolbarItem::new(ToolbarAction::Update), cx));
        }
        toolbar.into_any_element()
    }

    pub(super) fn run_toolbar_action(
        &mut self,
        action: ToolbarAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            ToolbarAction::Sidebar => {
                self.sidebar_visible = !self.sidebar_visible;
                self.sidebar_window_width = 0.;
                cx.notify();
            }
            ToolbarAction::Back => self.navigate(false, window, cx),
            ToolbarAction::Forward => self.navigate(true, window, cx),
            ToolbarAction::Preview | ToolbarAction::Source => {
                if self.active_reader().is_some_and(|r| r.can_preview()) {
                    self.set_reader_preview(matches!(action, ToolbarAction::Preview), window, cx);
                }
            }
            ToolbarAction::SideBySide | ToolbarAction::Inline => {
                if self
                    .active_reader()
                    .is_some_and(|r| r.diff.is_some() || r.historical)
                {
                    self.set_diff_mode(matches!(action, ToolbarAction::SideBySide), cx);
                }
            }
            ToolbarAction::ViewFile => {
                if let Some(reader) = self.active_reader().filter(|r| r.diff.is_some()) {
                    self.open_path(reader.path.clone(), window, cx);
                }
            }
            ToolbarAction::WordWrap => {
                if self.active_reader().is_some_and(|r| r.can_word_wrap()) {
                    self.toggle_word_wrap(window, cx);
                }
            }
            ToolbarAction::FileFullscreen => {
                if self.page == Page::Document && self.active_reader().is_some() {
                    self.set_file_fullscreen(!self.file_fullscreen, window);
                    self.results.clear();
                    self.search_task = None;
                    self.history_swipe = None;
                    // Keep the existing reader and its editing/preview state alive.
                    window.focus(&self.focus, cx);
                    cx.notify();
                }
            }
            ToolbarAction::Update => crate::app::updates::show(window, cx),
        }
    }
}
