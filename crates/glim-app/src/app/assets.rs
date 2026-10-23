use gpui_kit::{AssetSource, SharedString};
use std::borrow::Cow;

gpui_kit::assets::icon_assets!(
    ViewerIcons,
    [
        Files,
        ArrowLeft,
        ArrowRight,
        Download,
        X,
        House,
        PanelTop,
        PanelLeft,
        FolderOpen,
        GitCompareArrows,
        Settings2,
        Search,
        FileCode,
        FileImage,
        FileArchive,
        File,
        FileText,
        GitBranch,
        RefreshCw,
        Ellipsis,
        Eye,
        Code,
        List,
        ListTree,
        Columns2,
        Rows2,
        Plus,
        Minus,
        ChevronDown,
        ChevronRight,
        FolderGit2,
        ArrowUpRight,
        Clock,
        FilePlus,
        Folder,
        FolderPlus,
        Folders,
        PanelsTopLeft
    ]
);

#[path = "file_icon_assets.rs"]
mod file_icons;

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if path.starts_with("file-icons/") {
            let bytes = match file_icons::ASSETS.binary_search_by(|entry| entry.0.cmp(path)) {
                Ok(index) => file_icons::ASSETS[index].1,
                Err(_) => {
                    log::warn!("Unregistered file icon {path}; using the default document icon");
                    file_icons::FALLBACK
                }
            };
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        match path {
            "navigation/appearance.svg" => {
                return Ok(Some(Cow::Borrowed(include_bytes!(
                    "../../../../assets/navigation/appearance.svg"
                ))));
            }
            "navigation/file-fullscreen.svg" => {
                return Ok(Some(Cow::Borrowed(include_bytes!(
                    "../../../../assets/navigation/file-fullscreen.svg"
                ))));
            }
            "navigation/file-fullscreen-exit.svg" => {
                return Ok(Some(Cow::Borrowed(include_bytes!(
                    "../../../../assets/navigation/file-fullscreen-exit.svg"
                ))));
            }
            "navigation/wrap.svg" => {
                return Ok(Some(Cow::Borrowed(include_bytes!(
                    "../../../../assets/navigation/wrap.svg"
                ))));
            }
            "navigation/files.svg" => {
                return Ok(Some(Cow::Borrowed(include_bytes!(
                    "../../../../assets/navigation/files.svg"
                ))));
            }
            _ => {}
        }
        if path == "branding/glim.png" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../../assets/branding/glim.png"
            ))));
        }
        if path == "branding/glim-welcome.png" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../../assets/branding/glim-welcome.png"
            ))));
        }
        if let Some(icon) = ViewerIcons.load(path)? {
            return Ok(Some(icon));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let mut assets = gpui_kit::assets::Assets.list(path)?;
        assets.extend(ViewerIcons.list(path)?);
        assets.extend(
            file_icons::ASSETS
                .iter()
                .filter(|(name, _)| name.starts_with(path))
                .map(|(name, _)| SharedString::from(*name)),
        );
        if "branding/glim.png".starts_with(path) {
            assets.push("branding/glim.png".into());
        }
        if "branding/glim-welcome.png".starts_with(path) {
            assets.push("branding/glim-welcome.png".into());
        }
        Ok(assets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_file_icon_is_loadable_and_listed() {
        assert!(
            file_icons::ASSETS
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0),
            "File icon resources must be sorted by full path"
        );
        let source = Assets;
        let listed = source.list("file-icons/").unwrap();
        for (path, expected) in file_icons::ASSETS {
            let loaded = source
                .load(path)
                .unwrap()
                .unwrap_or_else(|| panic!("Missing embedded icon: {path}"));
            assert_eq!(loaded.as_ref(), *expected, "{path}");
            assert!(loaded.starts_with(b"\x89PNG\r\n\x1a\n"), "{path}");
            assert!(listed.iter().any(|entry| entry.as_ref() == *path), "{path}");
        }
    }
}
