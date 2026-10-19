import Cocoa
import WebKit
import AVKit
import PDFKit
import Quartz
import ImageIO
import UniformTypeIdentifiers
import CoreServices
import CoreText

struct Playlist: Decodable { let files: [String]; let shuffle: Bool; let autoplay: Bool?; let playbackFiles: [String: String]?; let matroskaFiles: [String]?; let mediaHelper: String?; let mediaDirectory: String?; let mediaErrors: [String: String]?; let office: Bool?; let image: Bool?; let imagePath: String?; let imageWidth: Double?; let imageHeight: Double?; let associations: String?; let font: Bool?; let sqlite: String?; let sqliteInfo: String?; let spreadsheet: String?; let htmlContent: String?; let language: String?; let localizations: [String: String]? }

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
        ("Word", ["doc", "docx", "rtf"]), ("Excel", ["xls", "xlsx"]), ("PowerPoint", ["ppt", "pptx"])
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
    init(url: URL) {
        super.init()
        let stack = NSStackView(); stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 12; stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 20), stack.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -20), stack.topAnchor.constraint(equalTo: view.topAnchor, constant: 16), stack.bottomAnchor.constraint(equalTo: view.bottomAnchor, constant: -16)])
        let title = NSTextField(labelWithString: url.lastPathComponent); title.font = .systemFont(ofSize: 17, weight: .semibold); stack.addArrangedSubview(title)
        selector.target = self; selector.action = #selector(changeFace); selector.isEnabled = false; stack.addArrangedSubview(selector)
        input.delegate = self; input.placeholderString = L("Type sample text…"); stack.addArrangedSubview(input)
        input.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
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
    @objc func changeFace() {
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

final class NativePreviewHost: NSView {
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
        let log = destination.appendingPathExtension("log")
        FileManager.default.createFile(atPath: log.path, contents: nil)
        let stderr = try FileHandle(forWritingTo: log)
        defer { try? stderr.close() }
        lock.lock()
        if cancelled { lock.unlock(); throw CocoaError(.userCancelled) }
        process.executableURL = helper
        process.arguments = ["--prepare-matroska", source.path, destination.path]
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
    let playerView = AVPlayerView()
    let pdfView = PDFView()
    let audioBackdrop = AudioBackdrop()
    private var artworkSnapshot: ArtworkSnapshot?
    private let artworkButton = NSButton(title: L("Open full artwork"), target: nil, action: nil)
    var metadataAsset: AVURLAsset?
    let titleLabel = NSTextField(labelWithString: "")
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
    var officeView: QLPreviewView?
    var spreadsheetView: WKWebView?
    var fontPreview: FontPreview?
    var imagePreview: ImagePreview?
    var htmlPreview: HTMLPreview?
    var htmlAnchor: String?
    var sqlitePreview: SQLitePreview?
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
        if let html = playlist.htmlContent {
            let preview = HTMLPreview(document: urls[0], contents: html); htmlPreview = preview
            preview.view.frame = host.bounds; preview.view.autoresizingMask = [.width, .height]
            host.addSubview(preview.view)
            if let anchor = htmlAnchor { preview.reveal(anchor) }
            return
        }
        if let info = playlist.sqliteInfo {
            let preview = SQLitePreview(path: playlist.sqlite, information: info); sqlitePreview = preview
            preview.view.frame = host.bounds
            preview.view.autoresizingMask = [.width, .height]
            host.addSubview(preview.view)
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
            showOffice(urls[0], spreadsheet: playlist.spreadsheet)
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
        titleLabel.lineBreakMode = .byTruncatingMiddle
        titleLabel.font = NSFont.systemFont(ofSize: 13, weight: .semibold)
        detailLabel.textColor = .secondaryLabelColor
        let toolbar = NSStackView()
        toolbar.edgeInsets = NSEdgeInsets(top: 10, left: 12, bottom: 10, right: 12)
        toolbar.addArrangedSubview(button(L("Previous"), #selector(previous)))
        toolbar.addArrangedSubview(button(L("Next"), #selector(next)))
        if pdf {
            toolbar.addArrangedSubview(button("−", #selector(zoomOut)))
            toolbar.addArrangedSubview(button("+", #selector(zoomIn)))
            toolbar.addArrangedSubview(button(L("Fit"), #selector(fit)))
        } else {
            toolbar.addArrangedSubview(button(L("Play / Pause"), #selector(togglePlay)))
            artworkButton.target = self; artworkButton.action = #selector(openArtwork)
            artworkButton.isHidden = true; toolbar.addArrangedSubview(artworkButton)
        }
        toolbar.addArrangedSubview(titleLabel)
        toolbar.addArrangedSubview(detailLabel)
        content.addArrangedSubview(toolbar)
        toolbar.widthAnchor.constraint(equalTo: content.widthAnchor).isActive = true
        if pdf {
            guard let document = PDFDocument(url: urls[0]) else { throw NSError(domain: "Glim", code: 2, userInfo: [NSLocalizedDescriptionKey: "This PDF could not be opened."]) }
            if document.isLocked {
                let alert = NSAlert(); alert.messageText = L("PDF password"); alert.addButton(withTitle: L("Open")); alert.addButton(withTitle: L("Cancel"))
                let password = NSSecureTextField(frame: NSRect(x: 0, y: 0, width: 260, height: 24)); alert.accessoryView = password
                guard alert.runModal() == .alertFirstButtonReturn && document.unlock(withPassword: password.stringValue) else { throw NSError(domain: "Glim", code: 3, userInfo: [NSLocalizedDescriptionKey: "PDF was not unlocked."]) }
            }
            pdfView.document = document; pdfView.autoScales = true; pdfView.displayMode = .singlePageContinuous
            content.addArrangedSubview(pdfView)
            pdfView.widthAnchor.constraint(equalTo: content.widthAnchor).isActive = true
            NotificationCenter.default.addObserver(self, selector: #selector(pageChanged), name: .PDFViewPageChanged, object: pdfView)
            titleLabel.stringValue = urls[0].lastPathComponent; pageChanged()
        } else {
            playerView.player = player
            playbackObserver = player.addPeriodicTimeObserver(forInterval: CMTime(seconds: 1.0 / 24.0, preferredTimescale: 600), queue: .main) { [weak self] time in
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
                    self?.audioBackdrop.playing = player.timeControlStatus == .playing
                    self?.audioBackdrop.needsDisplay = true
                }
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
    }
    func showOffice(_ url: URL, spreadsheet: String? = nil) {
        let toolbar = NSStackView()
        toolbar.translatesAutoresizingMaskIntoConstraints = false
        toolbar.spacing = 12
        titleLabel.stringValue = url.lastPathComponent
        titleLabel.lineBreakMode = .byTruncatingMiddle
        titleLabel.font = NSFont.systemFont(ofSize: 13, weight: .semibold)
        toolbar.addArrangedSubview(titleLabel)
        toolbar.addArrangedSubview(button(L("Open in Default App"), #selector(openOfficeExternally)))
        host.addSubview(toolbar)
        NSLayoutConstraint.activate([
            toolbar.topAnchor.constraint(equalTo: host.topAnchor, constant: 10),
            toolbar.leadingAnchor.constraint(equalTo: host.leadingAnchor, constant: 12),
            toolbar.trailingAnchor.constraint(equalTo: host.trailingAnchor, constant: -12)
        ])
        if let html = spreadsheet {
            let configuration = WKWebViewConfiguration()
            configuration.defaultWebpagePreferences.allowsContentJavaScript = false
            let web = WKWebView(frame: .zero, configuration: configuration)
            spreadsheetView = web
            web.translatesAutoresizingMaskIntoConstraints = false
            host.addSubview(web)
            NSLayoutConstraint.activate([
                web.topAnchor.constraint(equalTo: toolbar.bottomAnchor, constant: 10),
                web.leadingAnchor.constraint(equalTo: host.leadingAnchor),
                web.trailingAnchor.constraint(equalTo: host.trailingAnchor),
                web.bottomAnchor.constraint(equalTo: host.bottomAnchor)
            ])
            web.loadHTMLString(html, baseURL: nil)
            return
        }
        // Use the undecorated embedded presentation rather than Quick Look's
        // full preview frame, which adds a page-edge rule beside Office slides.
        // Auto Layout supplies the actual content bounds below the toolbar.
        guard let preview = QLPreviewView(frame: .zero, style: .compact) else {
            let alert = NSAlert(); alert.messageText = L("Document preview is unavailable"); alert.informativeText = L("Open this document in its default app to view it."); alert.runModal(); return
        }
        officeView = preview
        preview.translatesAutoresizingMaskIntoConstraints = false
        preview.shouldCloseWithWindow = true
        preview.autostarts = true
        host.addSubview(preview)
        NSLayoutConstraint.activate([
            preview.topAnchor.constraint(equalTo: toolbar.bottomAnchor, constant: 10),
            preview.leadingAnchor.constraint(equalTo: host.leadingAnchor),
            preview.trailingAnchor.constraint(equalTo: host.trailingAnchor),
            preview.bottomAnchor.constraint(equalTo: host.bottomAnchor)
        ])
        // Quick Look creates its remote content when previewItem is assigned.
        // Resolve the toolbar and preview bounds first, rather than loading
        // against the initial full-window frame and resizing it afterwards.
        host.layoutSubtreeIfNeeded()
        preview.layoutSubtreeIfNeeded()
        preview.previewItem = url as NSURL
    }
    @objc func openOfficeExternally() { if let url = urls.first { NSWorkspace.shared.open(url) } }

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
        titleLabel.stringValue = source.lastPathComponent; detailLabel.stringValue = L("Preparing video…")
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
            titleLabel.stringValue = urls[next].lastPathComponent
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
        titleLabel.stringValue = urls[index].lastPathComponent
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

    @objc private func openArtwork() { artworkSnapshot?.open() }
    func skipFailed(_ item: AVPlayerItem) {
        guard player.currentItem === item, failed.insert(urls[index]).inserted else { return }
        detailLabel.stringValue = L("Cannot play this format or codec")
        if index + 1 < urls.count { play(index + 1) }
        else { player.pause() }
    }
    @objc func playbackFailed(_ note: Notification) { if let item = note.object as? AVPlayerItem { skipFailed(item) } }
    @objc func finished(_ note: Notification) { if let item = note.object as? AVPlayerItem, item === player.currentItem { next() } }
    @objc func next() { if pdf { pdfView.goToNextPage(nil) } else { play(index + 1) } }
    @objc func previous() { if pdf { pdfView.goToPreviousPage(nil) } else { play(max(0, index - 1)) } }
    @objc func togglePlay() { if player.rate == 0 { player.play() } else { player.pause() } }
    @objc func zoomIn() { pdfView.zoomIn(nil) }
    @objc func zoomOut() { pdfView.zoomOut(nil) }
    @objc func fit() { pdfView.autoScales = true }
    @objc func pageChanged() { if let document = pdfView.document, let page = pdfView.currentPage { detailLabel.stringValue = "\(document.index(for: page) + 1) / \(document.pageCount)" } }
    func numberOfRows(in tableView: NSTableView) -> Int { urls.count }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let label = NSTextField(labelWithString: urls[row].lastPathComponent); label.lineBreakMode = .byTruncatingMiddle; label.toolTip = urls[row].lastPathComponent; return label
    }
    func tableViewSelectionDidChange(_ notification: Notification) { if table.selectedRow >= 0 && table.selectedRow != index { play(table.selectedRow) } }
    func close() {
        closed = true
        NotificationCenter.default.removeObserver(self)
        mediaPreparation?.cancel(); mediaPreparation = nil; mediaGeneration += 1
        imagePreview?.close()
        htmlPreview?.close()
        sqlitePreview?.close()
        officeView?.previewItem = nil
        officeView?.close()
        spreadsheetView?.stopLoading()
        if !host.isHidden {
            host.isHidden = true
            host.updateAccessibility()
        }
        host.removeFromSuperview()
        metadataAsset?.cancelLoading(); artworkSnapshot = nil; token = nil; playbackStateObserver = nil
        if let observer = playbackObserver { player.removeTimeObserver(observer); playbackObserver = nil }
        player.pause(); player.replaceCurrentItem(with: nil)
    }
}
