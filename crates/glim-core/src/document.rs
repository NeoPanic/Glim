use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentKind {
    Markdown,
    Text,
}

impl DocumentKind {
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some(extension)
                if extension.eq_ignore_ascii_case("md")
                    || extension.eq_ignore_ascii_case("markdown") =>
            {
                Self::Markdown
            }
            _ => Self::Text,
        }
    }
}

/// A successfully decoded, read-only document. No GPUI types cross this boundary.
#[derive(Debug)]
pub struct Document {
    pub path: PathBuf,
    pub kind: DocumentKind,
    pub text: String,
}

/// Tree-sitter registry names; unsupported extensions stay readable as plain text.
pub fn language_for_path(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    if lower == "dockerfile"
        || lower == "containerfile"
        || lower.starts_with("dockerfile.")
        || lower.starts_with("containerfile.")
        || lower.ends_with(".dockerfile")
    {
        return "dockerfile";
    }
    if matches!(
        name,
        ".npmrc"
            | ".yarnrc"
            | ".editorconfig"
            | ".gitconfig"
            | ".gitmodules"
            | ".pypirc"
            | "pip.conf"
            | "setup.cfg"
            | "tox.ini"
    ) {
        return "ini";
    }
    if name == ".env" || name.starts_with(".env.") {
        return "dotenv";
    }
    if matches!(
        name,
        ".gitignore" | ".dockerignore" | ".npmignore" | ".ignore" | ".gitattributes"
    ) {
        return "ignore";
    }
    if matches!(name, ".prettierrc" | ".stylelintrc" | ".swcrc") {
        return "json";
    }
    if name == "Cargo.lock" || name == "uv.lock" || name == "poetry.lock" || name == "Pipfile" {
        return "toml";
    }
    if matches!(
        name,
        "Gemfile"
            | "Rakefile"
            | "Guardfile"
            | "Podfile"
            | "Vagrantfile"
            | "Brewfile"
            | "Fastfile"
            | "Appfile"
            | ".irbrc"
            | ".pryrc"
    ) {
        return "ruby";
    }
    if name == "CMakeLists.txt" {
        return "cmake";
    }
    if matches!(name, "Makefile" | "GNUmakefile" | "makefile") {
        return "make";
    }
    if matches!(name, ".bashrc" | ".bash_profile" | ".zshrc" | ".zprofile") {
        return "bash";
    }
    if matches!(name, ".eslintrc" | ".babelrc" | ".czrc") {
        return "json";
    }
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jinja" | "jinja2" | "j2" | "njk" | "nunjucks" | "twig" => "jinja",
        "ini" | "cfg" | "conf" | "properties" | "service" | "desktop" => "ini",
        "env" => "dotenv",
        "nix" => "nix",
        "hcl" | "dhall" | "kdl" | "stan" | "m" | "http" | "d2" | "pikchr" | "obj" | "stl"
        | "bdf" | "srt" | "vtt" | "adoc" | "org" | "rst" => "structured",
        "rs" => "rust",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "json" | "jsonc" | "json5" | "ndjson" | "jsonl" | "ipynb" | "geojson"
        | "code-workspace" | "tsbuildinfo" => "json",
        "toml" => "toml",
        "py" | "pyi" | "pyx" | "pxd" | "pxi" => "python",
        "go" => "go",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "cu" | "cuh" | "metal" => "cpp",
        "css" | "less" | "scss" => "css",
        // HTML provides basic tag highlighting, not full Vue/XML language semantics.
        "opml" | "drawio" | "gpx" | "kml" | "html" | "htm" | "vue" | "xml" | "xsd" | "xsl"
        | "plist" | "mtlx" | "tmx" | "hbs" => "html",
        "yml" | "yaml" | "ocio" => "yaml",
        "sh" | "bash" | "zsh" | "command" => "bash",
        "swift" => "swift",
        "java" => "java",
        "rb" | "rake" | "gemspec" | "ru" | "podspec" => "ruby",
        "php" | "php3" | "php4" | "php5" | "phtml" => "php",
        "cs" | "csx" => "csharp",
        "kt" | "kts" => "kotlin",
        "lua" => "lua",
        "scala" | "sc" | "sbt" => "scala",
        "ex" | "exs" => "elixir",
        "zig" | "zon" => "zig",
        "graphql" | "gql" => "graphql",
        "proto" => "proto",
        "cmake" => "cmake",
        "astro" => "astro",
        "svelte" => "svelte",
        "erb" => "erb",
        "ejs" => "ejs",
        "sql" => "sql",
        "mk" => "make",
        // C-like syntax only; these retain shader-specific tokens as plain text.
        "glsl" | "vert" | "frag" | "geom" | "osl" => "c",
        "md" | "markdown" => "markdown",
        _ => "plain",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_source_aliases_without_misclassifying_unknown_files() {
        for (path, language) in [
            ("Main.JAVA", "java"),
            ("app.rb", "ruby"),
            ("Gemfile", "ruby"),
            ("Podfile", "ruby"),
            ("package.gemspec", "ruby"),
            ("index.php", "php"),
            ("Main.cs", "csharp"),
            ("build.gradle.kts", "kotlin"),
            ("init.lua", "lua"),
            ("build.sbt", "scala"),
            ("mix.exs", "elixir"),
            ("build.zig", "zig"),
            ("query.gql", "graphql"),
            ("api.proto", "proto"),
            ("CMakeLists.txt", "cmake"),
            ("Page.astro", "astro"),
            ("App.svelte", "svelte"),
            ("index.html.erb", "erb"),
            ("index.ejs", "ejs"),
            ("schema.sql", "sql"),
            ("Component.vue", "html"),
            ("material.mtlx", "html"),
            ("shader.glsl", "c"),
            ("kernel.cu", "cpp"),
            ("types.pxd", "python"),
            ("events.jsonl", "json"),
            ("Makefile", "make"),
            (".zshrc", "bash"),
            ("table.csv", "plain"),
            ("unknown.blend", "plain"),
        ] {
            assert_eq!(language_for_path(Path::new(path)), language, "{path}");
        }
    }
}
