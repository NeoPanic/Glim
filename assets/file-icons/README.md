# File icons

Material Icon Theme 5.39.0, under the MIT license (see LICENSE). SVG originals and 48 px transparent PNGs are checked in; normal app builds need no Node.js, network or image conversion tools. The npm archive and its integrity hash are recorded in manifest.json.

Coverage: 634 icon assets, 1410 extension rules and 2151 exact filename rules, including light-theme variants. Directory-qualified filename rules take precedence over basenames and longest compound suffixes. Embedded assets are sorted by complete resource paths (including .png), matching the runtime binary search. Glim also handles Dockerfile/Containerfile variants, Compose variants, environment variants and GitHub Actions workflow directories. Icons identify files; they do not imply reader or editing support.

Regenerate with NODE_PATH=/path/to/node_modules node scripts/generate-file-icons.mjs --source /path/to/extracted/material-icon-theme/package (requires sharp). The importer pins the package version, adds Glim's format associations, and generates both Rust tables from the same inventory. Run cargo fmt --all afterward.
