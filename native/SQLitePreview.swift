import Cocoa
import SQLite3

final class SQLitePreview: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    let view = NSView()
    let picker = NSPopUpButton()
    let grid = NSTableView()
    let status = NSTextField(wrappingLabelWithString: L("Loading database…"))
    let previous = NSButton(title: L("Previous"), target: nil, action: nil)
    let next = NSButton(title: L("Next"), target: nil, action: nil)
    let structure = NSButton(checkboxWithTitle: L("Structure"), target: nil, action: nil)
    let schemaButton = NSButton(title: "SQL", target: nil, action: nil)
    private let queue = DispatchQueue(label: "glim.sqlite", qos: .userInitiated)
    private var database: OpaquePointer?
    private var tables: [(name: String, sql: String)] = []
    private var rows: [[String]] = []
    private var offset = 0
    private var more = false
    private var busy = false
    private var closed = false
    private let moreTables = NSButton(title: L("Show more tables"), target: nil, action: nil)
    private let previousColumns = NSButton(title: L("Previous columns"), target: nil, action: nil)
    private let nextColumns = NSButton(title: L("Next columns"), target: nil, action: nil)
    private var catalogMore = false
    private var catalogOffset = 0
    private var columnOffset = 0
    private var columnCount = 0
    private var rowOrder = ""
    private let worksheetCells: Bool
    private let initialWorksheet: (Int64,String)?
    private let pageSize = ReaderLimits.SQLITE_PAGE_ROWS

    init(path: String?, information: String, modeControl: NSView? = nil, extraControls: [NSView] = [], worksheetCells: Bool = false, initialWorksheet: (Int64,String)? = nil) {
        self.worksheetCells = worksheetCells; self.initialWorksheet = initialWorksheet
        super.init()
        let stack = NSStackView(); stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 10; stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 16), stack.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -16), stack.topAnchor.constraint(equalTo: view.topAnchor, constant: 16), stack.bottomAnchor.constraint(equalTo: view.bottomAnchor, constant: -16)])
        let note = NSTextField(wrappingLabelWithString: information); note.textColor = .secondaryLabelColor; stack.addArrangedSubview(note)
        let toolbar = NSStackView()
        picker.target = self; picker.action = #selector(selectTable); picker.isEnabled = false
        previous.target = self; previous.action = #selector(previousPage)
        next.target = self; next.action = #selector(nextPage)
        structure.target = self; structure.action = #selector(selectTable)
        schemaButton.target = self; schemaButton.action = #selector(showSQL)
        moreTables.target = self; moreTables.action = #selector(loadMoreTables)
        previousColumns.target = self; previousColumns.action = #selector(previousColumnPage)
        nextColumns.target = self; nextColumns.action = #selector(nextColumnPage)
        grid.target = self; grid.doubleAction = #selector(showFullValue)
        if let modeControl = modeControl { toolbar.addArrangedSubview(modeControl) }
        let actions: [NSView] = worksheetCells ? [picker, previous, next, moreTables] : [picker, structure, schemaButton, previous, next, previousColumns, nextColumns, moreTables]
        actions.forEach { toolbar.addArrangedSubview($0) }
        if worksheetCells { moreTables.title = L("Show more worksheets") }
        grid.delegate = self; grid.dataSource = self; grid.usesAlternatingRowBackgroundColors = true; grid.rowHeight = 26; grid.columnAutoresizingStyle = .noColumnAutoresizing
        let scroll = NSScrollView(); scroll.documentView = grid; scroll.hasVerticalScroller = true; scroll.hasHorizontalScroller = true
        stack.addArrangedSubview(scroll); scroll.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        status.font = .systemFont(ofSize: 12); status.textColor = .secondaryLabelColor; stack.addArrangedSubview(status)
        extraControls.forEach { toolbar.addArrangedSubview($0) }
        scroll.contentInsets.bottom = 72
        FloatingReaderToolbar(items: toolbar).install(in: view)
        controls()
        guard let path = path else { status.stringValue = L("Open the matching database to browse tables."); return }
        queue.async { [self] in
            // Only the private snapshot is writable, to allow journal/WAL recovery.
            let code = sqlite3_open_v2(path, &database, SQLITE_OPEN_READWRITE | SQLITE_OPEN_PRIVATECACHE, nil)
            guard code == SQLITE_OK, let database = database else { fail(L("Cannot open SQLite snapshot.")); return }
            sqlite3_busy_timeout(database, Int32(ReaderLimits.SQLITE_BUSY_MILLISECONDS))
            // macOS system SQLite omits the extension-loading API.
            sqlite3_exec(database, "PRAGMA trusted_schema=OFF; PRAGMA query_only=ON; PRAGMA cache_size=-\(ReaderLimits.SQLITE_CACHE_KIB); PRAGMA temp_store=FILE;", nil, nil, nil)
            loadCatalog(offset: 0)

        }
    }
    // Called on the serial database queue; fetch definitions only when requested.
    private func loadCatalog(offset: Int) {
        do {
            let result = try query("SELECT name, type FROM sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name LIMIT \(ReaderLimits.SQLITE_TABLES + 1) OFFSET \(offset)", limit: ReaderLimits.SQLITE_TABLES + 1, textLimit: nil)
            let values = result.rows.prefix(ReaderLimits.SQLITE_TABLES).map { (name: $0[0], sql: $0[1]) }
            DispatchQueue.main.async { [weak self] in
                guard let self = self, !self.closed else { return }
                for value in values where !self.tables.contains(where: { $0.name == value.name }) {
                    self.tables.append(value); self.picker.addItem(withTitle: value.name)
                }
                self.catalogOffset = offset + values.count
                self.catalogMore = result.rows.count > ReaderLimits.SQLITE_TABLES
                self.status.stringValue = L(self.worksheetCells ? "%d worksheets" : "%d tables / views").replacingOccurrences(of: "%d", with: String(self.tables.count))
                self.busy = false; self.controls()
                if offset == 0 && !values.isEmpty {
                    if let initial = self.initialWorksheet {
                        let title = "\(initial.0): \(initial.1)"
                        if let index = self.tables.firstIndex(where: { $0.name == title }) { self.picker.selectItem(at: index) }
                        else { self.tables.append((name: title, sql: "table")); self.picker.addItem(withTitle: title); self.picker.selectItem(at: self.tables.count - 1) }
                    }
                    self.loadPage()
                }
            }
        } catch { fail(error.localizedDescription) }
    }
    @objc private func loadMoreTables() {
        guard !busy, catalogMore else { return }
        let offset = catalogOffset
        busy = true; controls()
        queue.async { [self] in loadCatalog(offset: offset) }
    }
    @objc private func previousColumnPage() { columnOffset = max(0, columnOffset - ReaderLimits.SQLITE_COLUMNS); loadPage() }
    @objc private func nextColumnPage() { columnOffset += ReaderLimits.SQLITE_COLUMNS; loadPage() }
    func close() {
        closed = true
        if let database = database { sqlite3_interrupt(database) }
        // Drain the bounded queries before Rust removes the private snapshot.
        queue.sync {
            if let database = database { sqlite3_close(database); self.database = nil }
        }
    }
    deinit { if let database = database { sqlite3_close(database) } }
    private func failure() -> NSError {
        NSError(domain: "SQLite", code: Int(sqlite3_errcode(database)), userInfo: [NSLocalizedDescriptionKey: database.map { String(cString: sqlite3_errmsg($0)) } ?? L("Database unavailable")])
    }
    private func query(_ sql: String, limit: Int, textLimit: Int? = ReaderLimits.SQLITE_TEXT_BYTES, columnOffset: Int = 0) throws -> (columns: [String], rows: [[String]], totalColumns: Int) {
        guard let database = database else { throw failure() }
        var deadline = CFAbsoluteTimeGetCurrent() + Double(ReaderLimits.SQLITE_QUERY_SECONDS)
        return try withUnsafeMutablePointer(to: &deadline) { pointer in
            sqlite3_progress_handler(database, 1000, { context in
                guard let context = context else { return 1 }
                return CFAbsoluteTimeGetCurrent() > context.assumingMemoryBound(to: Double.self).pointee ? 1 : 0
            }, pointer)
            defer { sqlite3_progress_handler(database, 0, nil, nil) }
            var statement: OpaquePointer?
            guard sqlite3_prepare_v2(database, sql, -1, &statement, nil) == SQLITE_OK, let statement = statement else { throw failure() }
            defer { sqlite3_finalize(statement) }
            guard sqlite3_stmt_readonly(statement) != 0 else { throw NSError(domain: "SQLite", code: 1, userInfo: [NSLocalizedDescriptionKey: "Only read-only queries are allowed."]) }
            let totalColumns = Int(sqlite3_column_count(statement))
            let start = min(columnOffset, totalColumns)
            let end = min(start + ReaderLimits.SQLITE_COLUMNS, totalColumns)
            let columns = (start..<end).map { String(cString: sqlite3_column_name(statement, Int32($0))) }
            var result: [[String]] = []
            while result.count < limit {
                let code = sqlite3_step(statement)
                if code == SQLITE_DONE { break }
                guard code == SQLITE_ROW else { throw failure() }
                result.append((start..<end).map { column in
                    let index = Int32(column)
                    if sqlite3_column_type(statement, index) == SQLITE_NULL { return "NULL" }
                    if sqlite3_column_type(statement, index) == SQLITE_BLOB && textLimit != nil {
                        let length = Int(sqlite3_column_bytes(statement, index))
                        guard let bytes = sqlite3_column_blob(statement, index)?.assumingMemoryBound(to: UInt8.self) else { return "BLOB · 0 bytes" }
                        let prefix = UnsafeBufferPointer(start: bytes, count: min(length, ReaderLimits.SQLITE_BLOB_BYTES)).map { String(format: "%02x", $0) }.joined(separator: " ")
                        return "BLOB · \(length) bytes · \(prefix)" + (length > ReaderLimits.SQLITE_BLOB_BYTES ? " …" : "")
                    }
                    guard let bytes = sqlite3_column_text(statement, index) else { return "" }
                    let length = Int(sqlite3_column_bytes(statement, index))
                    return String(decoding: UnsafeBufferPointer(start: bytes, count: min(length, textLimit ?? length)), as: UTF8.self) + (length > (textLimit ?? length) ? "…" : "")
                })
            }
            return (columns, result, totalColumns)
        }
    }
    func currentWorksheet() -> (Int64,String)? {
        let index = picker.indexOfSelectedItem
        guard index >= 0, index < tables.count else { return nil }
        let parts = tables[index].name.split(separator: ":", maxSplits: 1)
        guard parts.count == 2, let id = Int64(parts[0]) else { return nil }
        return (id, String(parts[1]).trimmingCharacters(in: .whitespaces))
    }
    private func controls() {
        picker.isEnabled = !busy && !tables.isEmpty
        structure.isEnabled = !busy && !tables.isEmpty
        schemaButton.isEnabled = !busy && !tables.isEmpty
        previous.isEnabled = !busy && offset > 0
        next.isEnabled = !busy && more
        moreTables.isEnabled = !busy && catalogMore
        previousColumns.isEnabled = !busy && columnOffset > 0
        nextColumns.isEnabled = !busy && columnOffset + ReaderLimits.SQLITE_COLUMNS < columnCount
        if !busy {
            previous.isHidden = offset == 0; next.isHidden = !more
            previousColumns.isHidden = columnOffset == 0
            nextColumns.isHidden = columnOffset + ReaderLimits.SQLITE_COLUMNS >= columnCount
            moreTables.isHidden = !catalogMore
        }
    }
    private func fail(_ message: String) {
        DispatchQueue.main.async { [weak self] in self?.busy = false; self?.status.stringValue = message; self?.controls() }
    }
    @objc func selectTable() { offset = 0; columnOffset = 0; loadPage() }
    @objc func previousPage() { offset = max(0, offset - pageSize); loadPage() }
    @objc func nextPage() { offset += pageSize; loadPage() }
    private func orderForTable(_ escapedName: String, isView: Bool) throws -> String {
        guard let database = database else { throw failure() }
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(database, "SELECT * FROM \"\(escapedName)\" LIMIT 0", -1, &statement, nil) == SQLITE_OK,
              let columns = statement else { throw failure() }
        let names = (0..<sqlite3_column_count(columns)).map { String(cString: sqlite3_column_name(columns, $0)).lowercased() }
        sqlite3_finalize(columns)
        if !isView {
            for alias in ["_rowid_", "rowid", "oid"] where !names.contains(alias) {
                statement = nil
                let code = sqlite3_prepare_v2(database, "SELECT \(alias) FROM \"\(escapedName)\" LIMIT 0", -1, &statement, nil)
                if let statement = statement { sqlite3_finalize(statement) }
                if code == SQLITE_OK { return " ORDER BY \(alias)" }
            }
        }
        return names.isEmpty ? "" : " ORDER BY " + (1...names.count).map(String.init).joined(separator: ",")
    }
    private func loadPage() {
        guard !closed, !busy, tables.indices.contains(picker.indexOfSelectedItem) else { return }
        let table = tables[picker.indexOfSelectedItem]
        let name = table.name.replacingOccurrences(of: "\"", with: "\"\"")
        let schemaName = table.name.replacingOccurrences(of: "'", with: "''")
        let schema = structure.state == .on
        busy = true; status.stringValue = L("Loading…"); controls()
        queue.async { [self] in
            do {
                let order = try orderForTable(name, isView: table.sql == "view")
                let queryText = schema ? "SELECT * FROM pragma_table_xinfo('\(schemaName)') ORDER BY cid LIMIT \(pageSize + 1) OFFSET \(offset)" : "SELECT * FROM \"\(name)\"\(order) LIMIT \(pageSize + 1) OFFSET \(offset)"
                let result = try query(queryText, limit: pageSize + 1, columnOffset: columnOffset)
                DispatchQueue.main.async { [weak self] in
                    guard let self = self, !self.closed else { return }
                    self.more = result.rows.count > self.pageSize
                    self.columnCount = result.totalColumns
                    self.rowOrder = order
                    self.rows = Array(result.rows.prefix(self.pageSize))
                    for column in self.grid.tableColumns { self.grid.removeTableColumn(column) }
                    for (index, name) in result.columns.enumerated() {
                        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(String(index))); column.title = name; column.width = 180; self.grid.addTableColumn(column)
                    }
                    self.grid.reloadData(); self.busy = false; self.controls()
                    self.status.stringValue = schema ? L("Table structure") : L("%rows rows · offset %offset · up to %columns columns · long values shown as summaries").replacingOccurrences(of: "%rows", with: String(self.rows.count)).replacingOccurrences(of: "%offset", with: String(self.offset)).replacingOccurrences(of: "%columns", with: String(ReaderLimits.SQLITE_COLUMNS))
                }
            } catch { fail(error.localizedDescription) }
        }
    }
    // The value dialog reads a bounded byte window, independent of value length.
    private func valuePage(_ title: String, source: String, start: Int = 0, history: [Int] = []) {
        busy = true; controls()
        queue.async { [self] in
            do {
                let result = try query("SELECT typeof(value),length(CAST(value AS BLOB)),hex(substr(CAST(value AS BLOB),\(start + 1),\(ReaderLimits.TEXT_PAGE_BYTES + 3))) FROM (\(source))", limit: 1, textLimit: nil)
                guard let row = result.rows.first, row.count == 3 else { throw failure() }
                let binary = row[0] == "blob"
                let total = Int(row[1]) ?? 0
                let encoded = Array(row[2].utf8)
                var bytes = Data(); bytes.reserveCapacity(encoded.count / 2)
                func nibble(_ byte: UInt8) -> UInt8 { byte <= 57 ? byte - 48 : byte - 55 }
                for index in stride(from: 0, to: encoded.count - encoded.count % 2, by: 2) {
                    bytes.append(nibble(encoded[index]) << 4 | nibble(encoded[index + 1]))
                }
                var length = min(bytes.count, ReaderLimits.TEXT_PAGE_BYTES)
                var content = ""
                if binary { content = bytes.prefix(length).map { String(format: "%02X", $0) }.joined(separator: " ") }
                else if row[0] == "null" { content = "NULL" }
                else {
                    // Keep UTF-8 code points together at page boundaries.
                    var complete = String(data: bytes.prefix(length), encoding: .utf8)
                    for _ in 0..<3 where complete == nil && length > 0 {
                        length -= 1; complete = String(data: bytes.prefix(length), encoding: .utf8)
                    }
                    content = complete ?? String(decoding: bytes.prefix(length), as: UTF8.self)
                }
                let end = start + length
                DispatchQueue.main.async { [weak self] in
                    guard let self = self, !self.closed else { return }
                    self.busy = false; self.controls()
                    let alert = NSAlert(); alert.messageText = title
                    alert.informativeText = "bytes \(start)–\(end) / \(total)"
                    let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 560, height: 320)); scroll.hasVerticalScroller = true
                    let text = NSTextView(frame: scroll.bounds); text.isEditable = false
                    text.font = .monospacedSystemFont(ofSize: 12, weight: .regular); text.string = content
                    scroll.documentView = text; alert.accessoryView = scroll
                    alert.addButton(withTitle: L("Close"))
                    alert.addButton(withTitle: L("Previous")).isEnabled = !history.isEmpty
                    alert.addButton(withTitle: L("Next page")).isEnabled = end < total && end > start
                    let response = alert.runModal()
                    if response == .alertSecondButtonReturn, let previous = history.last {
                        self.valuePage(title, source: source, start: previous, history: Array(history.dropLast()))
                    } else if response == .alertThirdButtonReturn, end < total && end > start {
                        self.valuePage(title, source: source, start: end, history: history + [start])
                    }
                }
            } catch { fail(error.localizedDescription) }
        }
    }
    @objc func showSQL() {
        guard !busy, tables.indices.contains(picker.indexOfSelectedItem) else { return }
        let name = tables[picker.indexOfSelectedItem].name.replacingOccurrences(of: "'", with: "''")
        valuePage(L("Table / View Definition"), source: "SELECT sql AS value FROM sqlite_master WHERE name='\(name)'")
    }
    @objc private func showFullValue() {
        guard !busy, structure.state == .off, grid.clickedRow >= 0, grid.clickedColumn >= 0,
              tables.indices.contains(picker.indexOfSelectedItem), grid.tableColumns.indices.contains(grid.clickedColumn) else { return }
        let name = tables[picker.indexOfSelectedItem].name.replacingOccurrences(of: "\"", with: "\"\"")
        let column = grid.tableColumns[grid.clickedColumn].title.replacingOccurrences(of: "\"", with: "\"\"")
        let row = offset + grid.clickedRow
        valuePage(L("Full cell value"), source: "SELECT \"\(column)\" AS value FROM (SELECT * FROM \"\(name)\"\(rowOrder) LIMIT 1 OFFSET \(row))")
    }
    func numberOfRows(in tableView: NSTableView) -> Int { rows.count }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let id = tableColumn?.identifier.rawValue, let column = Int(id), rows.indices.contains(row), rows[row].indices.contains(column) else { return nil }
        let label = NSTextField(labelWithString: rows[row][column]); label.lineBreakMode = .byTruncatingTail; label.isSelectable = true; label.toolTip = rows[row][column]; return label
    }
}
