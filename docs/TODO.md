# Roadmap and remaining work

**English** · [简体中文](TODO.zh-CN.md) · [Documentation](README.md)

## Implemented baseline

- Multiple file readers in workspace tabs, including macOS native previews and XMind.
- Ordinary UTF-8 editing/autosave, structured data previews and offline diagrams; [reader-specific limits](file-format-support.md) apply.
- macOS signed updates, recovery app backups and one-time update-session tab restoration.
- Normal startup restores workspace folders; untitled notes have local recovery checkpoints. Complete general tab-session restoration remains unfinished.

## Remaining implementation

- Fuller EPUB publisher stylesheet and fixed-layout reproduction. Dedicated EPUB, FB2/FB2 ZIP and CBZ reading, TOC, illustrations, typography settings, bookmarks and progress restoration are implemented.
- Developer ID signing and Apple notarization. Certificates/credentials are required.
- General session recovery for tabs and reader scroll/playback positions; continue improving untitled-note checkpoint reliability.
- Complete PlantUML Salt/Chen dialects and unrestricted diagram/HTML external-resource compatibility.
- Full Office style/chart reproduction, formula recalculation and richer archive content readers.
- GGUF/ONNX/NPZ metadata readers. NPY already has a basic bounded header/value preview.

## Remaining acceptance coverage

- Apple Silicon and Intel GUI interactions on real hardware, supported graphics/drivers and accessibility.
- Real GitHub production upgrades, administrator-required install locations, multi-instance conflicts, canceled authorization and interrupted replacement/recovery.
- Native document providers: password/corrupt documents, office packages, media codecs, font collections and accessibility behavior.
- Active SQLite/WAL sources, source-stability rejection, missing-main sidecars and preservation of the source database.
- Finder cold/warm opening, duplicate/cross-window activation, file-association changes/restoration and native view cleanup.
- Save conflicts and concurrent file operations across windows, watcher refreshes, large readers and platform gestures.

Existing Rust/native/isolated installer checks are useful but do not prove all of these scenarios. Run relevant checks when validating a change and report the actual coverage in its release notes.

## Outside current scope

Non-macOS platforms are outside the product scope. Existing public release assets are retained.

LSP and plugins are not part of the current product scope. Candidate work is not a release-date promise.

See [formats](file-format-support.md), [architecture](architecture.md), [updates](auto-update-design.md) and [release process](releasing.md).
