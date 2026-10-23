use super::actions::{
    About, AddFolder, CheckForUpdates, CloseWorkspace, NewFile, NewWindow, OpenFile, OpenFolder,
    OpenSettings, Quit, SaveFile, TogglePreview, ToggleSidebar, ToggleWordWrap,
};
use glim_services::locale::tr;
use gpui_kit::{App, Menu, MenuItem};

pub(super) fn update(cx: &mut App) {
    cx.set_dock_menu(vec![MenuItem::action(tr("New Window"), NewWindow)]);
    cx.set_menus(vec![
        Menu {
            name: "Glim".into(),
            disabled: false,
            items: vec![
                MenuItem::action(tr("About Glim"), About),
                MenuItem::action(
                    tr(match super::updates::check_label(cx) {
                        "Check…" => "Check for Updates…",
                        label => label,
                    }),
                    CheckForUpdates,
                ),
                MenuItem::separator(),
                MenuItem::action(tr("Settings…"), OpenSettings),
                MenuItem::separator(),
                MenuItem::action(tr("Quit Glim"), Quit),
            ],
        },
        Menu {
            name: tr("File").into(),
            disabled: false,
            items: vec![
                MenuItem::action(tr("New File"), NewFile),
                MenuItem::action(tr("New Window"), NewWindow),
                MenuItem::separator(),
                MenuItem::action(tr("Open File…"), OpenFile),
                MenuItem::action(tr("Open Folder…"), OpenFolder),
                MenuItem::action(tr("Add Folder to Workspace…"), AddFolder),
                MenuItem::separator(),
                MenuItem::action(tr("Save"), SaveFile),
                MenuItem::separator(),
                MenuItem::action(tr("Close Window"), CloseWorkspace),
            ],
        },
        Menu {
            name: tr("View").into(),
            disabled: false,
            items: vec![
                MenuItem::action(tr("Toggle Sidebar"), ToggleSidebar),
                MenuItem::separator(),
                MenuItem::action(tr("Toggle Preview"), TogglePreview),
                MenuItem::action(tr("Word wrap"), ToggleWordWrap),
            ],
        },
    ]);
}
