# Usage and format prioritization

[简体中文](telemetry.zh-CN.md)

Telemetry follows the existing Settings switch. Enabled installations report feature events and sanitized errors to the configured Aptabase endpoint. Disabling stops delivery immediately and clears queued events. Installation identity is a random 128-bit token, stored in Glim's preferences directory, created only by the enabled background worker. It is unrelated to hardware, account, paths or document contents. It persists across launches and opt-out/re-enable; deleting the preferences token creates a new identity. An unwritable identity store omits the token. Count distinct `installation_id` values to estimate reach; this measures installations, not people. Multiple devices, reinstallations, opted-out users and undelivered events affect that estimate. Aptabase's built-in session count must not be relabeled as unique users. Cross-session identity is a custom property; distinct installation counts require export analysis or a compatible custom query.

## Format ranking

`file_open` is the authoritative new-open attempt/result, with `file_extension`, `file_family`, `result` and `duration_ms`. Reusing an existing tab emits `tab_activated`, not another open. Background reader refreshes can emit `reader_opened`; do not count them as user opens. Known extensionless config names use fixed categories; unrecognized suffixes become `other`, never a user-defined name. Opening a folder emits `workspace_open` separately. Native document preparation occurs after initial routing: inspect `native_load` results as well as `file_open`. Preparation success does not guarantee successful display, media playback or Office conversion inside AppKit.

For a fixed time window and version, rank formats by distinct installations attempting an open, then open attempts. Count attempted reach even when opening fails; otherwise broken formats disappear from demand data. Include open failures, native-load failures, P50/P95 latency and foreground duration. The initial open duration measures routing/loading, not time until every pixel or native document is ready. The current safe extension inventory does not identify every supported format; `other` requires expanding the allowlist, never transmitting arbitrary suffixes.

Export Aptabase events as JSON array, JSONL or CSV (columns `eventName`/`event_name`, `sessionId`/`session_id`, and `props`/`properties`, with nested JSON properties). Run:

```sh
python3 scripts/telemetry-format-report.py events.json --output formats.csv
```

The report sorts by unique installations, reports missing identity separately, deduplicates sequenced events, and uses all identifiable installations present in the supplied export as the reach denominator. Export all events for the same interval for population reach; a file-only export yields reach within file-opening installations. Filter version/time in the export first. The script does not fetch production data or invent historical metrics; older `app_started`-only data cannot recover format history.

## Feature event map

| Area | Events and interpretation |
| --- | --- |
| Lifecycle | `app_started`, `telemetry_enabled`, `app_exited`, `window_opened`, `window_closed`; session duration and open tab/folder counts |
| Reading | `file_open_requested`, `file_open`, `workspace_open`, `reader_opened`, `tab_activated`, `tab_closed`; reader kind, preview/edit capabilities, availability |
| Engagement | `foreground_segment`; page, format, mode and milliseconds while window is active, excluding known prompts/loading/pickers; 30-second flush, transition/deactivation/close/quit flush |
| Editing | `draft_created`, `editing_started`, `file_save_requested`, `file_save`, `draft_save`, save-dialog decisions, rejected formats, close decisions, `draft_recovered`, `move_to_trash` |
| Navigation | `workspace_folder_added`, `workspace_search_completed`, preview and wrap changes, toolbar `reader_action`, Markdown task completion |
| Data and books | CSV/SQLite page requests and load results, SQLite table selections, paged mode/load, book load/page/chapter/bookmark |
| Diagrams | `diagram_render`, safe diagram kind and result; cache completion rather than per-frame paint count |
| Native controls | `native_reader_action`: image copy/OCR/zoom/paging, PDF fit/layout/presentation/navigation, Word mode/zoom, workbook paging/sheet, font selection, media playback, presentation/external-open actions; numeric bridge only |
| Preferences and updates | settings/shortcut openings, theme/language changes, update actions, updater phase transitions when updater is compiled in |
| Diagnostics | `operation_failed`, `error_logged`, `app_panicked`, `native_reader_error`, `error_repeated`, `telemetry_health` |

Page/load events include initialization and refresh; treat them as workload, not necessarily distinct intentional actions. Toolbar intent and actual state-change events describe different stages of one action: do not add them together. Native actions report invocation, not guaranteed completion. The shared native reader controls retain their existing targets and dispatch paths.

## Product decisions

Compare feature sessions against sessions with that capability, using `reader_opened.can_preview`, `can_edit`, format flags; capability does not prove a hidden toolbar was seen. High reach plus failures or slow loads supports urgent reliability work. High reach plus repeated preview switching, search or paging supports examining reader usability. Low reach with long foreground duration may describe a valuable specialist workflow. Foreground occupancy includes idle time and does not establish attention or satisfaction. Removal candidates need low opportunity-adjusted reach across several releases, low engagement, maintenance cost and user feedback; telemetry alone cannot establish that removal is safe. No automatic feature removal is implemented.

## Error and delivery semantics

Network payloads exclude document text, filenames, full paths, search queries, database/table/column names, URLs and raw log/error/panic text. Error properties contain classifications, application source basename/line, component/phase, safe fingerprints, OS error codes and up to three application/GPUI panic symbols. Recent context contains event names only. Full diagnostics remain local. Forced termination and crashes may prevent panic/shutdown delivery.

Structured operation outcomes are the failure-rate source. `operation_failed` and `error_logged` can describe the same failure; do not sum diagnostic events for operation failure rate. Repeated error signatures are suppressed within a minute and emitted as `error_repeated.occurrences` on a later flush or normal shutdown; bounded fingerprint eviction can lose repeat detail. `telemetry_health.errors_suppressed` describes transport compression and must not be added to occurrence totals.

The queue holds at most 256 events, batches at most 25, flushes approximately every two seconds, and uses cancellation, bounded shutdown flush and exponential upload backoff. No persistent outbox or raw-log upload exists. Network failure, contention and queue eviction can lose events; health counters disclose dropped events and upload failures when a later upload succeeds. A long-running process rotates Aptabase session identity daily; installation identity remains stable. Transport errors do not recursively enter diagnostic reporting.

## Detailed interaction and resource collection (schema 3)

Ordinary events carry `schema_version=3`, architecture and macOS build; Aptabase system properties include OS/app versions, app build, locale and SDK. `system_profile` reports physical RAM bytes, logical/active processors, Metal device count, unified-memory availability, uptime seconds, debug build and Rosetta translation when the OS query is available. No hardware serial, username, machine name or other applications' usage is collected.

`resource_sample` runs once immediately, then every 30 seconds while telemetry is enabled. It reports main-process resident, physical-footprint, virtual, compressed and peak-resident bytes; cumulative CPU nanoseconds; interval CPU percentage multiplied by 100; Metal allocated bytes when available; thermal/power state; open tabs/windows; and active reader/format context. CPU uses user+kernel time divided by elapsed wall time, may exceed 10000 on multiple cores, and has no interval value in the first sample. Availability flags distinguish missing counters from zero. `scope=main_process` and `includes_helpers=false` exclude renderer helpers, converters and separate WebKit services. `gpu_counter_scope=metal_device_allocations` and `gpu_total_vram_available=false` explicitly exclude whole-system/dedicated VRAM, complete GPU residency and GPU utilization. A zero device counter does not prove no GPU resources exist. Unified GPU allocations and physical/compressed memory can overlap; never add these counters together. Samples describe the whole main process while a format is active, including other tabs/caches, not memory exclusively attributable to that format.

`reader_scroll` groups continuous input after about 700 ms idle, with segments up to roughly 10 seconds. It records event count, duration and absolute horizontal/vertical travel. GPUI records points, lines×100 and positive/negative direction counts separately; native records delta×100 with `delta_unit` (points or lines). These are input deltas, not actual content displacement, scroll depth or reading completion. Native directional signs are not retained. GPUI reuses document event capture; native reuses the existing context router and deduplicates overlapping routers by event identity. No independent right-click monitor is added. Precision modes are separated rather than converted; do not directly combine unlike units or assume identical coverage. Native final segments rely on live timers and can be lost during termination.

`shortcut_used` reports resolved static GPUI command names, modifiers, allowlisted command-key tokens and active format. It includes app commands, book navigation, tree navigation and resolved editor copy/cut/paste, undo/redo, find and cursor commands. Text contents and arbitrary key strings are excluded; unknown binding keys become `other_bound_key`. Tokens distinguish alternate bindings such as Space and PageDown. Events consumed before GPUI resolution are not observed. `native_shortcut_used` covers recognized native copy/select-all, zoom/fit/actual size, preview copy, OCR, external-open and page shortcuts, with modifier flags and physical `key_code` for recognized commands only. Physical key codes do not describe characters produced by the user's keyboard layout. Internal Quick Look, WebKit, system-menu and AVPlayerView shortcuts bypassing these routes are not guaranteed. Native `input_source` is pointer/keyboard/menu/unknown; asynchronous outcomes and observed states do not infer a user source.

### Concrete operations

| Reader | Detail |
| --- | --- |
| CSV/Geo | Row/column previous/next, cell detail, offsets, table format category, load outcome/time |
| SQLite | First/last/previous/next/jump, filter-presence flags, sort direction, table selection, cell detail, extended timeout; separate open/columns/count/page/value outcomes |
| Books | Page/chapter, contents, font size/family, line spacing, page width, paper, bookmarks, links |
| XMind | Sheet/topic selection, node/all folding, expand all, reset, zoom buttons, copy |
| OPML/data tree | Outline expand/collapse; node folding, value detail and parse outcome/time |
| Markdown | Local/web/email link category, task toggles, preview and toolbar operations; no URLs or anchor text |
| Paged text/hex | Previous/next/expand page, bytes/report mode, copy page and load outcome/time |
| Native images | Image/selection copy, completion/failure, OCR observations/time, button/end-of-gesture zoom, fit/actual size, paging and menus |
| Word/HTML/workbook | Load completion/failure, reading mode/zoom/links/copy, workbook sheet/more-sheets, separate row/column paging |
| PDF/presentation/font | Zoom/fit/layout/presentation entry/exit/navigation/external-open/font selection |
| Media | Play/pause invocation, playlist navigation, failure, observed playback/waiting/pause/mute/volume, periodic position/duration |
| Workspace/explorer | Actual reader-fullscreen transitions, contextual toolbar calls, menu new/play/shuffle/open/reveal/copy-path/trash/remove-folder categories; no paths |

`reader_feature` describes attempts, not universally completed work. Some operations identify a reader family without an exact suffix (Word can combine doc/docx, books can combine epub/fb2); never invent that suffix. Exact format reach remains based on `file_open`. Native `semantic=invocation/outcome/observation` separates calls, outcomes and observed state. OCR outcomes include observation count and elapsed milliseconds, never recognized text; an empty result can be successful. Empty copy results use static error code 2. A playhead discrepancy over two seconds emits `playhead_jump_observed`; buffering/timing can trigger it, so it is not an exact seek-intent counter. Volume changes coalesce for 400 ms. Playback position is sampled about every 30 seconds while the native media host is visible. Disabling telemetry stops native collection and changes aggregation generation, so re-enabling cannot flush previous scroll/playback segments. Delivery remains bounded and best effort. Static integration and compilation do not establish interaction coverage; macOS interaction verification remains manual.

### Detail exports

```sh
python3 scripts/telemetry-detail-report.py events.json --output-dir report
```

Produces `timeline.csv` (events with safe properties), `actions.csv` (event/format/reader/action/source/result groups), `shortcuts.csv` (commands and chords), and `resources.csv` (main-process sample count, P50/P95/max by active format). Input is the same JSON/JSONL/CSV schema as the format report. Sequenced duplicate export rows count once. Timeline retains export order; inspect session+sequence for ordering, not a guaranteed global causal order. Filter version/time in the export before comparison. Missing installation identities never become invented people counts.
