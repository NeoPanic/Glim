# Local gpui-base patch

Based on crates.io gpui-base 0.7.1 (Apache-2.0), archive checksum `3cf698677b253118459f552fe15513858c669d79a25837a94e480db2ae330050`.

TextView exposes an optional `on_task_toggle` callback. Markdown task markers use the existing native Checkbox control only when a callback and source span are available; all other TextViews retain their original read-only rendering. The callback provides the list item's original parsed source range and requested checked state. Applications own snapshot validation, edits and persistence. Checkbox pointer activation does not start text selection; keyboard and accessibility behavior come from the existing Checkbox control.

The patch is confined to `src/text/text_view.rs`, `src/text/state.rs` and `src/text/node.rs`. The dependency version remains pinned at 0.7.1.
