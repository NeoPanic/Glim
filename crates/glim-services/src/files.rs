use crate::limits as reader_limits;
use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, Result, ensure};
use glim_core::{Document, DocumentKind};

/// In-memory editing budget; larger files use editable streaming-save pages.
pub const MAX_DOCUMENT_BYTES: u64 = reader_limits::TEXT_EDITABLE_BYTES;

pub(crate) static SAVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn read_document(path: &Path) -> Result<Document> {
    ensure!(path.is_file(), "Not a regular file: {}", path.display());
    let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    ensure!(
        file.metadata()?.is_file(),
        "Not a regular file: {}",
        path.display()
    );
    // Limit the read itself, so a file growing after metadata inspection is bounded too.
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1).read_to_end(&mut bytes)?;
    reader_limits::preview_budget(
        bytes.len() as u64 <= MAX_DOCUMENT_BYTES,
        "Use editable pages for this document",
    )?;
    ensure!(
        !bytes.contains(&0),
        "Binary files are not supported: {}",
        path.display()
    );
    let text = String::from_utf8(bytes)
        .with_context(|| format!("File is not UTF-8: {}", path.display()))?;
    Ok(Document {
        path: std::path::absolute(path)?,
        kind: DocumentKind::from_path(path),
        text,
    })
}

/// Save an existing text file only if it still matches the opened snapshot.
/// A same-directory temporary file avoids truncation on a failed write.
pub fn save_document(path: &Path, expected: &str, text: &str) -> Result<()> {
    save_document_cancellable(path, expected, text, &crate::work::Cancellation::default())
}
pub(crate) fn save_lock(
    cancel: &crate::work::Cancellation,
) -> Result<std::sync::MutexGuard<'static, ()>> {
    loop {
        cancel.check()?;
        match SAVE_LOCK.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(std::sync::TryLockError::Poisoned(_)) => anyhow::bail!("Save lock unavailable"),
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
        }
    }
}
pub fn save_document_cancellable(
    path: &Path,
    expected: &str,
    text: &str,
    cancel: &crate::work::Cancellation,
) -> Result<()> {
    let _guard = save_lock(cancel)?;
    ensure!(
        !text.contains('\0'),
        "Text contains a null byte and cannot be reopened as a supported text file"
    );
    let target = path
        .canonicalize()
        .context("The file was moved or deleted; your edits have not been saved")?;
    let metadata = std::fs::metadata(&target)?;
    ensure!(metadata.is_file(), "Not a regular file");
    ensure!(
        !metadata.permissions().readonly(),
        "File is read-only; your edits have not been saved"
    );
    ensure!(
        matches_disk(&target, expected, cancel)?,
        "File changed on disk. Save canceled to avoid overwriting external changes; your edits are still open."
    );
    let parent = target.parent().context("File has no parent folder")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .context("Cannot create a save file in this folder")?;
    cancel.copy(
        std::io::Cursor::new(text.as_bytes()),
        temporary.as_file_mut(),
    )?;
    temporary
        .as_file()
        .set_permissions(metadata.permissions())?;
    temporary.as_file().sync_all()?;
    ensure!(
        matches_disk(&target, expected, cancel)?,
        "File changed while saving; your edits are still open."
    );
    cancel.check()?;
    temporary
        .persist(&target)
        .map_err(|e| e.error)
        .context("Cannot replace the file; your edits are still open")?;
    Ok(())
}

// Compare in fixed-size chunks instead of allocating another complete document
// for each of the two optimistic concurrency checks during autosave.
fn matches_disk(path: &Path, expected: &str, cancel: &crate::work::Cancellation) -> Result<bool> {
    let mut file = File::open(path)?;
    if file.metadata()?.len() != expected.len() as u64 {
        return Ok(false);
    }
    let mut buffer = vec![0; reader_limits::IO_BUFFER_BYTES];
    let mut offset = 0;
    loop {
        cancel.check()?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            return Ok(offset == expected.len());
        }
        if expected.as_bytes().get(offset..offset + count) != Some(&buffer[..count]) {
            return Ok(false);
        }
        offset += count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn saving_preserves_text_and_rejects_external_changes_or_deletion() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("notes.txt");
        fs::write(&path, "old\r\n")?;
        save_document(&path, "old\r\n", "新内容\r\n")?;
        assert_eq!(fs::read_to_string(&path)?, "新内容\r\n");
        assert!(save_document(&path, "新内容\r\n", "invalid\0text").is_err());
        assert_eq!(fs::read_to_string(&path)?, "新内容\r\n");
        fs::write(&path, "external")?;
        assert!(save_document(&path, "新内容\r\n", "draft").is_err());
        assert_eq!(fs::read_to_string(&path)?, "external");
        fs::remove_file(&path)?;
        assert!(save_document(&path, "external", "draft").is_err());
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn saving_keeps_symlink_and_permissions_and_respects_readonly() -> Result<()> {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("script.sh");
        let link = dir.path().join("link.sh");
        fs::write(&target, "old")?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
        symlink(&target, &link)?;
        save_document(&link, "old", "new")?;
        assert!(fs::symlink_metadata(&link)?.file_type().is_symlink());
        assert_eq!(fs::read_to_string(&target)?, "new");
        assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o777, 0o755);
        fs::set_permissions(&target, fs::Permissions::from_mode(0o444))?;
        assert!(save_document(&target, "new", "blocked").is_err());
        assert_eq!(fs::read_to_string(&target)?, "new");
        Ok(())
    }
    #[test]
    fn two_windows_cannot_overwrite_each_others_saves() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("shared.txt");
        fs::write(&path, "base")?;
        let outcomes = std::thread::scope(|scope| {
            let a = scope.spawn(|| save_document(&path, "base", "first"));
            let b = scope.spawn(|| save_document(&path, "base", "second"));
            (a.join().unwrap().is_ok(), b.join().unwrap().is_ok())
        });
        assert_ne!(outcomes.0, outcomes.1);
        assert!(matches!(
            fs::read_to_string(&path)?.as_str(),
            "first" | "second"
        ));
        Ok(())
    }

    #[test]
    fn reads_markdown_without_modifying_the_file() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("notes.MD");
        let content = "# 你好\n\nRead-only.\n";
        fs::write(&path, content)?;
        let document = read_document(&path)?;
        assert_eq!(document.kind, DocumentKind::Markdown);
        assert_eq!(document.text, content);
        assert_eq!(fs::read_to_string(path)?, content);
        Ok(())
    }

    #[test]
    fn rejects_unsupported_input() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("input");
        for content in [
            vec![0, 1, 2],
            vec![0xff],
            vec![b'x'; MAX_DOCUMENT_BYTES as usize + 1],
        ] {
            fs::write(&path, content)?;
            assert!(read_document(&path).is_err());
        }
        assert!(read_document(directory.path()).is_err());
        assert!(read_document(&directory.path().join("missing")).is_err());
        Ok(())
    }
}
