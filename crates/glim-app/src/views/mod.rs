pub(crate) mod about;
mod changes;
pub mod explorer;
mod minimap;
mod native_menu_bridge;
mod reader;
mod welcome;
pub mod workspace;

mod file_icons;
mod split_diff;

pub(crate) mod updates;

/// Opaque selection fill shared by sidebar navigation and file-tree rows.
fn selection_background(cx: &gpui_kit::App) -> gpui_kit::Hsla {
    use gpui_kit::component::ActiveTheme;
    let strength = if cx.theme().is_dark() { 0.16 } else { 0.20 };
    cx.theme()
        .sidebar
        .blend(cx.theme().foreground.opacity(strength))
        .opacity(1.)
}
