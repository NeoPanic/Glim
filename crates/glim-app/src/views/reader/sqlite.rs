use glim_services::{
    limits,
    locale::tr,
    sqlite::browser::{self, Database, Query},
    work::Cancellation,
};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Sizable,
        button::Button,
        h_flex,
        input::{Input, InputEvent, InputState},
        table::{
            Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableSelection, TableState,
        },
        v_flex,
    },
    prelude::FluentBuilder,
    *,
};
use std::{ops::Range, path::PathBuf, sync::Arc};

struct Grid {
    owner: WeakEntity<SqliteReader>,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    start: usize,
    offset: usize,
    busy: bool,
}
impl TableDelegate for Grid {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len() + 1
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, index: usize, _: &App) -> Column {
        if index == 0 {
            Column::new("row", "#")
                .width(px(70.))
                .fixed_left()
                .movable(false)
        } else {
            Column::new(index.to_string(), self.columns[index - 1].clone())
                .width(px(220.))
                .sortable()
                .movable(false)
        }
    }
    fn cell_text(&self, row: usize, col: usize, _: &App) -> String {
        if col == 0 {
            return (self.offset + row + 1).to_string();
        }
        col.checked_sub(self.start + 1)
            .and_then(|index| self.rows.get(row)?.get(index))
            .cloned()
            .unwrap_or_default()
    }
    fn loading(&self, _: &App) -> bool {
        self.busy && self.rows.is_empty()
    }
    fn render_td(
        &mut self,
        row: usize,
        col: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let value = if col == 0 {
            (self.offset + row + 1).to_string()
        } else {
            col.checked_sub(self.start + 1)
                .and_then(|index| self.rows.get(row)?.get(index))
                .cloned()
                .unwrap_or_else(|| "…".into())
        };
        div()
            .truncate()
            .child(value.replace(['\r', '\n', '\t'], " "))
    }
    fn perform_sort(
        &mut self,
        col: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        if col == 0 {
            return;
        }
        let owner = self.owner.clone();
        cx.defer(move |cx| {
            let _ = owner.update(cx, |view, cx| {
                view.query.sort = match sort {
                    ColumnSort::Default => None,
                    ColumnSort::Ascending => Some((col - 1, false)),
                    ColumnSort::Descending => Some((col - 1, true)),
                };
                view.query.page = 0;
                view.reload(cx);
            });
        });
    }
    fn visible_columns_changed(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        // DataTable reports the scrollable range, excluding the fixed row-number column.
        let start = range.start;
        let end = range.end.min(self.columns.len());
        if start >= self.start && end <= self.start + limits::SQLITE_COLUMN_WINDOW {
            return;
        }
        let owner = self.owner.clone();
        cx.defer(move |cx| {
            let _ = owner.update(cx, |view, cx| {
                if view.query.start_column != start {
                    view.query.start_column = start;
                    view.load_page(cx);
                }
            });
        });
    }
}
struct Detail {
    row: usize,
    column: usize,
    offset: usize,
    more: bool,
    editor: super::source::SourceReader,
}
pub(super) struct SqliteReader {
    database: Option<Arc<Database>>,
    query: Query,
    grid: Entity<TableState<Grid>>,
    search: Entity<InputState>,
    table_search: Entity<InputState>,
    column_filter: Entity<InputState>,
    page_input: Entity<InputState>,
    filter_column: usize,
    total: Option<u64>,
    more: bool,
    error: Option<String>,
    count_error: bool,
    task: Option<Task<()>>,
    count_task: Option<Task<()>>,
    debounce: Option<Task<()>>,
    detail_task: Option<Task<()>>,
    detail: Option<Detail>,
    _subscriptions: Vec<Subscription>,
}
impl SqliteReader {
    pub fn new(path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let owner = cx.entity().downgrade();
        let grid = cx.new(|cx| {
            TableState::new(
                Grid {
                    owner,
                    columns: vec![],
                    rows: vec![],
                    start: 0,
                    offset: 0,
                    busy: true,
                },
                window,
                cx,
            )
            .cell_selectable(true)
            .row_header(false)
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(tr("Filter all rows…")));
        let table_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr("Filter tables…")));
        let column_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr("Filter selected column…")));
        let page_input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("Go to page")));
        let mut subscriptions =
            vec![cx.subscribe(&table_search, |_, _, _: &InputEvent, cx| cx.notify())];
        for input in [&search, &column_filter] {
            subscriptions.push(cx.subscribe(input, |view, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    view.debounce = Some(cx.spawn(async |view, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(250))
                            .await;
                        let _ = view.update(cx, |view, cx| {
                            view.query.search = view.search.read(cx).value().to_string();
                            view.query.filter = Some((
                                view.filter_column,
                                view.column_filter.read(cx).value().to_string(),
                            ));
                            view.query.page = 0;
                            view.reload(cx);
                        });
                    }));
                }
            }));
        }
        subscriptions.push(cx.subscribe(&page_input, |view, _, event, cx| {
            if matches!(event, InputEvent::PressEnter { .. })
                && let Ok(page) = view.page_input.read(cx).value().parse::<usize>()
            {
                view.query.page = page.saturating_sub(1).min(
                    view.last_page()
                        .unwrap_or(i64::MAX as usize / limits::SQLITE_PAGE_ROWS),
                );
                view.load_page(cx);
            }
        }));
        subscriptions.push(cx.subscribe_in(
            &grid,
            window,
            |view, _, event, window, cx| match event {
                TableEvent::SelectCell(_, col) | TableEvent::SelectColumn(col) if *col > 0 => {
                    if view.filter_column != col - 1 {
                        view.filter_column = col - 1;
                        let text = view.column_filter.read(cx).value().to_string();
                        view.query.filter = Some((view.filter_column, text.clone()));
                        if !text.is_empty() {
                            view.query.page = 0;
                            view.reload(cx);
                        }
                    }
                    cx.notify();
                }
                TableEvent::DoubleClickedCell(row, col) if *col > 0 => {
                    view.load_value(*row, col - 1, 0, window, cx)
                }
                _ => {}
            },
        ));
        let (cancel, guard) = Cancellation::task();
        let work = cx
            .background_executor()
            .spawn(async move { browser::open(&path, &cancel) });
        let task = cx.spawn(async move |view, cx| {
            let _guard = guard;
            let result = work.await;
            let _ = view.update(cx, |view, cx| {
                view.task = None;
                match result {
                    Ok(database) => {
                        view.query.table = database.tables.first().cloned().unwrap_or_default();
                        view.database = Some(database);
                        view.select_table(cx);
                    }
                    Err(error) => {
                        view.error = Some(error.to_string());
                        view.grid.update(cx, |s, cx| {
                            s.delegate_mut().busy = false;
                            cx.notify();
                        });
                    }
                }
                cx.notify();
            });
        });
        Self {
            database: None,
            query: Query::default(),
            grid,
            search,
            table_search,
            column_filter,
            page_input,
            filter_column: 0,
            total: None,
            more: false,
            error: None,
            count_error: false,
            task: Some(task),
            count_task: None,
            debounce: None,
            detail_task: None,
            detail: None,
            _subscriptions: subscriptions,
        }
    }
    fn last_page(&self) -> Option<usize> {
        self.total
            .map(|n| n.saturating_sub(1) as usize / limits::SQLITE_PAGE_ROWS)
    }
    fn select_table(&mut self, cx: &mut Context<Self>) {
        self.task = None;
        self.count_task = None;
        self.detail = None;
        self.detail_task = None;
        self.query.page = 0;
        self.query.start_column = 0;
        self.query.sort = None;
        self.query.columns.clear();
        self.total = None;
        self.more = false;
        self.error = None;
        self.count_error = false;
        self.grid.update(cx, |state, cx| {
            let grid = state.delegate_mut();
            grid.columns.clear();
            grid.rows.clear();
            grid.start = 0;
            grid.busy = true;
            state.refresh(cx);
        });
        self.query.filter = Some((0, self.column_filter.read(cx).value().to_string()));
        self.filter_column = 0;
        let Some(db) = self.database.clone() else {
            return;
        };
        if self.query.table.is_empty() {
            self.grid.update(cx, |s, cx| {
                s.delegate_mut().busy = false;
                cx.notify();
            });
            return;
        }
        let table = self.query.table.clone();
        self.debounce = None;
        self.query.search = self.search.read(cx).value().to_string();
        let (cancel, guard) = Cancellation::task();
        let work = cx
            .background_executor()
            .spawn(async move { db.columns(&table, &cancel) });
        self.task = Some(cx.spawn(async move |view, cx| {
            let _guard = guard;
            let result = work.await;
            let _ = view.update(cx, |view, cx| {
                view.task = None;
                match result {
                    Ok(columns) => {
                        view.query.order = columns.order;
                        view.query.columns = columns.names.clone();
                        view.grid.update(cx, |s, cx| {
                            let d = s.delegate_mut();
                            d.columns = columns.names;
                            d.rows.clear();
                            d.start = 0;
                            s.refresh(cx);
                        });
                        view.reload(cx);
                    }
                    Err(e) => {
                        view.error = Some(e.to_string());
                        view.grid.update(cx, |state, cx| {
                            state.delegate_mut().busy = false;
                            cx.notify();
                        });
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn reload(&mut self, cx: &mut Context<Self>) {
        // Input may change while table metadata is still loading.
        if self.query.columns.is_empty() {
            return;
        }
        self.count_task = None;
        self.total = None;
        self.count_error = false;
        self.detail = None;
        self.detail_task = None;
        self.load_page(cx);
        let Some(db) = self.database.clone() else {
            return;
        };
        let query = self.query.clone();
        let (cancel, guard) = Cancellation::task();
        let work = cx
            .background_executor()
            .spawn(async move { db.count(&query, &cancel) });
        self.count_task = Some(cx.spawn(async move |view, cx| {
            let _guard = guard;
            let result = work.await;
            let _ = view.update(cx, |view, cx| {
                view.count_task = None;
                match result {
                    Ok(total) => view.total = Some(total),
                    Err(_) => view.count_error = true,
                }
                cx.notify();
            });
        }));
    }
    fn load_page(&mut self, cx: &mut Context<Self>) {
        self.task = None;
        self.error = None;
        self.detail = None;
        self.detail_task = None;
        let Some(db) = self.database.clone() else {
            return;
        };
        if self.query.columns.is_empty() {
            return;
        }
        self.grid.update(cx, |s, cx| {
            let d = s.delegate_mut();
            d.rows.clear();
            d.busy = true;
            cx.notify();
        });
        let query = self.query.clone();
        let offset = query.page.saturating_mul(limits::SQLITE_PAGE_ROWS);
        let (cancel, guard) = Cancellation::task();
        let work = cx
            .background_executor()
            .spawn(async move { db.page(&query, &cancel) });
        self.task = Some(cx.spawn(async move |view, cx| {
            let _guard = guard;
            let result = work.await;
            let _ = view.update(cx, |view, cx| {
                view.task = None;
                view.grid.update(cx, |s, cx| {
                    let d = s.delegate_mut();
                    d.busy = false;
                    match result {
                        Ok(page) => {
                            d.rows = page.rows;
                            d.start = page.start_column;
                            d.offset = offset;
                            view.more = page.more;
                        }
                        Err(error) => view.error = Some(error.to_string()),
                    }
                    cx.notify();
                });
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn load_value(
        &mut self,
        row: usize,
        column: usize,
        offset: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.detail_task = None;
        let Some(db) = self.database.clone() else {
            return;
        };
        let query = self.query.clone();
        let (cancel, guard) = Cancellation::task();
        let work = cx
            .background_executor()
            .spawn(async move { db.value(&query, row, column, offset, &cancel) });
        self.detail_task = Some(cx.spawn_in(window, async move |view, cx| {
            let _guard = guard;
            let result = work.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.detail_task = None;
                match result {
                    Ok((text, more)) => {
                        view.detail = Some(Detail {
                            row,
                            column,
                            offset,
                            more,
                            editor: super::source::SourceReader::new(
                                &text,
                                "text",
                                true,
                                vec![],
                                ScrollHandle::new(),
                                window,
                                cx,
                            ),
                        })
                    }
                    Err(error) => view.error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
    }
}
impl Render for SqliteReader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let table_filter = self.table_search.read(cx).value().to_lowercase();
        let tables = self
            .database
            .as_ref()
            .map(|d| d.tables.clone())
            .unwrap_or_default();
        let busy = self.task.is_some();
        let last = self.last_page();
        let rows = self.grid.read(cx).delegate().rows.len();
        let mut main = v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .child(
                h_flex()
                    .gap_2()
                    .p_2()
                    .child(div().flex_1().child(Input::new(&self.search).small()))
                    .child(
                        div().max_w(px(180.)).truncate().child(
                            self.query
                                .columns
                                .get(self.filter_column)
                                .cloned()
                                .unwrap_or_default(),
                        ),
                    )
                    .child(
                        div()
                            .w(px(160.))
                            .child(Input::new(&self.column_filter).small()),
                    ),
            )
            .when_some(self.error.clone(), |el, error| {
                el.child(div().px_2().text_color(cx.theme().danger).child(error))
            })
            .when(self.error.is_some() || self.count_error, |el| {
                el.child(
                    Button::new("sqlite-retry")
                        .label(tr("Retry with more time"))
                        .disabled(self.database.is_none())
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.query.seconds = v
                                .query
                                .seconds
                                .max(limits::SQLITE_QUERY_SECONDS)
                                .saturating_mul(3);
                            if v.query.columns.is_empty() {
                                v.select_table(cx);
                            } else {
                                v.reload(cx);
                            }
                        })),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(DataTable::new(&self.grid).stripe(true).small()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .p_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("first")
                            .label("|‹")
                            .disabled(busy || self.query.page == 0)
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.query.page = 0;
                                v.load_page(cx);
                            })),
                    )
                    .child(
                        Button::new("previous")
                            .label("‹")
                            .disabled(busy || self.query.page == 0)
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.query.page = v.query.page.saturating_sub(1);
                                v.load_page(cx);
                            })),
                    )
                    .child(div().w(px(80.)).child(Input::new(&self.page_input).small()))
                    .child(div().child(format!(
                            "{} / {}",
                            self.query.page + 1,
                            last.map(|n| (n + 1).to_string())
                                .unwrap_or_else(|| "…".into())
                        )))
                    .child(
                        Button::new("next")
                            .label("›")
                            .disabled(busy || !self.more)
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.query.page += 1;
                                v.load_page(cx);
                            })),
                    )
                    .child(
                        Button::new("last")
                            .label("›|")
                            .disabled(busy || last.is_none() || last == Some(self.query.page))
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.query.page = v.last_page().unwrap_or(v.query.page);
                                v.load_page(cx);
                            })),
                    )
                    .child(div().flex_1().text_sm().child(format!(
                            "{}: {} · {}–{}",
                            tr("Rows"),
                            self.total
                                .map(|n| n.to_string())
                                .unwrap_or_else(|| "…".into()),
                            if rows == 0 {
                                0
                            } else {
                                self.query.page.saturating_mul(limits::SQLITE_PAGE_ROWS) + 1
                            },
                            self.query.page.saturating_mul(limits::SQLITE_PAGE_ROWS) + rows
                        ))),
            );
        if let Some(detail) = self.detail.as_mut() {
            let (row, column, offset, more) =
                (detail.row, detail.column, detail.offset, detail.more);
            let editor = detail.editor.render(cx);
            main = main.child(
                v_flex()
                    .h(px(260.))
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Button::new("value-close").label(tr("Close")).on_click(
                                cx.listener(|v, _, _, cx| {
                                    v.detail = None;
                                    v.detail_task = None;
                                    cx.notify();
                                }),
                            ))
                            .child(
                                Button::new("value-prev")
                                    .label(tr("Previous page"))
                                    .disabled(offset == 0)
                                    .on_click(cx.listener(move |v, _, window, cx| {
                                        v.load_value(
                                            row,
                                            column,
                                            offset.saturating_sub(limits::TEXT_PAGE_BYTES as usize),
                                            window,
                                            cx,
                                        )
                                    })),
                            )
                            .child(
                                Button::new("value-next")
                                    .label(tr("Next page"))
                                    .disabled(!more)
                                    .on_click(cx.listener(move |v, _, window, cx| {
                                        v.load_value(
                                            row,
                                            column,
                                            offset + limits::TEXT_PAGE_BYTES as usize,
                                            window,
                                            cx,
                                        )
                                    })),
                            ),
                    )
                    .child(div().flex_1().min_h_0().child(editor)),
            );
        }
        h_flex()
            .on_action(
                cx.listener(|view, _: &gpui_kit::component::input::Copy, _, cx| {
                    let state = view.grid.read(cx);
                    let grid = state.delegate();
                    let value = match state.selection() {
                        TableSelection::Cell(row, column) => Some(grid.cell_text(row, column, cx)),
                        TableSelection::Row(row) => grid.rows.get(row).map(|cells| {
                            cells
                                .iter()
                                .map(|cell| {
                                    if cell.contains(['\t', '\n', '\r', '"']) {
                                        format!("\"{}\"", cell.replace('"', "\"\""))
                                    } else {
                                        cell.clone()
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join("\t")
                        }),
                        _ => None,
                    };
                    if let Some(value) = value {
                        cx.write_to_clipboard(ClipboardItem::new_string(value));
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .size_full()
            .min_h_0()
            .child(
                v_flex()
                    .w(px(210.))
                    .h_full()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .child(div().p_2().child(Input::new(&self.table_search).small()))
                    .child(
                        div()
                            .id("sqlite-tables")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children(
                                tables
                                    .into_iter()
                                    .filter(|name| name.to_lowercase().contains(&table_filter))
                                    .enumerate()
                                    .map(|(index, name)| {
                                        let selected = name == self.query.table;
                                        div()
                                            .id(("sqlite-table", index))
                                            .px_3()
                                            .py_1()
                                            .cursor_pointer()
                                            .when(selected, |el| el.bg(cx.theme().secondary))
                                            .child(name.clone())
                                            .on_click(cx.listener(move |v, _, _, cx| {
                                                if v.query.table != name {
                                                    v.query.table = name.clone();
                                                    v.select_table(cx);
                                                }
                                            }))
                                    }),
                            ),
                    ),
            )
            .child(main)
    }
}
