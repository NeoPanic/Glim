import Cocoa
import WebKit
import AVKit
import PDFKit
import Quartz
import ImageIO
import UniformTypeIdentifiers
import CoreServices
import CoreText

struct Playlist: Decodable { let files: [String]; let shuffle: Bool; let autoplay: Bool?; let playbackFiles: [String: String]?; let matroskaFiles: [String]?; let mediaHelper: String?; let mediaDirectory: String?; let mediaErrors: [String: String]?; let office: Bool?; let image: Bool?; let imagePath: String?; let imageWidth: Double?; let imageHeight: Double?; let associations: String?; let font: Bool?; let sqlite: String?; let sqliteInfo: String?; let spreadsheet: String?; let htmlContent: String?; let htmlFile: Bool?; let fileBytes: Bool?; let spreadsheetIndex: Bool?; let wordReading: String?; let wordImagePages: Bool?; let spreadsheetCache: String?; let spreadsheetHelper: String?; let language: String?; let localizations: [String: String]? }

var nativeTranslations: [String: String] = [:]
func L(_ text: String) -> String { nativeTranslations[text] ?? text }
func localizeMenu(_ menu: NSMenu) {
    for item in menu.items {
        item.title = L(item.title)
        if let submenu = item.submenu { localizeMenu(submenu) }
    }
}

// Default handlers change only after a selection and explicit confirmation.
final class AssociationWindow: NSObject, NSWindowDelegate, NSTableViewDataSource, NSTableViewDelegate {
    let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 980, height: 860), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
    let bundleURL: URL
    let identifier: String
    let preferences = UserDefaults(suiteName: "io.github.jony4.glim.file-associations")!
    let groups: [(String, [String])] = [
        ("Markdown", ["md", "markdown"]), ("Plain Text", ["txt", "log"]),
        ("JSON", ["json", "jsonl", "geojson"]), ("YAML / TOML", ["yaml", "yml", "toml"]),
        ("HTML", ["html", "htm"]), ("XML", ["xml"]),
        ("Source Code", ["rs", "py", "js", "jsx", "ts", "tsx", "c", "h", "cpp", "hpp", "go", "swift", "java", "kt", "rb", "php", "sh", "sql", "css"]),
        ("SQLite", ["sqlite", "sqlite3", "db", "db3"]),
        ("Fonts", ["ttf", "otf", "ttc", "otc", "dfont"]),
        ("PDF", ["pdf"]), ("Mind maps", ["xmind"]), ("Ebooks", ["epub", "fb2", "cbz"]), ("Images", ["png", "jpg", "jpeg", "webp", "gif", "svg", "bmp", "tiff"]),
        ("Audio", ["mp3", "m4a", "wav", "aac", "flac", "aiff", "caf"]),
        ("Video", ["mp4", "mov", "m4v", "mkv"]),
        ("Keynote", ["key"]), ("Pages", ["pages"]), ("Numbers", ["numbers"]),
        ("Word", ["doc", "docx", "rtf"]), ("Excel", ["xls", "xlsx", "xlsm"]), ("PowerPoint", ["ppt", "pptx"])
    ]
    var checks: [NSButton] = []
    let table = NSTableView()
    var currentNames: [String] = []
    let selectAllButton = NSButton(checkboxWithTitle: L("Select All"), target: nil, action: nil)
    let status = NSTextField(wrappingLabelWithString: L("Select formats to change. Other file types keep their current defaults."))
    init(bundleURL: URL) throws {
        self.bundleURL = bundleURL
        guard let bundle = Bundle(url: bundleURL), let identifier = bundle.bundleIdentifier,
              identifier == "io.github.jony4.glimpse" else { throw NSError(domain: "Glim", code: 1, userInfo: [NSLocalizedDescriptionKey: "Cannot identify the installed Glim app."]) }
        self.identifier = identifier
        super.init()
        window.title = L("Default File Types — Glim"); window.minSize = NSSize(width: 760, height: 420); window.delegate = self; window.isReleasedWhenClosed = false
        let host = window.contentView!
        let stack = NSStackView(); stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 12; stack.translatesAutoresizingMaskIntoConstraints = false
        host.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo: host.leadingAnchor, constant: 18), stack.trailingAnchor.constraint(equalTo: host.trailingAnchor, constant: -18), stack.topAnchor.constraint(equalTo: host.topAnchor, constant: 18), stack.bottomAnchor.constraint(equalTo: host.bottomAnchor, constant: -18)])
        let heading = NSTextField(labelWithString: L("Open these file types with Glim")); heading.font = .systemFont(ofSize: 17, weight: .semibold); stack.addArrangedSubview(heading)
        let hint = NSTextField(wrappingLabelWithString: L("macOS applies defaults to document types. Extensions belonging to the same type may change together."))
        hint.textColor = .secondaryLabelColor; stack.addArrangedSubview(hint)
        selectAllButton.target = self; selectAllButton.action = #selector(selectAll)
        selectAllButton.allowsMixedState = true
        stack.addArrangedSubview(selectAllButton)
        for (title, width) in [("Select", 60.0), ("File Type", 180.0), ("Extensions", 300.0), ("Current Application", 360.0)] {
            let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(title))
            column.title = L(title); column.width = width; column.minWidth = title == "Select" ? 60 : 100
            table.addTableColumn(column)
        }
        currentNames = Array(repeating: "", count: groups.count)
        table.rowHeight = 20
        table.usesAlternatingRowBackgroundColors = true; table.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        table.allowsEmptySelection = true
        for _ in groups {
            let check = NSButton(checkboxWithTitle: "", target: self, action: #selector(selectionChanged(_:)))
            check.allowsMixedState = true; check.tag = checks.count; checks.append(check)
        }
        table.dataSource = self; table.delegate = self
        refresh()
        let scroll = NSScrollView(); scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true
        scroll.documentView = table; scroll.borderType = .bezelBorder
        stack.addArrangedSubview(scroll); scroll.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        scroll.heightAnchor.constraint(greaterThanOrEqualToConstant: 280).isActive = true
        status.font = .systemFont(ofSize: 12); stack.addArrangedSubview(status)
        let buttons = NSStackView()
        buttons.addArrangedSubview(NSButton(title: L("Set Selected to Glim"), target: self, action: #selector(apply)))
        stack.addArrangedSubview(buttons)
        let contentHeight = CGFloat(groups.count) * (table.rowHeight + table.intercellSpacing.height) + 240
        let screenHeight = NSScreen.main?.visibleFrame.height ?? 900
        window.setContentSize(NSSize(width: 900, height: min(contentHeight, screenHeight - 80)))
        window.center(); window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
    }
    func types(_ index: Int) -> [String] { Array(Set(groups[index].1.compactMap { UTType(filenameExtension: $0)?.identifier })).sorted() }
    func handler(_ type: String) -> String? {
        guard let value = LSCopyDefaultRoleHandlerForContentType(type as CFString, .all)?.takeRetainedValue() else { return nil }
        return value as String
    }
    func refresh() {
        var refreshedNames: [String] = []
        for index in groups.indices {
            let supported = types(index)
            let owned = supported.filter { handler($0) == identifier }.count
            checks[index].state = owned == 0 ? .off : (owned == supported.count ? .on : .mixed)
            let names = Set(types(index).map { type -> String in
                guard let id = handler(type) else { return L("Not set") }
                if id == identifier { return "Glim" }
                return NSWorkspace.shared.urlForApplication(withBundleIdentifier: id)?.deletingPathExtension().lastPathComponent ?? id
            })
            refreshedNames.append(names.isEmpty ? L("System type unavailable") : names.sorted().joined(separator: ", "))
        }
        currentNames = refreshedNames
        updateSelectAll(); table.reloadData()
    }
    func numberOfRows(in tableView: NSTableView) -> Int { groups.count }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard groups.indices.contains(row), checks.indices.contains(row), currentNames.indices.contains(row) else { return nil }
        switch tableColumn?.identifier.rawValue {
        case "Select": return checks[row]
        case "File Type": return NSTextField(labelWithString: L(groups[row].0))
        default:
            let value = tableColumn?.identifier.rawValue == "Extensions" ? groups[row].1.map { "." + $0 }.joined(separator: ", ") : currentNames[row]
            let label = NSTextField(labelWithString: value); label.lineBreakMode = .byTruncatingTail; label.toolTip = value
            return label
        }
    }
    func updateSelectAll() {
        selectAllButton.state = checks.allSatisfy { $0.state == .on } ? .on : (checks.allSatisfy { $0.state == .off } ? .off : .mixed)
    }
    @objc func selectionChanged(_ sender: NSButton) { updateSelectAll() }
    @objc func selectAll() {
        let state: NSControl.StateValue = checks.allSatisfy { $0.state == .on } ? .off : .on
        for check in checks { check.state = state }; updateSelectAll()
    }
    @objc func apply() { change(restore: false) }
    @objc func restore() { change(restore: true) }
    func change(restore: Bool) {
        let selected = groups.indices.filter { checks[$0].state != .off }
        guard !selected.isEmpty else { status.stringValue = L("Select at least one format first."); return }
        let alert = NSAlert(); alert.messageText = L(restore ? "Restore previous default applications?" : "Use Glim as the default application?")
        alert.informativeText = selected.map { L(groups[$0].0) }.joined(separator: ", ")
        alert.addButton(withTitle: L(restore ? "Restore" : "Apply")); alert.addButton(withTitle: L("Cancel"))
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        guard LSRegisterURL(bundleURL as CFURL, true) == noErr else { status.stringValue = L("macOS could not register Glim. Move it to Applications and retry."); return }
        var previous = preferences.dictionary(forKey: "previousHandlers") as? [String: String] ?? [:]
        let selectedTypes = Set(selected.flatMap { types($0) })
        var done = 0; var failures = 0
        for type in selectedTypes {
            let current = handler(type)
            let destination: String
            if restore {
                guard current == identifier, let old = previous[type] else { failures += 1; continue }
                destination = old
            } else {
                destination = identifier
                if let current = current, current != identifier { previous[type] = current }
            }
            let result = LSSetDefaultRoleHandlerForContentType(type as CFString, .all, destination as CFString)
            if result == noErr && handler(type) == destination { done += 1; if restore { previous.removeValue(forKey: type) } }
            else { failures += 1 }
        }
        preferences.set(previous, forKey: "previousHandlers")
        status.stringValue = L("Updated %d document types.").replacingOccurrences(of: "%d", with: String(done)) + (failures > 0 ? " " + L("%d could not be changed or had no saved default. You can also use Finder → Get Info → Open with → Change All.").replacingOccurrences(of: "%d", with: String(failures)) : "")
        refresh()
    }
    func windowWillClose(_ notification: Notification) { NSApp.terminate(nil) }
}

// Resource-fork fonts wrap ordinary sfnt data; Core Text may not enumerate
// the container URL even though it can decode the embedded font data.
func resourceFontDescriptors(_ url: URL) throws -> [CTFontDescriptor] {
    let data = try Data(contentsOf: url, options: .mappedIfSafe)

    let bytes = data
    func integer(_ offset: Int, _ count: Int) throws -> Int {
        guard offset >= 0, count > 0, offset <= bytes.count - count else {
            throw NSError(domain: "Glim", code: 1, userInfo: [NSLocalizedDescriptionKey: "Truncated dfont resource map"])
        }
        return bytes[offset..<(offset + count)].reduce(0) { ($0 << 8) | Int($1) }
    }
    let dataOffset = try integer(0, 4)
    let mapOffset = try integer(4, 4)
    let typeList = mapOffset + (try integer(mapOffset + 24, 2))
    let typeCount = (try integer(typeList, 2)) + 1
    var fonts: [CTFontDescriptor] = []
    for index in 0..<typeCount {
        let entry = typeList + 2 + index * 8
        let type = try integer(entry, 4)
        guard type == 0x73666e74 else { continue } // sfnt
        let count = (try integer(entry + 4, 2)) + 1
        let references = typeList + (try integer(entry + 6, 2))
        for face in 0..<count {
            let resource = dataOffset + (try integer(references + face * 12 + 5, 3))
            let length = try integer(resource, 4)
            let start = resource + 4
            guard length > 0, start <= bytes.count, length <= bytes.count - start else {
                throw NSError(domain: "Glim", code: 1, userInfo: [NSLocalizedDescriptionKey: "Truncated dfont sfnt resource"])
            }
            let fontData = data[start..<(start + length)]
            fonts += CTFontManagerCreateFontDescriptorsFromData(fontData as CFData) as? [CTFontDescriptor] ?? []
        }
    }
    return fonts
}

final class FontPreview: NSObject, NSTextFieldDelegate {
    let view = NSView()
    let selector = NSPopUpButton()
    let input = NSTextField(string: "The quick brown fox jumps over the lazy dog. 0123456789 中文字体预览")
    let metadata = NSTextField(wrappingLabelWithString: L("Loading font…"))
    var descriptors: [CTFontDescriptor] = []
    let sizes: [CGFloat] = [12, 18, 24, 36, 48, 72]
    var samples: [NSTextField] = []
    var availableFaces = 0
    init(url: URL, extraControls: [NSView] = []) {
        super.init()
        let stack = NSStackView(); stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 12; stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 20), stack.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -20), stack.topAnchor.constraint(equalTo: view.topAnchor, constant: 16), stack.bottomAnchor.constraint(equalTo: view.bottomAnchor, constant: -16)])
        let title = NSTextField(labelWithString: url.lastPathComponent); title.font = .systemFont(ofSize: 17, weight: .semibold); stack.addArrangedSubview(title)
        selector.target = self; selector.action = #selector(changeFace); selector.isEnabled = false
        input.delegate = self; input.placeholderString = L("Type sample text…")
        input.widthAnchor.constraint(equalToConstant: 280).isActive = true
        metadata.textColor = .secondaryLabelColor; metadata.font = .systemFont(ofSize: 12); stack.addArrangedSubview(metadata)
        let rows = NSStackView(); rows.orientation = .vertical; rows.alignment = .leading; rows.spacing = 20; rows.edgeInsets = NSEdgeInsets(top: 16, left: 0, bottom: 16, right: 12)
        for size in sizes {
            let group = NSStackView(); group.orientation = .vertical; group.alignment = .leading; group.spacing = 6
            let caption = NSTextField(labelWithString: "\(Int(size)) pt"); caption.textColor = .secondaryLabelColor; caption.font = .systemFont(ofSize: 11)
            let sample = NSTextField(wrappingLabelWithString: ""); sample.isSelectable = true
            group.addArrangedSubview(caption); group.addArrangedSubview(sample); rows.addArrangedSubview(group); samples.append(sample)
            group.widthAnchor.constraint(equalTo: rows.widthAnchor).isActive = true
            sample.widthAnchor.constraint(equalTo: group.widthAnchor).isActive = true
        }
        let scroll = NSScrollView(); scroll.hasVerticalScroller = true; scroll.documentView = rows
        rows.translatesAutoresizingMaskIntoConstraints = false; rows.widthAnchor.constraint(equalTo: scroll.contentView.widthAnchor).isActive = true
        stack.addArrangedSubview(scroll); scroll.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        let note = NSTextField(wrappingLabelWithString: L("Preview only — this font is not installed. Missing characters may use a system fallback.")); note.font = .systemFont(ofSize: 11); note.textColor = .secondaryLabelColor; stack.addArrangedSubview(note)
        let fontControls = NSStackView(); fontControls.addArrangedSubview(selector); fontControls.addArrangedSubview(input)
        extraControls.forEach { fontControls.addArrangedSubview($0) }
        scroll.contentInsets.bottom = 72
        FloatingReaderToolbar(items: fontControls).install(in: view)
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            var fonts = CTFontManagerCreateFontDescriptorsFromURL(url as CFURL) as? [CTFontDescriptor] ?? []
            var failure = "This font could not be decoded. It may be damaged or unsupported."
            if fonts.isEmpty && url.pathExtension.lowercased() == "dfont" {
                do {
                    fonts = try resourceFontDescriptors(url)
                    if fonts.isEmpty { failure = "No supported sfnt font resources found in this dfont." }
                } catch { failure = error.localizedDescription }
            }
            let loadedFonts = fonts
            let failureMessage = failure
            DispatchQueue.main.async {
                guard let self = self else { return }
                self.availableFaces = loadedFonts.count
                self.descriptors = loadedFonts
                guard !self.descriptors.isEmpty else {
                    self.metadata.stringValue = L(failureMessage)
                    self.selector.isHidden = true
                    self.input.isHidden = true
                    scroll.isHidden = true
                    note.isHidden = true
                    return
                }
                for descriptor in self.descriptors {
                    let font = CTFontCreateWithFontDescriptor(descriptor, 16, nil)
                    self.selector.addItem(withTitle: CTFontCopyFullName(font) as String)
                }
                self.selector.isEnabled = self.descriptors.count > 1
                self.changeFace()
            }
        }
    }
    @objc func changeFace() { ReaderTelemetry.record(8, 16, 0);
        let index = selector.indexOfSelectedItem
        guard descriptors.indices.contains(index) else { return }
        let font = CTFontCreateWithFontDescriptor(descriptors[index], 16, nil)
        metadata.stringValue = "\(CTFontCopyFamilyName(font)) · \(CTFontCopyPostScriptName(font)) · " + L("%glyphs glyphs · %faces faces").replacingOccurrences(of: "%glyphs", with: String(CTFontGetGlyphCount(font))).replacingOccurrences(of: "%faces", with: String(availableFaces))
        updateSamples()
    }
    func controlTextDidChange(_ notification: Notification) { updateSamples() }
    func updateSamples() {
        let index = selector.indexOfSelectedItem
        guard descriptors.indices.contains(index) else { return }
        let text = input.stringValue
        for (label, size) in zip(samples, sizes) {
            label.font = CTFontCreateWithFontDescriptor(descriptors[index], size, nil) as NSFont
            label.stringValue = text
        }
    }
}

// The active cover has one retained snapshot; opening it hands a complete
// image to the default viewer instead of limiting access to the thumbnail.
private final class ArtworkSnapshot {
    let url: URL
    private var opened = false
    init(data: Data, source: CGImageSource) throws {
        let ext = (CGImageSourceGetType(source) as String?).flatMap { UTType($0)?.preferredFilenameExtension } ?? "png"
        url = FileManager.default.temporaryDirectory.appendingPathComponent("glim-artwork-" + UUID().uuidString).appendingPathExtension(ext)
        try data.write(to: url, options: .atomic)
    }
    func open() { opened = NSWorkspace.shared.open(url) }
    deinit {
        guard !opened else { return } // The external viewer may still be opening this file.
        let path = url
        DispatchQueue.global(qos: .utility).async { try? FileManager.default.removeItem(at: path) }
    }
}

// An audio-only surface underneath AVKit's native playback controls.
final class AudioBackdrop: NSView {
    var song = "" { didSet { needsDisplay = true } }
    var artist = "" { didSet { needsDisplay = true } }
    var album = "" { didSet { needsDisplay = true } }
    var artwork: NSImage? { didSet { needsDisplay = true } }
    var motionTime: Double = 0
    var playing = false
    override var isFlipped: Bool { true }
    override var wantsDefaultClipping: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func draw(_ dirtyRect: NSRect) {
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        NSBezierPath(rect: bounds).addClip()
        let top = NSColor(calibratedRed: 0.10, green: 0.17, blue: 0.23, alpha: 1)
        let bottom = NSColor(calibratedRed: 0.05, green: 0.08, blue: 0.13, alpha: 1)
        NSGradient(starting: top, ending: bottom)?.draw(in: bounds, angle: 90)
        let side = max(64, min(280, min(bounds.width * 0.52, (bounds.height - 170) * 0.65)))
        let blockHeight = side + 158
        let y = max(24, (bounds.height - blockHeight) / 2)
        let cover = NSRect(x: bounds.midX - side / 2, y: y, width: side, height: side)
        NSGraphicsContext.saveGraphicsState()
        let shadow = NSShadow(); shadow.shadowColor = NSColor.black.withAlphaComponent(0.3); shadow.shadowBlurRadius = 22; shadow.shadowOffset = NSSize(width: 0, height: 8); shadow.set()
        NSColor(calibratedWhite: 0.12, alpha: 1).setFill()
        NSBezierPath(roundedRect: cover, xRadius: 14, yRadius: 14).fill()
        NSGraphicsContext.restoreGraphicsState()
        if let artwork = artwork {
            NSGraphicsContext.saveGraphicsState()
            NSBezierPath(roundedRect: cover, xRadius: 14, yRadius: 14).addClip()
            let size = artwork.size
            let scale = max(side / max(1, size.width), side / max(1, size.height))
            let rect = NSRect(x: cover.midX - size.width * scale / 2, y: cover.midY - size.height * scale / 2, width: size.width * scale, height: size.height * scale)
            artwork.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
            NSGraphicsContext.restoreGraphicsState()
        } else {
            let disc = cover.insetBy(dx: side * 0.06, dy: side * 0.06)
            NSColor(calibratedWhite: 0.055, alpha: 1).setFill(); NSBezierPath(ovalIn: disc).fill()
            NSColor.white.withAlphaComponent(0.08).setStroke()
            for fraction in [CGFloat(0.08), 0.14, 0.20, 0.26] {
                NSBezierPath(ovalIn: disc.insetBy(dx: side * fraction, dy: side * fraction)).stroke()
            }
            NSColor(calibratedRed: 0.39, green: 0.78, blue: 0.72, alpha: 1).setFill()
            NSBezierPath(ovalIn: cover.insetBy(dx: side * 0.35, dy: side * 0.35)).fill()
            NSColor(calibratedWhite: 0.1, alpha: 1).setFill()
            NSBezierPath(ovalIn: NSRect(x: cover.midX - 4, y: cover.midY - 4, width: 8, height: 8)).fill()
        }
        let style = NSMutableParagraphStyle(); style.alignment = .center; style.lineBreakMode = .byTruncatingTail
        func label(_ text: String, _ offset: CGFloat, _ size: CGFloat, _ color: NSColor, _ weight: NSFont.Weight) {
            (text as NSString).draw(in: NSRect(x: 28, y: cover.maxY + offset, width: max(1, bounds.width - 56), height: 30), withAttributes: [.font: NSFont.systemFont(ofSize: size, weight: weight), .foregroundColor: color, .paragraphStyle: style])
        }
        label(song, 22, 22, .white, .semibold)
        label(artist, 54, 15, NSColor.white.withAlphaComponent(0.72), .regular)
        label(album, 79, 12, NSColor.white.withAlphaComponent(0.45), .regular)
        // A decorative playback indicator, driven by playback time rather than
        // fabricated frequency/amplitude measurements. Respect Reduce Motion.
        let animate = playing && !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        let count = 29
        let spacing: CGFloat = min(8, max(4, (bounds.width - 64) / CGFloat(count)))
        let start = bounds.midX - CGFloat(count - 1) * spacing / 2
        let centerY = min(bounds.height - 84, cover.maxY + 126)
        if centerY > cover.maxY + 100 {
            for index in 0..<count {
                let position = Double(index) / Double(count - 1)
                let envelope = sin(position * .pi)
                let wave = (sin(motionTime * 4.1 + Double(index) * 0.72)
                    + sin(motionTime * 2.7 - Double(index) * 0.41) + 2) / 4
                let height: CGFloat = animate ? CGFloat(4 + 25 * envelope * wave) : CGFloat(3 + 3 * envelope)
                NSColor(calibratedRed: 0.39, green: 0.78, blue: 0.72, alpha: animate ? 0.68 : 0.25).setFill()
                NSBezierPath(roundedRect: NSRect(x: start + CGFloat(index) * spacing - 1.5,
                    y: centerY - height / 2, width: 3, height: height), xRadius: 1.5, yRadius: 1.5).fill()
            }
        }
    }
}

// Shared presentation input for PDFKit and Quick Look, scoped to one window.
enum SlideshowCommand { case previous, next, first, last, exit }
private final class SlideshowInput {
    private var monitor: Any?
    private var clickStarted = false
    private var clickDragged = false
    init(content: NSView, window: @escaping () -> NSWindow?, allowClick: @escaping (NSEvent) -> Bool = { _ in true }, perform: @escaping (SlideshowCommand) -> Void) {
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .leftMouseDown, .leftMouseDragged, .leftMouseUp]) { [weak content] event in
            guard let content = content, event.window === window() else { return event }
            if event.type == .keyDown {
                guard event.modifierFlags.intersection([.command, .control, .option]).isEmpty else { return event }
                if event.keyCode != 53, let editor = event.window?.firstResponder as? NSTextView, editor.isEditable { return event }
                let command: SlideshowCommand
                switch event.keyCode {
                case 53: command = .exit
                case 123, 126, 116: command = .previous
                case 124, 125, 121: command = .next
                case 49: command = event.modifierFlags.contains(.shift) ? .previous : .next
                case 115: command = .first
                case 119: command = .last
                default: return event
                }
                perform(command)
                return nil
            }
            if event.type == .leftMouseDragged {
                if self.clickStarted { self.clickDragged = true; return nil }
                return event
            }
            if event.type == .leftMouseDown {
                self.clickStarted = event.clickCount == 1 && event.modifierFlags.intersection([.shift, .command, .control, .option]).isEmpty && Self.isDocumentClick(event, content: content) && allowClick(event)
                self.clickDragged = false
                return self.clickStarted ? nil : event
            }
            let consumed = self.clickStarted
            let advance = self.clickStarted && !self.clickDragged && Self.isDocumentClick(event, content: content) && allowClick(event)
            self.clickStarted = false
            if advance { perform(.next) }
            return consumed ? nil : event
        }
    }
    private static func isDocumentClick(_ event: NSEvent, content: NSView) -> Bool {
        guard let parent = content.superview,
              let hit = content.hitTest(parent.convert(event.locationInWindow, from: nil)) else { return false }
        var ancestor: NSView? = hit
        while let view = ancestor {
            if view is NSControl || view is NSScroller || view is FloatingReaderToolbar { return false }
            if view === content { break }
            ancestor = view.superview
        }
        if let window = content.window,
           let accessible = hit.accessibilityHitTest(window.convertPoint(toScreen: event.locationInWindow)) as? NSAccessibilityProtocol,
           let role = accessible.accessibilityRole(), [NSAccessibility.Role.button, .link, .textField, .popUpButton, .slider, .scrollBar].contains(role) { return false }
        return true
    }
    func cancel() { if let monitor = monitor { NSEvent.removeMonitor(monitor); self.monitor = nil } }
    deinit { cancel() }
}

final class NativePreviewHost: NSView {
    private var presentationInput: SlideshowInput?
    private var gpuiOpacity: CGFloat = 1
    private var concealedGPUI = false
    var presentationCommand: ((SlideshowCommand) -> Void)?
    private var presentationObservers: [NSObjectProtocol] = []
    private weak var presentationWindow: NSWindow?
    private var embeddedFrame = NSRect.zero
    private var enteredWindowFullscreen = false
    private(set) var presentingSlides = false
    var presentationChanged: ((Bool) -> Void)?
    override var acceptsFirstResponder: Bool { true }

    func presentSlides() {
        if presentingSlides { stopPresentation(); return }
        guard let window = window, let parent = superview else { return }
        // QLPreviewView cannot safely migrate between windows: moving it through
        // NSView fullscreen triggers Quick Look's activate assertion on return.
        // Keep its complete view hierarchy in the same NSWindow throughout.
        presentingSlides = true
        presentationWindow = window
        embeddedFrame = frame
        enteredWindowFullscreen = !window.styleMask.contains(.fullScreen)
        frame = parent.bounds
        layer?.cornerRadius = 0
        presentationChanged?(true)
        if let gpui = gpuiView, !isDescendant(of: gpui) {
            gpuiOpacity = gpui.alphaValue
            gpui.alphaValue = 0
            concealedGPUI = true
        }
        presentationInput = SlideshowInput(content: self, window: { [weak self] in self?.presentationWindow }) { [weak self] command in
            guard let self = self else { return }
            if command == .exit {
                DispatchQueue.main.async { [weak self] in self?.stopPresentation() }
            } else { self.presentationCommand?(command) }
        }
        let center = NotificationCenter.default
        presentationObservers.append(center.addObserver(forName: NSWindow.didResizeNotification, object: window, queue: .main) { [weak self] _ in
            guard let self = self, self.presentingSlides, let parent = self.superview else { return }
            self.frame = parent.bounds
        })
        presentationObservers.append(center.addObserver(forName: NSWindow.didExitFullScreenNotification, object: window, queue: .main) { [weak self] _ in
            self?.stopPresentation(exitWindowFullscreen: false)
        })
        presentationObservers.append(center.addObserver(forName: NSWindow.willCloseNotification, object: window, queue: .main) { [weak self] _ in
            self?.stopPresentation(exitWindowFullscreen: false)
        })
        if enteredWindowFullscreen { window.toggleFullScreen(nil) }
    }
    func stopPresentation(exitWindowFullscreen: Bool = true) {
        guard presentingSlides else { return }
        presentingSlides = false
        presentationInput?.cancel(); presentationInput = nil
        if concealedGPUI { gpuiView?.alphaValue = gpuiOpacity; concealedGPUI = false }
        presentationObservers.forEach { NotificationCenter.default.removeObserver($0) }
        presentationObservers.removeAll()
        let window = presentationWindow
        presentationWindow = nil
        frame = embeddedFrame
        layer?.cornerRadius = 10
        presentationChanged?(false)
        if exitWindowFullscreen && enteredWindowFullscreen && window?.styleMask.contains(.fullScreen) == true {
            window?.toggleFullScreen(nil)
        }
        enteredWindowFullscreen = false
    }
    weak var gpuiView: NSView?
    private var originalAccessibilityChildren: [Any]?

    func updateAccessibility() {
        guard let window = window else { return }
        if isHidden {
            if let original = originalAccessibilityChildren {
                window.setAccessibilityChildren(original)
                originalAccessibilityChildren = nil
            }
        } else {
            let original = (window.accessibilityChildren() ?? []).filter { !($0 is NativePreviewHost) }
            originalAccessibilityChildren = original
            window.setAccessibilityChildren(original + [self])
        }
        NSAccessibility.post(element: window, notification: .layoutChanged)
    }
}

// Build the PDF reading menu from public PDFKit actions. System text services
// and browser/search integrations do not belong in this document context menu.
final class ReaderPDFView: PDFView {
    private var fitsWidth = false
    private var applyingWidth = false
    override func layout() { super.layout(); refitWidth() }
    @objc func fitToWidth() { ReaderTelemetry.record(2, 4, 0); fitsWidth = true; autoScales = false; needsLayout = true; refitWidth() }
    @objc private func widthPageChanged() { refitWidth() }
    private func refitWidth() {
        guard fitsWidth, !applyingWidth, let page = currentPage, bounds.width > 0 else { return }
        func width(_ page: PDFPage) -> CGFloat {
            let box = page.bounds(for: displayBox)
            return page.rotation % 180 == 0 ? box.width : box.height
        }
        var pageWidth = width(page)
        var pageCount = 1
        if displayMode == .twoUp || displayMode == .twoUpContinuous, let document = document {
            let index = document.index(for: page)
            if (0..<document.pageCount).contains(index) {
                let partner = displaysAsBook ? (index % 2 == 1 ? index + 1 : index - 1) : (index % 2 == 0 ? index + 1 : index - 1)
                let other = (0..<document.pageCount).contains(partner) ? document.page(at: partner) : nil
                pageWidth += other.map(width) ?? width(page)
                pageCount = 2
            }
        }
        let viewport = documentView?.enclosingScrollView?.contentSize.width ?? bounds.width
        let margins = displaysPageBreaks ? (pageBreakMargins.left + pageBreakMargins.right) * CGFloat(pageCount) : 0
        let available = viewport - margins
        guard pageWidth > 0, available > 0 else { return }
        let target = available / pageWidth
        guard target.isFinite, target > 0, target != scaleFactor else { return }
        applyingWidth = true; defer { applyingWidth = false }
        // Fit the viewport naturally, including unusually small/large pages.
        minScaleFactor = min(minScaleFactor, target)
        maxScaleFactor = max(maxScaleFactor, target)
        scaleFactor = target
    }
    override func zoomIn(_ sender: Any?) { ReaderTelemetry.record(2, 1, 0); fitsWidth = false; super.zoomIn(sender) }
    override func zoomOut(_ sender: Any?) { ReaderTelemetry.record(2, 2, 0); fitsWidth = false; super.zoomOut(sender) }
    override func magnify(with event: NSEvent) { fitsWidth = false; super.magnify(with: event) }
    private var contextMenuRouter: ReaderContextMenu?
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        NotificationCenter.default.removeObserver(self, name: .PDFViewPageChanged, object: self)
        if window != nil {
            NotificationCenter.default.addObserver(self, selector: #selector(widthPageChanged), name: .PDFViewPageChanged, object: self)
        }
        contextMenuRouter = nil
        guard window != nil else { return }
        // PDFKit's private document subviews may construct their own menus,
        // bypassing PDFView.menu(for:). Route their context click explicitly.
        contextMenuRouter = ReaderContextMenu(host: self, kind: 2) { [weak self] event in self?.menu(for: event) }
    }
    deinit { NotificationCenter.default.removeObserver(self) }

    override func menu(for event: NSEvent) -> NSMenu? {
        let menu = ReaderMenu.make()
        func add(_ title: String, _ action: Selector, enabled: Bool = true, checked: Bool = false, tag: Int = 0) {
            let item = NSMenuItem(title: L(title), action: action, keyEquivalent: "")
            ReaderMenu.own(item); item.target = self; item.isEnabled = enabled; item.state = checked ? .on : .off; item.tag = tag
            menu.addItem(item)
        }
        add("Copy", #selector(PDFView.copy(_:)), enabled: document?.allowsCopying == true && !(currentSelection?.string ?? "").isEmpty)
        menu.addItem(.separator())
        add("Fit width", #selector(fitToWidth), checked: fitsWidth)
        add("Fit page", #selector(fitPage), checked: !fitsWidth && autoScales)
        add("Zoom in", #selector(PDFView.zoomIn(_:)), enabled: canZoomIn)
        add("Zoom out", #selector(PDFView.zoomOut(_:)), enabled: canZoomOut)
        add("Actual size", #selector(actualSize))
        menu.addItem(.separator())
        for (title, mode) in [("Single Page", PDFDisplayMode.singlePage), ("Single Page Continuous", .singlePageContinuous), ("Two Pages", .twoUp), ("Two Pages Continuous", .twoUpContinuous)] {
            add(title, #selector(changeLayout(_:)), checked: displayMode == mode, tag: mode.rawValue)
        }
        menu.addItem(.separator())
        add("Next page", #selector(PDFView.goToNextPage(_:)), enabled: canGoToNextPage)
        add("Previous page", #selector(PDFView.goToPreviousPage(_:)), enabled: canGoToPreviousPage)
        return menu
    }
    @objc private func fitPage() { ReaderTelemetry.record(2, 4, 0); fitsWidth = false; autoScales = true }
    @objc private func actualSize() { ReaderTelemetry.record(2, 3, 0); fitsWidth = false; autoScales = false; scaleFactor = 1 }
    @objc private func changeLayout(_ sender: NSMenuItem) { ReaderTelemetry.record(2, 12, 0);
        if let mode = PDFDisplayMode(rawValue: sender.tag) { displayMode = mode; needsLayout = true; refitWidth() }
    }
}

// Shares the already-unlocked document; PDFKit renders only the current page.
final class PDFSlideshow: NSObject {
    private let surface = NSView()
    private let pages = ReaderPDFView()
    private let position = NSTextField(labelWithString: "")
    private var input: SlideshowInput?
    private var finished = false
    private let returned: (PDFPage?) -> Void

    init(document: PDFDocument, page: PDFPage?, returned: @escaping (PDFPage?) -> Void) {
        self.returned = returned
        super.init()
        pages.document = document
        pages.displayMode = .singlePage
        pages.displayDirection = .horizontal
        pages.displaysPageBreaks = false
        pages.backgroundColor = .black
        pages.autoScales = true
        if let page = page { pages.go(to: page) }
        pages.translatesAutoresizingMaskIntoConstraints = false
        surface.addSubview(pages)
        NSLayoutConstraint.activate([
            pages.leadingAnchor.constraint(equalTo: surface.leadingAnchor), pages.trailingAnchor.constraint(equalTo: surface.trailingAnchor),
            pages.topAnchor.constraint(equalTo: surface.topAnchor), pages.bottomAnchor.constraint(equalTo: surface.bottomAnchor)
        ])
        let toolbar = NSStackView()
        for (title, action) in [(L("Previous"), #selector(previous)), (L("Next"), #selector(next))] {
            toolbar.addArrangedSubview(NSButton(title: title, target: self, action: action))
        }
        position.font = .monospacedDigitSystemFont(ofSize: 13, weight: .medium)
        toolbar.addArrangedSubview(position)
        toolbar.addArrangedSubview(NSButton(title: L("Exit Slideshow"), target: self, action: #selector(exit)))
        FloatingReaderToolbar(items: toolbar).install(in: surface, managesContextMenu: false)
        NotificationCenter.default.addObserver(self, selector: #selector(updatePosition), name: .PDFViewPageChanged, object: pages)
        updatePosition()
    }
    func start(in host: NSView) -> Bool {
        guard let screen = host.window?.screen else { return false }
        surface.frame = host.bounds
        surface.autoresizingMask = [.width, .height]
        host.addSubview(surface)
        guard surface.enterFullScreenMode(screen, withOptions: [.fullScreenModeAllScreens: false]) else {
            surface.removeFromSuperview()
            return false
        }
        pages.autoScales = true
        surface.window?.makeFirstResponder(pages)
        input = SlideshowInput(content: pages, window: { [weak self] in self?.surface.window }, allowClick: { [weak self] event in
            guard let self = self else { return false }
            let point = self.pages.convert(event.locationInWindow, from: nil)
            if let page = self.pages.page(for: point, nearest: false),
               let annotation = page.annotation(at: self.pages.convert(point, to: page)), annotation.action != nil || annotation.url != nil { return false }
            return true
        }) { [weak self] command in
            guard let self = self else { return }
            switch command {
            case .exit: DispatchQueue.main.async { [weak self] in self?.exit() }
            case .previous: self.previous()
            case .next: self.next()
            case .first: self.pages.goToFirstPage(nil)
            case .last: self.pages.goToLastPage(nil)
            }
        }
        return true
    }
    @objc private func previous() { ReaderTelemetry.record(2, 6); pages.goToPreviousPage(nil) }
    @objc private func next() { ReaderTelemetry.record(2, 5); pages.goToNextPage(nil) }
    @objc private func updatePosition() {
        if let document = pages.document, let page = pages.currentPage {
            position.stringValue = "\(document.index(for: page) + 1) / \(document.pageCount)"
        }
    }
    @objc func exit() {
        guard !finished else { return }
        finished = true
        input?.cancel(); input = nil
        NotificationCenter.default.removeObserver(self)
        let page = pages.currentPage
        if surface.isInFullScreenMode { surface.exitFullScreenMode(options: nil) }
        surface.removeFromSuperview()
        pages.document = nil
        returned(page)
    }
    deinit {
        input?.cancel()
        NotificationCenter.default.removeObserver(self)
    }
}

// A selected video's helper is cancellable independently of the player and source file.
final class MediaPreparation {
    private let lock = NSLock()
    private let process = Process()
    private var cancelled = false
    func cancel() {
        lock.lock(); defer { lock.unlock() }
        cancelled = true
        if process.isRunning { process.terminate() }
    }
    func run(helper: URL, source: URL, destination: URL) throws {
        try run(helper: helper, arguments: ["--prepare-matroska", source.path, destination.path], destination: destination)
    }
    func run(helper: URL, arguments: [String], destination: URL) throws {
        let log = destination.appendingPathExtension("log")
        FileManager.default.createFile(atPath: log.path, contents: nil)
        let stderr = try FileHandle(forWritingTo: log)
        defer { try? stderr.close() }
        lock.lock()
        if cancelled { lock.unlock(); throw CocoaError(.userCancelled) }
        process.executableURL = helper
        process.arguments = arguments
        process.standardInput = FileHandle.nullDevice; process.standardOutput = FileHandle.nullDevice
        process.standardError = stderr
        do { try process.run(); lock.unlock() } catch { lock.unlock(); throw error }
        process.waitUntilExit()
        lock.lock(); let stopped = cancelled; lock.unlock()
        if stopped { throw CocoaError(.userCancelled) }
        if process.terminationStatus != 0 {
            let input = try FileHandle(forReadingFrom: log); defer { try? input.close() }
            let bytes = try input.read(upToCount: ReaderLimits.IO_BUFFER_BYTES) ?? Data()
            throw NSError(domain: "Glim", code: Int(process.terminationStatus), userInfo: [NSLocalizedDescriptionKey: String(decoding: bytes, as: UTF8.self)])
        }
    }
}

final class NativePreviewController: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    var closed = false
    let host = NativePreviewHost(frame: NSRect(x: 0, y: 0, width: 1000, height: 740))
    let player = AVPlayer()
    var pendingPlayback = false
    var playbackObserver: Any?
    var playbackStateObserver: NSKeyValueObservation?
    private var telemetryVolumeObserver: NSKeyValueObservation?
    private var telemetryMuteObserver: NSKeyValueObservation?
    private var telemetryVolumeWork: DispatchWorkItem?
    private weak var telemetryMediaItem: AVPlayerItem?
    private var telemetryMediaTime = 0.0
    private var telemetryMediaWall = 0.0
    private var telemetryPositionAt = 0.0
    private var telemetryGeneration: UInt64?
    let playerView = AVPlayerView()
    let pdfView = ReaderPDFView()
    var pdfSlideshow: PDFSlideshow?
    let audioBackdrop = AudioBackdrop()
    private var artworkSnapshot: ArtworkSnapshot?
    private let artworkButton = NSButton(title: L("Open full artwork"), target: nil, action: nil)
    var metadataAsset: AVURLAsset?
    let detailLabel = NSTextField(labelWithString: "")
    let table = NSTableView()
    var urls: [URL] = []
    var playbackFiles: [String: String] = [:]
    var mediaErrors: [String: String] = [:]
    var matroskaFiles = Set<String>()
    var mediaHelper: URL?
    var mediaDirectory: URL?
    var mediaPreparation: MediaPreparation?
    var mediaGeneration = 0
    var preparedMedia: URL?

    var index = 0
    var token: NSKeyValueObservation?
    var pdf = false
    private var previewCopy: PreviewCopy?
    private let copyRasterQueue = DispatchQueue(label: "glim.preview.copy-raster", qos: .userInitiated)
    private var presentationStartKeys: Any?
    var officeView: QLPreviewView?
    var spreadsheetView: WKWebView?
    var fontPreview: FontPreview?
    var imagePreview: ImagePreview?
    var htmlPreview: HTMLPreview?
    var htmlAnchor: String?
    var workbookPreview: WorkbookPreview?
    var wordPreview: WordPreview?
    var failed = Set<URL>()

    func button(_ title: String, _ action: Selector) -> NSButton {
        NSButton(title: title, target: self, action: action)
    }
    // Called on the application main thread, once per persistent reader tab.
    func load(_ playlist: Playlist) throws {
        nativeTranslations = playlist.localizations ?? [:]
        host.setAccessibilityElement(true)
        host.setAccessibilityRole(.group)
        host.setAccessibilityLabel(playlist.files.first.map { URL(fileURLWithPath: $0).lastPathComponent } ?? L("Preview"))
        urls = playlist.files.map { URL(fileURLWithPath: $0) }
            .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
        playbackFiles = playlist.playbackFiles ?? [:]
        mediaErrors = playlist.mediaErrors ?? [:]
        matroskaFiles = Set(playlist.matroskaFiles ?? [])
        mediaHelper = playlist.mediaHelper.map { URL(fileURLWithPath: $0) }
        mediaDirectory = playlist.mediaDirectory.map { URL(fileURLWithPath: $0) }
        if playlist.shuffle { urls.shuffle() }
        guard !urls.isEmpty else { throw NSError(domain: "Glim", code: 1, userInfo: [NSLocalizedDescriptionKey: "No media files found."]) }
        pdf = urls.count == 1 && urls[0].pathExtension.lowercased() == "pdf"
        if playlist.htmlContent != nil || playlist.htmlFile == true {
            let preview = HTMLPreview(document: urls[0], contents: playlist.htmlContent); htmlPreview = preview
            preview.view.frame = host.bounds; preview.view.autoresizingMask = [.width, .height]
            host.addSubview(preview.view)
            if let anchor = htmlAnchor { preview.reveal(anchor) }
            return
        }
        if playlist.spreadsheetIndex == true, let database = playlist.sqlite,
           let cache = playlist.spreadsheetCache, let helper = playlist.spreadsheetHelper {
            let preview = WorkbookPreview(database: database, cache: cache, helper: helper, information: playlist.sqliteInfo ?? "") { [weak self] in
                guard let self = self else { return [] }
                return self.contentCopyControls() + [self.externalOpenButton()]
            }
            workbookPreview = preview; preview.view.frame = host.bounds
            preview.view.autoresizingMask = [.width, .height]; host.addSubview(preview.view)
            return
        }
        if playlist.image == true {
            let imageURL = playlist.imagePath.map { URL(fileURLWithPath: $0) } ?? urls[0]
            let sourceSize = playlist.imageWidth.flatMap { width in playlist.imageHeight.map { NSSize(width: width, height: $0) } }
            let preview = ImagePreview(url: imageURL, sourceSize: sourceSize); imagePreview = preview
            preview.view.frame = host.bounds; preview.view.autoresizingMask = [.width, .height]
            host.addSubview(preview.view); return
        }
        if playlist.font == true {
            let preview = FontPreview(url: urls[0]); fontPreview = preview
            preview.view.frame = host.bounds
            preview.view.autoresizingMask = [.width, .height]
            host.addSubview(preview.view)
            return
        }
        if playlist.office == true {
            showOffice(urls[0], spreadsheet: playlist.spreadsheet, wordReading: playlist.wordReading, wordImagePages: playlist.wordImagePages == true)
            return
        }
        let content = NSStackView()
        content.orientation = .vertical
        content.spacing = 0
        content.translatesAutoresizingMaskIntoConstraints = false
        host.addSubview(content)
        NSLayoutConstraint.activate([
            content.leadingAnchor.constraint(equalTo: host.leadingAnchor), content.trailingAnchor.constraint(equalTo: host.trailingAnchor),
            content.topAnchor.constraint(equalTo: host.topAnchor), content.bottomAnchor.constraint(equalTo: host.bottomAnchor)
        ])
        detailLabel.textColor = .secondaryLabelColor
        let toolbar = NSStackView()
        toolbar.edgeInsets = NSEdgeInsets(top: 10, left: 12, bottom: 10, right: 12)
        toolbar.addArrangedSubview(button(L("Previous"), #selector(previous)))
        toolbar.addArrangedSubview(button(L("Next"), #selector(next)))
        if pdf {
            toolbar.addArrangedSubview(button("−", #selector(zoomOut)))
            toolbar.addArrangedSubview(button("+", #selector(zoomIn)))
            toolbar.addArrangedSubview(button(L("Fit width"), #selector(fit)))
            let slideshow = button(L("Play Slideshow"), #selector(presentPDF))
            slideshow.image = NSImage(systemSymbolName: "play.fill", accessibilityDescription: L("Play Slideshow"))
            slideshow.imagePosition = .imageLeading
            slideshow.toolTip = L("Present full screen; press Escape to return")
            toolbar.addArrangedSubview(slideshow)
            contentCopyControls().forEach { toolbar.addArrangedSubview($0) }
        } else {
            toolbar.addArrangedSubview(button(L("Play / Pause"), #selector(togglePlay)))
            artworkButton.target = self; artworkButton.action = #selector(openArtwork)
            artworkButton.isHidden = true; toolbar.addArrangedSubview(artworkButton)
        }
        toolbar.addArrangedSubview(detailLabel)
        if pdf {
            guard let document = PDFDocument(url: urls[0]) else { throw NSError(domain: "Glim", code: 2, userInfo: [NSLocalizedDescriptionKey: "This PDF could not be opened."]) }
            if document.isLocked {
                let alert = NSAlert(); alert.messageText = L("PDF password"); alert.addButton(withTitle: L("Open")); alert.addButton(withTitle: L("Cancel"))
                let password = NSSecureTextField(frame: NSRect(x: 0, y: 0, width: 260, height: 24)); alert.accessoryView = password
                guard alert.runModal() == .alertFirstButtonReturn && document.unlock(withPassword: password.stringValue) else { throw NSError(domain: "Glim", code: 3, userInfo: [NSLocalizedDescriptionKey: "PDF was not unlocked."]) }
            }
            pdfView.document = document; pdfView.displayMode = .singlePageContinuous; pdfView.fitToWidth()
            content.addArrangedSubview(pdfView)
            pdfView.widthAnchor.constraint(equalTo: content.widthAnchor).isActive = true
            NotificationCenter.default.addObserver(self, selector: #selector(pageChanged), name: .PDFViewPageChanged, object: pdfView)
            pageChanged()
        } else {
            playerView.player = player
            playbackObserver = player.addPeriodicTimeObserver(forInterval: CMTime(seconds: 1.0 / 24.0, preferredTimescale: 600), queue: .main) { [weak self] time in
                if let self = self { self.observePlayback(time) }
                guard let self = self, !self.audioBackdrop.isHidden,
                      self.host.window?.occlusionState.contains(.visible) == true,
                      !self.host.isHidden,
                      !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion else { return }
                let seconds = time.seconds
                if seconds.isFinite { self.audioBackdrop.motionTime = seconds }
                self.audioBackdrop.needsDisplay = true
            }
            playbackStateObserver = player.observe(\.timeControlStatus, options: [.initial, .new]) { [weak self] player, _ in
                DispatchQueue.main.async {
                    if self?.closed == false { ReaderTelemetry.record(5, 35, value: UInt64(player.timeControlStatus.rawValue)) }
                    self?.audioBackdrop.playing = player.timeControlStatus == .playing
                    self?.audioBackdrop.needsDisplay = true
                }
            }
            telemetryVolumeObserver = player.observe(\.volume, options: [.new]) { [weak self] _, _ in
                DispatchQueue.main.async {
                    guard let self = self, !self.closed else { return }
                    self.telemetryVolumeWork?.cancel()
                    let work = DispatchWorkItem { [weak self] in
                        guard let self = self, !self.closed else { return }
                        ReaderTelemetry.record(5, 23, value: UInt64(max(0, min(100, self.player.volume * 100))))
                    }
                    self.telemetryVolumeWork = work
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.4, execute: work)
                }
            }
            telemetryMuteObserver = player.observe(\.isMuted, options: [.new]) { [weak self] player, _ in
                guard self?.closed == false else { return }
                ReaderTelemetry.record(5, 36, value: player.isMuted ? 1 : 0)
            }
            playerView.controlsStyle = .inline
            if let overlay = playerView.contentOverlayView {
                audioBackdrop.frame = overlay.bounds
                audioBackdrop.autoresizingMask = [.width, .height]
                audioBackdrop.isHidden = true
                audioBackdrop.setAccessibilityElement(true)
                audioBackdrop.setAccessibilityRole(.image)
                overlay.addSubview(audioBackdrop)
            }
            let body = NSStackView(); body.orientation = .horizontal; body.spacing = 0
            body.addArrangedSubview(playerView)
            playerView.widthAnchor.constraint(equalTo: body.widthAnchor, constant: urls.count > 1 ? -240 : 0).isActive = true
            playerView.heightAnchor.constraint(equalTo: body.heightAnchor).isActive = true
            playerView.setContentHuggingPriority(.defaultLow, for: .horizontal)
            if urls.count > 1 {
                let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("file")); column.title = L("Playlist"); table.addTableColumn(column)
                table.headerView = nil; table.delegate = self; table.dataSource = self; table.rowHeight = 28
                let scroll = NSScrollView(); scroll.documentView = table; scroll.hasVerticalScroller = true
                scroll.widthAnchor.constraint(equalToConstant: 240).isActive = true
                body.addArrangedSubview(scroll)
                table.reloadData()
            }
            content.addArrangedSubview(body)
            body.widthAnchor.constraint(equalTo: content.widthAnchor).isActive = true
            NotificationCenter.default.addObserver(self, selector: #selector(finished(_:)), name: .AVPlayerItemDidPlayToEndTime, object: nil)
            NotificationCenter.default.addObserver(self, selector: #selector(playbackFailed(_:)), name: .AVPlayerItemFailedToPlayToEndTime, object: nil)
            play(0, autoplay: playlist.autoplay != false)
        }
        // Leave AVKit's timeline clear when its own playback controls appear.
        FloatingReaderToolbar(items: toolbar).install(in: host, bottomInset: pdf ? 16 : 80, managesContextMenu: !pdf)
    }
    func showOffice(_ url: URL, spreadsheet: String? = nil, wordReading: String? = nil, wordImagePages: Bool = false) {
        if ["doc", "docx", "rtf"].contains(url.pathExtension.lowercased()) {
            let preview = WordPreview(source: url, imagePages: wordImagePages, reading: wordReading.map { URL(fileURLWithPath: $0) }, extraControls: contentCopyControls() + [externalOpenButton()])
            wordPreview = preview
            preview.view.translatesAutoresizingMaskIntoConstraints = false; host.addSubview(preview.view)
            NSLayoutConstraint.activate([
                preview.view.leadingAnchor.constraint(equalTo: host.leadingAnchor), preview.view.trailingAnchor.constraint(equalTo: host.trailingAnchor),
                preview.view.topAnchor.constraint(equalTo: host.topAnchor), preview.view.bottomAnchor.constraint(equalTo: host.bottomAnchor)
            ])
            return
        }
        let toolbar = NSStackView()
        toolbar.translatesAutoresizingMaskIntoConstraints = false
        toolbar.spacing = 12
        if ["ppt", "pptx", "key"].contains(url.pathExtension.lowercased()) {
            let play = button(L("Play Slideshow"), #selector(presentSlides))
            play.image = NSImage(systemSymbolName: "play.fill", accessibilityDescription: L("Play Slideshow"))
            play.imagePosition = .imageLeading
            play.toolTip = L("Present full screen; press Escape to return")
            let previous = button(L("Previous"), #selector(presentationPrevious))
            let next = button(L("Next"), #selector(presentationNext))
            toolbar.addArrangedSubview(previous); toolbar.addArrangedSubview(next)
            toolbar.addArrangedSubview(play)
            // One stateful action avoids NSStackView visibility priorities
            // accidentally exposing both Play and Exit during layout updates.
            host.presentationChanged = { active in
                let title = L(active ? "Exit Slideshow" : "Play Slideshow")
                play.title = title
                play.action = active ? #selector(NativePreviewController.exitSlides) : #selector(NativePreviewController.presentSlides)
                play.image = NSImage(systemSymbolName: active ? "xmark" : "play.fill", accessibilityDescription: title)
                play.toolTip = title
                play.setAccessibilityLabel(title)
            }
            host.presentationCommand = { [weak self] command in self?.navigatePresentation(command) }
            presentationStartKeys = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
                guard let self = self, !self.closed, !self.host.isHidden,
                      event.window === self.host.window, !self.host.presentingSlides,
                      event.keyCode == 96, event.modifierFlags.intersection([.command, .control, .option]).isEmpty else { return event }
                DispatchQueue.main.async { [weak self] in self?.presentSlides() }
                return nil
            }
        }
        contentCopyControls().forEach { toolbar.addArrangedSubview($0) }
        toolbar.addArrangedSubview(externalOpenButton())
        if let html = spreadsheet {
            let configuration = WKWebViewConfiguration()
            configuration.defaultWebpagePreferences.allowsContentJavaScript = false
            let web = WKWebView(frame: .zero, configuration: configuration)
            spreadsheetView = web
            web.translatesAutoresizingMaskIntoConstraints = false
            host.addSubview(web)
            NSLayoutConstraint.activate([
                web.topAnchor.constraint(equalTo: host.topAnchor),
                web.leadingAnchor.constraint(equalTo: host.leadingAnchor),
                web.trailingAnchor.constraint(equalTo: host.trailingAnchor),
                web.bottomAnchor.constraint(equalTo: host.bottomAnchor)
            ])
            FloatingReaderToolbar(items: toolbar).install(in: host)
            web.loadHTMLString(html, baseURL: nil)
            return
        }
        // Only slides use the inspector-style presentation. In particular,
        // Numbers compact previews can render a small centered thumbnail card
        // even when the host fills the window; spreadsheets need normal layout.
        let slides = ["ppt", "pptx", "key"].contains(url.pathExtension.lowercased())
        let style: QLPreviewViewStyle = slides ? .compact : .normal
        guard let preview = QLPreviewView(frame: .zero, style: style) else {
            let alert = NSAlert(); alert.messageText = L("Document preview is unavailable"); alert.informativeText = L("Open this document in its default app to view it."); alert.runModal(); return
        }
        officeView = preview
        // The host supplies rounded clipping shared with GPUI; the remote
        // document itself fills that surface without an additional frame.
        host.wantsLayer = true
        host.layer?.masksToBounds = true
        preview.wantsLayer = true
        preview.layer?.cornerRadius = 0
        preview.layer?.masksToBounds = true
        preview.translatesAutoresizingMaskIntoConstraints = false
        preview.shouldCloseWithWindow = true
        preview.autostarts = true
        host.addSubview(preview)
        NSLayoutConstraint.activate([
            preview.topAnchor.constraint(equalTo: host.topAnchor),
            preview.leadingAnchor.constraint(equalTo: host.leadingAnchor),
            preview.trailingAnchor.constraint(equalTo: host.trailingAnchor),
            preview.bottomAnchor.constraint(equalTo: host.bottomAnchor)
        ])
        FloatingReaderToolbar(items: toolbar).install(in: host)
        // Quick Look creates its remote content when previewItem is assigned.
        // Resolve the toolbar and preview bounds first, rather than loading
        // against the initial full-window frame and resizing it afterwards.
        host.layoutSubtreeIfNeeded()
        preview.layoutSubtreeIfNeeded()
        preview.previewItem = url as NSURL
    }
    @objc private func presentPDF() { ReaderTelemetry.record(2, 13, 0);
        guard !closed, pdfSlideshow == nil, let document = pdfView.document else { return }
        let slideshow = PDFSlideshow(document: document, page: pdfView.currentPage) { [weak self] page in
            guard let self = self else { return }
            self.pdfSlideshow = nil
            if !self.closed, let page = page { self.pdfView.go(to: page) }
        }
        pdfSlideshow = slideshow
        if !slideshow.start(in: host) { slideshow.exit() }
    }
    // Remote Quick Look document providers expose navigation as accessibility
    // button actions even when their wrapper does not route keyboard events.
    // Identify semantic system controls, never document text or file names.
    private func pressPreviewNavigation(_ command: SlideshowCommand, in preview: QLPreviewView) -> Bool {
        let identifier: String
        switch command {
        case .previous: identifier = "Previous Page Button"
        case .next: identifier = "Next Page Button"
        default: return false
        }
        var pending: [Any] = preview.accessibilityChildren() ?? []
        var visited = Set<ObjectIdentifier>()
        while let element = pending.popLast() {
            guard let accessible = element as? NSAccessibilityProtocol else { continue }
            let identity = ObjectIdentifier(accessible as AnyObject)
            guard visited.insert(identity).inserted else { continue }
            if accessible.accessibilityRole() == .button && accessible.accessibilityIdentifier() == identifier {
                if !accessible.isAccessibilityEnabled() { return true }
                return accessible.accessibilityPerformPress()
            }
            pending.append(contentsOf: accessible.accessibilityChildren() ?? [])
        }
        return false
    }
    private func navigatePresentation(_ command: SlideshowCommand) {
        guard !closed, !host.isHidden, let preview = officeView, let window = preview.window else { return }
        if pressPreviewNavigation(command, in: preview) { return }
        let key: UInt16
        let character: String
        switch command {
        case .previous: key = 123; character = "\u{F702}"
        case .next: key = 124; character = "\u{F703}"
        case .first: key = 115; character = "\u{F729}"
        case .last: key = 119; character = "\u{F72B}"
        case .exit: host.stopPresentation(); return
        }
        // Normalize presenter/PageUp/Down/vertical arrows to the provider's
        // horizontal navigation keys. Direct window delivery avoids re-entering
        // the application's local event monitor.
        let focused = window.makeFirstResponder(preview)
        for type in [NSEvent.EventType.keyDown, .keyUp] {
            guard let event = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: .function, timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, characters: character, charactersIgnoringModifiers: character, isARepeat: false, keyCode: key) else { continue }
            if focused { window.sendEvent(event) }
            else if type == .keyDown { preview.keyDown(with: event) }
            else { preview.keyUp(with: event) }
        }
    }
    @objc private func presentationPrevious() { ReaderTelemetry.record(6, 6, 0); navigatePresentation(.previous) }
    @objc private func presentationNext() { ReaderTelemetry.record(6, 5, 0); navigatePresentation(.next) }
    @objc private func exitSlides() { ReaderTelemetry.record(6, 14, 0); host.stopPresentation() }
    @objc private func presentSlides() { ReaderTelemetry.record(6, 13, 0);
        guard !closed else { return }
        host.presentSlides()
    }
    func cancelContentCopy() { previewCopy?.cancel(); workbookPreview?.cancelCopy() }
    private func contentCopyControls() -> [NSView] {
        guard let source = urls.first else { return [] }
        let spreadsheet = ["xls", "xlsx", "xlsm", "numbers"].contains(source.pathExtension.lowercased())
        if let previewCopy = previewCopy { return previewCopy.controls(includeRecognition: !spreadsheet) }
        let copy = PreviewCopy(root: host, source: source)
        copy.textProvider = { [weak self] completion in
            guard let self = self, !self.closed else { completion(""); return }
            if let workbook = self.workbookPreview { workbook.copyText(completion) }
            else if let word = self.wordPreview { word.copyText(completion) }
            else if self.pdf {
                let selected = self.pdfView.currentSelection?.string ?? ""
                completion(selected.isEmpty ? (self.pdfView.currentPage?.string ?? "") : selected)
            }
            else { completion(PreviewCopy.text(in: self.host)) }
        }
        if source.pathExtension.lowercased() == "pdf" {
            copy.rasterProvider = { [weak self] completion in
                guard let self = self, let page = self.pdfView.currentPage else { completion(nil); return }
                let bounds = page.bounds(for: .cropBox)
                let edge = min(Double(ReaderLimits.IMAGE_PREVIEW_EDGE), floor(sqrt(Double(ReaderLimits.IMAGE_DECODED_BYTES) / 4)))
                let factor = edge / max(1, max(bounds.width, bounds.height))
                self.copyRasterQueue.async {
                    let image = page.thumbnail(of: NSSize(width: bounds.width * factor, height: bounds.height * factor), for: .cropBox)
                    let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil)
                    DispatchQueue.main.async { completion(cgImage) }
                }
            }
        } else if ["doc", "rtf"].contains(source.pathExtension.lowercased()) {
            copy.rasterProvider = { [weak self] completion in self?.wordPreview?.recognizeVisiblePage(completion) }
        }
        previewCopy = copy
        return copy.controls(includeRecognition: !spreadsheet)
    }
    private func isGlim(_ application: URL) -> Bool {
        Bundle(url: application)?.bundleIdentifier == "io.github.jony4.glimpse"
    }
    private func externalOpenButton() -> NSButton {
        return button(L("Open with Another App"), #selector(openOfficeExternally(_:)))
    }
    private func openDocument(_ document: URL, with application: URL) {
        guard !isGlim(application) else {
            let alert = NSAlert(); alert.messageText = L("Choose an application other than Glim.")
            alert.runModal(); return
        }
        NSWorkspace.shared.open([document], withApplicationAt: application, configuration: NSWorkspace.OpenConfiguration()) { _, error in
            guard let error = error else { return }
            DispatchQueue.main.async {
                let alert = NSAlert(); alert.messageText = L("Cannot open this file in the selected application.")
                alert.informativeText = error.localizedDescription; alert.runModal()
            }
        }
    }
    @objc func openOfficeExternally(_ sender: NSButton) { ReaderTelemetry.record(6, 11, 0);
        guard let document = urls.first else { return }
        // Resolve again on click: file associations may have changed while open.
        if let application = NSWorkspace.shared.urlForApplication(toOpen: document), !isGlim(application) {
            openDocument(document, with: application); return
        }
        let applications = (LSCopyApplicationURLsForURL(document as CFURL, .all)?.takeRetainedValue() as? [URL]) ?? []
        var seen = Set<String>()
        let available = applications.filter { application in
            guard !isGlim(application) else { return false }
            let key = Bundle(url: application)?.bundleIdentifier ?? application.standardizedFileURL.path
            return seen.insert(key).inserted
        }.sorted { FileManager.default.displayName(atPath: $0.path).localizedStandardCompare(FileManager.default.displayName(atPath: $1.path)) == .orderedAscending }
        let menu = NSMenu()
        for application in available {
            let item = NSMenuItem(title: FileManager.default.displayName(atPath: application.path), action: #selector(openInSelectedApplication(_:)), keyEquivalent: "")
            item.target = self; item.representedObject = application
            let icon = NSWorkspace.shared.icon(forFile: application.path); icon.size = NSSize(width: 16, height: 16); item.image = icon
            menu.addItem(item)
        }
        if !available.isEmpty { menu.addItem(.separator()) }
        let choose = NSMenuItem(title: L("Choose Application…"), action: #selector(chooseExternalApplication), keyEquivalent: "")
        choose.target = self; menu.addItem(choose)
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height), in: sender)
    }
    @objc private func openInSelectedApplication(_ sender: NSMenuItem) {
        guard let application = sender.representedObject as? URL, let document = urls.first else { return }
        openDocument(document, with: application)
    }
    @objc private func chooseExternalApplication() {
        guard let document = urls.first else { return }
        let panel = NSOpenPanel(); panel.title = L("Choose Application…")
        panel.message = L("Choose an application for this file. Its default application will not change.")
        panel.allowedContentTypes = [.applicationBundle]
        panel.directoryURL = URL(fileURLWithPath: "/Applications", isDirectory: true)
        panel.canChooseFiles = true; panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
        if panel.runModal() == .OK, let application = panel.url { openDocument(document, with: application) }
    }

    private func prepareVideo(_ next: Int, autoplay: Bool) {
        guard let helper = mediaHelper, let directory = mediaDirectory else { return }
        mediaPreparation?.cancel(); mediaGeneration += 1
        let generation = mediaGeneration
        let source = urls[next]
        let jobDirectory = directory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        let destination = jobDirectory.appendingPathComponent("preview.mp4")
        let obsolete = preparedMedia
        playbackFiles.removeAll(); preparedMedia = nil
        player.pause(); player.replaceCurrentItem(with: nil); pendingPlayback = false
        detailLabel.stringValue = L("Preparing video…")
        let preparation = MediaPreparation(); mediaPreparation = preparation
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            if let obsolete = obsolete { try? FileManager.default.removeItem(at: obsolete.deletingLastPathComponent()) }
            let result = Result {
                try FileManager.default.createDirectory(at: jobDirectory, withIntermediateDirectories: false)
                try preparation.run(helper: helper, source: source, destination: destination)
            }
            DispatchQueue.main.async { [weak self] in
                guard let self = self, !self.closed, self.mediaGeneration == generation else {
                    DispatchQueue.global().async { try? FileManager.default.removeItem(at: jobDirectory) }
                    return
                }
                self.mediaPreparation = nil
                switch result {
                case .success:
                    self.playbackFiles[source.path] = destination.path; self.preparedMedia = destination
                    self.play(next, autoplay: autoplay)
                case .failure(let error):
                    DispatchQueue.global().async { try? FileManager.default.removeItem(at: jobDirectory) }
                    self.mediaErrors[source.path] = error.localizedDescription
                    self.play(next, autoplay: autoplay)
                }
            }
        }
    }
    func play(_ next: Int, autoplay: Bool = true) {
        var next = next
        while urls.indices.contains(next), let error = mediaErrors[urls[next].path] {
            detailLabel.stringValue = error
            next += 1
        }
        guard urls.indices.contains(next) else {
            pendingPlayback = false
            player.pause()
            player.replaceCurrentItem(with: nil)
            return
        }
        token = nil; index = next
        if matroskaFiles.contains(urls[index].path) && playbackFiles[urls[index].path] == nil {
            prepareVideo(next, autoplay: autoplay); return
        }
        mediaPreparation?.cancel(); mediaPreparation = nil; mediaGeneration += 1
        let playbackURL = playbackFiles[urls[index].path].map { URL(fileURLWithPath: $0) } ?? urls[index]
        let item = AVPlayerItem(url: playbackURL)
        player.replaceCurrentItem(with: item)
        updateAudioArtwork(playbackURL, item: item)
        detailLabel.stringValue = "\(index + 1) / \(urls.count)"
        if urls.count > 1 {
            table.selectRowIndexes(IndexSet(integer: index), byExtendingSelection: false)
            table.scrollRowToVisible(index)
        }
        token = item.observe(\.status, options: [.initial, .new]) { [weak self] item, _ in
            DispatchQueue.main.async {
                guard let self = self, self.player.currentItem === item else { return }
                if item.status == .failed { self.skipFailed(item) }
            }
        }
        pendingPlayback = autoplay
        resumePendingPlayback()
    }
    func resumePendingPlayback() {
        if pendingPlayback && !host.isHidden {
            pendingPlayback = false
            player.play()
        }
    }
    func updateAudioArtwork(_ url: URL, item: AVPlayerItem) {
        metadataAsset?.cancelLoading()
        audioBackdrop.artwork = nil
        artworkSnapshot = nil; artworkButton.isHidden = true
        audioBackdrop.song = url.deletingPathExtension().lastPathComponent
        audioBackdrop.artist = ""
        audioBackdrop.album = url.pathExtension.uppercased() + " · Audio"
        audioBackdrop.isHidden = !["mp3", "m4a", "aac", "wav", "aif", "aiff", "caf", "flac"].contains(url.pathExtension.lowercased())
        let asset = AVURLAsset(url: url)
        metadataAsset = asset
        asset.loadValuesAsynchronously(forKeys: ["commonMetadata", "tracks"]) { [weak self, weak item] in
            var error: NSError?
            let loadedTracks = asset.statusOfValue(forKey: "tracks", error: &error) == .loaded
            let audioOnly = loadedTracks && asset.tracks(withMediaType: .video).isEmpty
            let metadata = asset.statusOfValue(forKey: "commonMetadata", error: &error) == .loaded ? asset.commonMetadata : []
            func value(_ key: AVMetadataKey) -> String? {
                AVMetadataItem.metadataItems(from: metadata, withKey: key, keySpace: .common).first?.stringValue
            }
            let song = value(.commonKeyTitle)
            let artist = value(.commonKeyArtist)
            let album = value(.commonKeyAlbumName)
            var image: CGImage?
            var artworkFile: ArtworkSnapshot?
            if let data = AVMetadataItem.metadataItems(from: metadata, withKey: AVMetadataKey.commonKeyArtwork, keySpace: .common).first?.dataValue,
                let source = CGImageSourceCreateWithData(data as CFData, nil) {
                artworkFile = try? ArtworkSnapshot(data: data, source: source)
                image = CGImageSourceCreateThumbnailAtIndex(source, 0, [kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceThumbnailMaxPixelSize: ReaderLimits.AUDIO_ARTWORK_PREVIEW_EDGE, kCGImageSourceCreateThumbnailWithTransform: true] as CFDictionary)
            }
            DispatchQueue.main.async {
                guard let self = self, let item = item, self.player.currentItem === item else { return }
                if loadedTracks { self.audioBackdrop.isHidden = !audioOnly }
                if let song = song, !song.isEmpty { self.audioBackdrop.song = song }
                self.audioBackdrop.artist = artist ?? ""
                if let album = album, !album.isEmpty { self.audioBackdrop.album = album }
                self.artworkSnapshot = artworkFile; self.artworkButton.isHidden = artworkFile == nil
                if let image = image { self.audioBackdrop.artwork = NSImage(cgImage: image, size: .zero) }
                self.audioBackdrop.setAccessibilityLabel([self.audioBackdrop.song, self.audioBackdrop.artist, self.audioBackdrop.album].filter { !$0.isEmpty }.joined(separator: ", "))
            }
        }
    }

    private func observePlayback(_ time: CMTime) {
        guard !closed, !host.isHidden, let generation = ReaderTelemetry.generation, let item = player.currentItem else { return }
        if telemetryGeneration != generation { telemetryGeneration = generation; telemetryMediaWall = 0; telemetryPositionAt = 0 }
        let seconds = time.seconds, now = ProcessInfo.processInfo.systemUptime
        guard seconds.isFinite, seconds >= 0 else { return }
        if telemetryMediaItem === item, telemetryMediaWall > 0 {
            let expected = telemetryMediaTime + (now - telemetryMediaWall) * Double(player.rate)
            if abs(seconds - expected) > 2 { ReaderTelemetry.record(5, 22, value: ReaderTelemetry.counter(seconds * 1000)) }
        }
        telemetryMediaItem = item; telemetryMediaTime = seconds; telemetryMediaWall = now
        if now - telemetryPositionAt >= 30 {
            telemetryPositionAt = now
            let duration = item.duration.seconds
            ReaderTelemetry.record(5, 34, value: ReaderTelemetry.counter(seconds * 1000), secondary: duration.isFinite && duration >= 0 ? ReaderTelemetry.counter(duration * 1000) : 0)
        }
    }
    @objc private func openArtwork() { artworkSnapshot?.open() }
    func skipFailed(_ item: AVPlayerItem) { ReaderTelemetry.record(5, 0, 1);
        guard player.currentItem === item, failed.insert(urls[index]).inserted else { return }
        detailLabel.stringValue = L("Cannot play this format or codec")
        if index + 1 < urls.count { play(index + 1) }
        else { player.pause() }
    }
    @objc func playbackFailed(_ note: Notification) { if let item = note.object as? AVPlayerItem { skipFailed(item) } }
    @objc func finished(_ note: Notification) { if let item = note.object as? AVPlayerItem, item === player.currentItem { next() } }
    @objc func next() { ReaderTelemetry.record(pdf ? 2 : 5, 5); if pdf { pdfView.goToNextPage(nil) } else { play(index + 1) } }
    @objc func previous() { ReaderTelemetry.record(pdf ? 2 : 5, 6); if pdf { pdfView.goToPreviousPage(nil) } else { play(max(0, index - 1)) } }
    @objc func togglePlay() { ReaderTelemetry.record(5, 10, 0); if player.rate == 0 { player.play() } else { player.pause() } }
    @objc func zoomIn() { pdfView.zoomIn(nil) }
    @objc func zoomOut() { pdfView.zoomOut(nil) }
    @objc func fit() { pdfView.fitToWidth() }
    @objc func pageChanged() { if let document = pdfView.document, let page = pdfView.currentPage { detailLabel.stringValue = "\(document.index(for: page) + 1) / \(document.pageCount)" } }
    func numberOfRows(in tableView: NSTableView) -> Int { urls.count }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let label = NSTextField(labelWithString: urls[row].lastPathComponent); label.lineBreakMode = .byTruncatingMiddle; label.toolTip = urls[row].lastPathComponent; return label
    }
    func tableViewSelectionDidChange(_ notification: Notification) { if table.selectedRow >= 0 && table.selectedRow != index { play(table.selectedRow) } }
    func close() {
        closed = true
        previewCopy?.cancel(); previewCopy = nil
        if let monitor = presentationStartKeys { NSEvent.removeMonitor(monitor); presentationStartKeys = nil }
        host.stopPresentation()
        pdfSlideshow?.exit(); pdfSlideshow = nil
        NotificationCenter.default.removeObserver(self)
        mediaPreparation?.cancel(); mediaPreparation = nil; mediaGeneration += 1
        imagePreview?.close()
        htmlPreview?.close()
        workbookPreview?.close()
        wordPreview?.close()
        officeView?.previewItem = nil
        officeView?.close()
        spreadsheetView?.stopLoading()
        if !host.isHidden {
            host.isHidden = true
            host.updateAccessibility()
        }
        host.removeFromSuperview()
        telemetryVolumeWork?.cancel(); telemetryVolumeWork = nil
        telemetryVolumeObserver = nil; telemetryMuteObserver = nil
        metadataAsset?.cancelLoading(); artworkSnapshot = nil; token = nil; playbackStateObserver = nil
        if let observer = playbackObserver { player.removeTimeObserver(observer); playbackObserver = nil }
        player.pause(); player.replaceCurrentItem(with: nil)
    }
}
