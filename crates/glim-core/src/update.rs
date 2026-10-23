//! Update/session values shared across UI and I/O; no platform dependencies.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateMode {
    Manual,
    Notify,
    #[default]
    Automatic,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSnapshot {
    pub enabled: bool,
    pub phase: String,
    pub mode: UpdateMode,
    pub current_version: String,
    pub current_build: String,
    pub version: String,
    pub build: String,
    pub notes: String,
    pub published: String,
    pub message: String,
    pub received: u64,
    pub expected: u64,
    pub progress_known: bool,
    pub can_cancel: bool,
    pub can_download: bool,
    pub can_install: bool,
    pub install_on_quit: bool,
    pub needs_install_approval: bool,
    pub exit_requested: bool,
    pub exit_permitted: bool,
    pub needs_quit_approval: bool,
    pub attention: u64,
    pub bundle_path: PathBuf,
    pub last_check: String,
}
impl UpdateSnapshot {
    pub fn has_update(&self) -> bool {
        !self.version.is_empty()
            && matches!(
                self.phase.as_str(),
                "available" | "downloading" | "extracting" | "ready" | "installing"
            )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionTab {
    File { path: PathBuf, preview: bool },
    Playlist { path: PathBuf, shuffle: bool },
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSession {
    pub roots: Vec<PathBuf>,
    pub tabs: Vec<SessionTab>,
    pub active: Option<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSession {
    pub schema: u32,
    pub source_build: String,
    pub target_build: String,
    pub workspaces: Vec<WorkspaceSession>,
}

/// A clean checkpoint is required before invoking a native install continuation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UpdateActivity {
    pub dirty: bool,
    pub saving: bool,
    pub loading: bool,
}
impl UpdateActivity {
    pub fn blocker(self) -> Option<&'static str> {
        if self.saving {
            Some("Wait for files to finish saving before updating.")
        } else if self.dirty {
            Some("Save all modified files before updating. Conflicting files need your attention.")
        } else if self.loading {
            Some("Wait for files to finish opening before updating.")
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dirty_or_busy_work_never_authorizes_installation() {
        assert!(UpdateActivity::default().blocker().is_none());
        for activity in [
            UpdateActivity {
                dirty: true,
                ..Default::default()
            },
            UpdateActivity {
                saving: true,
                ..Default::default()
            },
            UpdateActivity {
                loading: true,
                ..Default::default()
            },
        ] {
            assert!(activity.blocker().is_some());
        }
    }
}
