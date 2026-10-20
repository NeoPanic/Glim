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
private struct FileMenuItem: Decodable { let title: String?; let enabled: Bool }
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
                menu.addItem(item)
            } else {
                menu.addItem(.separator())
            }
        }
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
