# Use Glim

**English** · [简体中文](user-guide.zh-CN.md) · [Documentation](README.md)

Glim keeps documents, data and code in one Mac workspace. Start with a folder you already use: a report, its images, an export and the scripts that produced it.

## Open and find files

- **⌘O** opens a file; **⌘⇧O** opens a folder. Add Folder to Workspace appends another folder to the current window.
- The sidebar contains Home and your workspace file tree. Home shows opening actions and recent folder workspaces. Collapse the sidebar to keep a narrow icon rail; its folder icon expands the file list again.
- Title-bar search finds files by name or path within workspace folders, respecting the file-discovery scope. It does not search document contents. Use the source editor’s search for text within an editable document.
- Files open in tabs. Reopening a file already open in Glim activates its existing tab/window. Back and Forward revisit previously opened file targets.
- **⌘W** closes the active tab, or an empty window; **⌘⇧W** closes the current window. Closing the last window leaves Glim running; **⌘Q** quits.

## Choose a reading view

Markdown and HTML offer Preview and Source views. Markdown renders tables, formulas and diagrams; local links open related files. HTML uses WebKit, supports CSS and JavaScript, and can load network resources. Relative local HTML resources stay within the enclosing project, or the document directory when no project root is found.

JSON-family files offer a read-only tree alongside source. CSV/TSV offer a read-only table alongside editable source; inspect complete cell values when a table cell is truncated. Supported structured formats can also project data as an interactive graph; this view does not change the file.

PDFs, images, supported Office documents, ebooks and media have their own reading controls. Reader actions appear in the outer toolbar, a floating content toolbar, or the content menu as supported. For readers with a floating toolbar, pause the pointer near the bottom of the content to reveal it. Open **Settings → Keyboard Shortcuts** for keyboard help. File fullscreen gives the current reader more room; Escape returns to the workspace.

See the [format guide](file-format-support.md) for available actions and compatibility. A preview may reproduce only part of a format; the default application remains available for specialist editing or original layout.

## Edit and save text

Ordinary UTF-8 text supports typing, selection, undo/redo and explicit **⌘S**. Existing files save automatically after a short typing pause. Automatic saving writes the original file; keep that in mind when opening a configuration or shared file. If the on-disk content changes before a save, Glim preserves your unsaved editor content and reports the conflict.

**⌘N** creates an untitled text note. Its first save asks for a filename and location; later edits use the normal automatic-save behavior. Saving with a different extension does not convert text into a PDF, image or Office document.

Large or very long-line UTF-8 documents use editable pages. Save pending page edits before navigating where prompted. Generated text, binary views, database snapshots, images and document previews remain read-only. Editable source is available only where the format supports it.

Before closing unsaved work, Glim offers Save, Don’t Save and Cancel. A failed or canceled save leaves the note open.

## Browse SQLite without changing the source

Glim prepares a private snapshot, including associated WAL/journal data when present, and checks that the source stayed stable during copying. Open the main database file, select a table or view, then search across every column in all records, sort or page through results. Double-click a cell to read its full value. Slow queries can offer a retry with more time.

The snapshot is read-only. Glim does not execute arbitrary write SQL or checkpoint the source database. Encrypted databases are unsupported.

## Resume work

Home lists recent folder workspaces. Reopening one restores its folders; normal startup does not restore every file tab or every reader’s scroll/playback position. Ebook bookmarks and reading progress have their own saved state.

Untitled notes are checkpointed locally and can be recovered after an interrupted session. Edits not yet written to a checkpoint can be lost, and recovery errors are reported. Save important notes to a named file. These note checkpoints are separate from complete workspace-session recovery.

Configured in-app updates preserve supported file/playlist tabs through a one-time restart checkpoint. See [update behavior](auto-update-design.md); available features in a published build are described in its release notes.

## Settings, network content and help

Settings provides appearance, language, default file-type associations, update checks, keyboard help and optional anonymous usage telemetry. Local file reading does not require a hosted viewer. Markdown remote images, HTML network content and enabled update/telemetry requests can use the network; see [telemetry](telemetry.md) and [security](../SECURITY.md).

If a file fails to open or renders incorrectly, [report an issue](https://github.com/glimapp/glim/issues/new?template=bug_report.yml) with the Glim version, macOS version, format, steps and a sanitized example you can share. Do not include credentials or private document/database content. Some errors provide Retry or Open Log Folder; logs can contain local paths, so review them before sharing.
