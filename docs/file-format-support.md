# File formats and limits

**English** · [简体中文](file-format-support.zh-CN.md) · [Home](../README.md)

A focused file viewer for macOS.

Support means the behavior listed below, not full compatibility with every feature of a format. Files open in workspace tabs. The supported targets are macOS Apple Silicon and Intel; the table describes capabilities on those two architectures.

## Format catalog

| Formats | Current behavior |
| --- | --- |
| Rust, C/C++, Go, Swift, Java, C#, Kotlin, Zig, Python, Ruby, PHP, Lua, Scala, Elixir, shell | Source highlighting, folding, search, Minimap and basic UTF-8 editing |
| JS/TS/JSX/TSX, HTML, CSS, Astro, Svelte, ERB/EJS, Jinja/Jinja2/Nunjucks/Twig | Source highlighting; some templates use lightweight coloring rather than full grammars |
| Vue, XML/plist, SCSS/LESS, Cython, CUDA/Metal, GLSL/OSL | Reuse related language highlighting; no dedicated semantic validation |
| Dockerfile/Containerfile, Makefile/CMake, Nix, INI, `.env`, ignore files and related configuration | Highlighted or lightly colored text; editing, not execution |
| Markdown | Native reading layout, tables, task lists, footnotes, alerts, frontmatter, diagrams, formulas, relative images and local links; source editing |
| HTML/HTM | WebKit browser preview with CSS/JavaScript and editable source; relative local assets within the enclosing Git project (otherwise the document directory) load in streamed chunks |
| TXT, logs and other UTF-8 text | Basic editing; large or very long-line documents use editable UTF-8 pages |
| JSON/JSONC/JSON5, NDJSON/JSONL, GeoJSON, notebooks | Source and an on-demand read-only tree. JSONC accepts comments/trailing commas; NDJSON becomes a line-record array. GeoJSON is not a map and notebooks are not executed |
| CSV/TSV | Virtualized read-only table and editable source; quoted/multiline fields, BOM and uneven rows supported |
| XMind | Modern JSON and legacy XML containers; sheets, attached/floating topic trees, folding, zoom and topic details. Plain notes, labels, HTTP(S) links and relationship endpoints; read-only |
| PNG/JPEG/WebP/GIF/BMP/TIFF/ICO/SVG/ICNS/EXR/HDR/QOI | Image previews; GIF animation, multipage TIFF, SDR conversion for EXR/HDR. ICNS needs a PNG representation; SVG external file resources are not loaded |
| PDF — macOS | Embedded PDFKit preview with pages, zoom, text selection and password entry |
| DOC/DOCX/PPT/PPTX/RTF — macOS | Embedded Quick Look read-only preview; availability and layout depend on the system provider |
| XLSX / XLS — macOS | XLSX streams into a paged cell-address/value/formula index; XLS and unsupported structures use Quick Look. No formula recalculation or style/chart reproduction |
| KEY/Pages/Numbers — macOS | Quick Look read-only preview, including directory packages; no editing or presentation animations |
| MP3/MP4/MKV/M4A/M4V/MOV/AAC/WAV/AIF/AIFF/CAF/FLAC — macOS | Embedded AVKit playback and ordered/shuffled folder playlists. H.264/AAC-LC MKV is losslessly remuxed to a temporary MP4, including Matroska mislabeled `.mp4`. Other MKV codecs and embedded subtitles are unsupported; other media decoding depends on system codecs |
| TTF/OTF/TTC/OTC/DFONT — macOS | CoreText font/collection inspection, glyphs and custom samples; fonts are not installed |
| DB/DB3/SQLite/SQLite3 and recognized SQLite headers — macOS | Read-only temporary snapshot; tables/views, schema, SQL definitions and paged records. Associated WAL/SHM/journal files resolve to the main database when available |
| safetensors | Tensor names, shapes, dtypes, parameter counts and metadata from the header; weights are not loaded or executed |
| `.diff`/`.patch`, working/staged changes and commit history | Read-only diffs, inline/side-by-side modes, explicit staging/unstaging and staged-only commits |
| Binary dotfiles such as `.DS_Store` | Bounded hexadecimal/ASCII preview; no semantic decoding or editing |

A `.key` text file can be a cryptographic key: only ZIP containers and document packages enter the Keynote reader. File associations are based on extension/type and can affect both uses.

## Partial previews

| Formats | Current behavior and boundary |
| --- | --- |
| AsciiDoc/Org/RST | Basic headings, lists and code-block reading projection with editable original text; not complete dialect rendering |
| SRT/VTT | Read subtitles by timestamp; edit source. Not synchronized with media playback |
| MATLAB/Stan/HTTP/HCL/Dhall/KDL | Basic lexical coloring and editing; scripts, requests and configurations are not executed |
| D2/Pikchr/`*.vega.json` | Offline static diagrams; source editing. External imports, datasets and images disabled |
| OBJ/STL | Static mesh projection; OBJ faces and ASCII/binary STL. No interactive rotation, materials or textures |
| GPX/KML | Offline coordinate table and XML source; no map tiles or external resources |
| BDF | A bounded bitmap glyph preview; no font installation |
| ZIP/TAR | Entry names and sizes, with unsafe paths identified; no extraction |
| GZ/BZ2/XZ | On-demand decompression into complete source pages with a bounded rolling cache |
| EPUB / FB2 / FB2 ZIP / CBZ | Dedicated read-only book reader: spine/TOC navigation, chapters or comic pages, inline illustrations, typography/paper controls, bookmarks and saved position. Publisher CSS and fixed-layout EPUB are not reproduced |
| ODT/ODS/ODP | Container listing and selected document text extraction |
| NPY | v1/v2/v3 headers and a bounded preview of basic little-endian f8/f4/i8/i4/u1 values; other types retain metadata, no pickle execution |

XMind uses a consistent reading layout, not the original manual positions or styles. Images, attachments, boundaries and summaries are not reproduced. Encrypted/damaged documents show an error; links are never opened automatically.

## Markdown diagrams and formulas

| Engine | Fences and supported syntax |
| --- | --- |
| Mermaid | `mermaid`/`mmd`; flowcharts, subgraphs, sequence/class/state/ER diagrams, Gantt, pie, mind maps, Journey, Radar, Treemap and engine-supported types. `zenuml` also supported |
| Graphviz | `dot`/`graphviz` |
| PlantUML | `plantuml`/`puml`/`uml`; `@startuml` wrappers optional. Bundled C4/themes/emoji/OpenIconic; basic Salt and Chen ER adapters; `@startdot` routes to Graphviz |
| flowchart.js | `flow`/`flowchart`/`flowchart.js`; explicit Mermaid `flowchart`/`graph` declarations route to Mermaid |
| WaveDrom | `wavedrom`/`wavejson`; JSON/JSON5 input, not JavaScript |
| LaTeX | `$…$`/`$$…$$`, separate `$$` block lines, standalone `\[ … \]`, and `math`/`latex`/`tex`/`katex` fences |
| GitHub Alerts | Blockquotes beginning `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]` or `[!CAUTION]` |

Explicit Mermaid, PlantUML and Graphviz declarations can identify unlabelled fences. Diagrams become static images; source and saved contents stay unchanged. Failed blocks show their error and source. Engines are bundled for offline use and render through system WebKit on macOS.

Salt and Chen ER are partial adapters, not complete PlantUML dialects. Unsupported preprocessing/includes, rich content, external resources and incompatible `foreignObject` output fail visibly. Salt supports common wireframe controls; Chen supports common entities, attributes, relationships, cardinality and participation, not the full EER dialect. External images and unbundled includes are not loaded.

## Shared reader boundaries

Application thresholds are managed in [assets/reader-limits.json](../assets/reader-limits.json). The [generated inventory](reader-limits.md) lists every configured value, unit and effect. Rust, native Swift and diagram JavaScript consume that one policy. Fixed input caps, truncation, paging thresholds, preview dimensions and working-memory/time budgets remain separate concepts; non-Git total-file caps are replaced by streaming, paging or complete source fallback; Git/diff boundaries remain deferred.

Preview limits do not truncate source files or staged contents. Non-UTF-8 ordinary text is not currently decoded. Office previews do not edit, run macros or recalculate formulas.

SQLite copies the main database, WAL and rollback journal into a private snapshot and checks source stability. It does not checkpoint the original. Encrypted databases and arbitrary write SQL are unsupported.

MKV supports H.264/AAC-LC remuxing, including AVC configuration and Annex-B frames. Compressed/encrypted tracks, codec delay and duplicate-timestamp lacing remain unsupported. Original files and display names are preserved; temporary media is released with the native reader. Folder playlists enumerate direct children and skip unsupported items; restored playback is paused.

## Maintenance

`cargo run -p glim-services --example audit_formats --locked -- /path/to/project` samples an explicit format allowlist and prints counts, not private paths or file contents. It is not an exhaustive support audit or a UI test.

See [architecture](architecture.md), [build and release](releasing.md) and [roadmap](TODO.md).

Markdown remote images load over HTTP(S) in the background, using the shared response and timeout budgets. Failed downloads or decoding show a visible placeholder. Common picture light/dark candidates follow the theme; unknown media conditions retain the img fallback. SVG does not run scripts or read external resources. Relative local images retain existing path resolution. System curl is required for this Markdown image path. HTML previews use WebKit networking and layout instead.

Mach-O (including universal), ELF and PE files are recognized by their headers and inspected read-only. The preview shows architecture, word size, byte order, entry point, segments/sections, available dependency names and a hexadecimal excerpt. Metadata working budgets use the shared policy. Programs are never executed or decompiled; malformed inputs report errors; metadata working-budget exhaustion retains complete hex pages, and dependencies are shown only where the corresponding headers can be parsed.

Ebook limits are listed in the shared policy inventory. Text and image resources are loaded locally; remote images and active content are not fetched or executed. DRM EPUB, MOBI/AZW, PDF book layout, publisher stylesheet fidelity and fixed-layout EPUB are outside this reader. Space/Page Down and Shift-Space/Page Up turn viewport pages; Left/Right switch chapters. Reading preferences and bookmarks are stored separately from book files.

XLSX presents a sparse cell index rather than an Excel grid. GPX/KML coordinates and diagram batches have continuation controls. Complex books, XMind, mesh/font projections and tensor headers retain complete source fallbacks. Native images may use a scaled raster; actual-size zoom does not guarantee original-resolution decoding. SQLite full-value windows do not bound the engine’s internal expression allocations. ZIP directory indexing and native Quick Look/WebKit behavior remain library/platform constraints.
