use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Retains native filesystem subscriptions; coalesces event bursts for the UI timer.
pub struct WorkspaceWatch {
    _watcher: RecommendedWatcher,
    dirty: Arc<AtomicBool>,
}
impl WorkspaceWatch {
    pub fn new(root: &Path) -> Result<Self> {
        let dirty = Arc::new(AtomicBool::new(false));
        let flag = dirty.clone();
        let watched_root = root.to_path_buf();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if let Ok(event) = event
                    && !matches!(event.kind, notify::EventKind::Access(_))
                    && event.paths.iter().any(|p| {
                        !p.strip_prefix(&watched_root)
                            .unwrap_or(p)
                            .components()
                            .any(|c| {
                                matches!(
                                    c.as_os_str().to_str(),
                                    Some("target" | "node_modules" | "dist")
                                )
                            })
                    })
                {
                    flag.store(true, Ordering::Release);
                }
            })?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        Ok(Self {
            _watcher: watcher,
            dirty,
        })
    }
    pub fn add_path(&mut self, path: &Path) -> Result<()> {
        self._watcher.watch(path, RecursiveMode::Recursive)?;
        Ok(())
    }

    pub fn take_changed(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_atomic_file_replacement() -> Result<()> {
        let temp = tempfile::tempdir()?;
        std::fs::write(temp.path().join("doc.md"), "before")?;
        let watch = WorkspaceWatch::new(temp.path())?;
        std::fs::write(temp.path().join("next.md"), "after")?;
        std::fs::rename(temp.path().join("next.md"), temp.path().join("doc.md"))?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if watch.take_changed() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        anyhow::bail!("Watcher did not report file replacement")
    }
}
