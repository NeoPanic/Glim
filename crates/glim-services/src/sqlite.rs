//! Snapshot SQLite families before previewing, so WAL recovery never touches source files.
use anyhow::{Result, ensure};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

pub fn supports(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.ends_with("-wal")
        || name.ends_with("-shm")
        || name.ends_with("-journal")
        || matches!(
            path.extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
            "db" | "db3" | "sqlite" | "sqlite3" | "s3db" | "wal" | "shm"
        )
}
pub fn has_header(path: &Path) -> bool {
    let mut header = [0; 16];
    fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut header))
        .is_ok()
        && &header == b"SQLite format 3\0"
}
fn associated(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
fn main_path(path: &Path) -> PathBuf {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    for suffix in ["-wal", "-shm", "-journal", ".wal", ".shm"] {
        if let Some(base) = name.strip_suffix(suffix) {
            return path.with_file_name(base);
        }
    }
    path.to_path_buf()
}
#[derive(PartialEq)]
struct Stamp {
    size: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
}
fn stamp(path: &Path) -> Result<Option<Stamp>> {
    match fs::metadata(path) {
        Ok(m) => {
            ensure!(m.is_file(), "Not a database file: {}", path.display());
            Ok(Some(Stamp {
                size: m.len(),
                modified: m.modified().ok(),
                created: m.created().ok(),
            }))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub struct Snapshot {
    pub database: Option<PathBuf>,
    pub information: String,
    _directory: Option<tempfile::TempDir>,
}
pub fn prepare(path: &Path) -> Result<Snapshot> {
    prepare_cancellable(path, &crate::work::Cancellation::default())
}
pub fn prepare_cancellable(path: &Path, cancel: &crate::work::Cancellation) -> Result<Snapshot> {
    let main = main_path(path);
    if main != path && !main.is_file() {
        let mut info = format!(
            "{}\n\nRelated database: {}\nThe main database is missing. A sidecar does not contain a complete database; keep it beside its matching database and reopen.\n",
            path.file_name().unwrap_or_default().to_string_lossy(),
            main.display()
        );
        let size = fs::metadata(path)?.len();
        info.push_str(&format!("\nFile size: {size} bytes\n"));
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.ends_with("wal"))
        {
            let mut header = [0u8; 32];
            if fs::File::open(path)?.read_exact(&mut header).is_ok() {
                let magic = u32::from_be_bytes(header[0..4].try_into()?);
                if matches!(magic, 0x377f0682 | 0x377f0683) {
                    let page_size = u32::from_be_bytes(header[8..12].try_into()?);
                    if page_size > 0 {
                        info.push_str(&format!("WAL page size: {page_size} bytes\nFrame slots: {}\nFrames are not necessarily committed; the matching database is required.\n", size.saturating_sub(32) / (u64::from(page_size) + 24)));
                    }
                }
            }
        }
        return Ok(Snapshot {
            database: None,
            information: info,
            _directory: None,
        });
    }
    ensure!(
        has_header(&main),
        "Not a standard SQLite database (encrypted databases require a compatible reader)"
    );
    let files = [
        main.clone(),
        associated(&main, "-wal"),
        associated(&main, "-journal"),
    ];
    for _ in 0..3 {
        let before = files.iter().map(|p| stamp(p)).collect::<Result<Vec<_>>>()?;
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("snapshot.sqlite");
        for ((source, state), suffix) in files.iter().zip(&before).zip(["", "-wal", "-journal"]) {
            if let Some(state) = state {
                let mut input = fs::File::open(source)?.take(state.size + 1);
                let mut output = fs::File::create(associated(&database, suffix))?;
                let mut buffer = vec![0; crate::limits::IO_BUFFER_BYTES];
                let mut copied = 0;
                loop {
                    cancel.check()?;
                    let count = input.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    copied += count as u64;
                    ensure!(
                        copied <= state.size,
                        "Database changed while creating its preview; retry"
                    );
                    output.write_all(&buffer[..count])?;
                }
                ensure!(
                    copied == state.size,
                    "Database changed while creating its preview; retry"
                );
            }
        }
        let after = files.iter().map(|p| stamp(p)).collect::<Result<Vec<_>>>()?;
        if before == after {
            let wal = before[1].as_ref().is_some_and(|s| s.size > 0);
            return Ok(Snapshot {
                database: Some(database),
                information: format!(
                    "Read-only snapshot{} · Reopen to refresh. Source files are unchanged.",
                    if wal { " including WAL" } else { "" }
                ),
                _directory: Some(directory),
            });
        }
    }
    anyhow::bail!("The database is changing. Pause its writer or open a stable backup.")
}
