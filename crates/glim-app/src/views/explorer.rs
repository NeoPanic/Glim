use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use glim_core::DirectoryEntry;
use glim_services::workspace::list_directory;
use gpui_kit::{
    base::TestSupportExt,
    component::{
        ActiveTheme, FocusableExt, Icon,
        button::{Button, ButtonCustomVariant, ButtonVariants},
        menu::{PopupMenu, PopupMenuItem},
    },
    prelude::FluentBuilder,
    *,
};

pub enum ExplorerEvent {
    SelectionChanged,
    OpenFile(PathBuf),
    RemoveFolder(u64),
    TrashEntry(PathBuf),
    PlayFolder(PathBuf, bool),
    Error(String),
    ExpansionChanged(Vec<PathBuf>, Vec<PathBuf>),
}

gpui_kit::actions!(explorer, [Up, Down, Expand, Collapse, OpenSelected]);

#[derive(Clone)]
struct Row {
    root_index: usize,
    entry: DirectoryEntry,
    depth: usize,
    parent: Option<usize>,
    subtree_end: usize,
}

impl Row {
    fn element_id(&self, part: &'static str) -> ElementId {
        let id = entry_id(&self.entry.path, part);
        if self.root_index == 0 {
            id
        } else {
            ElementId::NamedChild(std::sync::Arc::new(id), self.root_index.to_string().into())
        }
    }
}

#[derive(Clone, Copy)]
enum FileMenuAction {
    Play(bool),
    Open,
    Reveal,
    CopyRelative,
    CopyAbsolute,
    Trash,
    RemoveFolder(u64),
}

/// Only expanded directories are loaded, and only visible rows are rendered.
pub struct Explorer {
    roots: Vec<PathBuf>,
    root_ids: Vec<u64>,
    next_root_id: u64,
    directories: HashMap<PathBuf, Vec<DirectoryEntry>>,
    expanded: Vec<HashSet<PathBuf>>,
    tasks: HashMap<PathBuf, Task<()>>,
    media_presence: HashMap<PathBuf, bool>,
    media_tasks: HashMap<PathBuf, Task<()>>,
    rows: Vec<Row>,
    clipped_labels: HashSet<(usize, PathBuf, bool)>,
    path_hint: super::path_hint::PathHint,
    selected: Option<usize>,
    clicked_path: Option<PathBuf>,
    clicked_root: Option<usize>,
    pending_reveal: Option<PathBuf>,
    preserve_next_reveal: Option<PathBuf>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    context_menu_task: Option<Task<()>>,
}

impl EventEmitter<ExplorerEvent> for Explorer {}

impl Explorer {
    pub fn init(cx: &mut App) {
        cx.bind_keys([
            KeyBinding::new("up", Up, Some("Explorer")),
            KeyBinding::new("down", Down, Some("Explorer")),
            KeyBinding::new("right", Expand, Some("Explorer")),
            KeyBinding::new("left", Collapse, Some("Explorer")),
            KeyBinding::new("enter", OpenSelected, Some("Explorer")),
        ]);
    }

    pub fn new(root: PathBuf, entries: Vec<DirectoryEntry>, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            roots: vec![root.clone()],
            root_ids: vec![0],
            next_root_id: 1,
            directories: HashMap::from([(root.clone(), entries)]),
            expanded: vec![HashSet::from([root])],
            tasks: HashMap::new(),
            media_presence: HashMap::new(),
            media_tasks: HashMap::new(),
            rows: Vec::new(),
            clipped_labels: HashSet::new(),
            path_hint: Default::default(),
            selected: None,
            clicked_path: None,
            clicked_root: None,
            pending_reveal: None,
            preserve_next_reveal: None,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            context_menu_task: None,
        };
        view.rebuild();
        view
    }

    pub fn add_folder(
        &mut self,
        root: PathBuf,
        entries: Vec<DirectoryEntry>,
        cx: &mut Context<Self>,
    ) {
        self.roots.push(root.clone());
        self.root_ids.push(self.next_root_id);
        self.next_root_id += 1;
        self.expanded.push(HashSet::from([root.clone()]));
        self.directories.insert(root, entries);
        self.rebuild();
        cx.notify();
    }

    /// Remove one root occurrence without touching its files or other root entries.
    pub fn remove_folder(&mut self, root_id: u64, cx: &mut Context<Self>) -> Option<usize> {
        let index = self.root_ids.iter().position(|id| *id == root_id)?;
        let selected = self
            .selected
            .and_then(|i| self.rows.get(i))
            .map(|row| (self.root_ids[row.root_index], row.entry.path.clone()));
        let clicked = self
            .clicked_root
            .and_then(|i| self.root_ids.get(i))
            .copied();
        self.roots.remove(index);
        self.root_ids.remove(index);
        self.expanded.remove(index);
        self.selected = None;
        self.clicked_root =
            clicked.and_then(|id| self.root_ids.iter().position(|root| *root == id));
        if self.clicked_root.is_none() {
            self.clicked_path = None;
        }
        self.pending_reveal = None;
        self.preserve_next_reveal = None;
        self.clipped_labels.clear();
        let roots = &self.roots;
        let retained = |path: &PathBuf| roots.iter().any(|root| path.starts_with(root));
        self.directories.retain(|path, _| retained(path));
        self.tasks.retain(|path, _| retained(path));
        self.media_presence.retain(|path, _| retained(path));
        self.media_tasks.retain(|path, _| retained(path));
        self.rows.clear();
        self.rebuild();
        if let Some((id, path)) = selected {
            self.selected = self
                .rows
                .iter()
                .position(|row| self.root_ids[row.root_index] == id && row.entry.path == path);
        }
        cx.notify();
        Some(index)
    }

    pub fn root_ids_under(&self, path: &std::path::Path) -> Vec<u64> {
        self.roots
            .iter()
            .zip(&self.root_ids)
            .filter(|(root, _)| root.starts_with(path))
            .map(|(_, id)| *id)
            .collect()
    }

    pub fn clear_clicked_selection(&mut self, cx: &mut Context<Self>) {
        self.clicked_path = None;
        self.clicked_root = None;
        self.pending_reveal = None;
        self.preserve_next_reveal = None;
        cx.notify();
    }

    pub fn expansion_state(&self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut expanded = self.expanded.iter().flatten().cloned().collect::<Vec<_>>();
        expanded.sort();
        (self.roots.clone(), expanded)
    }

    fn expansion_changed(&self, cx: &mut Context<Self>) {
        let (roots, expanded) = self.expansion_state();
        cx.emit(ExplorerEvent::ExpansionChanged(roots, expanded));
    }

    pub fn restore_expanded(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.expanded = self
            .roots
            .iter()
            .map(|root| {
                paths
                    .iter()
                    .filter(|path| {
                        !path
                            .components()
                            .any(|part| part == std::path::Component::ParentDir)
                            && path.starts_with(root)
                    })
                    .cloned()
                    .collect()
            })
            .collect();
        self.rebuild();
        self.load_expanded(cx);
        cx.notify();
    }

    fn load_expanded(&mut self, cx: &mut Context<Self>) {
        let paths = self
            .expanded
            .iter()
            .flatten()
            .filter(|path| {
                self.roots.contains(path)
                    || path
                        .parent()
                        .and_then(|parent| self.directories.get(parent))
                        .is_some_and(|entries| {
                            entries.iter().any(|entry| {
                                entry.path == **path && entry.is_dir && !entry.is_symlink
                            })
                        })
            })
            .cloned()
            .collect::<Vec<_>>();
        for path in paths {
            self.load_directory(path, cx);
        }
    }

    fn load_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.directories.contains_key(&path) || self.tasks.contains_key(&path) {
            return;
        }
        let read_path = path.clone();
        let read = cx
            .background_executor()
            .spawn(async move { list_directory(&read_path) });
        let key = path.clone();
        let task = cx.spawn(async move |view, cx| {
            let result = read.await;
            let _ = view.update(cx, |view, cx| {
                view.tasks.remove(&path);
                match result {
                    Ok(entries) => {
                        view.directories.insert(path, entries);
                    }
                    Err(error) => {
                        for expanded in &mut view.expanded {
                            expanded.remove(&path);
                        }
                        if view
                            .pending_reveal
                            .as_ref()
                            .is_some_and(|target| target.starts_with(&path))
                        {
                            view.pending_reveal = None;
                        }
                        view.expansion_changed(cx);
                        cx.emit(ExplorerEvent::Error(format!("{error:#}")));
                    }
                }
                view.rebuild();
                view.advance_reveal(cx);
                view.load_expanded(cx);
                cx.notify();
            });
        });
        self.tasks.insert(key, task);
    }

    pub fn reveal_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // Opening a row feeds back through tab activation/content installation.
        // Keep that click's scroll position; explicit tab changes still reveal.
        if self.preserve_next_reveal.take().as_ref() == Some(&path) {
            self.pending_reveal = None;
            cx.notify();
            return;
        }
        self.selected = None;
        self.pending_reveal = self
            .roots
            .iter()
            .any(|root| path.starts_with(root))
            .then_some(path);
        if self.pending_reveal.is_some() {
            self.clicked_path = None;
            self.clicked_root = None;
            cx.emit(ExplorerEvent::SelectionChanged);
        }
        self.advance_reveal(cx);
        cx.notify();
    }

    fn advance_reveal(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.pending_reveal.clone() else {
            return;
        };
        let Some((root_index, root)) = self
            .roots
            .iter()
            .enumerate()
            .filter(|(_, root)| target.starts_with(root))
            .max_by_key(|(_, root)| root.components().count())
            .map(|(index, root)| (index, root.clone()))
        else {
            return;
        };
        let mut ancestors = target
            .parent()
            .into_iter()
            .flat_map(|parent| parent.ancestors())
            .take_while(|path| path.starts_with(&root))
            .map(std::path::Path::to_path_buf)
            .collect::<Vec<_>>();
        ancestors.reverse();
        let mut changed = false;
        let mut ready = true;
        for path in ancestors {
            if path != root {
                let Some(entries) = path
                    .parent()
                    .and_then(|parent| self.directories.get(parent))
                else {
                    ready = false;
                    break;
                };
                if !entries
                    .iter()
                    .any(|entry| entry.path == path && entry.is_dir && !entry.is_symlink)
                {
                    self.pending_reveal = None;
                    ready = false;
                    break;
                }
            }
            changed |= self.expanded[root_index].insert(path.clone());
            if !self.directories.contains_key(&path) {
                self.load_directory(path, cx);
                ready = false;
                break;
            }
        }
        self.rebuild();
        if changed {
            self.expansion_changed(cx);
        }
        if ready {
            self.pending_reveal = None;
            if let Some(index) = self
                .rows
                .iter()
                .position(|row| row.root_index == root_index && row.entry.path == target)
            {
                self.selected = Some(index);
                self.clicked_path = Some(target);
                self.clicked_root = Some(root_index);
                // Center explicitly so pinned ancestor rows cannot cover the target.
                self.scroll
                    .scroll_to_item_strict(index, ScrollStrategy::Center);
            }
        }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.media_tasks.clear();
        self.media_presence.clear();
        let mut paths = self.expanded.iter().flatten().cloned().collect::<Vec<_>>();
        paths.extend(self.roots.iter().cloned());
        paths.sort();
        paths.dedup();
        for path in paths {
            let read_path = path.clone();
            let read = cx
                .background_executor()
                .spawn(async move { list_directory(&read_path) });
            let key = path.clone();
            let task = cx.spawn(async move |view, cx| {
                let result = read.await;
                let _ = view.update(cx, |view, cx| {
                    if let Ok(entries) = result {
                        view.directories.insert(path.clone(), entries);
                    } else {
                        view.directories.remove(&path);
                    }
                    view.tasks.remove(&path);
                    view.rebuild();
                    view.advance_reveal(cx);
                    view.load_expanded(cx);
                    cx.notify();
                });
            });
            self.tasks.insert(key, task);
        }
    }

    fn rebuild(&mut self) {
        fn append(
            path: &PathBuf,
            root_index: usize,
            depth: usize,
            parent: Option<usize>,
            this: &Explorer,
            rows: &mut Vec<Row>,
        ) {
            if let Some(children) = this.directories.get(path) {
                for entry in children {
                    let index = rows.len();
                    rows.push(Row {
                        root_index,
                        entry: entry.clone(),
                        depth,
                        parent,
                        subtree_end: index + 1,
                    });
                    if this.expanded[root_index].contains(&entry.path) {
                        append(&entry.path, root_index, depth + 1, Some(index), this, rows);
                    }
                    rows[index].subtree_end = rows.len();
                }
            }
        }
        let selected = self
            .selected
            .and_then(|i| self.rows.get(i))
            .map(|r| (r.root_index, r.entry.path.clone()));
        let mut rows = Vec::new();
        for (root_index, root) in self.roots.iter().enumerate() {
            let index = rows.len();
            rows.push(Row {
                root_index,
                entry: DirectoryEntry {
                    path: root.clone(),
                    is_dir: true,
                    is_symlink: false,
                },
                depth: 0,
                parent: None,
                subtree_end: index + 1,
            });
            if self.expanded[root_index].contains(root) {
                append(root, root_index, 1, Some(index), self, &mut rows);
            }
            rows[index].subtree_end = rows.len();
        }
        self.selected = selected.and_then(|(root_index, path)| {
            rows.iter()
                .position(|r| r.root_index == root_index && r.entry.path == path)
        });
        self.rows = rows;
    }

    fn check_folder_media(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.media_presence.contains_key(&path) || self.media_tasks.contains_key(&path) {
            return;
        }
        let read_path = path.clone();
        let read = cx.background_executor().spawn(async move {
            glim_services::preview::folder_has_media(&read_path).unwrap_or(false)
        });
        let key = path.clone();
        let task = cx.spawn(async move |view, cx| {
            let present = read.await;
            let _ = view.update(cx, |v, cx| {
                v.media_tasks.remove(&path);
                v.media_presence.insert(path, present);
                cx.notify();
            });
        });
        self.media_tasks.insert(key, task);
    }

    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index).cloned() else {
            return;
        };
        self.pending_reveal = None;
        self.preserve_next_reveal = None;
        self.selected = Some(index);
        window.focus(&self.focus, cx);
        if !row.entry.is_dir {
            self.preserve_next_reveal = Some(row.entry.path.clone());
            cx.emit(ExplorerEvent::OpenFile(row.entry.path));
        } else {
            self.pending_reveal = None;
            if !self.expanded[row.root_index].remove(&row.entry.path) {
                self.expanded[row.root_index].insert(row.entry.path.clone());
                self.load_directory(row.entry.path, cx);
            }
            self.expansion_changed(cx);
        }
        self.rebuild();
        cx.notify();
    }

    fn activate_path(
        &mut self,
        path: &std::path::Path,
        sticky: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.rows.iter().position(|row| row.entry.path == path) else {
            return;
        };
        let depth = self.rows[index].depth;
        self.activate(index, window, cx);
        if sticky {
            self.scroll
                .scroll_to_item_strict_with_offset(index, ScrollStrategy::Top, depth);
        }
    }

    fn render_row(&self, index: usize, sticky: bool, cx: &mut Context<Self>) -> AnyElement {
        let row_height = row_height(cx);
        let row = &self.rows[index];
        let expanded = self.expanded[row.root_index].contains(&row.entry.path);
        let name = row
            .entry
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let label_name = name.clone();
        let root_index = row.root_index;
        let label_key = (root_index, row.entry.path.clone(), sticky);
        let label_view = cx.entity().downgrade();
        let hint_bounds = std::rc::Rc::new(std::cell::Cell::new(Bounds::default()));
        let painted_hint_bounds = hint_bounds.clone();
        let show_hint = self.clipped_labels.contains(&label_key);
        let tooltip_name = name.clone();
        let tooltip_path = row.entry.path.display().to_string();
        let row_path = row.entry.path.clone();
        let right_click_path = row_path.clone();
        let menu_path = row.entry.path.clone();
        let menu_is_dir = row.entry.is_dir;
        let menu_root_id = (row.depth == 0).then_some(self.root_ids[root_index]);
        let hover_path = menu_path.clone();

        let menu_root = self.roots[root_index].clone();
        let entry = div()
            .id(row.element_id(if sticky { "sticky-row" } else { "tree-row" }))
            .relative()
            .child(
                canvas(
                    move |bounds, _, _| {
                        painted_hint_bounds.set(bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .on_hover(cx.listener(move |view, hovered: &bool, window, cx| {
                if *hovered {
                    if show_hint {
                        view.path_hint.show(
                            &tooltip_name,
                            &tooltip_path,
                            menu_is_dir,
                            hint_bounds.get(),
                            window,
                        );
                    }
                    if menu_is_dir {
                        view.check_folder_media(hover_path.clone(), cx);
                    }
                } else {
                    view.path_hint.hide_for(&tooltip_path, hint_bounds.get());
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, _, window, cx| {
                    if menu_is_dir {
                        view.check_folder_media(right_click_path.clone(), cx);
                    }
                    view.clicked_path = Some(right_click_path.clone());
                    view.clicked_root = Some(root_index);
                    cx.emit(ExplorerEvent::SelectionChanged);
                    view.selected = view.rows.iter().position(|r| {
                        r.root_index == root_index && r.entry.path == right_click_path
                    });
                    window.focus(&view.focus, cx);
                    cx.notify();
                }),
            )
            .h(px(row_height))
            .w_full()
            .rounded(px(6.))
            .bg(
                if self.clicked_root == Some(root_index)
                    && self.clicked_path.as_ref() == Some(&row.entry.path)
                {
                    super::selection_background(cx)
                } else {
                    cx.theme().sidebar
                },
            )
            .when(sticky, |row| row.occlude())
            .children((0..row.depth).map(|level| {
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(26. + level as f32 * 14.))
                    .w(px(1.))
                    .bg(cx.theme().border.opacity(0.5))
            }))
            .pl(px(if row.depth == 0 {
                0.
            } else {
                8. + row.depth as f32 * 14.
            }))
            .pr_2()
            .child(
                Button::new(row.element_id(if sticky { "sticky-entry" } else { "entry" }))
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .hover(cx.theme().foreground.opacity(0.04))
                            .active(cx.theme().foreground.opacity(0.07)),
                    )
                    .text_size(px(12.))
                    .focus_ring(false)
                    .accessibility_label(format!(
                        "{}  {name}",
                        glim_services::locale::tr(if !row.entry.is_dir {
                            "File"
                        } else if self.tasks.contains_key(&row.entry.path) {
                            "Loading folder"
                        } else if expanded {
                            "Expanded folder"
                        } else {
                            "Collapsed folder"
                        })
                    ))
                    .child(
                        div().w_full().text_left().truncate().child(
                            gpui_kit::component::h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_2()
                                .child(
                                    div()
                                        .w(px(16.))
                                        .h(px(16.))
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(if row.entry.is_dir {
                                            // Keep the same icon box during loading; replacing
                                            // a font glyph with an ellipsis makes it flicker.
                                            Icon::new(if expanded {
                                                gpui_kit::assets::IconName::ChevronDown
                                            } else {
                                                gpui_kit::assets::IconName::ChevronRight
                                            })
                                            .size(px(14.))
                                            .flex_shrink_0()
                                            .text_color(cx.theme().muted_foreground)
                                            .into_any_element()
                                        } else {
                                            super::file_icons::file_icon(&row.entry.path)
                                                .into_any_element()
                                        }),
                                )
                                .child(
                                    div()
                                        .id(row.element_id(if sticky {
                                            "sticky-label"
                                        } else {
                                            "label"
                                        }))
                                        .relative()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(name)
                                        .child(
                                            canvas(
                                                move |bounds, window, cx| {
                                                    let style = window.text_style();
                                                    let width = window
                                                        .text_system()
                                                        .layout_line(
                                                            &label_name,
                                                            style
                                                                .font_size
                                                                .to_pixels(window.rem_size()),
                                                            &[style.to_run(label_name.len())],
                                                            None,
                                                        )
                                                        .width;
                                                    let clipped =
                                                        width > bounds.size.width + px(0.5);
                                                    let key = label_key.clone();
                                                    let view = label_view.clone();
                                                    window.defer(cx, move |_, cx| {
                                                        let _ = view.update(cx, |view, cx| {
                                                            let changed = if clipped {
                                                                view.clipped_labels.insert(key)
                                                            } else {
                                                                view.clipped_labels.remove(&key)
                                                            };
                                                            if changed {
                                                                cx.notify();
                                                            }
                                                        });
                                                    });
                                                },
                                                |_, _, _, _| {},
                                            )
                                            .absolute()
                                            .size_full(),
                                        )
                                        .test_support(),
                                ),
                        ),
                    )
                    .w_full()
                    .h(px(row_height))
                    .justify_start()
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.clicked_path = Some(row_path.clone());
                        view.clicked_root = Some(root_index);
                        cx.emit(ExplorerEvent::SelectionChanged);
                        view.activate_path(&row_path, sticky, window, cx);
                    })),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, _, window, cx| {
                    cx.stop_propagation();
                    view.show_file_menu(
                        menu_path.clone(),
                        menu_root.clone(),
                        menu_root_id,
                        menu_is_dir,
                        window,
                        cx,
                    );
                }),
            );

        div()
            .w_full()
            .child(entry)
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .into_any_element()
    }

    fn show_file_menu(
        &mut self,
        path: PathBuf,
        root: PathBuf,
        root_id: Option<u64>,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use FileMenuAction::*;
        let mut items = Vec::new();
        if is_dir && self.media_presence.get(&path).copied() == Some(true) {
            items.extend([
                (Some("Play in Order"), Some(Play(false))),
                (Some("Shuffle Play"), Some(Play(true))),
                (None, None),
            ]);
        } else if !is_dir && glim_services::preview::supports(&path) {
            items.extend([(Some("Open Preview / Play"), Some(Open)), (None, None)]);
        }
        items.extend([
            (Some("Reveal in Finder"), Some(Reveal)),
            (None, None),
            (Some("Copy Relative Path"), Some(CopyRelative)),
            (Some("Copy Absolute Path"), Some(CopyAbsolute)),
            (None, None),
            (Some("Move to Trash…"), Some(Trash)),
        ]);
        if let Some(id) = root_id {
            items.extend([
                (None, None),
                (Some("Remove Folder from Workspace"), Some(RemoveFolder(id))),
            ]);
        }
        let titles = items
            .iter()
            .map(|(label, _)| (label.map(glim_services::locale::tr), true))
            .collect::<Vec<_>>();
        self.context_menu_task = None;
        let Some(menu) = super::native_menu_bridge::NativeMenu::new(&titles, window) else {
            return;
        };
        self.context_menu_task = Some(cx.spawn_in(window, async move |view, cx| {
            let selected = menu.choose(cx).await;
            let _ = view.update_in(cx, |_, _, cx| {
                if let Some(action) = selected
                    .and_then(|index| items.get(index))
                    .and_then(|(_, action)| *action)
                {
                    match action {
                        Play(shuffle) => cx.emit(ExplorerEvent::PlayFolder(path.clone(), shuffle)),
                        Open => cx.emit(ExplorerEvent::OpenFile(path.clone())),
                        Reveal => cx.reveal_path(&path),
                        CopyRelative => {
                            let relative = path.strip_prefix(&root).unwrap_or(&path);
                            let text = if relative.as_os_str().is_empty() {
                                ".".to_owned()
                            } else {
                                relative.to_string_lossy().into_owned()
                            };
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                        }
                        CopyAbsolute => cx.write_to_clipboard(ClipboardItem::new_string(
                            path.to_string_lossy().into_owned(),
                        )),
                        Trash => cx.emit(ExplorerEvent::TrashEntry(path.clone())),
                        RemoveFolder(id) => cx.emit(ExplorerEvent::RemoveFolder(id)),
                    }
                }
            });
        }));
    }

    fn show_add_folder_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.context_menu_task = None;
        let items = [(
            Some(glim_services::locale::tr("Add Folder to Workspace…")),
            true,
        )];
        let Some(menu) = super::native_menu_bridge::NativeMenu::new(&items, window) else {
            return;
        };
        self.context_menu_task = Some(cx.spawn_in(window, async move |view, cx| {
            if menu.choose(cx).await == Some(0) {
                let _ = view.update_in(cx, |_, window, cx| {
                    window.defer(cx, |window, cx| {
                        window.dispatch_action(Box::new(crate::app::actions::AddFolder), cx)
                    });
                });
            }
        }));
    }

    fn move_selection(&mut self, down: bool, cx: &mut Context<Self>) {
        if self.rows.is_empty() {
            return;
        }
        let selected = match self.selected {
            Some(i) if down => (i + 1).min(self.rows.len() - 1),
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.selected = Some(selected);
        self.scroll
            .scroll_to_item(selected, ScrollStrategy::Nearest);
        cx.notify();
    }
}

impl Render for Explorer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row_height = row_height(cx);
        let view = cx.entity().downgrade();
        gpui_kit::component::v_flex()
            .relative()
            .overflow_hidden()
            .size_full()
            .min_h_0()
            .key_context("Explorer")
            .track_focus(&self.focus)
            .on_action(cx.listener(|view, _: &Up, _, cx| view.move_selection(false, cx)))
            .on_action(cx.listener(|view, _: &Down, _, cx| view.move_selection(true, cx)))
            .on_action(cx.listener(|view, _: &OpenSelected, window, cx| {
                if let Some(index) = view.selected {
                    view.activate(index, window, cx);
                }
            }))
            .on_action(cx.listener(|view, _: &Expand, window, cx| {
                if let Some(index) = view.selected
                    && let Some(row) = view.rows.get(index)
                    && row.entry.is_dir
                    && !view.expanded[row.root_index].contains(&row.entry.path)
                {
                    view.activate(index, window, cx);
                }
            }))
            .on_action(cx.listener(|view, _: &Collapse, window, cx| {
                if let Some(index) = view.selected
                    && let Some(row) = view.rows.get(index)
                {
                    if view.expanded[row.root_index].contains(&row.entry.path) {
                        view.activate(index, window, cx);
                    } else if let Some(parent) = row.parent {
                        view.selected = Some(parent);
                        cx.notify();
                    }
                }
            }))
            .when(self.rows.is_empty(), |view| {
                view.child(
                    div()
                        .p_4()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(glim_services::locale::tr("This folder is empty.")),
                )
            })
            .child(
                uniform_list(
                    "files",
                    self.rows.len(),
                    cx.processor(|view, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|index| view.render_row(index, false, cx))
                            .collect()
                    }),
                )
                .w_full()
                .flex_1()
                .min_h_0()
                .track_scroll(&self.scroll),
            )
            .child(
                canvas(
                    move |bounds, window, cx| {
                        // Layout the pinned rows after the list, using this frame's actual
                        // scroll position. They overlay the list without changing its height.
                        let (mut pinned, blank_top) = view
                            .update(cx, |view, cx| {
                                let offset =
                                    -f32::from(view.scroll.0.borrow().base_handle.offset().y);
                                let blank_top =
                                    (view.rows.len() as f32 * row_height - offset).max(0.);
                                let pinned = sticky_rows_with_height(
                                    &view.rows,
                                    offset,
                                    f32::from(bounds.size.height),
                                    row_height,
                                )
                                .into_iter()
                                .map(|(index, y)| (y, view.render_row(index, true, cx)))
                                .collect::<Vec<_>>();
                                (pinned, blank_top)
                            })
                            .unwrap_or_default();
                        // A separate hit area below the final row: never an ancestor
                        // of file/folder menus, so both menus cannot open together.
                        let blank_view = view.clone();
                        let mut blank = if blank_top < f32::from(bounds.size.height) {
                            let mut element = div()
                                .id("files-blank-space")
                                .w(bounds.size.width)
                                .h(bounds.size.height - px(blank_top))
                                .on_mouse_down(MouseButton::Right, move |_, window, cx| {
                                    cx.stop_propagation();
                                    let _ = blank_view.update(cx, |view, cx| {
                                        view.show_add_folder_menu(window, cx)
                                    });
                                })
                                .into_any_element();
                            element.prepaint_as_root(
                                bounds.origin + point(px(0.), px(blank_top)),
                                size(bounds.size.width, bounds.size.height - px(blank_top)).into(),
                                window,
                                cx,
                            );
                            Some(element)
                        } else {
                            None
                        };
                        for (y, element) in &mut pinned {
                            element.prepaint_as_root(
                                bounds.origin + point(px(0.), px(*y)),
                                size(bounds.size.width, px(row_height)).into(),
                                window,
                                cx,
                            );
                        }
                        (pinned, blank.take())
                    },
                    |_, (pinned, blank), window, cx| {
                        if let Some(mut blank) = blank {
                            blank.paint(window, cx);
                        }
                        // A departing child folder slides behind its pinned parent.
                        for (_, mut element) in pinned.into_iter().rev() {
                            element.paint(window, cx);
                        }
                    },
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
    }
}

/// Menu actions operate on the row's path, independent of the open document.
pub(super) fn path_menu(
    menu: PopupMenu,
    path: &std::path::Path,
    root: &std::path::Path,
) -> PopupMenu {
    let reveal = path.to_path_buf();
    let relative = path.strip_prefix(root).unwrap_or(path);
    let relative = if relative.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        relative.to_string_lossy().into_owned()
    };
    let absolute = path.to_string_lossy().into_owned();
    menu.item(
        PopupMenuItem::new(glim_services::locale::tr("Reveal in Finder"))
            .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
    )
    .separator()
    .item(
        PopupMenuItem::new(glim_services::locale::tr("Copy Relative Path")).on_click(
            move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(relative.clone())),
        ),
    )
    .item(
        PopupMenuItem::new(glim_services::locale::tr("Copy Absolute Path")).on_click(
            move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(absolute.clone())),
        ),
    )
}

fn entry_id(path: &std::path::Path, part: &'static str) -> ElementId {
    ElementId::NamedChild(
        std::sync::Arc::new(ElementId::Path(path.into())),
        part.into(),
    )
}

fn row_height(_: &App) -> f32 {
    28.
}

#[cfg(test)]
fn sticky_rows(rows: &[Row], offset: f32, height: f32) -> Vec<(usize, f32)> {
    sticky_rows_with_height(rows, offset, height, 32.)
}

/// Pin only real expanded ancestor rows that have crossed their tree slot.
/// A sibling subtree pushes the previous ancestor out, rather than inheriting it.
fn sticky_rows_with_height(
    rows: &[Row],
    offset: f32,
    height: f32,
    row_height: f32,
) -> Vec<(usize, f32)> {
    let offset = offset.max(0.);
    let top = (offset / row_height).floor() as usize;
    let mut pinned = Vec::new();
    let capacity = ((height / row_height) as usize).saturating_sub(1);
    for slot in 0..capacity {
        let slot_y = slot as f32 * row_height;
        let probe = top.saturating_add(((offset % row_height + slot_y) / row_height) as usize);
        let Some(row) = rows.get(probe) else {
            break;
        };
        let mut ancestors = Vec::new();
        let mut current = if row.entry.is_dir && row.subtree_end > probe + 1 {
            Some(probe)
        } else {
            row.parent
        };
        while let Some(index) = current {
            ancestors.push(index);
            current = rows[index].parent;
        }
        ancestors.reverse();
        if pinned
            .iter()
            .enumerate()
            .any(|(level, (index, _))| ancestors.get(level) != Some(index))
        {
            break;
        }
        let Some(&index) = ancestors.get(slot) else {
            break;
        };
        let natural_y = index as f32 * row_height - offset;
        if natural_y >= slot_y {
            break;
        }
        let end_y = rows[index].subtree_end as f32 * row_height - offset;
        if end_y <= slot_y {
            break;
        }
        pinned.push((index, slot_y.min(end_y - row_height)));
    }
    pinned
}

#[cfg(test)]
mod tests {
    use super::{Row, sticky_rows};
    use glim_core::DirectoryEntry;

    fn row(name: &str, parent: Option<usize>, depth: usize, end: usize, is_dir: bool) -> Row {
        Row {
            root_index: 0,
            entry: DirectoryEntry {
                path: name.into(),
                is_dir,
                is_symlink: false,
            },
            parent,
            depth,
            subtree_end: end,
        }
    }
    fn tree() -> Vec<Row> {
        vec![
            row("a", None, 0, 8, true),
            row("a/b", Some(0), 1, 6, true),
            row("a/b/c", Some(1), 2, 6, true),
            row("a/b/c/one", Some(2), 3, 4, false),
            row("a/b/c/two", Some(2), 3, 5, false),
            row("a/b/c/three", Some(2), 3, 6, false),
            row("a/z", Some(0), 1, 8, true),
            row("a/z/file", Some(6), 2, 8, false),
            row("other", None, 0, 10, true),
            row("other/file", Some(8), 1, 10, false),
        ]
    }
    #[test]
    fn ancestors_pin_only_after_scrolling_and_keep_tree_order() {
        let rows = tree();
        assert!(sticky_rows(&rows, 0., 320.).is_empty());
        assert_eq!(sticky_rows(&rows, 8., 320.), [(0, 0.), (1, 32.), (2, 64.)]);
        assert_eq!(sticky_rows(&rows, 96., 320.), [(0, 0.), (1, 32.), (2, 64.)]);
        assert!(sticky_rows(&rows, 96., 32.).is_empty());
    }
    #[test]
    fn sibling_branches_push_out_old_ancestors() {
        let rows = tree();
        assert_eq!(
            sticky_rows(&rows, 120., 320.),
            [(0, 0.), (1, 32.), (2, 40.)]
        );
        assert_eq!(sticky_rows(&rows, 176., 320.), [(0, 0.), (6, 32.)]);
        assert_eq!(sticky_rows(&rows, 240., 320.), [(0, -16.)]);
        assert!(sticky_rows(&rows, 256., 320.).is_empty());
        assert_eq!(sticky_rows(&rows, 264., 320.), [(8, 0.)]);
    }
    #[test]
    fn collapsed_or_empty_folders_do_not_stick() {
        let rows = vec![
            row("empty", None, 0, 1, true),
            row("next", None, 0, 2, false),
        ];
        assert!(sticky_rows(&rows, 8., 320.).is_empty());
        assert!(sticky_rows(&[], 100., 320.).is_empty());
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::{Explorer, entry_id};
    use glim_core::DirectoryEntry;
    use gpui_kit::{
        AppContext, Bounds, Point, TestAppContext, WindowBounds, WindowOptions, px, size,
        test::TestWindowExt,
    };

    // These interaction fixtures need visible children; production roots start collapsed.
    fn expanded_fixture(
        root: std::path::PathBuf,
        entries: Vec<DirectoryEntry>,
        cx: &mut gpui_kit::Context<Explorer>,
    ) -> Explorer {
        let mut view = Explorer::new(root.clone(), entries, cx);
        view.expanded[0].insert(root);
        view.rebuild();
        view
    }

    #[gpui_kit::test]
    fn new_roots_expand_one_level_and_navigation_cancels_pending_reveal(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            let view = cx.new(|cx| {
                Explorer::new(
                    "/fixture".into(),
                    vec![
                        DirectoryEntry {
                            path: "/fixture/nested".into(),
                            is_dir: true,
                            is_symlink: false,
                        },
                        DirectoryEntry {
                            path: "/fixture/file.txt".into(),
                            is_dir: false,
                            is_symlink: false,
                        },
                    ],
                    cx,
                )
            });
            view.update(cx, |view, cx| {
                assert_eq!(view.rows.len(), 3);
                assert!(view.rows.iter().all(|row| row.depth <= 1));
                assert_eq!(view.expanded[0].len(), 1);
                assert!(view.expanded[0].contains(std::path::Path::new("/fixture")));
                view.add_folder(
                    "/another".into(),
                    vec![DirectoryEntry {
                        path: "/another/nested".into(),
                        is_dir: true,
                        is_symlink: false,
                    }],
                    cx,
                );
                assert_eq!(view.rows.len(), 5);
                assert_eq!(view.expanded[1].len(), 1);
                assert!(view.expanded[1].contains(std::path::Path::new("/another")));
                assert!(view.rows.iter().all(|row| row.depth <= 1));
                view.pending_reveal = Some("/fixture/file.txt".into());
                view.preserve_next_reveal = Some("/fixture/file.txt".into());
                view.clear_clicked_selection(cx);
                assert!(view.pending_reveal.is_none());
                assert!(view.preserve_next_reveal.is_none());
            });
        });
    }

    #[gpui_kit::test]
    fn file_tree_click_keeps_scroll_but_tab_reveal_moves_it(cx: &mut TestAppContext) {
        let target = std::path::PathBuf::from("/fixture/file-35.txt");
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(400.), px(500.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| {
                    cx.new(|cx| {
                        expanded_fixture(
                            "/fixture".into(),
                            (0..60)
                                .map(|i| DirectoryEntry {
                                    path: format!("/fixture/file-{i}.txt").into(),
                                    is_dir: false,
                                    is_symlink: false,
                                })
                                .collect(),
                            cx,
                        )
                    })
                },
            )
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            view.update(cx, |view, cx| {
                view.scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(gpui_kit::point(px(0.), px(-700.)));
                cx.notify();
            });
            window.render_frame(cx);
            let before = view.read(cx).scroll.0.borrow().base_handle.offset();
            window.click(entry_id(&target, "entry"), cx);
            view.update(cx, |view, cx| view.reveal_path(target.clone(), cx));
            window.render_frame(cx);
            assert_eq!(view.read(cx).scroll.0.borrow().base_handle.offset(), before);
            assert_eq!(view.read(cx).clicked_path.as_ref(), Some(&target));
            let other = std::path::PathBuf::from("/fixture/file-1.txt");
            view.update(cx, |view, cx| view.reveal_path(other.clone(), cx));
            window.render_frame(cx);
            assert_ne!(view.read(cx).scroll.0.borrow().base_handle.offset(), before);
            assert_eq!(view.read(cx).clicked_path.as_ref(), Some(&other));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn repeated_and_nested_roots_keep_independent_tree_state(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            let root = std::path::PathBuf::from("/fixture");
            let child = root.join("child");
            let entries = vec![DirectoryEntry {
                path: child.clone(),
                is_dir: true,
                is_symlink: false,
            }];
            let view = cx.new(|cx| Explorer::new(root.clone(), entries.clone(), cx));
            view.update(cx, |view, cx| {
                view.expanded[0].insert(root.clone());
                view.rebuild();
                view.add_folder(child.clone(), Vec::new(), cx);
                view.add_folder(root.clone(), entries, cx);
                assert_eq!(view.roots, vec![root.clone(), child, root.clone()]);
                let duplicate = view.rows.iter().position(|r| r.root_index == 2).unwrap();
                assert_ne!(
                    view.rows[0].element_id("entry"),
                    view.rows[duplicate].element_id("entry"),
                );
                view.selected = Some(duplicate);
                view.expanded[0].remove(&root);
                view.rebuild();
                assert!(view.expanded[2].contains(&root));
                assert_eq!(view.rows[view.selected.unwrap()].root_index, 2);
                assert_eq!(view.rows.iter().filter(|r| r.depth == 0).count(), 3);
                assert!(view.rows.iter().any(|r| r.root_index == 2 && r.depth == 1));
                assert!(!view.rows.iter().any(|r| r.root_index == 0 && r.depth == 1));
            });
        });
    }

    #[gpui_kit::test]
    fn only_clipped_file_labels_enable_tooltips(cx: &mut TestAppContext) {
        let short = std::path::PathBuf::from("/fixture/short.txt");
        let long =
            std::path::PathBuf::from(format!("/fixture/{}.txt", "long-filename-".repeat(12)));
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(300.), px(300.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| {
                    cx.new(|cx| {
                        expanded_fixture(
                            "/fixture".into(),
                            vec![short.clone(), long.clone()]
                                .into_iter()
                                .map(|path| DirectoryEntry {
                                    path,
                                    is_dir: false,
                                    is_symlink: false,
                                })
                                .collect(),
                            cx,
                        )
                    })
                },
            )
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
        cx.update(|cx| {
            let labels = &view.read(cx).clipped_labels;
            assert!(!labels.contains(&(0, short.clone(), false)));
            assert!(labels.contains(&(0, long.clone(), false)));
        });
    }

    #[gpui_kit::test]
    fn loading_arrow_and_expanded_arrow_keep_label_in_place(cx: &mut TestAppContext) {
        let path = std::path::PathBuf::from("/fixture/folder");
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(400.), px(500.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| {
                    cx.new(|cx| {
                        expanded_fixture(
                            "/fixture".into(),
                            vec![DirectoryEntry {
                                path: path.clone(),
                                is_dir: true,
                                is_symlink: false,
                            }],
                            cx,
                        )
                    })
                },
            )
            .unwrap()
        });
        let before = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                window.find(entry_id(&path, "label")).bounds()
            })
            .unwrap();
        cx.update(|cx| {
            view.update(cx, |v, cx| {
                v.tasks.insert(
                    path.clone(),
                    cx.spawn(async |_, _| std::future::pending::<()>().await),
                );
                v.expanded[0].insert(path.clone());
                cx.notify();
            })
        });
        let loading = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                window.find(entry_id(&path, "label")).bounds()
            })
            .unwrap();
        cx.update(|cx| {
            view.update(cx, |v, cx| {
                v.tasks.clear();
                cx.notify();
            })
        });
        let expanded = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                window.find(entry_id(&path, "label")).bounds()
            })
            .unwrap();
        assert_eq!(
            before, loading,
            "loading marker must not shift the filename"
        );
        assert_eq!(
            before, expanded,
            "disclosure state must not shift the filename"
        );
    }
    #[gpui_kit::test]
    fn clicking_sticky_folder_keeps_it_below_pinned_parent(cx: &mut TestAppContext) {
        let parent = std::path::PathBuf::from("/fixture/parent");
        let branch = parent.join("branch");
        let file = |path| DirectoryEntry {
            path,
            is_dir: false,
            is_symlink: false,
        };
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(400.), px(500.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| {
                    cx.new(|cx| {
                        let mut view = expanded_fixture(
                            "/fixture".into(),
                            vec![DirectoryEntry {
                                path: parent.clone(),
                                is_dir: true,
                                is_symlink: false,
                            }],
                            cx,
                        );
                        let mut siblings = (0..30)
                            .map(|i| file(parent.join(format!("before-{i}"))))
                            .collect::<Vec<_>>();
                        siblings.push(DirectoryEntry {
                            path: branch.clone(),
                            is_dir: true,
                            is_symlink: false,
                        });
                        siblings.extend((0..100).map(|i| file(parent.join(format!("after-{i}")))));
                        view.directories.insert(parent.clone(), siblings);
                        view.directories.insert(
                            branch.clone(),
                            (0..10)
                                .map(|i| file(branch.join(format!("file-{i}"))))
                                .collect(),
                        );
                        view.expanded[0].extend([parent.clone(), branch.clone()]);
                        view.rebuild();
                        view
                    })
                },
            )
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.update(|cx| {
            view.update(cx, |v, cx| {
                v.scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(gpui_kit::point(px(0.), px(-1088.)));
                cx.notify();
            })
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let before = window.find(entry_id(&branch, "sticky-label")).bounds();
            window.click(entry_id(&branch, "sticky-entry"), cx);
            window.render_frame(cx);
            let after = window.find(entry_id(&branch, "label")).bounds();
            assert_eq!(
                before.origin, after.origin,
                "collapse must preserve the clicked folder's visual slot"
            );
        })
        .unwrap();
        assert!(!cx.update(|cx| view.read(cx).expanded[0].contains(&branch)));
    }

    #[gpui_kit::test]
    fn repeated_cached_expansion_keeps_clicked_label_fixed(cx: &mut TestAppContext) {
        let parent = std::path::PathBuf::from("/fixture/folder");
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(400.), px(500.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| {
                    cx.new(|cx| {
                        let mut view = expanded_fixture(
                            "/fixture".into(),
                            vec![DirectoryEntry {
                                path: parent.clone(),
                                is_dir: true,
                                is_symlink: false,
                            }],
                            cx,
                        );
                        view.directories.insert(
                            parent.clone(),
                            (0..30)
                                .map(|i| DirectoryEntry {
                                    path: parent.join(format!("file-{i}")),
                                    is_dir: false,
                                    is_symlink: false,
                                })
                                .collect(),
                        );
                        view
                    })
                },
            )
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let original = window.find(entry_id(&parent, "label")).bounds();
            for _ in 0..20 {
                window.click(entry_id(&parent, "entry"), cx);
                window.render_frame(cx);
                assert_eq!(window.find(entry_id(&parent, "label")).bounds(), original);
            }
        })
        .unwrap();
        assert!(!cx.update(|cx| view.read(cx).expanded[0].contains(&parent)));
    }
}
