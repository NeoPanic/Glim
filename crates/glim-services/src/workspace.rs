use crate::limits as reader_limits;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use glim_core::DirectoryEntry;
use ignore::WalkBuilder;

pub struct FolderSnapshot {
    pub root: PathBuf,
    pub entries: Vec<DirectoryEntry>,
}

pub fn open_folder(path: &Path) -> Result<FolderSnapshot> {
    let root = path
        .canonicalize()
        .with_context(|| format!("Cannot open {}", path.display()))?;
    let entries = list_directory(&root)?;
    Ok(FolderSnapshot { root, entries })
}

/// Read only immediate children; do not follow directory symlinks or scan the whole tree.
pub fn list_directory(path: &Path) -> Result<Vec<DirectoryEntry>> {
    ensure!(path.is_dir(), "Not a directory: {}", path.display());
    let mut entries = Vec::new();
    for entry in WalkBuilder::new(path)
        .max_depth(Some(1))
        .hidden(false)
        .follow_links(false)
        .require_git(false)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build()
    {
        let entry = entry.with_context(|| format!("Cannot read {}", path.display()))?;
        if entry.depth() == 0 {
            continue;
        }
        let Some(kind) = entry.file_type() else {
            continue;
        };
        let is_document_package = kind.is_dir() && crate::preview::is_iwork(entry.path());
        entries.push(DirectoryEntry {
            path: entry.into_path(),
            is_dir: kind.is_dir() && !is_document_package,
            is_symlink: kind.is_symlink(),
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.path.file_name().cmp(&b.path.file_name()))
    });
    Ok(entries)
}

pub struct SearchPage {
    pub paths: Vec<PathBuf>,
    pub more: bool,
}
pub fn search_files(root: &Path, query: &str) -> Vec<PathBuf> {
    search_page(
        &[root.to_path_buf()],
        query,
        0,
        &crate::work::Cancellation::default(),
    )
    .map(|page| page.paths)
    .unwrap_or_default()
}
pub fn search_page(
    roots: &[PathBuf],
    query: &str,
    offset: usize,
    cancel: &crate::work::Cancellation,
) -> Result<SearchPage> {
    let query = query.trim().to_lowercase();
    let mut paths = Vec::new();
    if query.is_empty() {
        return Ok(SearchPage { paths, more: false });
    }
    let mut roots = roots.to_vec();
    roots.sort();
    roots.dedup();
    let roots: Vec<_> = roots
        .iter()
        .filter(|root| {
            !roots
                .iter()
                .any(|other| other != *root && root.starts_with(other))
        })
        .collect();
    let mut skipped = 0;
    for root in roots {
        for entry in WalkBuilder::new(root)
            .hidden(false)
            .follow_links(false)
            .filter_entry(|entry| entry.file_name() != ".git")
            .sort_by_file_name(|a, b| a.cmp(b))
            .build()
        {
            cancel.check()?;
            let Ok(entry) = entry else {
                continue;
            };
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let path = entry.into_path();
            if !path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_lowercase()
                .contains(&query)
            {
                continue;
            }
            if skipped < offset {
                skipped += 1;
                continue;
            }
            if paths.len() == reader_limits::WORKSPACE_SEARCH_RESULTS {
                return Ok(SearchPage { paths, more: true });
            }
            paths.push(path);
        }
    }
    Ok(SearchPage { paths, more: false })
}

/// Move the entry itself to macOS Trash. Never resolve a symlink to its target.
pub fn trash_entry(path: &Path) -> Result<()> {
    use std::{os::unix::fs::PermissionsExt, process::Command};
    let _guard = crate::files::SAVE_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("File operation lock unavailable"))?;
    let path = std::path::absolute(path)?;
    ensure!(
        path.parent().is_some() && path.file_name().is_some(),
        "Cannot trash a filesystem root"
    );
    std::fs::symlink_metadata(&path).context("The item no longer exists")?;
    let directory = tempfile::tempdir()?;
    let helper = directory.path().join("glim-file-operation");
    std::fs::write(
        &helper,
        include_bytes!(concat!(env!("OUT_DIR"), "/glim-preview")),
    )?;
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))?;
    let output = Command::new(helper).arg("--trash").arg(&path).output()?;
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn opening_folder_does_not_wait_for_git_or_scan_descendants() -> Result<()> {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join(".git"), "gitdir: /does/not/exist")?;
        fs::create_dir(temp.path().join("nested"))?;
        fs::write(temp.path().join("nested/hidden-from-first-level.txt"), "")?;
        let snapshot = open_folder(temp.path())?;
        assert_eq!(snapshot.root, temp.path().canonicalize()?);
        assert_eq!(snapshot.entries.len(), 1);
        assert_eq!(snapshot.entries[0].path.file_name().unwrap(), "nested");
        Ok(())
    }

    #[test]
    fn lists_one_level_respects_ignore_and_keeps_dotfiles() -> Result<()> {
        let temp = tempfile::tempdir()?;
        fs::create_dir(temp.path().join("src"))?;
        fs::write(temp.path().join("src/nested.rs"), "")?;
        fs::create_dir(temp.path().join(".git"))?;
        fs::write(temp.path().join(".gitignore"), "ignored\n")?;
        fs::write(temp.path().join("ignored"), "")?;
        fs::write(temp.path().join("main.rs"), "")?;
        let entries = list_directory(temp.path())?;
        let names: Vec<_> = entries
            .iter()
            .map(|e| e.path.file_name().unwrap().to_string_lossy())
            .collect();
        assert_eq!(names, ["src", ".gitignore", "main.rs"]);
        assert!(entries[0].is_dir);
        Ok(())
    }

    #[test]
    fn directory_symlinks_do_not_create_recursive_trees() -> Result<()> {
        let temp = tempfile::tempdir()?;
        std::os::unix::fs::symlink(temp.path(), temp.path().join("loop"))?;
        let entries = list_directory(temp.path())?;
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_symlink);
        assert!(!entries[0].is_dir);
        Ok(())
    }
}
