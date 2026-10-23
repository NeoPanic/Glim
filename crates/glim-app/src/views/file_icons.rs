use gpui_kit::{component::ActiveTheme, *};
use std::path::Path;

#[path = "file_icons_data.rs"]
mod associations;

type Association = (&'static str, &'static str, &'static str);

fn lookup(table: &[Association], name: &str, light: bool) -> Option<&'static str> {
    table
        .binary_search_by(|entry| entry.0.cmp(name))
        .ok()
        .map(|index| {
            if light {
                table[index].2
            } else {
                table[index].1
            }
        })
}

fn icon_name(path: &Path, light: bool) -> &'static str {
    // Some upstream associations are qualified names such as .config/eslintrc
    // or .github/funding.yml. Match whole path components before the basename.
    let qualified = path.to_string_lossy().to_lowercase();
    for start in
        std::iter::once(0).chain(qualified.match_indices('/').map(|(offset, _)| offset + 1))
    {
        if let Some(icon) = lookup(associations::PATH_NAMES, &qualified[start..], light) {
            return icon;
        }
    }
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if let Some(icon) = lookup(associations::FILE_NAMES, &name, light) {
        return icon;
    }
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .unwrap_or_default();
    if matches!(extension, "yml" | "yaml")
        && path.parent().is_some_and(|parent| {
            parent
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("workflows"))
                && parent
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name.eq_ignore_ascii_case(".github"))
        })
    {
        return "github-actions-workflow";
    }
    if name.starts_with("dockerfile.")
        || name.starts_with("containerfile.")
        || matches!(extension, "yml" | "yaml")
            && (name.starts_with("docker-compose.") || name.starts_with("compose."))
    {
        return "docker";
    }
    if name.starts_with(".env.") {
        return "tune";
    }
    // Compound suffixes (d.ts, test.tsx, blade.php, etc.) win over the final
    // extension. Exact filenames remain the most specific association.
    let mut rest = name.as_str();
    while let Some((_, suffix)) = rest.split_once('.') {
        if let Some(icon) = lookup(associations::EXTENSIONS, suffix, light) {
            return icon;
        }
        rest = suffix;
    }
    associations::DEFAULT
}

pub fn file_icon(path: &Path, cx: &App) -> impl IntoElement {
    let icon = icon_name(path, !cx.theme().is_dark());
    img(SharedString::from(format!("file-icons/{icon}.png")))
        .size(px(16.))
        .flex_shrink_0()
}

#[cfg(test)]
mod tests {
    use super::{associations, icon_name};
    use std::path::Path;

    #[test]
    fn declared_filename_and_path_associations_resolve_in_both_themes() {
        for &(name, dark, light) in associations::FILE_NAMES
            .iter()
            .chain(associations::PATH_NAMES)
        {
            for (is_light, expected) in [(false, dark), (true, light)] {
                for path in [
                    name.to_owned(),
                    name.to_uppercase(),
                    format!("/project/{name}"),
                ] {
                    assert_eq!(icon_name(Path::new(&path), is_light), expected, "{path}");
                }
            }
        }
    }

    #[test]
    fn declared_compound_extensions_resolve_without_losing_their_specific_icon() {
        for &(extension, dark, light) in associations::EXTENSIONS {
            for (is_light, expected) in [(false, dark), (true, light)] {
                for extension in [extension.to_owned(), extension.to_uppercase()] {
                    let path = format!("glim-association-audit.{extension}");
                    assert_eq!(icon_name(Path::new(&path), is_light), expected, "{path}");
                }
            }
        }
    }
}
