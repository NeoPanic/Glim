//! Bounded raster previews for inline document images. Call from a background executor.
use crate::limits as reader_limits;
use anyhow::{Context, Result, ensure};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    sync::Mutex,
};

const MAX_EDGE: u32 = reader_limits::INLINE_IMAGE_PREVIEW_EDGE;
// Avoid simultaneous full-size native decode work for a document containing many images.
static PREPARATION: Mutex<()> = Mutex::new(());

pub fn from_file(path: &Path) -> Result<image::Frame> {
    from_file_cancellable(path, &crate::work::Cancellation::default())
}
pub fn from_file_cancellable(
    path: &Path,
    cancel: &crate::work::Cancellation,
) -> Result<image::Frame> {
    cancel.check()?;
    let _guard = PREPARATION
        .lock()
        .map_err(|_| anyhow::anyhow!("Image preparation was interrupted"))?;
    cancel.check()?;
    native_thumbnail(path, cancel)
}

pub fn from_bytes(bytes: &[u8]) -> Result<image::Frame> {
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(bytes)?;
    from_file(file.path())
}

fn native_thumbnail(path: &Path, cancel: &crate::work::Cancellation) -> Result<image::Frame> {
    let png = thumbnail_png_cancellable(path, MAX_EDGE, cancel)?;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(png)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(reader_limits::INLINE_IMAGE_DECODED_BYTES);
    reader.limits(limits);
    Ok(image::Frame::new(reader.decode()?.into_rgba8()))
}

pub(crate) fn thumbnail_png(path: &Path, max_edge: u32) -> Result<Vec<u8>> {
    thumbnail_png_cancellable(path, max_edge, &crate::work::Cancellation::default())
}
pub(crate) fn thumbnail_png_cancellable(
    path: &Path,
    max_edge: u32,
    cancel: &crate::work::Cancellation,
) -> Result<Vec<u8>> {
    cancel.check()?;
    let directory = tempfile::tempdir()?;
    let output = directory.path().join("preview.png");
    let mut signature = [0; 4];
    use std::io::Read;
    let qoi = std::fs::File::open(path)?.read(&mut signature)? == 4 && &signature == b"qoif";
    let converted = if qoi {
        Some(crate::qoi::prepare(path, cancel)?)
    } else {
        None
    };
    let path = converted.as_ref().map_or(path, |file| file.path());
    let dimensions = image::ImageReader::open(path)
        .ok()
        .and_then(|reader| reader.with_guessed_format().ok())
        .and_then(|reader| reader.into_dimensions().ok());
    // Specify both dimensions for thin images: sips' longest-edge option can
    // round the short edge to zero even though the source is valid.
    let mut command = Command::new("/usr/bin/sips");
    if let Some((width, height)) = dimensions {
        let longest = u64::from(width.max(height)).max(1);
        let edge = u64::from(max_edge).min(longest);
        let width = (u64::from(width) * edge / longest).max(1);
        let height = (u64::from(height) * edge / longest).max(1);
        command.args([
            "--resampleHeightWidth",
            &height.to_string(),
            &width.to_string(),
        ]);
    } else {
        command.args(["--resampleHeightWidthMax", &max_edge.to_string()]);
    }
    command
        .args(["--setProperty", "format", "png", "--out"])
        .arg(&output)
        .arg(std::path::absolute(path)?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let result =
        crate::work::run(&mut command, cancel).context("Cannot prepare inline image thumbnail")?;
    let output = if result.success() {
        output
    } else {
        // Quick Look handles vector/system image formats that ImageIO's CLI does not.
        let svg = crate::svg_preview::prepare(path, cancel)?;
        let mut command = Command::new("/usr/bin/qlmanage");
        command
            .args(["-t", "-s", &max_edge.to_string(), "-o"])
            .arg(directory.path())
            .arg(svg.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let result = crate::work::run(&mut command, cancel)?;
        ensure!(result.success(), "Cannot prepare image thumbnail");
        std::fs::read_dir(directory.path())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|ext| ext == "png"))
            .context("Thumbnail provider produced no image")?
    };
    ensure!(
        std::fs::metadata(&output)?.len()
            <= u64::from(max_edge) * u64::from(max_edge) * 4
                + reader_limits::INLINE_IMAGE_OUTPUT_OVERHEAD_BYTES,
        "Image thumbnail exceeds output budget"
    );
    let (width, height) = image::ImageReader::open(&output)?.into_dimensions()?;
    ensure!(
        width > 0 && height > 0 && width <= max_edge && height <= max_edge,
        "Invalid thumbnail dimensions"
    );
    Ok(std::fs::read(output)?)
}
