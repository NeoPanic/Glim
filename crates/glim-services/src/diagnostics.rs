//! Bounded asynchronous diagnostic logging. Initialize before starting the UI.
use chrono::Local;
use std::{
    backtrace::Backtrace,
    cell::RefCell,
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, mpsc},
};

const MAX_FILES: usize = 14;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_MESSAGE: usize = 64 * 1024;
static SENDER: OnceLock<mpsc::SyncSender<Entry>> = OnceLock::new();
static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
static FAILED: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();
thread_local! {
    static CONTEXT: RefCell<Option<(PathBuf, &'static str)>> = const { RefCell::new(None) };
    static RECOVER: RefCell<bool> = const { RefCell::new(false) };
}
struct Entry {
    time: chrono::DateTime<Local>,
    message: String,
    ack: Option<mpsc::SyncSender<()>>,
}

pub struct FileScope(Option<(PathBuf, &'static str)>);
impl FileScope {
    pub fn enter(path: &Path, phase: &'static str) -> Self {
        Self(CONTEXT.with(|context| context.replace(Some((path.to_path_buf(), phase)))))
    }
}
impl Drop for FileScope {
    fn drop(&mut self) {
        CONTEXT.with(|context| {
            context.replace(self.0.take());
        });
    }
}
pub fn failure(path: &Path) -> Option<String> {
    FAILED.get()?.lock().ok()?.get(path).cloned()
}
pub fn retry(path: &Path) {
    if let Some(failed) = FAILED.get() {
        failed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(path);
    }
    event("INFO", &format!("retry file={}", path.display()));
}
/// Only a panic attributed to a protected file may discard a GPUI frame.
pub fn take_render_failure() -> bool {
    let recovered = RECOVER.with(|recover| recover.replace(false));
    if recovered {
        event("ERROR", "discarded failed file frame; scheduling fallback");
    }
    recovered
}
pub fn directory() -> PathBuf {
    DIRECTORY.get().cloned().unwrap_or_default()
}
pub fn event(level: &str, message: &str) {
    enqueue(
        format!("{level} thread={:?} {message}", std::thread::current().id()),
        false,
    );
}
fn enqueue(mut message: String, durable: bool) {
    if message.len() > MAX_MESSAGE {
        let mut end = MAX_MESSAGE;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push_str(" [truncated]");
    }
    let Some(sender) = SENDER.get() else {
        return;
    };
    let (ack, received) = mpsc::sync_channel(0);
    let entry = Entry {
        time: Local::now(),
        message,
        ack: durable.then_some(ack),
    };
    if durable {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut pending = entry;
        loop {
            match sender.try_send(pending) {
                Ok(()) => {
                    let _ = received.recv_timeout(
                        deadline.saturating_duration_since(std::time::Instant::now()),
                    );
                    break;
                }
                Err(mpsc::TrySendError::Full(entry)) if std::time::Instant::now() < deadline => {
                    pending = entry;
                    std::thread::park_timeout(std::time::Duration::from_millis(1));
                }
                Err(_) => break,
            }
        }
    } else {
        // Keep rendering non-blocking, with bounded queued memory.
        let _ = sender.try_send(entry);
    }
}
struct Logger;
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            if record.level() == log::Level::Error {
                let context = CONTEXT.with(|context| context.borrow().clone());
                crate::telemetry::logged_error(
                    record.target(),
                    record.file(),
                    record.line(),
                    &record.args().to_string(),
                    context
                        .as_ref()
                        .map(|(path, phase)| (path.as_path(), *phase)),
                );
            }
            event(
                record.level().as_str(),
                &format!("{} {}", record.target(), record.args()),
            );
        }
    }
    fn flush(&self) {
        enqueue("INFO flush".into(), true);
    }
}

pub fn initialize() -> io::Result<()> {
    let directory =
        PathBuf::from(std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME missing"))?)
            .join("Library/Logs/Glim");
    fs::create_dir_all(&directory)?;
    prune(&directory)?;
    let _ = DIRECTORY.set(directory.clone());
    let _ = FAILED.set(Mutex::new(HashMap::new()));
    let (sender, receiver) = mpsc::sync_channel::<Entry>(256);
    std::thread::Builder::new()
        .name("glim-diagnostics".into())
        .spawn(move || {
            let mut output: Option<(String, File)> = None;
            while let Ok(entry) = receiver.recv() {
                if let Err(error) = write_entry(&directory, &mut output, &entry) {
                    eprintln!("Cannot write Glim diagnostics: {error}");
                }
                if let Some(ack) = entry.ack {
                    let _ = ack.send(());
                }
            }
        })?;
    let _ = SENDER.set(sender);
    let _ = log::set_logger(&Logger);
    log::set_max_level(log::LevelFilter::Info);
    std::panic::set_hook(Box::new(|info| {
        RECOVER.with(|recover| {
            recover.replace(false);
        });
        let context = CONTEXT.with(|context| context.borrow().clone());
        let summary = info.to_string();
        if let Some((path, phase)) = &context {
            if let Some(failed) = FAILED.get() {
                failed
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(path.clone(), summary.clone());
            }
            if matches!(*phase, "render" | "layout" | "prepaint" | "paint") {
                RECOVER.with(|recover| {
                    recover.replace(true);
                });
            }
        }
        let backtrace = Backtrace::force_capture().to_string();
        crate::telemetry::panic_event(
            info.location()
                .map(|location| (location.file(), location.line())),
            &summary,
            context
                .as_ref()
                .map(|(path, phase)| (path.as_path(), *phase)),
            &backtrace,
        );
        enqueue(
            format!(
                "PANIC thread={:?} name={:?} context={context:?}\n{summary}\n{}",
                std::thread::current().id(),
                std::thread::current().name(),
                backtrace
            ),
            true,
        );
        eprintln!("{summary}");
    }));
    enqueue(
        format!(
            "INFO startup version={} pid={} arch={} logs={} max_files={MAX_FILES} max_bytes_per_file={MAX_BYTES}",
            env!("CARGO_PKG_VERSION"),
            std::process::id(),
            std::env::consts::ARCH,
            self::directory().display()
        ),
        true,
    );
    Ok(())
}
fn write_entry(
    directory: &Path,
    output: &mut Option<(String, File)>,
    entry: &Entry,
) -> io::Result<()> {
    let day = entry.time.format("%Y-%m-%d").to_string();
    let rotate = match output {
        Some((date, file)) => {
            date != &day || file.metadata()?.len() + entry.message.len() as u64 + 128 > MAX_BYTES
        }
        None => true,
    };
    if rotate {
        let mut index = 0;
        loop {
            let path = directory.join(format!("glim-{day}-{index:06}.log"));
            if !path.exists()
                || path.metadata()?.len() + entry.message.len() as u64 + 128 <= MAX_BYTES
            {
                let file = OpenOptions::new().create(true).append(true).open(path)?;
                *output = Some((day, file));
                break;
            }
            index += 1;
        }
        prune(directory)?;
    }
    let file = &mut output.as_mut().expect("opened above").1;
    writeln!(file, "{} {}", entry.time.to_rfc3339(), entry.message)?;
    file.flush()?;
    if entry.ack.is_some() {
        file.sync_data()?;
    }
    Ok(())
}
fn prune(directory: &Path) -> io::Result<()> {
    let mut files = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("glim-") && n.ends_with(".log"))
        })
        .collect::<Vec<_>>();
    files.sort();
    let excess = files.len().saturating_sub(MAX_FILES);
    for path in files.into_iter().take(excess) {
        fs::remove_file(path)?;
    }
    Ok(())
}
