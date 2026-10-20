import Cocoa
import Quartz
import SQLite3

// The provider receives a bounded presentation workbook for the selected window,
// while the original complete cell index remains available through Cell data.
final class WorkbookPreview: NSObject {
    struct Sheet { let id: Int64; let name: String; let rows: Int; let columns: Int }
    let view = NSView()
    private let body = NSView()
    private let loading = NSTextField(labelWithString: L("Loading preview…"))
    private let picker = NSPopUpButton()
    private let mode = NSSegmentedControl(labels: [L("Preview"), L("Cell data")], trackingMode: .selectOne, target: nil, action: nil)
    private let previous = NSButton(title: L("Previous"), target: nil, action: nil)
    private let next = NSButton(title: L("Next page"), target: nil, action: nil)
    private let previousColumns = NSButton(title: L("Previous columns"), target: nil, action: nil)
    private let nextColumns = NSButton(title: L("Next columns"), target: nil, action: nil)
    private let moreSheets = NSButton(title: L("Show more worksheets"), target: nil, action: nil)
    private let status = NSTextField(labelWithString: "")
    private let queue = DispatchQueue(label: "glim.workbook.pages", qos: .userInitiated)
    private let cache: URL
    private let database: String
    private let helper: URL
    private let information: String
    private let extraControls: () -> [NSView]
    private var sheets: [Sheet] = []
    private var selected = 0
    private var row = 0
    private var column = 0
    private var catalogMore = false
    private var catalogOffset = 0
    private var busy = true
    private var closed = false
    private var generation = 0
    private var preparation: MediaPreparation?
    private var preview: QLPreviewView?
    private var previewDirectory: URL?
    private var previewKey: String?
    private var data: SQLitePreview?
    private var previewToolbar: FloatingReaderToolbar!
    private var dataMode = false

    init(database: String, cache: String, helper: String, information: String, extraControls: @escaping () -> [NSView]) {
        self.database = database; self.cache = URL(fileURLWithPath: cache)
        self.helper = URL(fileURLWithPath: helper); self.information = information
        self.extraControls = extraControls
        super.init()
        body.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(body)
        loading.textColor = .secondaryLabelColor; loading.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(loading)
        NSLayoutConstraint.activate([
            body.leadingAnchor.constraint(equalTo: view.leadingAnchor), body.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            body.topAnchor.constraint(equalTo: view.topAnchor), body.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            loading.centerXAnchor.constraint(equalTo: body.centerXAnchor), loading.centerYAnchor.constraint(equalTo: body.centerYAnchor),
            loading.widthAnchor.constraint(lessThanOrEqualTo: body.widthAnchor, constant: -48)
        ])
        mode.selectedSegment = 0; mode.target = self; mode.action = #selector(changeMode(_:))
        picker.target = self; picker.action = #selector(selectSheet)
        previous.target = self; previous.action = #selector(previousPage)
        next.target = self; next.action = #selector(nextPage)
        previousColumns.target = self; previousColumns.action = #selector(previousColumnPage)
        nextColumns.target = self; nextColumns.action = #selector(nextColumnPage)
        moreSheets.target = self; moreSheets.action = #selector(loadMoreSheets)
        status.font = .systemFont(ofSize: 12); status.textColor = .secondaryLabelColor
        loading.maximumNumberOfLines = 4; loading.lineBreakMode = .byTruncatingTail
        let items = NSStackView()
        [mode, picker, previous, next, previousColumns, nextColumns, moreSheets, status].forEach { items.addArrangedSubview($0) }
        extraControls().forEach { items.addArrangedSubview($0) }
        previewToolbar = FloatingReaderToolbar(items: items); previewToolbar.install(in: view)
        controls()
        loadCatalog(offset: 0)
    }
    private static func querySheets(cache: URL, sql: String) throws -> [Sheet] {
        var cacheDatabase: OpaquePointer?
        guard sqlite3_open_v2(cache.path, &cacheDatabase, SQLITE_OPEN_READONLY | SQLITE_OPEN_PRIVATECACHE, nil) == SQLITE_OK else {
            if let database = cacheDatabase { sqlite3_close(database) }
            throw NSError(domain: "Glim", code: 1, userInfo: [NSLocalizedDescriptionKey: L("Cannot open workbook cache.")])
        }
        defer { sqlite3_close(cacheDatabase) }
        sqlite3_busy_timeout(cacheDatabase, Int32(ReaderLimits.SQLITE_BUSY_MILLISECONDS))
        sqlite3_exec(cacheDatabase, "PRAGMA query_only=ON; PRAGMA cache_size=-\(ReaderLimits.SQLITE_CACHE_KIB);", nil, nil, nil)
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(cacheDatabase, sql, -1, &statement, nil) == SQLITE_OK, let statement = statement else {
            throw NSError(domain: "Glim", code: 1, userInfo: [NSLocalizedDescriptionKey: L("Cannot open workbook cache.")])
        }
        defer { sqlite3_finalize(statement) }
        var result: [Sheet] = []
        var code = sqlite3_step(statement)
        while code == SQLITE_ROW {
            let text = sqlite3_column_text(statement, 1)
            let count = Int(sqlite3_column_bytes(statement, 1))
            let name = text.map { String(decoding: UnsafeBufferPointer(start: $0, count: count), as: UTF8.self) } ?? ""
            result.append(Sheet(id: sqlite3_column_int64(statement, 0), name: name,
                rows: Int(sqlite3_column_int64(statement, 2)), columns: Int(sqlite3_column_int64(statement, 3))))
            code = sqlite3_step(statement)
        }
        guard code == SQLITE_DONE else { throw NSError(domain: "Glim", code: Int(code)) }
        return result
    }
    private func loadCatalog(offset: Int) {
        let cache = cache
        queue.async { [weak self] in
            do {
                let found = try Self.querySheets(cache: cache, sql: "SELECT id,name,max_row,max_column FROM sheets ORDER BY id LIMIT \(ReaderLimits.SQLITE_TABLES + 1) OFFSET \(offset)")
                let page = Array(found.prefix(ReaderLimits.SQLITE_TABLES))
                DispatchQueue.main.async { [weak self] in
                    guard let self = self, !self.closed else { return }
                    let selectedID = self.selected < self.sheets.count ? self.sheets[self.selected].id : nil
                    for sheet in page where !self.sheets.contains(where: { $0.id == sheet.id }) { self.sheets.append(sheet) }
                    self.sheets.sort { $0.id < $1.id }
                    self.picker.removeAllItems(); self.picker.addItems(withTitles: self.sheets.map { $0.name })
                    if let id = selectedID, let index = self.sheets.firstIndex(where: { $0.id == id }) { self.selected = index; self.picker.selectItem(at: index) }
                    self.catalogOffset = offset + page.count
                    self.catalogMore = found.count > ReaderLimits.SQLITE_TABLES
                    self.busy = false; self.controls()
                    if offset == 0 { self.loadPage() }
                }
            } catch {
                DispatchQueue.main.async { [weak self] in self?.fail(error.localizedDescription) }
            }
        }
    }
    @objc private func loadMoreSheets() {
        guard !busy, catalogMore else { return }
        busy = true; controls(); loadCatalog(offset: catalogOffset)
    }
    @objc private func selectSheet() {
        guard picker.indexOfSelectedItem >= 0 else { return }
        selected = picker.indexOfSelectedItem; row = 0; column = 0; loadPage()
    }
    @objc private func previousPage() { row = max(0, row - ReaderLimits.SQLITE_PAGE_ROWS); loadPage() }
    @objc private func nextPage() { row += ReaderLimits.SQLITE_PAGE_ROWS; loadPage() }
    @objc private func previousColumnPage() { column = max(0, column - ReaderLimits.SQLITE_COLUMNS); loadPage() }
    @objc private func nextColumnPage() { column += ReaderLimits.SQLITE_COLUMNS; loadPage() }
    private func controls() {
        picker.isEnabled = !busy && !sheets.isEmpty
        previous.isEnabled = !busy && row > 0
        next.isEnabled = !busy && selected < sheets.count && row + ReaderLimits.SQLITE_PAGE_ROWS < sheets[selected].rows
        previousColumns.isEnabled = !busy && column > 0
        nextColumns.isEnabled = !busy && selected < sheets.count && column + ReaderLimits.SQLITE_COLUMNS < sheets[selected].columns
        if !busy {
            previous.isHidden = row == 0
            next.isHidden = !next.isEnabled
            previousColumns.isHidden = column == 0
            nextColumns.isHidden = !nextColumns.isEnabled
        }
        moreSheets.isHidden = !catalogMore; moreSheets.isEnabled = !busy && catalogMore
    }
    private func updateStatus() {
        guard selected < sheets.count else { return }
        let sheet = sheets[selected]
        status.stringValue = L("Rows %start–%end / %total · Columns %first–%last")
            .replacingOccurrences(of: "%start", with: String(row + 1))
            .replacingOccurrences(of: "%end", with: String(min(sheet.rows, row + ReaderLimits.SQLITE_PAGE_ROWS)))
            .replacingOccurrences(of: "%total", with: String(sheet.rows))
            .replacingOccurrences(of: "%first", with: String(column + 1))
            .replacingOccurrences(of: "%last", with: String(min(sheet.columns, column + ReaderLimits.SQLITE_COLUMNS)))
    }
    private func loadPage() {
        guard !closed, !dataMode, selected < sheets.count else { return }
        preparation?.cancel(); preparation = nil; generation += 1
        let sheet = sheets[selected]
        let key = "\(sheet.id):\(row):\(column)"
        updateStatus()
        if previewKey == key, let preview = preview {
            preview.isHidden = false; loading.isHidden = true; busy = false; controls(); return
        }
        preview?.isHidden = true
        loading.stringValue = L("Loading preview…"); loading.isHidden = false
        busy = true; controls()
        let preparation = MediaPreparation(); self.preparation = preparation
        let generation = generation, row = row, column = column, cache = cache, helper = helper
        queue.async { [weak self] in
            var directory: URL?
            do {
                let folder = FileManager.default.temporaryDirectory.appendingPathComponent("glim-workbook-\(UUID().uuidString)", isDirectory: true)
                directory = folder; try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
                let output = folder.appendingPathComponent("page.xlsx")
                try preparation.run(helper: helper, arguments: ["--prepare-spreadsheet-page", cache.path, String(sheet.id), String(row), String(column), output.path], destination: output)
                DispatchQueue.main.async { [weak self] in
                    guard let self = self, !self.closed, self.generation == generation, !self.dataMode else {
                        try? FileManager.default.removeItem(at: folder); return
                    }
                    self.preview?.previewItem = nil; self.preview?.close(); self.preview?.removeFromSuperview()
                    if let previous = self.previewDirectory { try? FileManager.default.removeItem(at: previous) }
                    self.previewDirectory = folder
                    guard let preview = QLPreviewView(frame: .zero, style: .normal) else {
                        self.busy = false; self.loading.stringValue = L("Workbook preview is unavailable. Use Cell data to read the complete workbook."); self.controls(); return
                    }
                    preview.translatesAutoresizingMaskIntoConstraints = false
                    preview.autostarts = true; preview.shouldCloseWithWindow = false
                    self.body.addSubview(preview)
                    NSLayoutConstraint.activate([
                        preview.leadingAnchor.constraint(equalTo: self.body.leadingAnchor),
                        preview.trailingAnchor.constraint(equalTo: self.body.trailingAnchor),
                        preview.topAnchor.constraint(equalTo: self.body.topAnchor),
                        preview.bottomAnchor.constraint(equalTo: self.body.bottomAnchor)
                    ])
                    // Resolve the full reading viewport before the remote provider
                    // is created; compact style is an inspector-sized card.
                    self.view.layoutSubtreeIfNeeded(); self.body.layoutSubtreeIfNeeded(); preview.layoutSubtreeIfNeeded()
                    preview.previewItem = output as NSURL
                    self.preview = preview; self.previewKey = key; self.preparation = nil
                    self.busy = false; self.loading.isHidden = true; self.controls()
                }
            } catch {
                if let folder = directory { try? FileManager.default.removeItem(at: folder) }
                DispatchQueue.main.async { [weak self] in
                    guard let self = self, !self.closed, self.generation == generation, !self.dataMode else { return }
                    self.preparation = nil; self.busy = false
                    self.loading.stringValue = L("Workbook preview is unavailable. Use Cell data to read the complete workbook.") + "\n" + String(error.localizedDescription.prefix(ReaderLimits.TEXT_LAYOUT_RUN_BYTES))
                    self.controls()
                }
            }
        }
    }
    @objc private func changeMode(_ sender: NSSegmentedControl) {
        let showingData = sender.selectedSegment == 1
        preparation?.cancel(); preparation = nil; generation += 1
        dataMode = showingData; mode.selectedSegment = sender.selectedSegment
        previewToolbar.isHidden = showingData; loading.isHidden = true; preview?.isHidden = true
        if showingData {
            if let data = data { data.close(); data.view.removeFromSuperview() }
            let segment = NSSegmentedControl(labels: [L("Preview"), L("Cell data")], trackingMode: .selectOne, target: self, action: #selector(changeMode(_:)))
            segment.selectedSegment = 1
            let sheet = selected < sheets.count ? sheets[selected] : nil
            let data = SQLitePreview(path: database, information: information, modeControl: segment, extraControls: extraControls(), worksheetCells: true, initialWorksheet: sheet.map { ($0.id, $0.name) })
            data.view.translatesAutoresizingMaskIntoConstraints = false
            body.addSubview(data.view)
            NSLayoutConstraint.activate([
                data.view.leadingAnchor.constraint(equalTo: body.leadingAnchor),
                data.view.trailingAnchor.constraint(equalTo: body.trailingAnchor),
                data.view.topAnchor.constraint(equalTo: body.topAnchor),
                data.view.bottomAnchor.constraint(equalTo: body.bottomAnchor)
            ])
            self.data = data; busy = false
        } else {
            data?.view.isHidden = true
            if let worksheet = data?.currentWorksheet() {
                if let index = sheets.firstIndex(where: { $0.id == worksheet.0 }) {
                    if selected != index { selected = index; row = 0; column = 0; picker.selectItem(at: index) }
                } else {
                    busy = true; loading.isHidden = false; loading.stringValue = L("Loading preview…"); controls()
                    let cache = cache, generation = generation
                    queue.async { [weak self] in
                        do {
                            let found = try Self.querySheets(cache: cache, sql: "SELECT id,name,max_row,max_column FROM sheets WHERE id=\(worksheet.0)")
                            DispatchQueue.main.async { [weak self] in
                                guard let self = self, !self.closed, self.generation == generation, let sheet = found.first else { return }
                                self.sheets.append(sheet); self.sheets.sort { $0.id < $1.id }
                                self.picker.removeAllItems(); self.picker.addItems(withTitles: self.sheets.map { $0.name })
                                self.selected = self.sheets.firstIndex(where: { $0.id == sheet.id }) ?? 0
                                self.picker.selectItem(at: self.selected); self.row = 0; self.column = 0
                                self.busy = false; self.loadPage()
                            }
                        } catch { DispatchQueue.main.async { [weak self] in self?.fail(error.localizedDescription) } }
                    }
                    return
                }
            }
            busy = false; loadPage()
        }
    }
    private func fail(_ message: String) {
        DispatchQueue.main.async { [weak self] in
            guard let self = self, !self.closed else { return }
            self.busy = false; self.loading.isHidden = false; self.loading.stringValue = message; self.controls()
        }
    }
    func close() {
        closed = true; generation += 1; preparation?.cancel(); preparation = nil
        preview?.previewItem = nil; preview?.close(); preview = nil
        data?.close(); data = nil
        if let directory = previewDirectory { try? FileManager.default.removeItem(at: directory) }; previewDirectory = nil
    }
}
