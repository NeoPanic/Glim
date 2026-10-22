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
    @objc func invoke(_ sender: Any?) { run() }
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
        if item.image == nil, let symbol = symbol(for: item.title) {
            item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: item.title)
        }
    }
    private static func symbol(for title: String) -> String? {
        let entries = [
            ("Copy", "doc.on.doc"), ("Copy Text", "doc.on.doc"), ("Copy Image", "photo.on.rectangle"),
            ("Copy Preview Text", "doc.on.doc"), ("Cut", "scissors"), ("Paste", "doc.on.clipboard"),
            ("Select All", "selection.pin.in.out"), ("Extract Text", "text.viewfinder"),
            ("Recognize Image Text", "text.viewfinder"), ("Fit width", "arrow.left.and.right"),
            ("Fit image", "arrow.up.left.and.arrow.down.right"), ("Actual size", "1.magnifyingglass"),
            ("Zoom in", "plus.magnifyingglass"), ("Zoom out", "minus.magnifyingglass"),
            ("Reset zoom", "arrow.counterclockwise"), ("Previous", "chevron.left"), ("Next", "chevron.right"),
            ("Previous page", "chevron.left"), ("Next page", "chevron.right"),
            ("Play Slideshow", "play.rectangle"), ("Play / Pause", "playpause"),
            ("Open with Another App", "arrow.up.forward.app"), ("Exit Slideshow", "arrow.down.right.and.arrow.up.left")
        ]
        return entries.first(where: { L($0.0) == title })?.1
    }
    static func decorate(_ menu: NSMenu) {
        guard !menu.items.contains(where: { $0.view is ReaderMenuIconRow }) else { return }
        for item in menu.items where !item.isSeparatorItem {
            if item.image == nil, let symbol = symbol(for: item.title) {
                item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: item.title)
            }
        }
        // Retain the full textual menu. Destructive actions stay in named rows.
        let quick = menu.items.filter { item in
            guard item.submenu == nil, item.image != nil, !item.isHidden, !item.isSeparatorItem else { return false }
            if let symbol = item.representedObject as? String,
               ["trash", "folder.badge.minus"].contains(symbol) { return false }
            return true
        }.prefix(6)
        guard quick.count >= 2 else { return }
        let row = NSMenuItem(); row.identifier = NSUserInterfaceItemIdentifier("glim-reader-action")
        row.view = ReaderMenuIconRow(items: Array(quick)); row.isEnabled = true
        menu.insertItem(.separator(), at: 0); menu.insertItem(row, at: 0)
    }
}

// Retained by each reader/toolbar; does not hide, replace or snapshot content.
final class ReaderContextMenu {
    private var monitor: Any?
    init(host: NSView, provider: @escaping (NSEvent) -> NSMenu?) {
        monitor = NSEvent.addLocalMonitorForEvents(matching: .rightMouseDown) { [weak host] event in
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
                if NSApp.sendAction(action, to: nil, from: responder) { return nil }
                return event
            }
            if modifiers == .command && key == "a" {
                if NSApp.sendAction(NSSelectorFromString("selectAll:"), to: nil, from: responder) { return nil }
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
                button.performClick(nil); return nil
            }
            return event
        }
    }
    deinit { if let monitor = monitor { NSEvent.removeMonitor(monitor) } }
}


private final class ReaderMenuIconAction: NSObject {
    weak var item: NSMenuItem?
    init(_ item: NSMenuItem) { self.item = item }
    @objc func invoke(_ sender: NSButton) {
        guard let item = item, item.isEnabled, !item.isHidden, let action = item.action else { return }
        item.menu?.cancelTracking()
        NSApp.sendAction(action, to: item.target, from: item)
    }
}
private final class ReaderMenuIconRow: NSView {
    private var actions: [ReaderMenuIconAction] = []
    init(items: [NSMenuItem]) {
        super.init(frame: NSRect(x: 0, y: 0, width: 16 + items.count * 34, height: 38))
        for (index, item) in items.enumerated() {
            let command = ReaderMenuIconAction(item); actions.append(command)
            let button = NSButton(image: item.image!, target: command, action: #selector(ReaderMenuIconAction.invoke(_:)))
            button.frame = NSRect(x: 8 + index * 34, y: 4, width: 30, height: 30)
            button.bezelStyle = .rounded; button.isBordered = true; button.imagePosition = .imageOnly
            button.isEnabled = item.isEnabled; button.toolTip = item.title; button.setAccessibilityLabel(item.title)
            addSubview(button)
        }
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is unsupported") }
}
