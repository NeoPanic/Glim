# Shared reader policy

All application reader boundaries take their numeric values from [assets/reader-limits.json](../assets/reader-limits.json). Edit a category’s `value`, preserving its `type`, `unit` and `effect`, then rebuild and install. Null values retire total-size/count gates. Finite values govern a specific feature, page or resource budget; see the behavior description.

Build scripts use `scripts/generate-reader-limits.py` to generate Rust constants, Swift ReaderLimits and embedded JSON in Cargo OUT_DIR. The diagram WebView receives that same JSON. Rendering does not read the policy from disk. Format validity, codec support and UI geometry remain responsibilities of their implementations.

This page is generated from the same policy. After editing it, run `python3 scripts/generate-reader-limits.py --docs` to update both language inventories. Ordinary builds write generated files only to OUT_DIR.

## text

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `text.editable_bytes` | 32 MiB | bytes | fallback | In-memory editor working budget; larger/long-line documents use editable pages and streaming atomic saves, without a file-size rejection. |
| `text.editable_newlines` | 400,000 | items | fallback | In-memory editor working budget; larger/long-line documents use editable pages and streaming atomic saves, without a file-size rejection. |
| `text.line_bytes` | 64 KiB | bytes | fallback | In-memory editor working budget; larger/long-line documents use editable pages and streaming atomic saves, without a file-size rejection. |
| `text.page_bytes` | 1 MiB | bytes | budget | Bytes per normal text page; following pages preserve the rest of the file. |
| `text.full_bytes` | 256 MiB | bytes | budget | Expanded-page working budget, not a full-file gate; previous/next controls keep the rest accessible. Editable and streamed sources also retain their own working budgets. |
| `text.full_newlines` | 4,000,000 | items | budget | Expanded-page working budget, not a full-file gate; previous/next controls keep the rest accessible. Editable and streamed sources also retain their own working budgets. |
| `text.layout_run_bytes` | 1 KiB | bytes | budget | Maximum text run per virtual read-only row; long logical lines wrap into further rows without losing source bytes. |

## binary

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `binary.preview_bytes` | 256 KiB | bytes | page | Bytes per hex page; previous/next controls expose the complete file. |

## image

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `image.input_bytes` | None | bytes | unlimited | File-backed native image loading has no application input-byte or total-pixel gate; File bytes preserves complete source access. |
| `image.pixels` | None | pixels | unlimited | File-backed native image loading has no application input-byte or total-pixel gate; File bytes preserves complete source access. |
| `image.decoded_bytes` | 512 MiB | bytes | budget | Active native/QOI preview raster working memory. QOI samples pixels while streaming instead of rejecting wide source scanlines; actual-size mode may use a scaled raster and complete original file bytes remain accessible. |
| `image.preview_edge` | 4,000 | pixels | budget | Initial ImageIO thumbnail resolution. Actual-size mode may increase the raster within image.decoded_bytes; it never rejects a file based on original dimensions. |

## gif

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `gif.preview_edge` | 2,048 | pixels | budget | Displayed animation texture edge, independent of source size and frame count. |
| `gif.canvas_bytes` | 512 MiB | bytes | budget | Compositing working memory; oversized source canvases are sampled while decoding rows instead of rejecting the animation. |
| `gif.batch_bytes` | 16 MiB | bytes | budget | Per-batch animation prefetch budget; consumed frames are released and later frames remain available. |
| `gif.batch_frames` | 3 | items | budget | Per-batch animation prefetch budget; consumed frames are released and later frames remain available. |

## tiff

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `tiff.decode_buffer_bytes` | None | bytes | unlimited | ImageIO loads the requested TIFF page on demand, with no application page-count or aggregate-preview total gate. |
| `tiff.pages` | None | items | unlimited | ImageIO loads the requested TIFF page on demand, with no application page-count or aggregate-preview total gate. |
| `tiff.preview_decoded_bytes` | None | bytes | unlimited | ImageIO loads the requested TIFF page on demand, with no application page-count or aggregate-preview total gate. |
| `tiff.preview_encoded_bytes` | None | bytes | unlimited | ImageIO loads the requested TIFF page on demand, with no application page-count or aggregate-preview total gate. |

## inline_image

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `inline_image.preview_edge` | 2,048 | pixels | budget | Inline thumbnail resolution/decoded/output working budget; the original remains available through its standalone reader or source URL. |
| `inline_image.encoded_bytes` | None | bytes | unlimited | File-backed or streamed image preparation has no application encoded-file-size gate. |
| `inline_image.decoded_bytes` | 64 MiB | bytes | budget | Inline thumbnail resolution/decoded/output working budget; the original remains available through its standalone reader or source URL. |
| `inline_image.output_overhead_bytes` | 1 MiB | bytes | budget | Inline thumbnail resolution/decoded/output working budget; the original remains available through its standalone reader or source URL. |

## font

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `font.input_bytes` | None | bytes | unlimited | System file-backed font metadata retains all faces, resources and sample text without application total gates. |
| `font.faces` | None | items | unlimited | System file-backed font metadata retains all faces, resources and sample text without application total gates. |
| `font.resource_types` | None | items | unlimited | System file-backed font metadata retains all faces, resources and sample text without application total gates. |
| `font.sample_chars` | None | characters | unlimited | System file-backed font metadata retains all faces, resources and sample text without application total gates. |

## playlist

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `playlist.files` | None | items | unlimited | Virtualized playlist rows retain every eligible file; no application file-count gate. |

## mkv

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `mkv.playlist_bytes` | None | bytes | unlimited | Read frames from disk, index timestamps on disk and write fragmented MP4 for the selected playlist item in a cancellable helper; no total size/count/duration gate. |
| `mkv.input_bytes` | None | bytes | unlimited | Read frames from disk, index timestamps on disk and write fragmented MP4 for the selected playlist item in a cancellable helper; no total size/count/duration gate. |
| `mkv.frame_bytes` | 16 MiB | bytes | budget | One active compressed-frame/codec-metadata allocation budget, independent of movie size; original file bytes remain accessible. |
| `mkv.frames` | None | items | unlimited | Read frames from disk, index timestamps on disk and write fragmented MP4 for the selected playlist item in a cancellable helper; no total size/count/duration gate. |
| `mkv.parser_read_bytes` | 2 GiB | bytes | budget | Parser work allowance resets after each frame, not cumulatively across the movie; optional seek metadata is skipped in a streaming pass. |
| `mkv.parser_operations` | 20,000,000 | operations | budget | Parser work allowance resets after each frame, not cumulatively across the movie; optional seek metadata is skipped in a streaming pass. |
| `mkv.tracks` | None | items | unlimited | Read frames from disk, index timestamps on disk and write fragmented MP4 for the selected playlist item in a cancellable helper; no total size/count/duration gate. |
| `mkv.duration_seconds` | None | seconds | unlimited | Read frames from disk, index timestamps on disk and write fragmented MP4 for the selected playlist item in a cancellable helper; no total size/count/duration gate. |
| `mkv.nal_units` | None | items | unlimited | Read frames from disk, index timestamps on disk and write fragmented MP4 for the selected playlist item in a cancellable helper; no total size/count/duration gate. |
| `mkv.nesting` | 16 | levels | budget | Track-metadata parser stack budget; optional chapters/tags/cues are skipped, and file-byte inspection remains available. |
| `mkv.elements` | None | items | unlimited | Read frames from disk, index timestamps on disk and write fragmented MP4 for the selected playlist item in a cancellable helper; no total size/count/duration gate. |

## json

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `json.input_bytes` | 16 MiB | bytes | fallback | Structured-tree working budget; Source retains the complete editable document, with pages for large files. |
| `json.field_chars` | 512 | characters | budget | Tree-label summary only; expand containers or double-click to inspect the complete key/value. Source retains all content. |
| `json.nodes` | 250,000 | items | fallback | Structured-tree working budget; Source retains the complete editable document, with pages for large files. |
| `json.description_chars` | 80 | characters | budget | Tree-label summary only; expand containers or double-click to inspect the complete key/value. Source retains all content. |
| `json.array_summary_chars` | 120 | characters | budget | Tree-label summary only; expand containers or double-click to inspect the complete key/value. Source retains all content. |
| `json.auto_expand_children` | 16 | items | budget | Initial auto-expansion policy only; collapsed children can still be expanded explicitly. |

## csv

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `csv.input_bytes` | 16 MiB | bytes | budget | Active page/record decoding memory, not a total-file gate. Oversized individual records remain accessible in editable Source pages. |
| `csv.rows` | 100,000 | items | page | Active table page/window size; row and column navigation exposes the remaining records. |
| `csv.columns` | 256 | items | page | Active table page/window size; row and column navigation exposes the remaining records. |
| `csv.cells` | 1,000,000 | items | page | Active table page/window size; row and column navigation exposes the remaining records. |
| `csv.cell_chars` | 4,096 | characters | budget | Visible single-line cell excerpt; double-click retains the complete value and uses paged viewing for large values. |

## spreadsheet

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `spreadsheet.sheets` | None | items | unlimited | XLSX cells, sheet names and saved formulas stream into a disk-backed index with paged values; there is no input/sheet/row/column/value/HTML total gate. Legacy XLS and oversized individual XML records use native Quick Look without an application size cap. |
| `spreadsheet.rows` | None | items | unlimited | XLSX cells, sheet names and saved formulas stream into a disk-backed index with paged values; there is no input/sheet/row/column/value/HTML total gate. Legacy XLS and oversized individual XML records use native Quick Look without an application size cap. |
| `spreadsheet.columns` | None | items | unlimited | XLSX cells, sheet names and saved formulas stream into a disk-backed index with paged values; there is no input/sheet/row/column/value/HTML total gate. Legacy XLS and oversized individual XML records use native Quick Look without an application size cap. |
| `spreadsheet.cell_chars` | None | characters | unlimited | XLSX cells, sheet names and saved formulas stream into a disk-backed index with paged values; there is no input/sheet/row/column/value/HTML total gate. Legacy XLS and oversized individual XML records use native Quick Look without an application size cap. |
| `spreadsheet.input_bytes` | None | bytes | unlimited | XLSX cells, sheet names and saved formulas stream into a disk-backed index with paged values; there is no input/sheet/row/column/value/HTML total gate. Legacy XLS and oversized individual XML records use native Quick Look without an application size cap. |
| `spreadsheet.sheet_text_bytes` | None | bytes | unlimited | XLSX cells, sheet names and saved formulas stream into a disk-backed index with paged values; there is no input/sheet/row/column/value/HTML total gate. Legacy XLS and oversized individual XML records use native Quick Look without an application size cap. |
| `spreadsheet.html_bytes` | None | bytes | unlimited | XLSX cells, sheet names and saved formulas stream into a disk-backed index with paged values; there is no input/sheet/row/column/value/HTML total gate. Legacy XLS and oversized individual XML records use native Quick Look without an application size cap. |
| `spreadsheet.column_window` | 256 | columns | page | Columns per worksheet presentation page, independent of the SQLite browser. |
| `spreadsheet.catalog_window` | 5,000 | items | page | Worksheet catalog entries per page. |

## xmind

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `xmind.title_bytes` | 16 KiB | bytes | fallback | Mind-map working budget. Exceeding it, including parser stack exhaustion, opens complete content.json/content.xml pages with cancellable on-demand decompression. |
| `xmind.nesting` | 64 | levels | fallback | Mind-map working budget. Exceeding it, including parser stack exhaustion, opens complete content.json/content.xml pages with cancellable on-demand decompression. |
| `xmind.content_bytes` | 16 MiB | bytes | fallback | Mind-map working budget. Exceeding it, including parser stack exhaustion, opens complete content.json/content.xml pages with cancellable on-demand decompression. |
| `xmind.topics` | 10,000 | items | fallback | Mind-map working budget. Exceeding it, including parser stack exhaustion, opens complete content.json/content.xml pages with cancellable on-demand decompression. |
| `xmind.input_bytes` | None | bytes | unlimited | Open the ZIP container from disk without a total file-size or entry-count gate. |
| `xmind.archive_entries` | None | items | unlimited | Open the ZIP container from disk without a total file-size or entry-count gate. |
| `xmind.sheets` | 100 | items | fallback | Mind-map working budget. Exceeding it, including parser stack exhaustion, opens complete content.json/content.xml pages with cancellable on-demand decompression. |
| `xmind.relationships` | 10,000 | items | fallback | Mind-map working budget. Exceeding it, including parser stack exhaustion, opens complete content.json/content.xml pages with cancellable on-demand decompression. |
| `xmind.note_bytes` | 64 KiB | bytes | fallback | Mind-map working budget. Exceeding it, including parser stack exhaustion, opens complete content.json/content.xml pages with cancellable on-demand decompression. |
| `xmind.labels` | 128 | items | fallback | Mind-map working budget. Exceeding it, including parser stack exhaustion, opens complete content.json/content.xml pages with cancellable on-demand decompression. |
| `xmind.xml_nodes` | 200,000 | items | fallback | Mind-map working budget. Exceeding it, including parser stack exhaustion, opens complete content.json/content.xml pages with cancellable on-demand decompression. |
| `xmind.xml_nesting` | 256 | levels | fallback | Mind-map working budget. Exceeding it, including parser stack exhaustion, opens complete content.json/content.xml pages with cancellable on-demand decompression. |
| `xmind.link_bytes` | 16 KiB | bytes | fallback | Mind-map working budget. Exceeding it, including parser stack exhaustion, opens complete content.json/content.xml pages with cancellable on-demand decompression. |
| `xmind.labels_bytes` | 16 KiB | bytes | fallback | Mind-map working budget. Exceeding it, including parser stack exhaustion, opens complete content.json/content.xml pages with cancellable on-demand decompression. |

## archive

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `archive.entries` | None | items | unlimited | Enumerate all entries and spool the report for bounded page display; no total entry-count gate. |
| `archive.bytes` | 32 MiB | bytes | budget | Structured XML projection working buffer; larger content streams in full into the paged source report. |

## numpy

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `numpy.header_bytes` | 64 KiB | bytes | budget | Header string-token working buffer. Scan fields across the complete streamed header; preserve all header bytes and all supported numeric values in the paged report. |
| `numpy.values` | None | items | unlimited | Supported numeric values stream to a disk-backed paged report; no prefix-only value count. |

## geometry

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `geometry.input_bytes` | 16 MiB | bytes | fallback | Static mesh projection working budget; larger projections open complete source/hex pages rather than rejecting or silently cutting the file. |
| `geometry.vertices` | 100,000 | items | fallback | Static mesh projection working budget; larger projections open complete source/hex pages rather than rejecting or silently cutting the file. |
| `geometry.faces` | 100,000 | items | fallback | Static mesh projection working budget; larger projections open complete source/hex pages rather than rejecting or silently cutting the file. |
| `geometry.polygon_vertices` | 256 | items | fallback | Static mesh projection working budget; larger projections open complete source/hex pages rather than rejecting or silently cutting the file. |

## bdf

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `bdf.glyph_edge` | 64 | pixels | fallback | Bitmap-font projection working budget; beyond it, complete BDF source pages remain available instead of a silently shortened glyph list. |
| `bdf.glyphs` | 64 | items | fallback | Bitmap-font projection working budget; beyond it, complete BDF source pages remain available instead of a silently shortened glyph list. |

## executable

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `executable.input_bytes` | None | bytes | unlimited | No file/name/count/report total gate. Read metadata through the io.cache_bytes working cache and spool reports; cache exhaustion opens complete file-byte pages. CString scans do not use the library prefix cutoff. |
| `executable.name_chars` | None | characters | unlimited | No file/name/count/report total gate. Read metadata through the io.cache_bytes working cache and spool reports; cache exhaustion opens complete file-byte pages. CString scans do not use the library prefix cutoff. |
| `executable.architectures` | None | items | unlimited | No file/name/count/report total gate. Read metadata through the io.cache_bytes working cache and spool reports; cache exhaustion opens complete file-byte pages. CString scans do not use the library prefix cutoff. |
| `executable.hex_bytes` | 4 KiB | bytes | budget | Hex excerpt within the metadata report; complete file bytes remain accessible through raw paging. |
| `executable.segments` | None | items | unlimited | No file/name/count/report total gate. Read metadata through the io.cache_bytes working cache and spool reports; cache exhaustion opens complete file-byte pages. CString scans do not use the library prefix cutoff. |
| `executable.sections` | None | items | unlimited | No file/name/count/report total gate. Read metadata through the io.cache_bytes working cache and spool reports; cache exhaustion opens complete file-byte pages. CString scans do not use the library prefix cutoff. |
| `executable.report_bytes` | None | bytes | unlimited | No file/name/count/report total gate. Read metadata through the io.cache_bytes working cache and spool reports; cache exhaustion opens complete file-byte pages. CString scans do not use the library prefix cutoff. |
| `executable.libraries` | None | items | unlimited | No file/name/count/report total gate. Read metadata through the io.cache_bytes working cache and spool reports; cache exhaustion opens complete file-byte pages. CString scans do not use the library prefix cutoff. |
| `executable.dynamic_entries` | None | items | unlimited | No file/name/count/report total gate. Read metadata through the io.cache_bytes working cache and spool reports; cache exhaustion opens complete file-byte pages. CString scans do not use the library prefix cutoff. |
| `executable.name_scan_bytes` | None | bytes | unlimited | No file/name/count/report total gate. Read metadata through the io.cache_bytes working cache and spool reports; cache exhaustion opens complete file-byte pages. CString scans do not use the library prefix cutoff. |

## tensor

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `tensor.header_bytes` | None | bytes | unlimited | Stream metadata records and dtype counts to disk. Oversized individual records use complete header pages without loading tensor weights. No header/name/shape/count/report total gate. |
| `tensor.rows` | None | items | unlimited | Stream metadata records and dtype counts to disk. Oversized individual records use complete header pages without loading tensor weights. No header/name/shape/count/report total gate. |
| `tensor.report_bytes` | None | bytes | unlimited | Stream metadata records and dtype counts to disk. Oversized individual records use complete header pages without loading tensor weights. No header/name/shape/count/report total gate. |
| `tensor.shape_dimensions` | None | items | unlimited | Stream metadata records and dtype counts to disk. Oversized individual records use complete header pages without loading tensor weights. No header/name/shape/count/report total gate. |
| `tensor.metadata_entries` | None | items | unlimited | Stream metadata records and dtype counts to disk. Oversized individual records use complete header pages without loading tensor weights. No header/name/shape/count/report total gate. |
| `tensor.name_chars` | None | characters | unlimited | Stream metadata records and dtype counts to disk. Oversized individual records use complete header pages without loading tensor weights. No header/name/shape/count/report total gate. |
| `tensor.dtype_chars` | None | characters | unlimited | Stream metadata records and dtype counts to disk. Oversized individual records use complete header pages without loading tensor weights. No header/name/shape/count/report total gate. |
| `tensor.metadata_key_chars` | None | characters | unlimited | Stream metadata records and dtype counts to disk. Oversized individual records use complete header pages without loading tensor weights. No header/name/shape/count/report total gate. |
| `tensor.metadata_value_chars` | None | characters | unlimited | Stream metadata records and dtype counts to disk. Oversized individual records use complete header pages without loading tensor weights. No header/name/shape/count/report total gate. |

## sqlite

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `sqlite.snapshot_bytes` | None | bytes | unlimited | Copy the snapshot in fixed-size blocks, without an application database total-size gate. |
| `sqlite.page_rows` | 500 | rows | page | Rows buffered per table/schema page, independent of total table size; a persistent pager provides row counts, page jumps and first/last navigation. |
| `sqlite.tables` | None | items | unlimited | List all tables and views without an application catalog-size cap. |
| `sqlite.columns` | None | columns | unlimited | All SQLite columns are accessible; horizontal virtualization bounds the working set. |
| `sqlite.value_bytes` | None | bytes | unlimited | Use native SQLite format constraints; full values and definitions are read in text.page_bytes windows with previous/next navigation. |
| `sqlite.text_bytes` | 512 | bytes | budget | Visible cell summary only; double-click opens a complete paged value, with binary values shown as hex. |
| `sqlite.schema_text_bytes` | None | bytes | unlimited | Use native SQLite format constraints; full values and definitions are read in text.page_bytes windows with previous/next navigation. |
| `sqlite.blob_bytes` | 16 | bytes | budget | Visible cell summary only; double-click opens a complete paged value, with binary values shown as hex. |
| `sqlite.query_seconds` | 5 | seconds | timeout | Initial cancellable query time allowance; Retry with more time explicitly extends it. |
| `sqlite.busy_milliseconds` | 1,000 | milliseconds | timeout | Native SQLite lock-wait allowance; a busy database can be retried and is not rejected by file size. |
| `sqlite.cache_kib` | 8,192 | KiB | budget | SQLite page-cache working memory; larger databases read further pages from disk. |
| `sqlite.column_window` | 32 | columns | page | Columns fetched per horizontal viewport window, without a total column limit. |

## markup

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `markup.scan_bytes` | None | bytes | unlimited | Scan the current text for image references; large documents use source pages instead of a total reference-scan cutoff. |

## remote_image

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `remote_image.input_bytes` | None | bytes | unlimited | HTTP downloads stream to a temporary file and prepare bounded thumbnails; source input size is not an application rejection gate. |
| `remote_image.timeout_seconds` | 20 | seconds | timeout | Idle/no-progress timeout; active downloads have no fixed total elapsed-time ceiling. |
| `remote_image.first_attempt_seconds` | 8 | seconds | timeout | Connection establishment allowance, including the first GitHub raw URL attempt before API fallback. |
| `remote_image.svg_embedded_bytes` | 64 MiB | bytes | budget | Per-embedded-image working memory in SVG preparation; original SVG content remains available through Source or file-byte access. |
| `remote_image.redirects` | 5 | items | budget | Redirect-chain budget prevents request loops; it does not constrain image-file size. |
| `remote_image.connect_seconds` | 8 | seconds | timeout | Connection establishment allowance, including the first GitHub raw URL attempt before API fallback. |
| `remote_image.minimum_bytes_per_second` | 1 | bytes/second | timeout | Minimum progress for the idle timeout, not a bandwidth ceiling. |

## diagram

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `diagram.blocks` | 8 | items | budget | Maximum diagrams per background render batch. Visible blocks queue automatically; this does not limit total document formulas or require manual paging. |
| `diagram.source_bytes` | 256 KiB | bytes | fallback | Per-diagram structured-render working budget; failures preserve the complete block source. Other blocks remain available on demand. |
| `diagram.svg_bytes` | 8 MiB | bytes | fallback | Per-diagram structured-render working budget; failures preserve the complete block source. Other blocks remain available on demand. |
| `diagram.aggregate_encoded_bytes` | 64 MiB | bytes | budget | Encoded/decoded working budget for the viewport diagram cache and active render batch. Older resources are evicted and can be requested again; total document blocks remain unrestricted. |
| `diagram.aggregate_decoded_bytes` | 128 MiB | bytes | budget | Encoded/decoded working budget for the viewport diagram cache and active render batch. Older resources are evicted and can be requested again; total document blocks remain unrestricted. |
| `diagram.preview_edge` | 2,400 | pixels | budget | Active raster resolution/scale budget. Page raster dimensions also adapt to aggregate working memory, independently of document length. |
| `diagram.startup_seconds` | 15 | seconds | timeout | Cancellable renderer startup/per-block time allowance; an unfinished block shows its complete source instead of rejecting the Markdown document. |
| `diagram.block_seconds` | 8 | seconds | timeout | Cancellable renderer startup/per-block time allowance; an unfinished block shows its complete source instead of rejecting the Markdown document. |
| `diagram.ipc_bytes` | 128 MiB | bytes | budget | One page/request or one block IPC working buffer; document batches are paged and oversized render results preserve the complete source block. |
| `diagram.slot_bytes` | 16 MiB | bytes | budget | One page/request or one block IPC working buffer; document batches are paged and oversized render results preserve the complete source block. |
| `diagram.max_upscale` | 2 | scale | budget | Active raster resolution/scale budget. Page raster dimensions also adapt to aggregate working memory, independently of document length. |
| `diagram.request_bytes` | 64 MiB | bytes | budget | One page/request or one block IPC working buffer; document batches are paged and oversized render results preserve the complete source block. |
| `diagram.message_bytes` | 16388 KiB | bytes | budget | One page/request or one block IPC working buffer; document batches are paged and oversized render results preserve the complete source block. |
| `diagram.mermaid_edges` | 1,000 | edges | fallback | Per-diagram structured-render working budget; failures preserve the complete block source. Other blocks remain available on demand. |
| `diagram.math_macros` | 1,000 | expansions | budget | Math interpreter expansion budget to bound recursive work; the complete formula source remains available when rendering cannot finish. |

## book

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `book.chapter_bytes` | 4 MiB | bytes | fallback | Structured chapter working budget. When exceeded, retain chapter navigation and provide complete UTF-8 source pages, with streamed transcoding for supported legacy encodings. No chapter is silently cut off. |
| `book.xml_nodes` | 250,000 | nodes | fallback | Structured chapter working budget. When exceeded, retain chapter navigation and provide complete UTF-8 source pages, with streamed transcoding for supported legacy encodings. No chapter is silently cut off. |
| `book.resource_bytes` | None | bytes | unlimited | Requested archive images stream to a private file and use native thumbnail decoding; no source-resource byte or pixel-edge gate. Only the active chapter/page previews are retained. |
| `book.preview_edge` | 2,400 | pixels | budget | Thumbnail resolution for the requested illustration/comic page; this does not reject the original image. |
| `book.input_bytes` | None | bytes | unlimited | No application book/archive/chapter/catalog/metadata total gate. Structured parsing keeps a working budget; oversized metadata exposes every archive member as complete source or byte pages. |
| `book.text_bytes` | 64 MiB | bytes | fallback | Structured chapter working budget. When exceeded, retain chapter navigation and provide complete UTF-8 source pages, with streamed transcoding for supported legacy encodings. No chapter is silently cut off. |
| `book.archive_entries` | None | items | unlimited | No application book/archive/chapter/catalog/metadata total gate. Structured parsing keeps a working budget; oversized metadata exposes every archive member as complete source or byte pages. |
| `book.chapters` | None | items | unlimited | No application book/archive/chapter/catalog/metadata total gate. Structured parsing keeps a working budget; oversized metadata exposes every archive member as complete source or byte pages. |
| `book.metadata_bytes` | None | bytes | unlimited | No application book/archive/chapter/catalog/metadata total gate. Structured parsing keeps a working budget; oversized metadata exposes every archive member as complete source or byte pages. |
| `book.toc_entries` | None | items | unlimited | No application book/archive/chapter/catalog/metadata total gate. Structured parsing keeps a working budget; oversized metadata exposes every archive member as complete source or byte pages. |
| `book.toc_depth` | 32 | levels | budget | Visual TOC indentation only; retain all nested entries and their actual hierarchy depth instead of cutting the catalog. |
| `book.fb2_depth` | 64 | levels | fallback | Structured chapter working budget. When exceeded, retain chapter navigation and provide complete UTF-8 source pages, with streamed transcoding for supported legacy encodings. No chapter is silently cut off. |
| `book.html_depth` | 128 | levels | fallback | Structured chapter working budget. When exceeded, retain chapter navigation and provide complete UTF-8 source pages, with streamed transcoding for supported legacy encodings. No chapter is silently cut off. |
| `book.chapter_decoded_bytes` | 128 MiB | bytes | fallback | Structured chapter working budget. When exceeded, retain chapter navigation and provide complete UTF-8 source pages, with streamed transcoding for supported legacy encodings. No chapter is silently cut off. |
| `book.chapter_images` | 128 | items | fallback | Structured chapter working budget. When exceeded, retain chapter navigation and provide complete UTF-8 source pages, with streamed transcoding for supported legacy encodings. No chapter is silently cut off. |
| `book.image_decoded_bytes` | None | bytes | unlimited | Requested archive images stream to a private file and use native thumbnail decoding; no source-resource byte or pixel-edge gate. Only the active chapter/page previews are retained. |
| `book.image_edge` | None | pixels | unlimited | Requested archive images stream to a private file and use native thumbnail decoding; no source-resource byte or pixel-edge gate. Only the active chapter/page previews are retained. |

## book_state

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `book_state.bookmarks` | None | items | unlimited | Persist all bookmarks, labels and reading-state entries; no application bookmark-count or input-size gate. |
| `book_state.label_bytes` | None | bytes | unlimited | Persist all bookmarks, labels and reading-state entries; no application bookmark-count or input-size gate. |
| `book_state.input_bytes` | None | bytes | unlimited | Persist all bookmarks, labels and reading-state entries; no application bookmark-count or input-size gate. |
| `book_state.label_chars` | 120 | characters | budget | Derived bookmark/header/catalog label excerpt; chapter targets and full-metadata/text dialogs retain the complete content. |

## geo

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `geo.points` | 10,000 | items | page | Coordinates per table page; previous/next navigation covers all GPX/KML points and Source preserves the complete XML. |

## workspace

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `workspace.search_results` | 60 | items | page | Visible page/batch size; later search results or saved workspaces remain accessible through continuation controls. |
| `workspace.search_entries` | None | items | unlimited | Search traverses all eligible entries with cancellable result pages; there is no entry-scan total cutoff. |
| `workspace.recent_workspaces` | 30 | items | page | Visible page/batch size; later search results or saved workspaces remain accessible through continuation controls. |

## update

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `update.session_bytes` | None | bytes | unlimited | Stream session JSON and preserve all actual windows, roots and tabs while validating identities and active indexes. |
| `update.workspaces` | None | items | unlimited | Stream session JSON and preserve all actual windows, roots and tabs while validating identities and active indexes. |
| `update.roots_per_workspace` | None | items | unlimited | Stream session JSON and preserve all actual windows, roots and tabs while validating identities and active indexes. |
| `update.tabs_per_workspace` | None | items | unlimited | Stream session JSON and preserve all actual windows, roots and tabs while validating identities and active indexes. |

## audio_artwork

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `audio_artwork.input_bytes` | None | bytes | unlimited | Native metadata artwork is thumbnailed without an application byte gate. Open full artwork exposes its complete original snapshot. |
| `audio_artwork.preview_edge` | 600 | pixels | budget | Active cover thumbnail resolution; Open full artwork retains full-resolution source access. |

## io

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `io.buffer_bytes` | 64 KiB | bytes | budget | Fixed working buffer for cancellable streaming reads, copies and report generation, independent of total source size. |
| `io.cache_bytes` | 64 MiB | bytes | budget | Working cache/projection allowance for decoded text, executable metadata and vector/font projection buffers. Old text ranges regenerate; oversized representations retain complete source/byte pages. |
| `io.record_bytes` | 64 MiB | bytes | budget | One active XML/structured metadata record or value, independent of total file size; oversized records retain complete Source/header pages or native preview access. |

## svg

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `svg.event_bytes` | 64 MiB | bytes | budget | One active XML markup/text record in the sanitizer, not a document total gate; native File bytes retains the complete source when preparation exceeds this budget. |

## word

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `word.zoom_min_percent` | 25 | percent | budget | Minimum Word reading-view presentation zoom; does not constrain source content. |
| `word.zoom_max_percent` | 400 | percent | budget | Maximum Word reading-view presentation zoom; does not constrain source content. |
| `word.zoom_step_percent` | 25 | percent | budget | Zoom-in/out button increment in Word reading view; reset restores 100 percent. |

## drawio

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `drawio.xml_bytes` | 32 MiB | bytes | budget | Maximum source or expanded diagram XML per preview; larger documents fall back to source pages. |
| `drawio.nesting` | 64 | levels | budget | Bound parent traversal and detect cyclic groups. |
| `drawio.preview_bytes` | 128 MiB | bytes | budget | Aggregate encoded PNG page memory; larger documents fall back to source pages. |

## opml

| Key | Value | Unit | Effect | Behavior |
| --- | ---: | --- | --- | --- |
| `opml.xml_bytes` | 16 MiB | bytes | budget | OPML XML preview budget; fall back to source pages when exceeded. |
| `opml.nodes` | 100,000 | items | budget | Maximum resident outline nodes. |
| `opml.nesting` | 256 | levels | budget | Maximum outline nesting depth. |
