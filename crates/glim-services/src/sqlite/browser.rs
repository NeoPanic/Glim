//! Read-only, bounded SQLite queries against a private WAL-aware snapshot.
use super::Snapshot;
use crate::{limits, work::Cancellation};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, params_from_iter};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

pub struct Database {
    snapshot: Snapshot,
    pub tables: Vec<String>,
}
#[derive(Clone, Default)]
pub struct Query {
    pub table: String,
    pub columns: Vec<String>,
    pub order: Vec<String>,
    pub search: String,
    pub filter: Option<(usize, String)>,
    pub sort: Option<(usize, bool)>,
    pub page: usize,
    pub start_column: usize,
    pub seconds: u64,
}
pub struct Columns {
    pub names: Vec<String>,
    pub order: Vec<String>,
}
pub struct Page {
    pub rows: Vec<Vec<String>>,
    pub more: bool,
    pub start_column: usize,
}
pub fn open(path: &Path, cancel: &Cancellation) -> Result<Arc<Database>> {
    let snapshot = super::prepare_cancellable(path, cancel)?;
    let path = snapshot
        .database
        .as_ref()
        .ok_or_else(|| anyhow!(snapshot.information.clone()))?;
    // Recovery is allowed only on the private copy, then all browser connections are read-only.
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_millis(
        limits::SQLITE_BUSY_MILLISECONDS as u64,
    ))?;
    let token = cancel.clone();
    let started = Instant::now();
    connection.progress_handler(
        1000,
        Some(move || {
            token.check().is_err()
                || started.elapsed() > Duration::from_secs(limits::SQLITE_QUERY_SECONDS)
        }),
    );
    connection.execute_batch("PRAGMA query_only=ON;")?;
    let tables = connection
        .prepare("SELECT name FROM sqlite_schema WHERE type IN ('table','view') ORDER BY name")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(Arc::new(Database { snapshot, tables }))
}
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
impl Database {
    fn connection(&self, cancel: &Cancellation, seconds: u64) -> Result<Connection> {
        let connection = Connection::open_with_flags(
            self.snapshot.database.as_ref().unwrap(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        connection.busy_timeout(Duration::from_millis(
            limits::SQLITE_BUSY_MILLISECONDS as u64,
        ))?;
        connection.execute_batch(&format!(
            "PRAGMA query_only=ON; PRAGMA cache_size=-{};",
            limits::SQLITE_CACHE_KIB
        ))?;
        let cancel = cancel.clone();
        let started = Instant::now();
        let allowance = Duration::from_secs(seconds.max(limits::SQLITE_QUERY_SECONDS));
        connection.progress_handler(
            1000,
            Some(move || cancel.check().is_err() || started.elapsed() > allowance),
        );
        Ok(connection)
    }
    pub fn columns(&self, table: &str, cancel: &Cancellation) -> Result<Columns> {
        let connection = self.connection(cancel, limits::SQLITE_QUERY_SECONDS)?;
        let entries = connection
            .prepare("SELECT name, pk FROM pragma_table_xinfo(?) WHERE hidden != 1 ORDER BY cid")?
            .query_map([table], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let names = entries
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let mut keys = entries
            .into_iter()
            .filter(|(_, pk)| *pk > 0)
            .collect::<Vec<_>>();
        keys.sort_by_key(|(_, pk)| *pk);
        let (kind, definition): (String, String) = connection.query_row(
            "SELECT type, coalesce(sql,'') FROM sqlite_schema WHERE name=?",
            [table],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let rowid = ["_rowid_", "rowid", "oid"]
            .into_iter()
            .find(|alias| !names.iter().any(|name| name.eq_ignore_ascii_case(alias)));
        let order = if kind == "table"
            && !definition.to_ascii_uppercase().contains("WITHOUT ROWID")
            && let Some(rowid) = rowid
        {
            vec![rowid.to_string()]
        } else if !keys.is_empty() {
            keys.into_iter().map(|(name, _)| name).collect()
        } else {
            names.clone()
        };
        Ok(Columns { names, order })
    }
    fn predicate(query: &Query) -> (String, Vec<String>) {
        let mut conditions = Vec::new();
        let mut values = Vec::new();
        if !query.search.is_empty() {
            // One parameter reused across all columns avoids SQLite's variable limit.
            values.push(query.search.clone());
            fn disjunction(parts: &[String]) -> String {
                match parts.len() {
                    0 => "0".into(),
                    1 => parts[0].clone(),
                    len => format!(
                        "({} OR {})",
                        disjunction(&parts[..len / 2]),
                        disjunction(&parts[len / 2..])
                    ),
                }
            }
            conditions.push(disjunction(
                &query
                    .columns
                    .iter()
                    .map(|column| {
                        format!(
                            "instr(lower(CAST({} AS TEXT)), lower(?1)) > 0",
                            quote(column)
                        )
                    })
                    .collect::<Vec<_>>(),
            ));
        }
        if let Some((index, text)) = &query.filter
            && let Some(column) = query.columns.get(*index).filter(|_| !text.is_empty())
        {
            values.push(text.clone());
            conditions.push(format!(
                "instr(lower(CAST({} AS TEXT)), lower(?{})) > 0",
                quote(column),
                values.len()
            ));
        }
        (
            if conditions.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", conditions.join(" AND "))
            },
            values,
        )
    }
    pub fn count(&self, query: &Query, cancel: &Cancellation) -> Result<u64> {
        let connection = self.connection(cancel, query.seconds)?;
        let (predicate, values) = Self::predicate(query);
        Ok(connection.query_row(
            &format!("SELECT count(*) FROM {}{}", quote(&query.table), predicate),
            params_from_iter(values.iter()),
            |row| row.get(0),
        )?)
    }
    fn tail(query: &Query) -> (String, Vec<String>) {
        let (mut tail, values) = Self::predicate(query);
        let mut order = Vec::new();
        if let Some((index, descending)) = query.sort
            && let Some(column) = query.columns.get(index)
        {
            order.push(format!(
                "{} {}",
                quote(column),
                if descending { "DESC" } else { "ASC" }
            ));
        }
        order.extend(query.order.iter().map(|column| quote(column)));
        if !order.is_empty() {
            tail.push_str(&format!(" ORDER BY {}", order.join(",")));
        }
        (tail, values)
    }
    pub fn page(&self, query: &Query, cancel: &Cancellation) -> Result<Page> {
        let connection = self.connection(cancel, query.seconds)?;
        ensure!(!query.columns.is_empty(), "Table has no readable columns");
        let start = query.start_column.min(query.columns.len() - 1);
        let projection = query.columns.iter().skip(start).take(limits::SQLITE_COLUMN_WINDOW).map(|column| {
            let name = quote(column);
            format!("CASE WHEN typeof({name})='null' THEN 'NULL' WHEN typeof({name})='blob' THEN 'BLOB ' || hex(substr({name},1,{})) ELSE CAST(substr(CAST({name} AS BLOB),1,{}) AS TEXT) END", limits::SQLITE_BLOB_BYTES, limits::SQLITE_TEXT_BYTES)
        }).collect::<Vec<_>>().join(",");
        let (tail, values) = Self::tail(query);
        let sql = format!(
            "SELECT {projection} FROM {}{tail} LIMIT {} OFFSET {}",
            quote(&query.table),
            limits::SQLITE_PAGE_ROWS + 1,
            query.page.saturating_mul(limits::SQLITE_PAGE_ROWS)
        );
        let mut statement = connection.prepare(&sql)?;
        let count = statement.column_count();
        let mut cursor = statement.query(params_from_iter(values.iter()))?;
        let mut rows = Vec::new();
        while let Some(row) = cursor.next()? {
            cancel.check()?;
            rows.push(
                (0..count)
                    .map(|index| match row.get_ref(index) {
                        Ok(rusqlite::types::ValueRef::Text(value)) => {
                            String::from_utf8_lossy(value).into_owned()
                        }
                        _ => String::new(),
                    })
                    .collect(),
            );
        }
        let more = rows.len() > limits::SQLITE_PAGE_ROWS;
        rows.truncate(limits::SQLITE_PAGE_ROWS);
        Ok(Page {
            rows,
            more,
            start_column: start,
        })
    }
    pub fn value(
        &self,
        query: &Query,
        row: usize,
        column: usize,
        offset: usize,
        cancel: &Cancellation,
    ) -> Result<(String, bool)> {
        let connection = self.connection(cancel, query.seconds)?;
        let name = quote(
            query
                .columns
                .get(column)
                .ok_or_else(|| anyhow!("Unknown column"))?,
        );
        let (tail, values) = Self::tail(query);
        let size = limits::TEXT_PAGE_BYTES as usize;
        let sql = format!(
            "SELECT typeof({name}), substr(CAST({name} AS BLOB),{}, {}) FROM {}{tail} LIMIT 1 OFFSET {}",
            offset + 1,
            size + 1,
            quote(&query.table),
            query
                .page
                .saturating_mul(limits::SQLITE_PAGE_ROWS)
                .saturating_add(row)
        );
        let (kind, mut bytes): (String, Vec<u8>) =
            connection.query_row(&sql, params_from_iter(values.iter()), |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, Option<Vec<u8>>>(1)?.unwrap_or_default(),
                ))
            })?;
        let more = bytes.len() > size;
        bytes.truncate(size);
        Ok((
            if kind == "blob" {
                bytes.iter().map(|byte| format!("{byte:02X} ")).collect()
            } else if kind == "null" {
                "NULL".into()
            } else {
                String::from_utf8_lossy(&bytes).into_owned()
            },
            more,
        ))
    }
}
