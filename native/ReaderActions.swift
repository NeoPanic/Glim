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
        item.identifier = NSUserInterfaceItemIdentifier("glim-reader-action")
        item.target = command; item.representedObject = command
        item.isEnabled = enabled; item.state = checked ? .on : .off
        menu.addItem(item)
    }
    static func own(_ item: NSMenuItem) { item.identifier = NSUserInterfaceItemIdentifier("glim-reader-action") }
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
