# Glim development

- Product scope: a macOS viewer (Apple Silicon and Intel) for Markdown, source code, and Git diffs, with explicit staging/unstaging controls and a Git commit input (Command-Enter, staged contents only); no cloning. Ordinary UTF-8 files support basic notepad-style editing, automatic saving and explicit saving; diffs, images and unsupported files remain read-only.
- Platform strategy: maintain and release only macOS Apple Silicon and Intel. The repository is macOS-only: do not add non-macOS backends, platform branches, dependencies or release jobs without explicit user direction. Keep existing public release assets intact.
- Keep crate dependencies one-way: glim-app -> glim-services -> glim-core;
  glim-app may also depend on glim-core directly.
- glim-core must not depend on GPUI or perform I/O. Services do not import UI.
- Keep main.rs thin. Put application startup/actions in app/ and presentation in views/.
- Never perform file reads, directory scans, or Git commands inside render methods.
  Schedule blocking work through the background executor and retain/cancel task handles.
- Keep GPUI entities alive across renders. Use theme tokens for UI colors.
- Define reader operations through `views/workspace/reader_actions.rs` (GPUI) or `native/ReaderActions.swift` (native). Inject format-specific ordered controls into the shared toolbar; reuse the same live actions for content context menus. Route native context clicks through `ReaderContextMenu` and build menus with `ReaderMenu`; do not add independent system-menu filtering or reader-specific right-click event monitors. Preserve source-editor editing menus and embedded web/media interactions.
- Do not impose arbitrary file-size limits or reject a supported file because of its size. Do not add file-size caps unless explicitly requested by the user. Support large files through streaming, paging, incremental processing and on-demand decoding. Bound active memory, buffers and concurrent work independently of total file size; keep operations cancellable. When changing an existing reader with a fixed file-size cap, replace that cap with scalable processing instead of preserving or increasing it.
- Manage application reader boundaries only in `assets/reader-limits.json`, with explicit units and effects. Rust, Swift and diagram JavaScript must consume the generated shared policy; do not duplicate numeric thresholds in readers or UI hints. Regenerate the bilingual inventories with `python3 scripts/generate-reader-limits.py --docs` after changing policy values. Format-validity rules and implementation invariants stay with their parsers.
- Keep editing minimal: typing, selection, undo/redo, automatic save and explicit save. Do not add LSP, plugins, speculative abstraction layers, or empty placeholder modules.
- Pin GPUI Kit; retain Cargo.lock. Enable extra features only when used.
- After requested code changes, build/package and install the updated local Glim.app by default, verify the installed bundle and launch it. Keep at most the two most recent installation backups. Do not automatically run tests or UI smoke tests; the user handles testing unless explicitly requested.
- When validation is requested, use cargo fmt --all --check, cargo clippy --workspace --all-targets --locked -- -D warnings, cargo test --workspace --locked, and cargo build --locked.
- When UI smoke testing is requested, use macOS when available and report unverified interactions.
- Document changes to module boundaries in docs/architecture.md.
- When packaging Glim, install the new build over the existing local Glim.app, preserving a recoverable backup and retaining at most two installation backups, unless the user asks for packaging only. Verify the installed bundle and launch it.

- Keep documentation focused on current behavior, architecture and unfinished work. Put release notes in GitHub Releases; do not accumulate per-session operation logs, historical verification reports or personal filesystem inventories in the repository.

- Every GitHub Release must include equivalent English and Chinese release notes, including changes, support limitations, validation and installation requirements.
