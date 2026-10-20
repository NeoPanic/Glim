use crate::views::reader::Reader;
use gpui_kit::assets::IconName;

#[derive(Clone, Copy)]
pub(super) enum ToolbarAction {
    Sidebar,
    Back,
    Forward,
    Preview,
    Source,
    SideBySide,
    Inline,
    ViewFile,
    WordWrap,
    FileFullscreen,
    Update,
}

pub(super) struct ToolbarItem {
    pub(super) action: ToolbarAction,
    pub(super) label: &'static str,
    pub(super) icon: Option<IconName>,
    pub(super) selected: bool,
    pub(super) enabled: bool,
}

impl ToolbarAction {
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Sidebar => "toggle-sidebar",
            Self::Back => "back",
            Self::Forward => "forward",
            Self::Preview => "reader-preview",
            Self::Source => "reader-source",
            Self::SideBySide => "diff-split",
            Self::Inline => "diff-inline",
            Self::ViewFile => "show-file",
            Self::WordWrap => "word-wrap",
            Self::FileFullscreen => "file-fullscreen",
            Self::Update => "update-available",
        }
    }
}

impl ToolbarItem {
    pub(super) fn new(action: ToolbarAction) -> Self {
        let (label, icon) = match action {
            ToolbarAction::Sidebar => ("Toggle Sidebar", Some(IconName::PanelLeft)),
            ToolbarAction::Back => ("Back", Some(IconName::ArrowLeft)),
            ToolbarAction::Forward => ("Forward", Some(IconName::ArrowRight)),
            ToolbarAction::Preview => ("Preview", Some(IconName::Eye)),
            ToolbarAction::Source => ("Source", Some(IconName::Code)),
            ToolbarAction::SideBySide => ("Side by side", Some(IconName::Columns2)),
            ToolbarAction::Inline => ("Inline", Some(IconName::Rows2)),
            ToolbarAction::ViewFile => ("View file", Some(IconName::FileText)),
            ToolbarAction::WordWrap => ("Word wrap", None),
            ToolbarAction::FileFullscreen => ("Enter File Fullscreen", None),
            ToolbarAction::Update => ("Update Available", Some(IconName::Download)),
        };
        Self {
            action,
            label,
            icon,
            selected: false,
            enabled: true,
        }
    }
}

/// Fixed order: viewing mode, related file action, then text layout.
/// Capabilities come from the active reader, not a separate extension allowlist.
pub(super) fn document_items(reader: &Reader, word_wrap: bool) -> Vec<ToolbarItem> {
    let mut items = Vec::new();
    if reader.diff.is_some() || reader.historical {
        let mut split = ToolbarItem::new(ToolbarAction::SideBySide);
        split.selected = reader.side_by_side();
        split.enabled = reader.split_available();
        let mut inline = ToolbarItem::new(ToolbarAction::Inline);
        inline.selected = !split.selected;
        items.extend([split, inline]);
        if reader.diff.is_some() {
            items.push(ToolbarItem::new(ToolbarAction::ViewFile));
        }
    } else if reader.can_preview() {
        let mut preview = ToolbarItem::new(ToolbarAction::Preview);
        if reader.is_json() {
            preview.label = "JSON tree";
            preview.icon = Some(IconName::ListTree);
        } else if reader.is_csv() {
            preview.label = if reader
                .path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("tsv"))
            {
                "TSV table"
            } else {
                "CSV table"
            };
            preview.icon = Some(IconName::Rows2);
        }
        preview.selected = reader.toggle_label() == "Source";
        let mut source = ToolbarItem::new(ToolbarAction::Source);
        source.selected = !preview.selected;
        items.extend([preview, source]);
    }
    if reader.can_word_wrap() {
        let mut wrap = ToolbarItem::new(ToolbarAction::WordWrap);
        wrap.selected = word_wrap;
        wrap.enabled = !reader.can_preview() || reader.toggle_label() != "Source";
        if !wrap.enabled {
            wrap.label = "Word wrap · Source view";
        }
        items.push(wrap);
    }
    items
}
