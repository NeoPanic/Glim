<p align="center"><img src="assets/branding/repository-banner.svg" alt="Glim — documents, data and code in one Mac workspace. Free and open source; Apple Silicon and Intel." width="100%" /></p>


# Glim — Documents, data, and code. One Mac workspace.

Glim is a free, open-source file viewer for macOS. Open a project folder and read its Markdown, PDFs, images, JSON, CSV and source code in the same window. Switch between previews and source, inspect local data, and make small text edits without leaving your files.

<p align="center">Free and open source · macOS · Apple Silicon &amp; Intel · <b>English</b> / <a href="README.zh-CN.md">简体中文</a></p>

<p align="center"><a href="https://glimapp.github.io/">Website / 官网</a> · <a href="https://github.com/glimapp/glim/releases/latest"><strong>Download Glim →</strong></a></p>

<p align="center"><a href="https://github.com/glimapp/glim/releases/latest">Latest release · Release notes &amp; checksums</a> · <a href="#download">Installation help</a></p>

<p align="center"><sub>Mac downloads are not yet notarized by Apple. You may need to approve the first launch in System Settings. <a href="#macos">See instructions</a></sub></p>

<p align="center">
  <a href="https://github.com/glimapp/glim/releases"><img src="https://img.shields.io/github/downloads/glimapp/glim/total?style=flat-square&amp;color=268879" alt="Total release downloads" /></a>
  <a href="https://github.com/glimapp/glim/actions/workflows/ci.yml"><img src="https://github.com/glimapp/glim/actions/workflows/ci.yml/badge.svg" alt="Desktop builds" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="MIT License" /></a>
  <a href="https://github.com/glimapp/glim/stargazers"><img src="https://img.shields.io/github/stars/glimapp/glim?style=social" alt="GitHub stars" /></a>
</p>

## Read a folder, not just a file

<p align="center"><img src="assets/readme/workspace.png" alt="Glim showing a Markdown report with an embedded GitHub Star chart, a project file tree, and PNG, Markdown and SVG tabs." width="100%" /></p>

A Markdown report, its chart images, the scripts that produced them and the underlying database can stay together. The screenshot shows a Markdown report with an embedded chart, with related PNG and SVG files open in adjacent tabs.

| Your task | In Glim |
| --- | --- |
| Read a report or project documentation | Render Markdown tables, diagrams and formulas; follow local links and view related images |
| Inspect an export or API response | Expand JSON objects, read CSV/TSV tables and inspect complete cell values |
| Check a local database | Browse SQLite tables, filter and sort records, and inspect schema in a read-only snapshot |
| Review documents and reference material | Keep PDFs, supported Office files, images and ebooks in workspace tabs |
| Fix a typo or configuration value | Edit UTF-8 text with undo/redo, automatic saving and explicit ⌘S |

### Inspect the data behind the document

<p align="center"><img src="assets/readme/sqlite.png" alt="Glim browsing a read-only SQLite snapshot with a table list, records filtered by drupal, sortable columns and page navigation." width="100%" loading="lazy" /></p>

SQLite opens in a private read-only snapshot. Choose a table or view, search across every column in all records, sort, or double-click a value to read it in full. Glim leaves the source database unchanged.

## Start with your own files

1. **Open a file** with ⌘O, or **open a folder** with ⌘⇧O. The sidebar lets you browse related files; title-bar search finds files by name or path in workspace folders.
2. **Choose the view that fits the content.** Markdown, HTML and supported data readers provide preview/source controls. Documents and images use their format-specific reading controls.
3. **Make a small edit when needed.** Existing UTF-8 files save automatically. ⌘N starts an untitled note; its first ⌘S asks where to save it.

[User guide](docs/user-guide.md) · [Formats and compatibility](docs/file-format-support.md) · [Download Glim](https://github.com/glimapp/glim/releases/latest)

## Formats at a glance

<a href="docs/file-format-support.md">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/readme/formats-dark.svg" />
    <source media="(prefers-color-scheme: light)" srcset="assets/readme/formats.svg" />
    <img src="assets/readme/formats.svg" alt="Representative Glim file formats; dagger marks partial previews" width="100%" />
  </picture>
</a>

*† Partial preview. See the [format guide](docs/file-format-support.md) for each reader’s actual behavior.*

| Content | Representative formats |
| --- | --- |
| Documents and text | Markdown, HTML, UTF-8 text and logs; diagrams and formulas in Markdown |
| Data and code | JSON/JSONC/JSON5, CSV/TSV, YAML, TOML, XML, SQLite and highlighted source code |
| Images and documents | PNG, JPEG, WebP, GIF, SVG, TIFF, PDF, Word, Excel, PowerPoint and iWork |
| Books and mind maps | EPUB, FB2, CBZ and XMind |
| Media and specialist files | System-supported audio/video, fonts, safetensors metadata, archive listings and selected text previews |

### What to expect

- **Editing:** UTF-8 text supports basic editing and saving. Tables, data trees, database snapshots, images and document previews are read-only; editable formats retain source access. Large or long-line text uses editable pages.
- **Compatibility:** Office previews can differ from the original layout; spreadsheet formulas use saved values without recalculation. Media playback depends on macOS codecs. Some formats provide a partial preview or source fallback.
- **Local files and network content:** Files open locally. Markdown can fetch remote images, and HTML previews support CSS, JavaScript and network resources. Optional anonymous usage telemetry is controlled in Settings; see [telemetry](docs/telemetry.md).
- **Platform:** macOS on Apple Silicon and Intel. Current downloads are ad-hoc signed and have not been notarized by Apple; installation steps are below.

[Full format guide](docs/file-format-support.md) · [Documentation](docs/README.md) · [Remaining work](docs/TODO.md)

## Download

Open the [latest Release page](https://github.com/glimapp/glim/releases/latest) and choose the matching installer under **Assets**. That page contains the current version, release notes and checksums.

| Your device | Installer filename suffix |
| --- | --- |
| Mac · Apple Silicon (M series) | `-macos-arm64.dmg` |
| Mac · Intel | `-macos-x86_64.dmg` |

Not sure which Mac download to choose? Open **Apple menu → About This Mac**. An Apple M-series chip means Apple Silicon; an Intel processor means Intel.

### macOS

Open the DMG and drag Glim into Applications. The app is ad-hoc signed, without Developer ID signing or notarization. If macOS blocks it, try opening it once, then go to **System Settings → Privacy & Security → Open Anyway**. Confirm the download came from this project's releases; there is no need to disable system security. [Apple's instructions](https://support.apple.com/en-us/102445).

[Release notes & SHA-256 checksums](https://github.com/glimapp/glim/releases/latest) · [All releases](https://github.com/glimapp/glim/releases) · [Build from source](docs/releasing.md)

## Get involved

Found a file Glim should open better? [Report a bug](https://github.com/glimapp/glim/issues/new?template=bug_report.yml), [request a format](https://github.com/glimapp/glim/issues/new?template=feature_request.yml), or [start a discussion](https://github.com/glimapp/glim/discussions). English and Chinese are welcome.

[Contributing](CONTRIBUTING.md) · [Build from source](docs/releasing.md) · [Roadmap](docs/TODO.md) · [Security](SECURITY.md) · [MIT License](LICENSE)

## Contributors

Thanks to everyone helping make Glim better.

<a href="https://github.com/glimapp/glim/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=glimapp/glim" alt="Glim contributors" />
</a>

## Star history

<a href="https://star-history.com/#glimapp/glim&amp;Date">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=glimapp/glim&amp;type=Date&amp;theme=dark" />
    <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=glimapp/glim&amp;type=Date" />
    <img src="https://api.star-history.com/svg?repos=glimapp/glim&amp;type=Date" alt="Glim GitHub star history" width="100%" />
  </picture>
</a>

## Download history

<a href="https://github.com/glimapp/glim/tree/codex/download-stats">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/glimapp/glim/refs/heads/codex/download-stats/downloads-dark.svg" />
    <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/glimapp/glim/refs/heads/codex/download-stats/downloads.svg" />
    <img src="https://raw.githubusercontent.com/glimapp/glim/refs/heads/codex/download-stats/downloads.svg" alt="Glim release download history" width="100%" />
  </picture>
</a>

Daily snapshots of cumulative public GitHub release-asset downloads, including installers, update feeds, checksums and release-verification requests. Tracking starts on **2026-10-12**; earlier history is unavailable. These counts are downloads, not unique users or installs.
