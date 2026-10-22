use gpui_kit::*;
use std::path::Path;

pub fn file_icon(path: &Path) -> impl IntoElement {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let icon = match name.as_str() {
        "gemfile" | "rakefile" | "podfile" | "guardfile" | "vagrantfile" | "brewfile" => "ruby",
        "dockerfile" | "docker-compose.yml" => "docker",
        ".gitignore" | ".gitattributes" | ".gitmodules" => "git",
        "cargo.lock" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" => "lock",
        _ => match extension.as_str() {
            "rs" => "rust",
            "ts" => "typescript",
            "js" | "mjs" | "cjs" => "javascript",
            "tsx" | "jsx" => "react",
            "json" | "jsonc" | "jsonl" => "json",
            "md" | "mdx" => "markdown",
            "py" | "pyi" | "pyx" | "pxd" => "python",
            "go" => "go",
            "java" => "java",
            "c" | "h" => "c",
            "cpp" | "cc" | "hpp" | "cu" | "metal" => "cpp",
            "css" | "scss" | "less" => "css",
            "html" => "html",
            "yml" | "yaml" | "ocio" => "yaml",
            "toml" => "toml",
            "swift" => "swift",
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "icns" | "exr" | "hdr" => {
                "image"
            }
            "pdf" => "pdf",
            "zip" | "gz" | "tar" | "7z" => "zip",
            "mp3" | "wav" | "flac" => "audio",
            "mp4" | "mov" | "mkv" => "video",
            "sql" | "db" | "db3" | "sqlite" | "sqlite3" | "s3db" | "wal" | "shm" | "db-wal"
            | "sqlite-wal" | "sqlite3-wal" | "db-shm" | "sqlite-shm" | "sqlite3-shm" => "database",
            "opml" | "drawio" | "xml" | "plist" | "mtlx" | "tmx" => "xml",
            "vue" => "vue",
            "svelte" => "svelte",
            "kt" | "kts" => "kotlin",
            "rb" | "rake" | "gemspec" | "ru" | "podspec" | "erb" => "ruby",
            "php" => "php",
            "sh" | "zsh" | "bash" | "ini" | "conf" => "settings",
            _ => "document",
        },
    };
    img(SharedString::from(format!("file-icons/{icon}.png")))
        .size(px(16.))
        .flex_shrink_0()
}
