import Cocoa

// Opaque handles are retained by one Rust reader, and used only on the UI thread.
@_cdecl("glim_preview_create")
func glimPreviewCreate(_ bytes: UnsafePointer<UInt8>, _ length: Int) -> UnsafeMutableRawPointer {
    precondition(Thread.isMainThread)
    let preview = NativePreviewController()
    preview.host.isHidden = true
    let data = Data(bytes: bytes, count: length)
    // AppKit/PDFKit/Quick Look loading occurs outside GPUI render/layout callbacks.
    DispatchQueue.main.async { [weak preview] in
        guard let preview = preview, !preview.closed else { return }
        do {
            try preview.load(JSONDecoder().decode(Playlist.self, from: data))
        } catch {
            preview.host.subviews.forEach { $0.removeFromSuperview() }
            let label = NSTextField(wrappingLabelWithString: error.localizedDescription)
            label.frame = preview.host.bounds.insetBy(dx: 24, dy: 24)
            label.autoresizingMask = [.width, .height]
            preview.host.addSubview(label)
        }
    }
    return Unmanaged.passRetained(preview).toOpaque()
}

@_cdecl("glim_preview_layout")
func glimPreviewLayout(_ handle: UnsafeMutableRawPointer, _ parentPointer: UnsafeMutableRawPointer,
                       _ x: Double, _ y: Double, _ width: Double, _ height: Double) {
    precondition(Thread.isMainThread)
    let preview = Unmanaged<NativePreviewController>.fromOpaque(handle).takeUnretainedValue()
    let gpuiView = Unmanaged<NSView>.fromOpaque(parentPointer).takeUnretainedValue()
    // Share the content view with GPUI. Register native accessibility children
    // at the window level because AccessKit owns the content view's AX tree.
    let parent = gpuiView.superview ?? gpuiView
    if preview.host.presentingSlides {
        if width > 0 && height > 0 && preview.host.superview === parent {
            preview.host.frame = parent.bounds
            return
        }
        preview.host.stopPresentation()
    }
    if preview.host.superview !== parent {
        preview.host.removeFromSuperview()
        parent.addSubview(preview.host)
        preview.host.gpuiView = gpuiView
        preview.host.nextResponder = gpuiView
    }
    // GPUI uses top-left logical pixels; AppKit views may use bottom-left points.
    let rect = NSRect(x: gpuiView.bounds.minX + x,
        y: gpuiView.isFlipped ? gpuiView.bounds.minY + y : gpuiView.bounds.maxY - y - height,
        width: max(0, width), height: max(0, height))
    preview.host.frame = parent.convert(rect, from: gpuiView)
    // Native readers are siblings of GPUI's Metal view, so they need matching
    // clipping rather than relying on the GPUI container's rounded overflow.
    preview.host.wantsLayer = true
    preview.host.layer?.cornerRadius = preview.host.frame == parent.bounds ? 0 : 10
    // Tabs belong to the outer rounded workspace frame. The reader begins
    // below them: only its bottom corners meet that frame's rounded boundary.
    preview.host.layer?.maskedCorners = [.layerMinXMinYCorner, .layerMaxXMinYCorner]
    preview.host.layer?.masksToBounds = true
    let hidden = width <= 0 || height <= 0
    if preview.host.isHidden != hidden {
        preview.host.isHidden = hidden
        preview.host.updateAccessibility()
    }
    preview.imagePreview?.resume()
    preview.resumePendingPlayback()
}

@_cdecl("glim_preview_hide")
func glimPreviewHide(_ handle: UnsafeMutableRawPointer) {
    precondition(Thread.isMainThread)
    let preview = Unmanaged<NativePreviewController>.fromOpaque(handle).takeUnretainedValue()
    guard !preview.host.isHidden else { return }
    preview.host.stopPresentation()
    preview.pdfSlideshow?.exit()
    preview.cancelContentCopy()
    if let responder = preview.host.window?.firstResponder as? NSView,
       responder.isDescendant(of: preview.host) {
        preview.host.window?.makeFirstResponder(preview.host.gpuiView)
    }
    preview.host.isHidden = true
    preview.host.updateAccessibility()
    preview.pendingPlayback = false
    preview.player.pause()
    preview.imagePreview?.pause()
}

@_cdecl("glim_preview_release")
func glimPreviewRelease(_ handle: UnsafeMutableRawPointer) {
    precondition(Thread.isMainThread)
    let preview = Unmanaged<NativePreviewController>.fromOpaque(handle).takeRetainedValue()
    preview.close()
}

@_cdecl("glim_preview_take_navigation")
func glimPreviewTakeNavigation(_ handle: UnsafeMutableRawPointer) -> UnsafeMutablePointer<CChar>? {
    precondition(Thread.isMainThread)
    let preview = Unmanaged<NativePreviewController>.fromOpaque(handle).takeUnretainedValue()
    guard let html = preview.htmlPreview, let url = html.pendingNavigation else { return nil }
    html.pendingNavigation = nil
    return url.withCString { strdup($0) }
}
@_cdecl("glim_preview_free_string")
func glimPreviewFreeString(_ string: UnsafeMutablePointer<CChar>) { free(string) }
@_cdecl("glim_preview_reveal_anchor")
func glimPreviewRevealAnchor(_ handle: UnsafeMutableRawPointer, _ bytes: UnsafePointer<UInt8>, _ length: Int) {
    precondition(Thread.isMainThread)
    let preview = Unmanaged<NativePreviewController>.fromOpaque(handle).takeUnretainedValue()
    if let anchor = String(data: Data(bytes: bytes, count: length), encoding: .utf8) {
        preview.htmlAnchor = anchor; preview.htmlPreview?.reveal(anchor)
    }
}

// AppKit context menus live above both GPUI and native document surfaces.
// They never hide, pause or reconstruct a document reader.
private struct FileMenuItem: Decodable { let title: String?; let enabled: Bool; let symbol: String? }
private final class FileContextMenu: NSObject {
    let menu = NSMenu()
    var selection: Int64 = -2
    init(items: [FileMenuItem]) {
        super.init()
        menu.autoenablesItems = false
        for (index, specification) in items.enumerated() {
            if let title = specification.title {
                let item = NSMenuItem(title: title, action: #selector(choose(_:)), keyEquivalent: "")
                item.target = self; item.tag = index; item.isEnabled = specification.enabled
                if let symbol = specification.symbol {
                    item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: title)
                    item.representedObject = symbol
                }
                menu.addItem(item)
            } else {
                menu.addItem(.separator())
            }
        }
        ReaderMenu.decorate(menu)
    }
    @objc private func choose(_ item: NSMenuItem) { selection = Int64(item.tag) }
}
@_cdecl("glim_context_menu_create")
func glimContextMenuCreate(_ parent: UnsafeMutableRawPointer, _ x: Double, _ y: Double,
                          _ bytes: UnsafePointer<UInt8>, _ length: Int) -> UnsafeMutableRawPointer? {
    precondition(Thread.isMainThread)
    guard let items = try? JSONDecoder().decode([FileMenuItem].self, from: Data(bytes: bytes, count: length)) else { return nil }
    let controller = FileContextMenu(items: items)
    let view = Unmanaged<NSView>.fromOpaque(parent).takeUnretainedValue()
    DispatchQueue.main.async { [weak controller, weak view] in
        guard let controller = controller else { return }
        guard let view = view, view.window != nil else { controller.selection = -1; return }
        let point = NSPoint(x: view.bounds.minX + x, y: view.isFlipped ? view.bounds.minY + y : view.bounds.maxY - y)
        controller.menu.popUp(positioning: nil, at: point, in: view)
        if controller.selection == -2 { controller.selection = -1 }
    }
    return Unmanaged.passRetained(controller).toOpaque()
}
@_cdecl("glim_context_menu_selection")
func glimContextMenuSelection(_ handle: UnsafeMutableRawPointer) -> Int64 {
    precondition(Thread.isMainThread)
    return Unmanaged<FileContextMenu>.fromOpaque(handle).takeUnretainedValue().selection
}
@_cdecl("glim_context_menu_release")
func glimContextMenuRelease(_ handle: UnsafeMutableRawPointer) {
    precondition(Thread.isMainThread)
    let controller = Unmanaged<FileContextMenu>.fromOpaque(handle).takeRetainedValue()
    if controller.selection == -2 { controller.menu.cancelTrackingWithoutAnimation() }
}

private final class PathHintPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}
private final class FilePathHint {
    private weak var parent: NSWindow?
    private let anchor: NSRect
    private let panel: PathHintPanel
    private var task: DispatchWorkItem?
    private var monitor: Any?
    private var observers: [NSObjectProtocol] = []
    private var closed = false
    init(parent: NSWindow, anchor: NSRect, title: String, path: String, directory: Bool) {
        self.parent = parent; self.anchor = anchor
        panel = PathHintPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.isReleasedWhenClosed = false; panel.isOpaque = false; panel.backgroundColor = .clear
        panel.hasShadow = true; panel.ignoresMouseEvents = true; panel.hidesOnDeactivate = true
        panel.appearance = parent.effectiveAppearance
        panel.level = .floating; panel.collectionBehavior = [.transient, .ignoresCycle, .fullScreenAuxiliary]
        let surface = NSVisualEffectView(); surface.material = .popover; surface.blendingMode = .withinWindow; surface.state = .active
        surface.wantsLayer = true; surface.layer?.cornerRadius = 10; surface.layer?.masksToBounds = true
        surface.layer?.borderWidth = 0.5; surface.layer?.borderColor = NSColor.separatorColor.cgColor
        let stack = NSStackView(); stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 7
        stack.translatesAutoresizingMaskIntoConstraints = false; surface.addSubview(stack)
        let heading = NSStackView(); heading.orientation = .horizontal; heading.spacing = 8
        let icon = NSImageView(image: NSImage(systemSymbolName: directory ? "folder" : "doc.text", accessibilityDescription: nil) ?? NSImage())
        icon.contentTintColor = .secondaryLabelColor
        icon.widthAnchor.constraint(equalToConstant: 16).isActive = true; icon.heightAnchor.constraint(equalToConstant: 16).isActive = true
        let name = NSTextField(wrappingLabelWithString: title); name.font = .systemFont(ofSize: 13, weight: .semibold)
        name.textColor = .labelColor; name.maximumNumberOfLines = 0; name.lineBreakMode = .byCharWrapping
        heading.addArrangedSubview(icon); heading.addArrangedSubview(name); stack.addArrangedSubview(heading)
        let location = NSTextField(wrappingLabelWithString: path); location.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        location.textColor = .secondaryLabelColor; location.lineBreakMode = .byCharWrapping; location.maximumNumberOfLines = 0
        stack.addArrangedSubview(location)
        let screen = parent.screen?.visibleFrame ?? NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 800, height: 600)
        let width = min(460, screen.width - 32)
        name.preferredMaxLayoutWidth = width - 52; location.preferredMaxLayoutWidth = width - 28
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: surface.leadingAnchor, constant: 14),
            stack.trailingAnchor.constraint(equalTo: surface.trailingAnchor, constant: -14),
            stack.topAnchor.constraint(equalTo: surface.topAnchor, constant: 11),
            stack.bottomAnchor.constraint(equalTo: surface.bottomAnchor, constant: -11)
        ])
        let titleHeight = name.cell?.cellSize(forBounds: NSRect(x: 0, y: 0, width: width - 52, height: CGFloat.greatestFiniteMagnitude)).height ?? 16
        let pathHeight = location.cell?.cellSize(forBounds: NSRect(x: 0, y: 0, width: width - 28, height: CGFloat.greatestFiniteMagnitude)).height ?? 14
        let height = min(max(60, ceil(max(16, titleHeight) + pathHeight + 29)), screen.height - 32)
        let x = min(max(screen.minX + 12, anchor.minX + 12), screen.maxX - width - 12)
        var y = anchor.minY - height - 8
        if y < screen.minY + 12 { y = min(anchor.maxY + 8, screen.maxY - height - 12) }
        panel.setFrame(NSRect(x: x, y: y, width: width, height: height), display: false)
        panel.contentView = surface; surface.layoutSubtreeIfNeeded()
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.mouseMoved, .leftMouseDragged, .rightMouseDragged, .scrollWheel, .leftMouseDown, .rightMouseDown, .keyDown]) { [weak self] event in
            guard let self = self else { return event }
            if event.type == .mouseMoved && self.anchor.contains(NSEvent.mouseLocation) && NSEvent.pressedMouseButtons == 0 {
                self.panel.orderOut(nil); self.schedule()
            } else { self.close() }
            return event
        }
        for notification in [NSWindow.didResignKeyNotification, NSWindow.didMoveNotification, NSWindow.didResizeNotification, NSWindow.willCloseNotification] {
            observers.append(NotificationCenter.default.addObserver(forName: notification, object: parent, queue: .main) { [weak self] _ in self?.close() })
        }
        schedule()
    }
    private func schedule() {
        task?.cancel()
        guard !closed else { return }
        let task = DispatchWorkItem { [weak self] in
            guard let self = self, !self.closed, let parent = self.parent, parent.isKeyWindow,
                  self.anchor.contains(NSEvent.mouseLocation), NSEvent.pressedMouseButtons == 0 else { return }
            if parent.childWindows?.contains(self.panel) != true { parent.addChildWindow(self.panel, ordered: .above) }
            self.panel.orderFront(nil)
        }
        self.task = task; DispatchQueue.main.asyncAfter(deadline: .now() + 0.45, execute: task)
    }
    func close() {
        guard !closed else { return }; closed = true; task?.cancel(); task = nil
        if let monitor = monitor { NSEvent.removeMonitor(monitor); self.monitor = nil }
        observers.forEach { NotificationCenter.default.removeObserver($0) }; observers.removeAll()
        parent?.removeChildWindow(panel); panel.orderOut(nil)
    }
    deinit { close() }
}
@_cdecl("glim_path_hint_create")
func glimPathHintCreate(_ parentPointer: UnsafeMutableRawPointer, _ x: Double, _ y: Double, _ width: Double, _ height: Double,
                       _ title: UnsafePointer<UInt8>, _ titleLength: Int, _ path: UnsafePointer<UInt8>, _ pathLength: Int,
                       _ directory: Bool) -> UnsafeMutableRawPointer? {
    precondition(Thread.isMainThread)
    let view = Unmanaged<NSView>.fromOpaque(parentPointer).takeUnretainedValue()
    guard let window = view.window else { return nil }
    let rect = NSRect(x: view.bounds.minX + x, y: view.isFlipped ? view.bounds.minY + y : view.bounds.maxY - y - height, width: width, height: height)
    let anchor = window.convertToScreen(view.convert(rect, to: nil))
    let hint = FilePathHint(parent: window, anchor: anchor,
        title: String(decoding: UnsafeBufferPointer(start: title, count: titleLength), as: UTF8.self),
        path: String(decoding: UnsafeBufferPointer(start: path, count: pathLength), as: UTF8.self), directory: directory)
    return Unmanaged.passRetained(hint).toOpaque()
}
@_cdecl("glim_path_hint_release")
func glimPathHintRelease(_ pointer: UnsafeMutableRawPointer) {
    precondition(Thread.isMainThread)
    let hint = Unmanaged<FilePathHint>.fromOpaque(pointer).takeRetainedValue(); hint.close()
}
