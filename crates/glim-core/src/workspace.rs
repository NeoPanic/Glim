use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
}
