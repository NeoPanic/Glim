import Cocoa

// Reader-owned controls are the live state: no second copy of enabled/selected
// flags exists in the menu. IDs and array order stay stable when state changes.
struct ReaderControl {
    let id: String
    let view: NSView
    init(_ id: String, _ view: NSView) { self.id = id; self.view = view }
}

final class ReaderControls {
    let entries: [ReaderControl]
    init(_ entries: [ReaderControl]) {
        precondition(Set(entries.map { $0.id }).count == entries.count)
        self.entries = entries
    }
    convenience init(stack: NSStackView) {
        self.init(stack.arrangedSubviews.enumerated().map { ReaderControl("control-\($0.offset)", $0.element) })
    }
    func stack() -> NSStackView {
        let stack = NSStackView()
        entries.forEach { stack.addArrangedSubview($0.view) }
        return stack
    }
    func menu() -> NSMenu {
        let menu = ReaderMenu.make()
        for entry in entries where !entry.view.isHidden {
            if let popup = entry.view as? NSPopUpButton {
                let item = NSMenuItem(title: popup.selectedItem?.title ?? "", action: nil, keyEquivalent: "")
                let choices = ReaderMenu.make()
                for (index, choice) in popup.itemArray.enumerated() where !choice.isHidden {
                    if choice.isSeparatorItem { choices.addItem(.separator()); continue }
                    ReaderMenu.add(to: choices, title: choice.title, enabled: popup.isEnabled && choice.isEnabled, checked: index == popup.indexOfSelectedItem) { [weak popup] in
                        guard let popup = popup, popup.isEnabled else { return }
                        popup.selectItem(at: index); popup.sendAction(popup.action, to: popup.target)
                    }
                }
                ReaderMenu.own(item); item.submenu = choices; item.isEnabled = popup.isEnabled; menu.addItem(item)
            } else if let button = entry.view as? NSButton {
                ReaderMenu.add(to: menu, title: button.title.isEmpty ? (button.toolTip ?? "") : button.title, enabled: button.isEnabled, checked: button.state == .on) { [weak button] in
                    guard let button = button, button.isEnabled, !button.isHidden else { return }
                    // Preserve the original sender, including external-open popovers.
                    button.performClick(nil)
                }
            }
        }
        return menu
    }
}

private final class ReaderMenuCommand: NSObject {
    let run: () -> Void
    init(_ run: @escaping () -> Void) { self.run = run }
    @objc func invoke(_ sender: Any?) { ReaderTelemetry.withSource(3, run) }
}

// Shared policy also strips AppKit-injected Services/Writing Tools entries.
private final class ReaderMenuPolicy: NSObject, NSMenuDelegate {
    static let shared = ReaderMenuPolicy()
    func menuNeedsUpdate(_ menu: NSMenu) {
        for item in menu.items where !item.isSeparatorItem && item.identifier?.rawValue != "glim-reader-action" {
            menu.removeItem(item)
        }
        ReaderMenu.decorate(menu)
    }
}

enum ReaderMenu {
    static func make() -> NSMenu {
        let menu = NSMenu(); menu.autoenablesItems = false
        menu.allowsContextMenuPlugIns = false
        if #available(macOS 15.2, *) { menu.automaticallyInsertsWritingToolsItems = false }
        menu.delegate = ReaderMenuPolicy.shared
        return menu
    }
    static func add(to menu: NSMenu, title: String, enabled: Bool = true, checked: Bool = false, action: @escaping () -> Void) {
        let command = ReaderMenuCommand(action)
        let item = NSMenuItem(title: title, action: #selector(ReaderMenuCommand.invoke(_:)), keyEquivalent: "")
        own(item)
        item.target = command; item.representedObject = command
        item.isEnabled = enabled; item.state = checked ? .on : .off
        menu.addItem(item)
    }
    static func own(_ item: NSMenuItem) {
        item.identifier = NSUserInterfaceItemIdentifier("glim-reader-action")
        showImage(item)
        if item.image == nil, let symbol = symbol(for: item.title) {
            item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: item.title)
        }
    }
    private static func showImage(_ item: NSMenuItem) {
        // macOS 27 defaults to hiding menu images. Use the public property's
        // setter dynamically so builds with older SDKs remain supported.
        if item.responds(to: NSSelectorFromString("setPreferredImageVisibility:")) {
            item.setValue(1, forKey: "preferredImageVisibility")
        }
    }
    private static func symbol(for title: String) -> String? {
        let entries = [
            ("Copy", "doc.on.doc"), ("Copy Text", "doc.on.doc"), ("Copy Image", "photo.on.rectangle"),
            ("Copy Preview Text", "doc.on.doc"), ("Cut", "scissors"), ("Paste", "doc.on.clipboard"),
            ("Select All", "selection.pin.in.out"), ("Extract Text", "text.viewfinder"),
            ("Recognize Image Text", "text.viewfinder"), ("Fit width", "arrow.left.and.right"),
            ("Fit page", "arrow.up.left.and.arrow.down.right"),
            ("Single Page", "doc"), ("Single Page Continuous", "doc.text"),
            ("Two Pages", "book"), ("Two Pages Continuous", "book.pages"),
            ("Fit image", "arrow.up.left.and.arrow.down.right"), ("Actual size", "1.magnifyingglass"),
            ("Zoom in", "plus.magnifyingglass"), ("Zoom out", "minus.magnifyingglass"),
            ("Original layout", "doc.richtext"), ("Reading mode", "text.alignleft"),
            ("Reset zoom", "arrow.counterclockwise"), ("Previous", "chevron.left"), ("Next", "chevron.right"),
            ("Previous page", "chevron.left"), ("Next page", "chevron.right"),
            ("Play Slideshow", "play.rectangle"), ("Play / Pause", "playpause"),
            ("Open with Another App", "arrow.up.forward.app"), ("Exit Slideshow", "arrow.down.right.and.arrow.up.left")
        ]
        return entries.first(where: { L($0.0) == title })?.1
    }
    static func decorate(_ menu: NSMenu) {
        for item in menu.items where !item.isSeparatorItem {
            showImage(item)
            if let submenu = item.submenu { decorate(submenu) }
            if item.image == nil, let symbol = symbol(for: item.title) {
                item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: item.title)
            }
        }

    }
}

// Retained by each reader/toolbar; does not hide, replace or snapshot content.
final class ReaderContextMenu {
    private var monitor: Any?
    init(host: NSView, kind: Int32 = 0, provider: @escaping (NSEvent) -> NSMenu?) {
        let scroll = ReaderScrollUsage(kind: kind)
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.rightMouseDown, .scrollWheel]) { [weak host] event in
            guard let host = host, !host.isHiddenOrHasHiddenAncestor, event.window === host.window,
                  let parent = host.superview,
                  let hit = parent.hitTest(parent.superview?.convert(event.locationInWindow, from: nil) ?? event.locationInWindow),
                  hit === host || hit.isDescendant(of: host) else { return event }
            var ancestor: NSView? = hit
            while let view = ancestor {
                if view is FloatingReaderToolbar { return event }
                if view === host { break }
                ancestor = view.superview
            }
            if event.type == .scrollWheel { scroll.record(event); return event }
            ReaderTelemetry.record(kind, 31)
            guard let menu = provider(event) else { return event }
            if !menu.items.isEmpty { NSMenu.popUpContextMenu(menu, with: event, for: host) }
            return nil
        }
    }
    deinit { if let monitor = monitor { NSEvent.removeMonitor(monitor) } }
}

// Shortcuts share the live toolbar controls, including visibility/enabled state.
// Local monitors run before GPUI's key dispatch, but only for the focused reader.
final class ReaderKeyboardShortcuts {
    private var monitor: Any?
    init(host: NSView, controls: ReaderControls) {
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak host, weak controls] event in
            guard let host = host, let controls = controls,
                  !host.isHiddenOrHasHiddenAncestor, event.window === host.window,
                  let responder = event.window?.firstResponder as? NSView,
                  responder === host || responder.isDescendant(of: host) else { return event }
            let modifiers = event.modifierFlags.intersection([.command, .control, .option, .shift])
            let key = event.charactersIgnoringModifiers?.lowercased() ?? ""
            // Preserve the native selection, including WKContentView/PDFKit.
            if (modifiers == .command || modifiers == .control) && key == "c" {
                // Image/Word readers own their selection-aware copy handlers.
                if controls.entries.contains(where: { entry in
                    guard !entry.view.isHidden, let button = entry.view as? NSButton, let action = button.action else { return false }
                    return ["fitImage", "increaseZoom"].contains(NSStringFromSelector(action))
                }) { return event }
                let action = NSSelectorFromString("copy:")
                if NSApp.sendAction(action, to: nil, from: responder) { ReaderTelemetry.record(ReaderTelemetry.kind(controls), 107); return nil }
                return event
            }
            if modifiers == .command && key == "a" {
                if NSApp.sendAction(NSSelectorFromString("selectAll:"), to: nil, from: responder) { ReaderTelemetry.record(ReaderTelemetry.kind(controls), 117); return nil }
                return event
            }
            if let editor = responder as? NSTextView, editor.isEditable { return event }
            let actions: [String]
            switch (modifiers, key) {
            case (.command, "+"), (.command, "="), ([.command, .shift], "+"), ([.command, .shift], "="):
                actions = ["zoomIn", "increaseZoom"]
            case (.command, "-"), (.command, "−"):
                actions = ["zoomOut", "decreaseZoom"]
            case (.command, "0"):
                actions = ["resetZoom", "fitImage", "fit"]
            case (.command, "1"):
                actions = ["actualSize"]
            case ([.command, .shift], "c"):
                actions = ["copyText"]
            case ([.command, .shift], "e"):
                actions = ["openOfficeExternally:"]
            case ([.command, .shift], "i"):
                actions = ["extractText", "recognizeImages"]
            default:
                // Option + PageUp/PageDown leaves bare navigation to the document.
                if modifiers == .option && event.keyCode == 116 { actions = ["previousPage", "previous"] }
                else if modifiers == .option && event.keyCode == 121 { actions = ["nextPage", "next"] }
                else { return event }
            }
            for entry in controls.entries where !entry.view.isHidden {
                guard let button = entry.view as? NSButton, button.isEnabled,
                      let action = button.action, actions.contains(NSStringFromSelector(action)) else { continue }
                ReaderTelemetry.record(ReaderTelemetry.kind(controls), 100 + ReaderTelemetry.action(NSStringFromSelector(action)))
                button.performClick(nil); return nil
            }
            return event
        }
    }
    deinit { if let monitor = monitor { NSEvent.removeMonitor(monitor) } }
}

// Numeric-only ABI: document strings, paths and error messages cannot cross it.
typealias ReaderTelemetryCallback = @convention(c) (Int32, Int32, Int32, Int32, UInt64, UInt64, UInt64, UInt64) -> Void
private var readerTelemetryCallback: ReaderTelemetryCallback?
private let readerTelemetryLock = NSLock()
private var readerTelemetryEnabled = false
private var readerTelemetryGeneration: UInt64 = 0
@_cdecl("glim_set_telemetry_enabled")
func glimSetTelemetryEnabled(_ enabled: Int32) {
    readerTelemetryLock.lock()
    readerTelemetryEnabled = enabled != 0
    readerTelemetryGeneration &+= 1
    readerTelemetryLock.unlock()
}
@_cdecl("glim_set_reader_telemetry")
func glimSetReaderTelemetry(_ callback: ReaderTelemetryCallback?) {
    readerTelemetryLock.lock(); readerTelemetryCallback = callback; readerTelemetryLock.unlock()
}
enum ReaderTelemetry {
    // Observability must never trap on malformed native floating-point metadata.
    static func counter(_ value: Double) -> UInt64 {
        guard value.isFinite, value > 0 else { return 0 }
        return UInt64(min(value, Double(UInt64.max / 2)))
    }
    static var generation: UInt64? {
        readerTelemetryLock.lock(); defer { readerTelemetryLock.unlock() }
        return readerTelemetryEnabled ? readerTelemetryGeneration : nil
    }
    static func withSource(_ source: Int32, _ body: () -> Void) {
        let key = "glim.reader-telemetry-source"
        let previous = Thread.current.threadDictionary[key]
        Thread.current.threadDictionary[key] = source
        defer { Thread.current.threadDictionary[key] = previous }
        body()
    }
    static func record(_ kind: Int32, _ action: Int32, _ error: Int32 = 0,
                       value: UInt64 = 0, secondary: UInt64 = 0, count: UInt64 = 0, duration: UInt64 = 0) {
        guard generation != nil else { return }
        let source: Int32
        if [Int32(0), 22, 23, 24, 25, 26, 33, 34, 35, 36].contains(action) { source = 0 }
        else if action == 30 || action == 32 { source = 1 }
        else if let override = Thread.current.threadDictionary["glim.reader-telemetry-source"] as? Int32 { source = override }
        else if Thread.isMainThread, let event = NSApp.currentEvent, ProcessInfo.processInfo.systemUptime - event.timestamp < 0.5 {
            source = event.type == .keyDown ? 2 : [.leftMouseDown, .leftMouseUp, .rightMouseDown, .rightMouseUp, .scrollWheel].contains(event.type) ? 1 : 0
        } else { source = 0 }
        readerTelemetryLock.lock(); let callback = readerTelemetryCallback; readerTelemetryLock.unlock()
        if action >= 100, Thread.isMainThread, let event = NSApp.currentEvent, event.type == .keyDown {
            let modifiers = event.modifierFlags.intersection([.command, .control, .option, .shift])
            callback?(kind, action, error, source, UInt64(modifiers.rawValue), UInt64(event.keyCode), count, duration)
        } else { callback?(kind, action, error, source, value, secondary, count, duration) }
    }
    static func fileKind(_ source: URL) -> Int32 {
        switch source.pathExtension.lowercased() {
        case "doc", "docx", "rtf", "odt", "pages": return 3
        case "xls", "xlsx", "xlsm", "ods", "numbers": return 4
        case "ppt", "pptx", "odp", "key": return 6
        case "pdf": return 2
        case "html", "htm", "svg": return 7
        default: return 0
        }
    }
    static func action(_ selector: String) -> Int32 {
        switch selector {
        case "zoomIn", "increaseZoom": return 1
        case "zoomOut", "decreaseZoom": return 2
        case "resetZoom", "actualSize": return 3
        case "fitImage", "fit": return 4
        case "nextPage", "next": return 5
        case "previousPage", "previous": return 6
        case "copyText", "copyImage": return 7
        case "extractText", "recognizeImages": return 8
        case "openOfficeExternally:": return 11
        default: return 0
        }
    }
    static func kind(_ controls: ReaderControls) -> Int32 {
        let selectors = controls.entries.compactMap { ($0.view as? NSControl)?.action }.map(NSStringFromSelector)
        if selectors.contains("selectSheet") { return 4 }
        if selectors.contains("toggleMode") || selectors.contains("increaseZoom") { return 3 }
        if selectors.contains("fitImage") { return 1 }
        if selectors.contains("changeFace") { return 8 }
        if selectors.contains("togglePlay") { return 5 }
        if selectors.contains("openOfficeExternally:") { return 6 }
        if selectors.contains("fit") || selectors.contains("fitToWidth") { return 2 }
        return 0
    }
}

// Existing context router observes input without consuming native scrolling.
// One timer per burst; no file work, per-frame event, or additional event monitor.
private final class ReaderScrollUsage {
    private static weak var lastEvent: NSEvent?
    let kind: Int32
    var started = ProcessInfo.processInfo.systemUptime
    var last = ProcessInfo.processInfo.systemUptime
    var x = 0.0, y = 0.0
    var count: UInt64 = 0
    var precise = true
    var generation: UInt64?
    var timer: DispatchWorkItem?
    init(kind: Int32) { self.kind = kind }
    func record(_ event: NSEvent) {
        if Self.lastEvent === event { return }
        Self.lastEvent = event
        guard let enabledGeneration = ReaderTelemetry.generation else { count = 0; x = 0; y = 0; return }
        if generation != enabledGeneration { count = 0; x = 0; y = 0; generation = enabledGeneration }
        let now = ProcessInfo.processInfo.systemUptime
        if count > 0 && precise != event.hasPreciseScrollingDeltas { flush() }
        if count == 0 { started = now; precise = event.hasPreciseScrollingDeltas }
        last = now; count += 1
        x += abs(event.scrollingDeltaX); y += abs(event.scrollingDeltaY)
        if timer == nil { schedule() }
    }
    private func schedule() {
        let work = DispatchWorkItem { [weak self] in
            guard let self = self else { return }
            self.timer = nil
            let now = ProcessInfo.processInfo.systemUptime
            if now - self.last >= 0.7 || now - self.started >= 10 { self.flush() }
            else { self.schedule() }
        }
        timer = work; DispatchQueue.main.asyncAfter(deadline: .now() + 0.8, execute: work)
    }
    private func flush() {
        guard count > 0, generation == ReaderTelemetry.generation else { count = 0; x = 0; y = 0; return }
        ReaderTelemetry.record(kind, precise ? 30 : 32, value: ReaderTelemetry.counter((x * 100).rounded()),
                               secondary: ReaderTelemetry.counter((y * 100).rounded()),
                               count: count, duration: ReaderTelemetry.counter((last - started) * 1000))
        count = 0; x = 0; y = 0
    }
    deinit { timer?.cancel(); flush() }
}
