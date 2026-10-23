//! Small user preferences stored separately from workspace contents.
use anyhow::{Context, Result};
use std::{io::Write, path::PathBuf};

fn wrap_path_for(application: &str) -> Result<PathBuf> {
    let base = PathBuf::from(std::env::var_os("HOME").context("Home directory is unavailable")?)
        .join("Library/Application Support");
    Ok(base.join(application).join("word-wrap"))
}

pub(crate) fn directory() -> Result<PathBuf> {
    Ok(wrap_path_for("Glim")?
        .parent()
        .expect("Preference path has a parent")
        .to_path_buf())
}

fn wrap_path() -> Result<PathBuf> {
    wrap_path_for("Glim")
}

pub fn workspace_folders() -> Result<Vec<PathBuf>> {
    let path = wrap_path()?.with_file_name("workspace.json");
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

pub fn save_workspace_folders(folders: &[PathBuf]) -> Result<()> {
    let path = wrap_path()?.with_file_name("workspace.json");
    let parent = path.parent().context("Invalid preferences path")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut file, folders)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

pub fn explorer_expansion(roots: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let path = directory()?.join("explorer.json");
    let states: std::collections::BTreeMap<PathBuf, Vec<PathBuf>> = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    Ok(roots
        .iter()
        .filter_map(|root| states.get(root))
        .flatten()
        .cloned()
        .collect())
}

pub fn save_explorer_expansion(roots: &[PathBuf], expanded: &[PathBuf]) -> Result<()> {
    let parent = directory()?;
    std::fs::create_dir_all(&parent)?;
    let path = parent.join("explorer.json");
    let mut states: std::collections::BTreeMap<PathBuf, Vec<PathBuf>> = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(error) => return Err(error.into()),
    };
    for root in roots {
        states.insert(
            root.clone(),
            expanded
                .iter()
                .filter(|path| path.starts_with(root))
                .cloned()
                .collect(),
        );
    }
    let mut file = tempfile::NamedTempFile::new_in(&parent)?;
    serde_json::to_writer(&mut file, &states)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

/// A window's ordered folder roots, including repeated paths.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RecentWorkspace {
    pub id: String,
    pub folders: Vec<PathBuf>,
}

pub fn recent_workspaces() -> Result<Vec<RecentWorkspace>> {
    let path = directory()?.join("recent-workspaces.json");
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let folders = workspace_folders()?;
            Ok(if folders.is_empty() {
                Vec::new()
            } else {
                vec![RecentWorkspace {
                    id: "legacy".into(),
                    folders,
                }]
            })
        }
        Err(error) => Err(error.into()),
    }
}

pub fn remember_recent_workspace(workspace: RecentWorkspace) -> Result<Vec<RecentWorkspace>> {
    let mut recent = recent_workspaces()?;
    recent.retain(|entry| {
        entry.id != workspace.id
            && (workspace.folders.is_empty() || entry.folders != workspace.folders)
    });
    if !workspace.folders.is_empty() {
        recent.insert(0, workspace);
    }
    let path = directory()?.join("recent-workspaces.json");
    let parent = path.parent().context("Invalid preferences path")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut file, &recent)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(recent)
}

pub fn word_wrap() -> bool {
    wrap_path()
        .ok()
        .and_then(|path| match std::fs::read_to_string(path) {
            Ok(value) => Some(value),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // Keep the previous preference until Glim saves its own value.
                wrap_path_for("Glimpse")
                    .ok()
                    .and_then(|legacy| std::fs::read_to_string(legacy).ok())
            }
            Err(_) => None,
        })
        .is_some_and(|value| value.trim() == "true")
}

pub fn save_word_wrap(enabled: bool) -> Result<()> {
    let path = wrap_path()?;
    let parent = path.parent().context("Invalid preferences path")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(file, "{enabled}")?;
    file.persist(path)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

pub fn appearance() -> Appearance {
    match wrap_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path.with_file_name("appearance")).ok())
        .as_deref()
        .map(str::trim)
    {
        Some("light") => Appearance::Light,
        Some("dark") => Appearance::Dark,
        _ => Appearance::System,
    }
}

pub fn save_appearance(appearance: Appearance) -> Result<()> {
    let path = wrap_path()?.with_file_name("appearance");
    let parent = path.parent().context("Invalid preferences path")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(
        file,
        "{}",
        match appearance {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    )?;
    file.persist(path)?;
    Ok(())
}

pub fn preview_mode(kind: &str) -> bool {
    preview_path(kind)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map_or(kind != "csv", |value| value.trim() != "source")
}
fn preview_path(kind: &str) -> Result<PathBuf> {
    anyhow::ensure!(
        matches!(kind, "html" | "markdown" | "csv"),
        "Unknown preview preference"
    );
    Ok(wrap_path()?.with_file_name(format!("{kind}-view")))
}
pub fn save_preview_mode(kind: &str, preview: bool) -> Result<()> {
    let path = preview_path(kind)?;
    let parent = path.parent().context("Invalid preferences path")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(file, "{}", if preview { "preview" } else { "source" })?;
    file.persist(path)?;
    Ok(())
}

pub fn language() -> Option<String> {
    let path = wrap_path().ok()?.with_file_name("language");
    let value = std::fs::read_to_string(path).ok()?;
    crate::locale::supported(value.trim()).then(|| value.trim().to_owned())
}
pub fn save_language(language: &str) -> Result<()> {
    anyhow::ensure!(crate::locale::supported(language), "Unknown language");
    let path = wrap_path()?.with_file_name("language");
    let parent = path.parent().context("Invalid preferences path")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(file, "{language}")?;
    file.persist(path)?;
    Ok(())
}

/// Telemetry defaults to enabled; unreadable preferences fail closed.
pub fn telemetry_enabled() -> bool {
    let Ok(directory) = directory() else {
        return false;
    };
    match std::fs::read_to_string(directory.join("telemetry")) {
        Ok(value) => value.trim() == "true",
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

pub fn save_telemetry_enabled(enabled: bool) -> Result<()> {
    let parent = directory()?;
    std::fs::create_dir_all(&parent)?;
    let mut file = tempfile::NamedTempFile::new_in(&parent)?;
    writeln!(file, "{enabled}")?;
    file.persist(parent.join("telemetry"))?;
    Ok(())
}
