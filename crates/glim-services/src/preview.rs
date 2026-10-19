//! Prepare native preview data off the UI thread; launch the settings helper only.
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

pub fn is_media(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "mp3"
                | "mp4"
                | "mkv"
                | "m4a"
                | "m4v"
                | "mov"
                | "aac"
                | "wav"
                | "aif"
                | "aiff"
                | "caf"
                | "flac"
        )
    })
}
pub fn folder_has_media(path: &Path) -> Result<bool> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if is_media(&entry.path()) && entry.file_type()?.is_file() {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn is_image(path: &Path) -> bool {
    crate::media::supports(path)
        && !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("gif"))
}

pub fn is_font(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "ttf" | "otf" | "ttc" | "otc" | "dfont"
        )
    })
}

pub fn is_office(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "rtf" | "key" | "pages" | "numbers"
        )
    })
}

pub fn is_iwork(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "key" | "pages" | "numbers"))
}

/// `.key` is also used by plain-text cryptographic keys. Only document packages
/// and ZIP containers should be handed to Keynote's Quick Look provider.
pub fn can_open(path: &Path) -> bool {
    if !supports(path) && !crate::sqlite::has_header(path) {
        return false;
    }
    if path.is_dir() {
        return is_iwork(path);
    }
    if !path.is_file() {
        return false;
    }
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("key"))
    {
        use std::io::Read;
        let mut prefix = [0; 4];
        return std::fs::File::open(path)
            .and_then(|mut file| file.read_exact(&mut prefix))
            .is_ok()
            && prefix == *b"PK\x03\x04";
    }
    true
}

pub fn supports(path: &Path) -> bool {
    crate::sqlite::supports(path)
        || is_media(path)
        || is_image(path)
        || is_font(path)
        || is_office(path)
        || path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("pdf"))
}

fn playlist(path: &Path, folder: bool, cancel: &crate::work::Cancellation) -> Result<Vec<PathBuf>> {
    if !folder {
        ensure!(can_open(path), "Unsupported or missing preview file");
        return Ok(vec![path.to_path_buf()]);
    }
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(path)? {
        cancel.check()?;
        let entry = entry?;
        if is_media(&entry.path()) && entry.file_type()?.is_file() {
            paths.push(entry.path());
        }
    }
    paths.sort();
    ensure!(
        !paths.is_empty(),
        "This folder contains no supported audio or video files"
    );
    Ok(paths)
}

pub struct PreparedPreview {
    pub image_stamp: Option<(u64, Option<std::time::SystemTime>)>,
    pub configuration: Vec<u8>,
    // The native SQLite view must close before its private snapshot is removed.
    _sqlite: Option<crate::sqlite::Snapshot>,
    _spreadsheet: Option<crate::spreadsheet::Spreadsheet>,
    _media_directory: Option<tempfile::TempDir>,
    _image: Option<tempfile::NamedTempFile>,
}

impl PreparedPreview {
    pub fn is_spreadsheet(&self) -> bool {
        self._spreadsheet.is_some()
    }
}

pub fn prepare(
    path: &Path,
    folder: bool,
    shuffle: bool,
    autoplay: bool,
) -> Result<PreparedPreview> {
    prepare_cancellable(
        path,
        folder,
        shuffle,
        autoplay,
        &crate::work::Cancellation::default(),
    )
}
pub fn prepare_cancellable(
    path: &Path,
    folder: bool,
    shuffle: bool,
    autoplay: bool,
    cancel: &crate::work::Cancellation,
) -> Result<PreparedPreview> {
    let image_stamp = if !folder && is_image(path) {
        let metadata = std::fs::metadata(path)?;
        Some((metadata.len(), metadata.modified().ok()))
    } else {
        None
    };
    let paths = playlist(path, folder, cancel)?;
    let files = paths
        .iter()
        .map(|path| path.to_str().context("Native preview requires UTF-8 paths"))
        .collect::<Result<Vec<_>>>()?;
    let mut matroska = Vec::new();
    for path in paths.iter().filter(|p| is_media(p)) {
        cancel.check()?;
        match crate::matroska::has_header(path) {
            Ok(true) => matroska.push(path.to_str().context("Invalid media path")?.to_owned()),
            Err(error) if !folder => return Err(error),
            _ => {}
        }
    }
    let media_directory = if matroska.is_empty() {
        None
    } else {
        Some(tempfile::tempdir()?)
    };
    let sqlite = if !folder && (crate::sqlite::supports(path) || crate::sqlite::has_header(path)) {
        Some(crate::sqlite::prepare_cancellable(path, cancel)?)
    } else {
        None
    };
    let spreadsheet = if !folder
        && path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("xls") || s.eq_ignore_ascii_case("xlsx"))
    {
        crate::spreadsheet::prepare(path, cancel)?
    } else {
        None
    };
    let mut image_size = None;
    let image = if !folder
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("qoi"))
    {
        let (file, size) = crate::qoi::prepare_with_size(path, cancel)?;
        image_size = Some(size);
        Some(file)
    } else if !folder
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
    {
        Some(crate::svg_preview::prepare(path, cancel)?)
    } else {
        None
    };
    let mut configuration = serde_json::json!({"files": files, "shuffle": shuffle, "autoplay": autoplay, "office": !folder && is_office(path), "font": !folder && is_font(path),
        "sqlite": spreadsheet.as_ref().map(|s| s.database.as_path()).or_else(|| sqlite.as_ref().and_then(|s| s.database.as_deref())).and_then(|p| p.to_str()),
        "sqliteInfo": spreadsheet.as_ref().map(|s| &s.information).or_else(|| sqlite.as_ref().map(|s| &s.information))});
    configuration["image"] = serde_json::json!(!folder && is_image(path));
    configuration["imageWidth"] = serde_json::json!(image_size.map(|size| size.0));
    configuration["imageHeight"] = serde_json::json!(image_size.map(|size| size.1));
    configuration["imagePath"] =
        serde_json::json!(image.as_ref().and_then(|image| image.path().to_str()));
    configuration["matroskaFiles"] = serde_json::json!(matroska);
    if let Some(directory) = &media_directory {
        configuration["mediaDirectory"] = serde_json::json!(
            directory
                .path()
                .to_str()
                .context("Invalid media directory")?
        );
        configuration["mediaHelper"] = serde_json::json!(
            std::env::current_exe()?
                .to_str()
                .context("Invalid helper path")?
        );
    }
    configuration["language"] = serde_json::json!(crate::locale::code());
    configuration["localizations"] = crate::locale::translations();
    Ok(PreparedPreview {
        image_stamp,
        configuration: serde_json::to_vec(&configuration)?,
        _sqlite: sqlite,
        _spreadsheet: spreadsheet,
        _media_directory: media_directory,
        _image: image,
    })
}

pub fn prepare_matroska_file(source: &Path, destination: &Path) -> Result<()> {
    let directory = destination
        .parent()
        .context("Media destination has no parent")?;
    ensure!(!destination.exists(), "Media destination already exists");
    let file = crate::matroska::prepare(source, directory, &crate::work::Cancellation::default())?;
    file.persist_noclobber(destination)
        .map_err(|error| error.error)?;
    Ok(())
}

pub fn open_associations(cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Result<()> {
    let executable = std::env::current_exe()?;
    let bundle = executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .context("Cannot locate Glim.app")?;
    ensure!(
        bundle.extension().is_some_and(|s| s == "app")
            && bundle.join("Contents/Info.plist").is_file(),
        "Install Glim.app before configuring default applications"
    );
    launch(
        serde_json::json!({"files": [], "shuffle": false, "associations": bundle.to_str().context("Invalid application path")?}),
        cancelled,
    )
}

fn launch(
    mut configuration: serde_json::Value,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<()> {
    configuration["language"] = serde_json::json!(crate::locale::code());
    configuration["localizations"] = crate::locale::translations();

    use std::{
        os::unix::fs::PermissionsExt,
        process::{Command, Stdio},
    };
    let directory = tempfile::tempdir()?;
    // Only default-file-type settings use an independent helper application.
    let contents = directory.path().join("Glim Settings.app/Contents");
    let binaries = contents.join("MacOS");
    std::fs::create_dir_all(&binaries)?;
    let resources = contents.join("Resources");
    std::fs::create_dir_all(&resources)?;
    std::fs::write(
        resources.join("Glim.icns"),
        include_bytes!("../../../assets/macos/Glim.icns"),
    )?;
    let info = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Glim Settings</string>
<key>CFBundleDisplayName</key><string>Glim Settings</string>
<key>CFBundleIdentifier</key><string>io.github.jony4.glim.settings</string>
<key>CFBundleExecutable</key><string>glim-preview</string>
<key>CFBundleIconFile</key><string>Glim</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>"#;
    std::fs::write(contents.join("Info.plist"), info)?;
    let executable = binaries.join("glim-preview");
    std::fs::write(
        &executable,
        include_bytes!(concat!(env!("OUT_DIR"), "/glim-preview")),
    )?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
    let manifest = directory.path().join("request.json");
    std::fs::write(&manifest, serde_json::to_vec(&configuration)?)?;
    // Keep the temporary executable and manifest alive until its window closes.
    let mut child = Command::new(executable)
        .arg(manifest)
        .env_remove("__CFBundleIdentifier")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    loop {
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            ensure!(
                status.success(),
                crate::locale::tr("The native preview could not open this file")
            );
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(())
}
