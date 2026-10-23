use crate::app::actions::{AddFolder, NewFile, NewWindow, OpenFile, OpenFolder};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Icon,
        button::{Button, ButtonCustomVariant, ButtonVariants},
        h_flex, v_flex,
    },
    prelude::FluentBuilder,
    *,
};

fn quick_action(
    id: &'static str,
    label: &'static str,
    detail: &'static str,
    shortcut: &'static str,
    icon: IconName,
    cx: &App,
) -> Button {
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(cx.theme().secondary.opacity(0.25))
                .hover(cx.theme().secondary)
                .active(cx.theme().secondary.opacity(0.8)),
        )
        .flex_1()
        .flex_basis(px(300.))
        .min_w_0()
        .h(px(88.))
        .px_4()
        .rounded(px(12.))
        .border_1()
        .border_color(cx.theme().border)
        .accessibility_label(glim_services::locale::tr(label))
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_3()
                .child(
                    div()
                        .size(px(36.))
                        .flex_shrink_0()
                        .rounded(px(10.))
                        .bg(cx.theme().primary.opacity(0.08))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon).size(px(19.)).text_color(cx.theme().primary)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .text_left()
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(glim_services::locale::tr(label)),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(shortcut),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .truncate()
                                .text_color(cx.theme().muted_foreground)
                                .child(glim_services::locale::tr(detail)),
                        ),
                ),
        )
}

pub fn welcome(
    current: &super::workspace::Workspace,
    current_id: EntityId,
    cx: &mut Context<super::workspace::Workspace>,
) -> impl IntoElement {
    let show_open_folder = current.folder_roots().is_empty();
    let recent = cx
        .try_global::<crate::app::RecentWorkspaces>()
        .map(|history| {
            history
                .0
                .iter()
                .filter(|workspace| !current.matches_recent_workspace(workspace))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let more = recent.len() > current.recent_visible();
    let recent_count = recent.len();
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let content = v_flex()
        .w_full()
        .max_w(px(720.))
        .min_w_0()
        .flex_shrink_0()
        .gap_8()
        .child(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_5()
                .child(
                    h_flex()
                        .items_center()
                        .gap_3()
                        .child(
                            img("branding/glim-welcome.png")
                                .size(px(56.))
                                .object_fit(ObjectFit::Contain)
                                .flex_shrink_0(),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(px(30.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Glim"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(glim_services::locale::tr(
                                            "Documents, data, and code. One workspace.",
                                        )),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .flex_wrap()
                        .gap_3()
                        .child(
                            quick_action(
                                "welcome-open-file",
                                "Open File…",
                                "Read documents, images, and data",
                                "⌘O",
                                IconName::FileText,
                                cx,
                            )
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(OpenFile), cx)
                            }),
                        )
                        .child(
                            quick_action(
                                "welcome-new-file",
                                "New File",
                                "Start an untitled note",
                                "⌘N",
                                IconName::FilePlus,
                                cx,
                            )
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(NewFile), cx)
                            }),
                        )
                        .child(
                            quick_action(
                                "welcome-open-folder",
                                if show_open_folder {
                                    "Open Folder"
                                } else {
                                    "Add Folder…"
                                },
                                if show_open_folder {
                                    "Keep related files together"
                                } else {
                                    "Add a folder to this workspace"
                                },
                                if show_open_folder {
                                    "⌘⇧O"
                                } else {
                                    "⌘⌥O"
                                },
                                if show_open_folder {
                                    IconName::FolderOpen
                                } else {
                                    IconName::FolderPlus
                                },
                                cx,
                            )
                            .on_click(move |_, window, cx| {
                                if show_open_folder {
                                    window.dispatch_action(Box::new(OpenFolder), cx);
                                } else {
                                    window.dispatch_action(Box::new(AddFolder), cx);
                                }
                            }),
                        )
                        .child(
                            quick_action(
                                "welcome-new-workspace",
                                "Open New Workspace",
                                "Work in a separate window",
                                "⌘⇧N",
                                IconName::PanelsTopLeft,
                                cx,
                            )
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(NewWindow), cx)
                            }),
                        ),
                ),
        )
        .when(!recent.is_empty(), |view| {
            view.child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .text_left()
                    .child(
                        h_flex()
                            .px_2()
                            .gap_2()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(Icon::new(IconName::Clock).size(px(15.)))
                            .child(
                                div()
                                    .flex_1()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(glim_services::locale::tr("Recent Workspaces")),
                            )
                            .child(div().text_xs().child(recent_count.to_string())),
                    )
                    .child(
                        v_flex()
                            .id("recent-workspaces")
                            .w_full()
                            .min_w_0()
                            .gap_1()
                            .children(
                                recent
                                    .into_iter()
                                    .take(current.recent_visible())
                                    .enumerate()
                                    .map(|(index, workspace)| {
                                        let title = workspace
                                            .folders
                                            .iter()
                                            .map(|path| {
                                                path.file_name()
                                                    .unwrap_or(path.as_os_str())
                                                    .to_string_lossy()
                                                    .into_owned()
                                            })
                                            .collect::<Vec<_>>()
                                            .join(" · ");
                                        let paths = workspace
                                            .folders
                                            .iter()
                                            .map(|path| {
                                                home.as_ref()
                                                    .and_then(|home| path.strip_prefix(home).ok())
                                                    .map(|relative| {
                                                        format!("~/{}", relative.display())
                                                    })
                                                    .unwrap_or_else(|| path.display().to_string())
                                            })
                                            .collect::<Vec<_>>()
                                            .join(" · ");
                                        let action = glim_services::locale::tr(
                                            if crate::app::recent_workspace_is_open(
                                                &workspace, current_id, cx,
                                            ) {
                                                "Switch to Window"
                                            } else {
                                                "Open in New Window"
                                            },
                                        );
                                        Button::new(("recent-workspace", index))
                                            .ghost()
                                            .w_full()
                                            .h(px(66.))
                                            .px_3()
                                            .rounded(px(10.))
                                            .flex_shrink_0()
                                            .tooltip(format!("{}\n{}", action, paths))
                                            .accessibility_label(format!("{}: {}", action, title))
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .min_w_0()
                                                    .gap_3()
                                                    .child(
                                                        div()
                                                            .size(px(34.))
                                                            .flex_shrink_0()
                                                            .rounded(px(9.))
                                                            .bg(cx.theme().secondary.opacity(0.6))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .child(
                                                                Icon::new(
                                                                    if workspace.folders.len() > 1 {
                                                                        IconName::Folders
                                                                    } else {
                                                                        IconName::Folder
                                                                    },
                                                                )
                                                                .size(px(17.))
                                                                .text_color(
                                                                    cx.theme().muted_foreground,
                                                                ),
                                                            ),
                                                    )
                                                    .child(
                                                        v_flex()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .text_left()
                                                            .gap_1()
                                                            .child(
                                                                div()
                                                                    .text_sm()
                                                                    .truncate()
                                                                    .child(title),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .truncate()
                                                                    .text_color(
                                                                        cx.theme().muted_foreground,
                                                                    )
                                                                    .child(paths),
                                                            ),
                                                    )
                                                    .child(
                                                        Icon::new(IconName::ArrowUpRight)
                                                            .size(px(14.))
                                                            .flex_shrink_0()
                                                            .text_color(
                                                                cx.theme().muted_foreground,
                                                            ),
                                                    ),
                                            )
                                            .on_click(move |_, _, cx| {
                                                crate::app::open_recent_workspace(
                                                    workspace.clone(),
                                                    cx,
                                                );
                                            })
                                    }),
                            ),
                    )
                    .when(more, |view| {
                        view.child(
                            Button::new("recent-more")
                                .ghost()
                                .icon(IconName::ChevronDown)
                                .label(glim_services::locale::tr("Show more"))
                                .on_click(cx.listener(|view, _, _, cx| view.show_more_recent(cx))),
                        )
                    }),
            )
        });

    v_flex()
        .id("welcome-page")
        .size_full()
        .min_h_0()
        .overflow_y_scroll()
        .items_center()
        .px_6()
        .pt(px(48.))
        .pb_8()
        .child(content)
}
