use super::{DEFAULT_SIDEBAR_RATIO, Page, SIDEBAR_COLLAPSE_WIDTH, Sidebar, Workspace};
use crate::{
    app::actions::{
        AddFolder, CloseWindow, CloseWorkspace, NavigateBack, NavigateForward, NextTab, OpenFile,
        OpenFolder, OpenSettings, PreviousTab, Refresh, SaveFile, SearchWorkspace, TogglePreview,
        ToggleSidebar, ToggleWordWrap,
    },
    views::welcome::welcome,
};
use gpui_kit::{
    base::InteractiveElementExt as _,
    component::{
        ActiveTheme, Icon,
        button::{Button, ButtonCustomVariant, ButtonVariants},
        h_flex,
        menu::{ContextMenuExt, PopupMenuItem},
        resizable::{h_resizable, resizable_panel},
        v_flex,
    },
    prelude::FluentBuilder,
    *,
};

// Keep the Git implementation available while the reading-focused navigation is refined.
const GIT_NAVIGATION_VISIBLE: bool = false;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.page != Page::Document || self.active_reader().is_none() {
            self.set_file_fullscreen(false, window);
        }
        if let Some(mode) = cx
            .try_global::<crate::app::ReaderPreferences>()
            .and_then(|p| p.diff_side_by_side)
        {
            for reader in &mut self.tabs {
                reader.set_side_by_side(mode);
            }
        }
        let loading_preview = self.page == Page::Document
            && if self.loading {
                self.loading_preview
            } else {
                self.incoming_file_task.is_some()
                    && self
                        .incoming_files
                        .front()
                        .is_some_and(|path| Self::csv_preview_for_path(path, cx))
            };
        for (index, reader) in self.tabs.iter().enumerate() {
            if self.page != Page::Document || self.active != Some(index) || loading_preview {
                reader.hide_native(cx);
            }
        }
        let header = self.header(cx);
        let tab_bar = self.tab_bar(cx);
        let view = cx.entity().downgrade();
        let open: crate::views::reader::OpenLink =
            std::sync::Arc::new(move |path, anchor, window, cx| {
                let _ = view.update(cx, |v, cx| {
                    v.sidebar = Sidebar::Files;
                    v.open_path(path.clone(), window, cx);
                    if let Some(anchor) = anchor {
                        if let Some(i) = v
                            .active
                            .filter(|i| v.tabs[*i].path == path && v.tabs[*i].diff.is_none())
                        {
                            v.tabs[i].reveal_anchor(&anchor, cx);
                        } else {
                            v.pending_anchor = Some((path, anchor));
                        }
                    }
                    cx.notify();
                });
            });
        let window_width = f32::from(window.viewport_size().width);
        let sidebar_width = window_width * self.sidebar_ratio;
        if (self.sidebar_window_width - window_width).abs() > 0.5 {
            self.sidebar_window_width = window_width;
            let layout = self.sidebar_layout.clone();
            window.defer(cx, move |window, cx| {
                layout.update(cx, |state, cx| {
                    state.resize_panel(0, px(sidebar_width), window, cx)
                });
            });
        }
        let content = match self.page {
            Page::Settings => self.settings_page(cx),
            Page::Start => welcome(self, cx.entity_id(), cx).into_any_element(),
            Page::Document if loading_preview => crate::views::reader::preview_loading(cx),
            Page::Document => match self.active.and_then(|i| self.tabs.get_mut(i)) {
                Some(reader) => reader.render(open, cx),
                None => welcome(self, cx.entity_id(), cx).into_any_element(),
            },
        };
        // Preview actions use the same live capabilities and dispatcher as the
        // toolbar. Text editors keep their selection-aware editing menu.
        let content = if self.page == Page::Document
            && self
                .active_reader()
                .is_some_and(|reader| reader.can_preview() && reader.toggle_label() == "Source")
        {
            let workspace = cx.entity().downgrade();
            div()
                .size_full()
                .child(content)
                .context_menu(move |mut menu, _, cx| {
                    if let Some(workspace) = workspace.upgrade() {
                        for item in workspace.read(cx).reader_actions() {
                            let action = item.action;
                            let workspace = workspace.downgrade();
                            menu = menu.item(
                                PopupMenuItem::new(glim_services::locale::tr(item.label))
                                    .disabled(!item.enabled)
                                    .checked(item.selected)
                                    .on_click(move |_, window, cx| {
                                        let _ = workspace.update(cx, |workspace, cx| {
                                            workspace.run_toolbar_action(action, window, cx)
                                        });
                                    }),
                            );
                        }
                    }
                    menu
                })
                .into_any_element()
        } else {
            content
        };
        let document_page = self.page == Page::Document;
        let gesture_view = cx.entity().downgrade();
        let viewer = v_flex()
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .when(!self.file_fullscreen, |view| {
                view.rounded(px(10.))
                    .border_1()
                    .border_color(cx.theme().border)
            })
            .bg(cx.theme().background)
            .when(
                self.page == Page::Document && !self.tabs.is_empty() && !self.file_fullscreen,
                |view| view.child(tab_bar),
            )
            .when_some(
                self.active_reader()
                    .filter(|_| self.page == Page::Document && !self.file_fullscreen)
                    .and_then(|reader| reader.history_summary()),
                |view, (subject, detail)| {
                    view.child(
                        v_flex()
                            .flex_shrink_0()
                            .px_4()
                            .py_2()
                            .gap_1()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(subject),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(detail),
                            ),
                    )
                },
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(content)
                    .when(self.loading && document_page && !loading_preview, |view| {
                        view.child(
                            div()
                                .absolute()
                                .top(px(12.))
                                .right(px(12.))
                                .px_3()
                                .py_2()
                                .rounded(px(8.))
                                .bg(cx.theme().background)
                                .border_1()
                                .border_color(cx.theme().border)
                                .text_sm()
                                .child(glim_services::locale::tr("Loading…")),
                        )
                    })
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                let view = gesture_view.clone();
                                window.on_mouse_event(
                                    move |event: &ScrollWheelEvent, phase, window, cx| {
                                        if document_page
                                            && phase == DispatchPhase::Capture
                                            && bounds.contains(&event.position)
                                        {
                                            let _ = view.update(cx, |v, cx| {
                                                v.history_swipe(event, window, cx)
                                            });
                                        }
                                    },
                                );
                            },
                        )
                        .absolute()
                        .size_full(),
                    ),
            );
        let panel_content = match self.sidebar {
            Sidebar::Files => self.explorer.as_ref().map(|v| v.clone().into_any_element()),
            Sidebar::Changes => self.changes.as_ref().map(|v| v.clone().into_any_element()),
        }
        .unwrap_or_else(|| {
            v_flex()
                .p_5()
                .gap_5()
                .text_sm()
                .when(self.sidebar == Sidebar::Changes, |view| {
                    view.child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(glim_services::locale::tr("Source Control")),
                    )
                    .child(
                        div().text_color(cx.theme().muted_foreground).child(
                            glim_services::locale::tr("Open a folder containing a Git repository."),
                        ),
                    )
                })
                .child(
                    Button::new("empty-open-folder")
                        .w_full()
                        .label(glim_services::locale::tr("Open Folder"))
                        .on_click(cx.listener(|v, _, w, cx| v.choose_path(true, w, cx))),
                )
                .into_any_element()
        });
        let nav_height = 34.;
        let navigation = |id: &'static str, icon, label: &'static str, selected: bool| {
            let background = if selected {
                crate::views::selection_background(cx)
            } else {
                cx.theme().sidebar
            };
            Button::new(id)
                .custom(
                    ButtonCustomVariant::new(cx)
                        .color(background)
                        .hover(background.blend(cx.theme().foreground.opacity(0.06)))
                        .active(background.blend(cx.theme().foreground.opacity(0.10))),
                )
                // Override the component's translucent idle custom fill.
                .bg(background)
                .font_weight(if selected {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .w_full()
                .h(px(nav_height))
                .rounded(px(8.))
                .justify_start()
                .accessibility_label(glim_services::locale::tr(label))
                .child(
                    h_flex()
                        .w_full()
                        .gap_3()
                        .child(Icon::new(icon).size(px(17.)))
                        .child(glim_services::locale::tr(label)),
                )
        };
        let start = navigation(
            "start-navigation",
            gpui_kit::assets::IconName::House,
            "Start",
            self.sidebar_selection == Some("start"),
        );
        let files = navigation(
            "files-navigation",
            gpui_kit::assets::IconName::Files,
            "Files",
            self.sidebar_selection == Some("files"),
        );
        let changes = navigation(
            "changes-navigation",
            gpui_kit::assets::IconName::GitCompareArrows,
            "Changes",
            self.sidebar_selection == Some("changes"),
        );
        let sidebar = v_flex()
            .size_full()
            .px_4()
            .pb_1()
            .gap_1()
            .bg(cx.theme().sidebar)
            .child(start.on_click(cx.listener(|view, _, _, cx| {
                view.select_sidebar_navigation("start", cx);
                view.page = Page::Start;
                cx.notify();
            })))
            .child(files.on_click(cx.listener(|view, _, _, cx| {
                view.select_sidebar_navigation("files", cx);
                view.page = Page::Document;
                view.sidebar = Sidebar::Files;
                cx.notify();
            })))
            .when(GIT_NAVIGATION_VISIBLE, |sidebar| {
                sidebar.child(changes.on_click(cx.listener(|view, _, window, cx| {
                    view.select_sidebar_navigation("changes", cx);
                    view.page = Page::Document;
                    view.sidebar = Sidebar::Changes;
                    if !view.changes.as_ref().is_some_and(|c| c.read(cx).is_busy()) {
                        view.refresh(window, cx);
                    }
                    cx.notify();
                })))
            })
            .child(
                div()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(glim_services::locale::tr(
                        if self.sidebar == Sidebar::Files {
                            "Workspace"
                        } else {
                            "Changes"
                        },
                    )),
            )
            .child(div().flex_1().min_h_0().w_full().child(panel_content))
            .id("workspace-sidebar");
        let sidebar = if self.sidebar == Sidebar::Files && self.explorer.is_none() {
            sidebar
                .context_menu(|menu, _, _| {
                    menu.item(
                        PopupMenuItem::new(glim_services::locale::tr("Add Folder to Workspace…"))
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(AddFolder), cx);
                            }),
                    )
                })
                .into_any_element()
        } else {
            sidebar.into_any_element()
        };
        let body = if self.file_fullscreen {
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(viewer)
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .pr(px(2.))
                .pb(px(2.))
                .when(!self.sidebar_visible, |view| view.pl_2())
                .child(
                    h_resizable("workspace-panels")
                        .with_state(&self.sidebar_layout)
                        .on_resize(cx.listener(
                            |view,
                             state: &Entity<gpui_kit::component::resizable::ResizableState>,
                             window,
                             cx| {
                                if let Some(width) = state.read(cx).sizes().first().copied() {
                                    if f32::from(width) <= SIDEBAR_COLLAPSE_WIDTH {
                                        view.sidebar_visible = false;
                                        view.sidebar_window_width = 0.;
                                    } else {
                                        view.sidebar_ratio = (f32::from(width)
                                            / f32::from(window.viewport_size().width))
                                        .min(0.5);
                                    }
                                    cx.notify();
                                }
                            },
                        ))
                        .with_handle_appearance({
                            let appearance =
                                gpui_kit::component::resizable::resize_handle_appearance();
                            let layout = self.sidebar_layout.clone();
                            let workspace = cx.entity().downgrade();
                            std::rc::Rc::new(move |handle, window, cx| {
                                if handle.is_active()
                                    && layout.read(cx).sizes().first().is_some_and(|size| {
                                        f32::from(*size) <= SIDEBAR_COLLAPSE_WIDTH
                                    })
                                {
                                    let workspace = workspace.clone();
                                    cx.defer(move |cx| {
                                        let _ = workspace.update(cx, |view, cx| {
                                            view.sidebar_visible = false;
                                            view.sidebar_window_width = 0.;
                                            cx.notify();
                                        });
                                    });
                                }
                                appearance(handle, window, cx).map(|line| {
                                    let layout = layout.clone();
                                    let workspace = workspace.clone();
                                    // Keep the visible line inset, but use the full drag band for double clicks.
                                    div()
                                        .id("workspace-sidebar-resize-handle")
                                        .flex()
                                        .justify_center()
                                        .w(px(9.))
                                        .ml(px(-4.))
                                        .h_full()
                                        .pt(px(14.))
                                        .pb(px(14.))
                                        .child(line)
                                        .on_double_click(move |_, window, cx| {
                                            let _ = workspace.update(cx, |view, cx| {
                                                view.sidebar_ratio = DEFAULT_SIDEBAR_RATIO;
                                                cx.notify();
                                            });
                                            layout.update(cx, |state, cx| {
                                                state.resize_panel(
                                                    0,
                                                    window.viewport_size().width
                                                        * DEFAULT_SIDEBAR_RATIO,
                                                    window,
                                                    cx,
                                                );
                                            });
                                            cx.stop_propagation();
                                        })
                                        .into_any_element()
                                })
                            })
                        })
                        .child(
                            resizable_panel()
                                .visible(self.sidebar_visible)
                                .size(px(sidebar_width))
                                .flex_none()
                                .size_range(px(80.)..px(window_width * 0.5))
                                .child(sidebar),
                        )
                        .child(
                            resizable_panel()
                                .size_range(px(280.)..px(10000.))
                                .child(viewer),
                        ),
                )
                .into_any_element()
        };
        v_flex()
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().foreground)
            .when(!self.file_fullscreen, |view| view.child(header))
            .capture_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                if view.file_fullscreen && event.keystroke.key == "escape" {
                    view.set_file_fullscreen(false, window);
                    view.sidebar_window_width = 0.;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|v, _, w, cx| {
                    v.history_swipe = None;
                    v.navigate(false, w, cx);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|v, _, w, cx| {
                    v.history_swipe = None;
                    v.navigate(true, w, cx);
                    cx.stop_propagation();
                }),
            )
            .on_action(cx.listener(|v, _: &OpenSettings, window, cx| {
                v.show_settings(window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|v, _: &SearchWorkspace, w, cx| {
                v.search.update(cx, |search, cx| search.focus(w, cx));
                cx.notify();
            }))
            .on_action(cx.listener(|v, _: &ToggleSidebar, _, cx| {
                v.sidebar_visible = !v.sidebar_visible;
                v.sidebar_window_width = 0.;
                cx.notify();
            }))
            .on_action(cx.listener(|v, _: &TogglePreview, w, cx| {
                if let Some(reader) = v.active_reader().filter(|reader| reader.can_preview()) {
                    let preview = reader.toggle_label() != "Source";
                    v.set_reader_preview(preview, w, cx);
                }
            }))
            .on_action(cx.listener(|v, _: &ToggleWordWrap, w, cx| v.toggle_word_wrap(w, cx)))
            .on_action(
                cx.listener(|v, _: &gpui_kit::component::input::Copy, _, cx| {
                    if !v
                        .active_reader()
                        .is_some_and(|reader| reader.copy_image(cx))
                    {
                        cx.propagate();
                    }
                }),
            )
            .on_action(cx.listener(|v, _: &NextTab, w, cx| {
                if !v.tabs.is_empty() {
                    v.activate_tab((v.active.unwrap_or(0) + 1) % v.tabs.len(), w, cx);
                }
            }))
            .on_action(cx.listener(|v, _: &PreviousTab, w, cx| {
                if !v.tabs.is_empty() {
                    v.activate_tab(
                        (v.active.unwrap_or(0) + v.tabs.len() - 1) % v.tabs.len(),
                        w,
                        cx,
                    );
                }
            }))
            .on_action(cx.listener(|v, _: &NavigateBack, w, cx| v.navigate(false, w, cx)))
            .on_action(cx.listener(|v, _: &NavigateForward, w, cx| v.navigate(true, w, cx)))
            .on_action(cx.listener(|v, _: &CloseWorkspace, w, cx| {
                v.request_close_window(w, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|v, _: &AddFolder, w, cx| v.choose_add_folder(w, cx)))
            .on_action(cx.listener(|v, _: &OpenFile, w, cx| v.choose_path(false, w, cx)))
            .on_action(cx.listener(|v, _: &OpenFolder, w, cx| v.choose_path(true, w, cx)))
            .on_action(cx.listener(|v, _: &Refresh, w, cx| v.refresh(w, cx)))
            .on_action(cx.listener(|v, _: &SaveFile, w, cx| v.save_active(w, cx)))
            .on_action(cx.listener(|v, _: &CloseWindow, w, cx| {
                if let Some(i) = v.active {
                    v.close_tab(i, w, cx);
                } else {
                    v.request_close_window(w, cx);
                }
                cx.stop_propagation();
            }))
            .child(body)
            .when_some(self.error.clone(), |view, error| {
                view.child(
                    deferred(
                        v_flex()
                            .absolute()
                            .right(px(16.))
                            .bottom(px(16.))
                            .w(px(420.))
                            .max_w(relative(0.9))
                            .p_3()
                            .gap_2()
                            .rounded_lg()
                            .bg(cx.theme().background)
                            .border_1()
                            .border_color(cx.theme().border)
                            .shadow_lg()
                            .occlude()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .child(
                                        div().text_sm().text_color(cx.theme().danger).child(
                                            glim_services::locale::tr("Operation incomplete"),
                                        ),
                                    )
                                    .child(
                                        Button::new("dismiss-error")
                                            .ghost()
                                            .label("×")
                                            .accessibility_label(glim_services::locale::tr(
                                                "Dismiss error",
                                            ))
                                            .on_click(cx.listener(|v, _, _, cx| {
                                                v.error = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .id("workspace-error-message")
                                    .max_h(px(160.))
                                    .overflow_y_scroll()
                                    .text_sm()
                                    .whitespace_normal()
                                    .text_color(cx.theme().foreground)
                                    .child(error),
                            ),
                    )
                    .with_priority(2),
                )
            })
    }
}
