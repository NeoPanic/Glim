//! Anonymous Aptabase events. Producers only enqueue bounded, typed properties;
//! one cancellable background worker owns configuration and HTTP delivery.
use crate::work::Cancellation;
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const CONFIG_URL: &str =
    "https://raw.githubusercontent.com/glimapp/glim/main/config/telemetry.json";
// Queue/transport invariants, not reader or file-size limits. Aptabase accepts
// at most 25 events per batch and stores property strings up to 180 characters.
const QUEUE_CAPACITY: usize = 256;
const BATCH_SIZE: usize = 25;
const ERROR_KEYS: usize = 128;
const FLUSH_INTERVAL: Duration = Duration::from_secs(2);
const ERROR_INTERVAL: Duration = Duration::from_secs(60);
static CURRENT: Mutex<Option<Arc<Session>>> = Mutex::new(None);

/// Owned strings are deliberately not accepted by the public property API.
/// File names, document text, URLs and arbitrary error messages cannot be
/// accidentally passed through an ordinary instrumentation call.
pub struct Property(serde_json::Value);
pub fn text(value: &'static str) -> Property {
    Property(value.into())
}
pub fn number(value: u64) -> Property {
    Property(value.into())
}
pub fn flag(value: bool) -> Property {
    Property(value.into())
}
fn identifier(value: &str) -> Property {
    Property(serde_json::Value::String(value.chars().take(180).collect()))
}
#[derive(Serialize)]
struct Event {
    timestamp: String,
    #[serde(rename = "eventName")]
    name: &'static str,
    props: BTreeMap<&'static str, serde_json::Value>,
}
struct ErrorCount {
    at: Instant,
    suppressed: u64,
    props: BTreeMap<&'static str, serde_json::Value>,
}
#[derive(Default)]
struct Queue {
    events: VecDeque<Event>,
    errors: HashMap<String, ErrorCount>,
    recent: VecDeque<&'static str>,
    urgent: bool,
}
struct Session {
    queue: Mutex<Queue>,
    wake: mpsc::SyncSender<()>,
    accepting: AtomicBool,
    closing: AtomicBool,
    dropped: AtomicU64,
    suppressed: AtomicU64,
    sequence: AtomicU64,
    id: String,
    build_number: &'static str,
}
pub struct Worker {
    session: Arc<Session>,
    wake: mpsc::Receiver<()>,
}

pub fn start(build_number: &'static str) -> Worker {
    let now = chrono::Utc::now();
    let (tx, rx) = mpsc::sync_channel(1);
    let session = Arc::new(Session {
        queue: Mutex::new(Queue::default()),
        build_number,
        wake: tx,
        accepting: AtomicBool::new(true),
        closing: AtomicBool::new(false),
        dropped: AtomicU64::new(0),
        suppressed: AtomicU64::new(0),
        sequence: AtomicU64::new(0),
        id: format!(
            "{}{:08}",
            now.timestamp(),
            (now.timestamp_subsec_nanos() as u64 ^ std::process::id() as u64) % 100_000_000
        ),
    });
    if let Some(old) = CURRENT
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .replace(session.clone())
    {
        old.accepting.store(false, Ordering::Relaxed);
    }
    Worker { session, wake: rx }
}
pub fn stop(flush: bool) {
    if let Some(session) = CURRENT
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
    {
        session.accepting.store(false, Ordering::Relaxed);
        if flush {
            session.closing.store(true, Ordering::Relaxed);
        } else {
            session
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .events
                .clear();
        }
        let _ = session.wake.try_send(());
    }
}
pub fn track(name: &'static str, props: impl IntoIterator<Item = (&'static str, Property)>) {
    let Some(session) = CURRENT.try_lock().ok().and_then(|state| state.clone()) else {
        return;
    };
    if !session.accepting.load(Ordering::Relaxed) {
        return;
    }
    let Ok(mut queue) = session.queue.try_lock() else {
        session.dropped.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let mut props = props
        .into_iter()
        .map(|(key, value)| (key, value.0))
        .collect::<BTreeMap<_, _>>();
    props.insert("architecture", std::env::consts::ARCH.into());
    props.insert("schema_version", 3.into());
    let urgent = matches!(
        name,
        "error_logged" | "operation_failed" | "app_panicked" | "native_reader_error" | "app_exited"
    );
    if urgent
        && let Some(key) = props
            .get("error_signature")
            .and_then(|value| value.as_str())
    {
        let key = key.to_owned();
        if let Some(count) = queue.errors.get_mut(&key) {
            if count.at.elapsed() < ERROR_INTERVAL {
                count.suppressed += 1;
                session.suppressed.fetch_add(1, Ordering::Relaxed);
                return;
            }
            // Repeated occurrences are emitted by the worker, including at shutdown.
            // Keep the bucket alive until that aggregate has been drained.
            if count.suppressed > 0 {
                count.suppressed += 1;
                session.suppressed.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        if queue.errors.len() >= ERROR_KEYS
            && let Some(oldest) = queue
                .errors
                .iter()
                .min_by_key(|(_, count)| count.at)
                .map(|(key, _)| key.clone())
        {
            queue.errors.remove(&oldest);
        }
        queue.errors.insert(
            key,
            ErrorCount {
                at: Instant::now(),
                suppressed: 0,
                props: props.clone(),
            },
        );
        for (key, event) in ["previous_event_1", "previous_event_2", "previous_event_3"]
            .into_iter()
            .zip(queue.recent.iter().rev())
        {
            props.insert(key, (*event).into());
        }
    } else {
        if queue.recent.len() == 3 {
            queue.recent.pop_front();
        }
        queue.recent.push_back(name);
    }
    props.insert(
        "event_sequence",
        session.sequence.fetch_add(1, Ordering::Relaxed).into(),
    );
    if queue.events.len() == QUEUE_CAPACITY {
        queue.events.pop_front();
        session.dropped.fetch_add(1, Ordering::Relaxed);
    }
    queue.events.push_back(Event {
        timestamp: chrono::Utc::now().to_rfc3339(),
        name,
        props,
    });
    queue.urgent |= urgent;
    let _ = session.wake.try_send(());
}

/// Only recognized extension tokens leave the device; unknown extensions use
/// "other" rather than leaking a user-chosen suffix or extensionless filename.
pub fn file_properties(path: &Path) -> Vec<(&'static str, Property)> {
    const EXTENSIONS: &str = "md markdown mdx txt log csv tsv json jsonc json5 jsonl yaml yml toml xml html htm css scss less js jsx mjs cjs ts tsx rs py pyi go java kt kts c cc cpp h hpp cs swift m mm rb php sh bash zsh fish sql ini conf env gitignore make cmake dockerfile vue svelte astro dart lua r jl scala ex exs erl hs clj lisp nim zig nix tf proto graphql ipynb pdf doc docx rtf odt xls xlsx xlsm ods numbers ppt pptx odp key pages epub fb2 cbz png jpg jpeg gif svg webp avif heic tif tiff bmp ico icns raw exr hdr qoi mp3 m4a aac wav aiff flac ogg mp4 mov mkv m4v webm zip tar gz bz2 xz zst 7z rar dmg iso db db3 sqlite sqlite3 opml xmind drawio dot gv mermaid mmd d2 plantuml puml pikchr vega wavedrom tex otf ttf ttc woff woff2 obj stl bdf npy safetensors plist";
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let named = match name.as_str() {
        "dockerfile" | "containerfile" => Some("dockerfile"),
        "makefile" | "gnumakefile" => Some("make"),
        "cmakelists.txt" => Some("cmake"),
        ".gitignore" | ".dockerignore" | ".ignore" => Some("gitignore"),
        ".env" => Some("env"),
        _ if name.starts_with(".env.") => Some("env"),
        _ => None,
    };
    let suffix = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let extension = named.unwrap_or_else(|| {
        EXTENSIONS
            .split_whitespace()
            .find(|candidate| *candidate == suffix)
            .unwrap_or("other")
    });
    let family = match extension {
        "md" | "markdown" | "mdx" => "markdown",
        "pdf" => "pdf",
        "doc" | "docx" | "rtf" | "odt" | "xls" | "xlsx" | "xlsm" | "ods" | "numbers" | "ppt"
        | "pptx" | "odp" | "key" | "pages" => "office",
        "epub" | "fb2" | "cbz" => "ebook",
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "avif" | "heic" | "tif" | "tiff"
        | "bmp" | "ico" | "icns" | "raw" | "exr" | "hdr" | "qoi" => "image",
        "mp3" | "m4a" | "aac" | "wav" | "aiff" | "flac" | "ogg" => "audio",
        "mp4" | "mov" | "mkv" | "m4v" | "webm" => "video",
        "zip" | "tar" | "gz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "dmg" | "iso" => "archive",
        "csv" | "tsv" | "json" | "jsonc" | "json5" | "jsonl" | "db" | "db3" | "sqlite"
        | "sqlite3" | "npy" | "safetensors" => "data",
        "opml" | "xmind" | "drawio" | "dot" | "gv" | "mermaid" | "mmd" | "d2" | "plantuml"
        | "puml" | "pikchr" | "vega" | "wavedrom" => "diagram",
        "otf" | "ttf" | "ttc" | "woff" | "woff2" | "bdf" => "font",
        "txt" | "log" => "text",
        "other" => "other",
        _ => "source_or_config",
    };
    vec![
        ("file_extension", text(extension)),
        ("file_family", text(family)),
    ]
}
pub fn file_event(
    name: &'static str,
    path: &Path,
    extra: impl IntoIterator<Item = (&'static str, Property)>,
) {
    let mut props = file_properties(path);
    props.extend(extra);
    track(name, props);
}
pub fn result<T>(event: &'static str, path: Option<&Path>, started: Instant, outcome: &Result<T>) {
    let mut props = vec![
        (
            "result",
            text(if outcome.is_ok() { "success" } else { "failed" }),
        ),
        (
            "duration_ms",
            number(started.elapsed().as_millis().min(u64::MAX as u128) as u64),
        ),
    ];
    if let Some(path) = path {
        props.extend(file_properties(path));
    }
    track(event, props);
    if let Err(error) = outcome {
        operation_failed(event, path, error);
    }
}
pub fn native_event(
    kind: &'static str,
    action: &'static str,
    error_code: u32,
    source: &'static str,
    value: u64,
    secondary: u64,
) {
    let mut props = vec![
        ("reader_kind", text(kind)),
        ("action", text(action)),
        ("input_source", text(source)),
        ("value", number(value)),
        ("secondary_value", number(secondary)),
    ];
    let outcome = matches!(
        action,
        "ocr_completed" | "copy_completed" | "load_completed"
    );
    let observation = matches!(
        action,
        "playhead_jump_observed"
            | "playback_position_observed"
            | "playback_state_observed"
            | "mute_state_observed"
            | "volume"
            | "zoom_changed"
    );
    props.push((
        "semantic",
        text(if outcome {
            "outcome"
        } else if observation {
            "observation"
        } else {
            "invocation"
        }),
    ));
    if outcome {
        props.push((
            "result",
            text(if error_code == 0 { "success" } else { "failed" }),
        ));
    }
    match action {
        "ocr_completed" => props.extend([
            ("observation_count", number(value)),
            ("duration_ms", number(secondary)),
        ]),
        "playhead_jump_observed" | "playback_position_observed" => props.extend([
            ("position_ms", number(value)),
            ("media_duration_ms", number(secondary)),
        ]),
        "volume" => props.push(("volume_percent", number(value))),
        "mute_state_observed" => props.push(("muted", flag(value != 0))),
        "playback_state_observed" => props.push((
            "playback_state",
            text(match value {
                0 => "paused",
                1 => "waiting",
                2 => "playing",
                _ => "unknown",
            }),
        )),
        "zoom_changed" => props.push(("zoom_percent", number(value))),
        _ => {}
    }
    if error_code != 0 {
        props.extend([
            ("error_code", number(error_code as u64)),
            ("error_signature", signature(&["native", kind, action])),
        ]);
    }
    track(
        if error_code == 0 {
            "native_reader_action"
        } else {
            "native_reader_error"
        },
        props,
    );
}

fn error_kind(message: &str) -> &'static str {
    let value = message
        .chars()
        .take(4096)
        .collect::<String>()
        .to_ascii_lowercase();
    if value.contains("cancel") || value.contains("canceled") {
        "cancelled"
    } else if value.contains("permission") || value.contains("read-only") {
        "permission_denied"
    } else if value.contains("embedded resource not found") {
        "embedded_asset_missing"
    } else if value.contains("out of memory") || value.contains("allocation") {
        "memory"
    } else if value.contains("borrow") {
        "entity_borrow"
    } else if value.contains("out of bounds") || value.contains("index out") {
        "bounds"
    } else if value.contains("timed out") || value.contains("timeout") {
        "timeout"
    } else if value.contains("changed") || value.contains("conflict") {
        "conflict"
    } else if value.contains("not found")
        || value.contains("no such file")
        || value.contains("deleted")
    {
        "not_found"
    } else if value.contains("syntax")
        || value.contains("parse")
        || value.contains("invalid")
        || value.contains("utf-8")
    {
        "invalid_data"
    } else if value.contains("unsupported") || value.contains("not supported") {
        "unsupported"
    } else if value.contains("assertion") {
        "assertion"
    } else if value.contains("unwrap") {
        "unwrap"
    } else {
        "other"
    }
}
fn signature(parts: &[&str]) -> Property {
    use sha2::{Digest, Sha256};
    let digest = format!("{:x}", Sha256::digest(parts.join("|")));
    identifier(&digest[..16])
}
/// Logs keep their original detail locally. Remote diagnostics carry stable
/// compiler locations and classifications, never raw messages or stack paths.
pub fn logged_error(
    component: &str,
    file: Option<&str>,
    line: Option<u32>,
    message: &str,
    context: Option<(&Path, &'static str)>,
) {
    if CURRENT.try_lock().ok().is_none_or(|state| state.is_none()) {
        return;
    }
    let component = component
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ':' | '_' | '-')
        })
        .take(160)
        .collect::<String>();
    let site = format!(
        "{}:{}",
        file.and_then(|path| path.rsplit('/').next())
            .unwrap_or("unknown"),
        line.unwrap_or(0)
    );
    let kind = error_kind(message);
    let phase = context.map_or("unknown", |(_, phase)| phase);
    let mut props = vec![
        ("component", identifier(&component)),
        ("code_site", identifier(&site)),
        ("error_kind", text(kind)),
        ("phase", text(phase)),
        (
            "error_signature",
            signature(&[&component, &site, kind, phase]),
        ),
    ];
    if let Some((path, _)) = context {
        props.extend(file_properties(path));
    }
    if kind == "embedded_asset_missing" {
        let group = if message.contains("file-icons/") {
            "file_icons"
        } else if message.contains("branding/") {
            "branding"
        } else if message.contains("navigation/") {
            "navigation"
        } else {
            "other"
        };
        props.push(("asset_group", text(group)));
    }
    track("error_logged", props);
}
#[track_caller]
pub fn operation_failed(operation: &'static str, path: Option<&Path>, error: &anyhow::Error) {
    let location = std::panic::Location::caller();
    let site = format!(
        "{}:{}",
        location.file().rsplit('/').next().unwrap_or("unknown"),
        location.line()
    );
    let kind = error_kind(&format!("{error:#}"));
    let mut props = vec![
        ("operation", text(operation)),
        ("error_kind", text(kind)),
        ("code_site", identifier(&site)),
        (
            "error_signature",
            signature(&[
                operation,
                kind,
                &site,
                &path
                    .map(file_properties)
                    .and_then(|p| p.into_iter().find(|(k, _)| *k == "file_extension"))
                    .map(|(_, p)| p.0.to_string())
                    .unwrap_or_default(),
            ]),
        ),
    ];
    if let Some(path) = path {
        props.extend(file_properties(path));
    }
    for cause in error.chain() {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            if let Some(code) = io.raw_os_error() {
                props.push(("os_error_code", number(code.unsigned_abs() as u64)));
            }
            break;
        }
    }
    track("operation_failed", props);
}
pub fn panic_event(
    location: Option<(&str, u32)>,
    message: &str,
    context: Option<(&Path, &'static str)>,
    backtrace: &str,
) {
    let site = location
        .map(|(file, line)| format!("{}:{line}", file.rsplit('/').next().unwrap_or("unknown")))
        .unwrap_or_else(|| "unknown".into());
    let kind = error_kind(message);
    let phase = context.map_or("unknown", |(_, phase)| phase);
    let mut props = vec![
        ("code_site", identifier(&site)),
        ("error_kind", text(kind)),
        ("phase", text(phase)),
        ("error_signature", signature(&["panic", kind, &site, phase])),
    ];
    if let Some((path, _)) = context {
        props.extend(file_properties(path));
    }
    let frames = backtrace
        .lines()
        .filter_map(|line| line.trim().split_once(": ").map(|(_, symbol)| symbol))
        .filter(|symbol| {
            symbol.contains("glim::")
                || symbol.contains("glim_services::")
                || symbol.contains("gpui::")
        })
        .filter(|symbol| !symbol.contains(['/', '\\', '@', '=', '"']))
        .take(3);
    for (key, symbol) in ["stack_frame_1", "stack_frame_2", "stack_frame_3"]
        .into_iter()
        .zip(frames)
    {
        props.push((key, identifier(symbol)));
    }
    track("app_panicked", props);
}

#[derive(serde::Deserialize)]
struct Config {
    host: String,
    app_key: String,
}
impl Config {
    fn valid(&self) -> bool {
        let Ok(url) = url::Url::parse(&self.host) else {
            return false;
        };
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && self.app_key.starts_with("A-SH-")
            && self.app_key.len() > 5
            && self.app_key.len() < 128
            && self
                .app_key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }
}
fn curl() -> Command {
    let mut command = Command::new("/usr/bin/curl");
    command
        .args([
            "--disable",
            "--silent",
            "--fail",
            "--connect-timeout",
            "2",
            "--max-time",
            "5",
            "--proto",
            "=http,https",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}
fn remote_config(cancel: &Cancellation) -> Result<Config> {
    let file = tempfile::NamedTempFile::new()?;
    let status = crate::work::run(
        curl()
            .args(["--max-filesize", "16384", "--url", CONFIG_URL])
            .stdout(Stdio::from(file.reopen()?)),
        cancel,
    )?;
    ensure!(status.success(), "Unavailable configuration");
    let config: Config = serde_json::from_reader(file.reopen()?.take(16385))?;
    ensure!(config.valid(), "Invalid configuration");
    Ok(config)
}
fn configuration(cancel: &Cancellation) -> Result<Config> {
    let bundled: Config = serde_json::from_str(include_str!("../../../config/telemetry.json"))?;
    let fallback = Config {
        host: option_env!("GLIM_TELEMETRY_HOST")
            .unwrap_or(&bundled.host)
            .to_owned(),
        app_key: option_env!("GLIM_TELEMETRY_APP_KEY")
            .unwrap_or(&bundled.app_key)
            .to_owned(),
    };
    let config = remote_config(cancel).unwrap_or(fallback);
    ensure!(config.valid(), "Telemetry unconfigured");
    Ok(config)
}
fn os_version(argument: &str, cancel: &Cancellation) -> String {
    let read = || -> Result<String> {
        let output = tempfile::NamedTempFile::new()?;
        let mut command = Command::new("/usr/bin/sw_vers");
        command
            .arg(argument)
            .stdin(Stdio::null())
            .stdout(Stdio::from(output.reopen()?))
            .stderr(Stdio::null());
        ensure!(
            crate::work::run(&mut command, cancel)?.success(),
            "Unavailable OS version"
        );
        let mut version = String::new();
        output.reopen()?.take(100).read_to_string(&mut version)?;
        Ok(version
            .trim()
            .chars()
            .filter(|character| character.is_ascii_alphanumeric() || *character == '.')
            .collect())
    };
    read().unwrap_or_default()
}
// Random installation identity is unrelated to account, hardware, home or documents.
// Creation runs only in the enabled background worker. An unwritable store omits
// reach identity rather than silently treating each launch as a different person.
fn installation_id() -> Result<String> {
    let directory = crate::preferences::directory()?;
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("telemetry-installation-id");
    let read = || -> Result<String> {
        let value = std::fs::read_to_string(&path)?;
        ensure!(
            value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid telemetry identity"
        );
        Ok(value)
    };
    if path.exists() {
        return read();
    }
    let mut bytes = [0_u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let value = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    use std::io::Write as _;
    temporary.write_all(value.as_bytes())?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(&path) {
        Ok(_) => Ok(value),
        Err(_) => read(),
    }
}
impl Worker {
    pub fn run(self, cancel: &Cancellation) {
        // Telemetry transport errors must never enter the ordinary error logger
        // and recursively create more telemetry. Counters report later recovery.
        let _ = self.run_inner(cancel);
    }
    fn run_inner(&self, cancel: &Cancellation) -> Result<()> {
        cancel.check()?;
        let installation = installation_id().ok();
        let config = configuration(cancel)?;
        let os_build = os_version("-buildVersion", cancel);
        let system = serde_json::json!({
            "osName": "macOS", "osVersion": os_version("-productVersion", cancel),
            "engineName": "GPUI", "engineVersion": "0.7.1",
            "appVersion": env!("CARGO_PKG_VERSION"), "isDebug": cfg!(debug_assertions),
            "appBuildNumber": self.session.build_number,
            "sdkVersion": "glim@3", "locale": crate::locale::code(),
        });
        let mut session_id = self.session.id.clone();
        let mut session_started = Instant::now();
        let mut last_flush = Instant::now();
        let mut failures = 0_u64;
        let mut next_upload = Instant::now();
        loop {
            cancel.check()?;
            if session_started.elapsed() >= Duration::from_secs(24 * 60 * 60) {
                session_id = format!(
                    "{}{:08}",
                    chrono::Utc::now().timestamp(),
                    std::process::id() % 100_000_000
                );
                session_started = Instant::now();
            }
            if !self.session.accepting.load(Ordering::Relaxed)
                && !self.session.closing.load(Ordering::Relaxed)
            {
                return Ok(());
            }
            if Instant::now() < next_upload && !self.session.closing.load(Ordering::Relaxed) {
                let _ = self.wake.recv_timeout(
                    next_upload
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_secs(1)),
                );
                continue;
            }
            let ready = {
                let queue = self
                    .session
                    .queue
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                queue.urgent
                    || queue.events.len() >= BATCH_SIZE
                    || last_flush.elapsed() >= FLUSH_INTERVAL
                    || self.session.closing.load(Ordering::Relaxed)
            };
            if !ready {
                let _ = self.wake.recv_timeout(Duration::from_millis(100));
                continue;
            }
            let events = {
                let mut queue = self
                    .session
                    .queue
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                queue.urgent = false;
                let closing = self.session.closing.load(Ordering::Relaxed);
                let slots = QUEUE_CAPACITY.saturating_sub(queue.events.len());
                let repeats = queue
                    .errors
                    .values_mut()
                    .filter(|count| {
                        count.suppressed > 0 && (closing || count.at.elapsed() >= ERROR_INTERVAL)
                    })
                    .take(slots)
                    .map(|count| {
                        let mut props = count.props.clone();
                        props.insert("occurrences", count.suppressed.into());
                        count.suppressed = 0;
                        count.at = Instant::now();
                        Event {
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            name: "error_repeated",
                            props,
                        }
                    })
                    .collect::<Vec<_>>();
                queue.events.extend(repeats);
                let count = queue.events.len().min(BATCH_SIZE);
                queue.events.drain(..count).collect::<Vec<_>>()
            };
            let dropped = self.session.dropped.swap(0, Ordering::Relaxed);
            let suppressed = self.session.suppressed.swap(0, Ordering::Relaxed);
            let mut payload = events
                .iter()
                .map(|event| {
                    let mut props = event.props.clone();
                    props.insert("os_build", os_build.clone().into());
                    if let Some(id) = &installation {
                        props.insert("installation_id", id.clone().into());
                    }
                    serde_json::json!({
                        "timestamp": event.timestamp, "sessionId": session_id,
                        "eventName": event.name, "systemProps": system, "props": props
                    })
                })
                .collect::<Vec<_>>();
            let health_included =
                (dropped > 0 || suppressed > 0 || failures > 0) && payload.len() < BATCH_SIZE;
            if health_included {
                payload.push(serde_json::json!({"timestamp": chrono::Utc::now().to_rfc3339(), "sessionId": session_id,
                    "eventName": "telemetry_health", "systemProps": system,
                    "props": {"events_dropped": dropped, "errors_suppressed": suppressed, "upload_failures": failures}}));
            } else {
                self.session.dropped.fetch_add(dropped, Ordering::Relaxed);
                self.session
                    .suppressed
                    .fetch_add(suppressed, Ordering::Relaxed);
            }
            last_flush = Instant::now();
            if !payload.is_empty() {
                let body = tempfile::NamedTempFile::new()?;
                serde_json::to_writer(body.reopen()?, &payload)?;
                let delivered = crate::work::run(
                    curl().args([
                        "--request",
                        "POST",
                        "--header",
                        "Content-Type: application/json",
                        "--header",
                        &format!("App-Key: {}", config.app_key),
                        "--data-binary",
                        &format!("@{}", body.path().display()),
                        "--url",
                        &format!("{}/api/v0/events", config.host.trim_end_matches('/')),
                    ]),
                    cancel,
                )
                .is_ok_and(|status| status.success());
                if delivered {
                    failures = 0;
                } else {
                    failures = failures.saturating_add(1);
                    next_upload = Instant::now() + Duration::from_secs(1 << failures.min(6));
                    self.session.dropped.fetch_add(
                        events.len() as u64 + if health_included { dropped } else { 0 },
                        Ordering::Relaxed,
                    );
                    if health_included {
                        self.session
                            .suppressed
                            .fetch_add(suppressed, Ordering::Relaxed);
                    }
                }
            }
            if self.session.closing.load(Ordering::Relaxed)
                && self
                    .session
                    .queue
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .events
                    .is_empty()
            {
                return Ok(());
            }
        }
    }
}

/// Concrete reader operation, independent of repaint and background refresh.
pub fn reader_feature(
    kind: &'static str,
    action: &'static str,
    extra: impl IntoIterator<Item = (&'static str, Property)>,
) {
    let mut props = vec![("reader_kind", text(kind)), ("action", text(action))];
    props.extend(extra);
    track("reader_feature", props);
}
