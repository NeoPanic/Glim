import Cocoa

// One content-level capsule shared by the native readers. It overlays content
// rather than consuming document height; wide controls remain horizontally scrollable.
final class FloatingReaderToolbar: NSView {
    private let controls: ReaderControls
    private var contextMenu: ReaderContextMenu?
    private var keyboardShortcuts: ReaderKeyboardShortcuts?
    private let items: [NSView]
    private let document = NSView()
    private let material: NSView
    private let interior: NSView
    private var measuredSize = NSSize.zero
    private var measuredViews: [ObjectIdentifier] = []
    private let viewport = NSScrollView()
    private let scrollLeft = NSButton(title: "‹", target: nil, action: nil)
    private let scrollRight = NSButton(title: "›", target: nil, action: nil)
    private var expanded = false
    private var hoverArea: NSTrackingArea?
    private var collapseTask: DispatchWorkItem?
    private var expandTask: DispatchWorkItem?
    private var releaseMonitor: Any?
    convenience init(items: NSStackView) { self.init(controls: ReaderControls(stack: items)) }
    init(controls: ReaderControls) {
        self.controls = controls
        let items = controls.entries.map { $0.view }
        self.items = items
        let interior = NSView(); self.interior = interior
        #if GLIM_NATIVE_GLASS
        if #available(macOS 26.0, *) {
            let glass = NSGlassEffectView()
            glass.style = .regular; glass.contentView = interior
            material = glass
        } else {
            let effect = NSVisualEffectView()
            effect.material = .hudWindow; effect.blendingMode = .withinWindow; effect.state = .active
            effect.addSubview(interior); material = effect
        }
        #else
        let effect = NSVisualEffectView()
        effect.material = .hudWindow; effect.blendingMode = .withinWindow; effect.state = .active
        effect.addSubview(interior); material = effect
        #endif
        super.init(frame: .zero)
        for item in items {
            // The scroll document has one frame-based layout owner. Reparenting
            // removes stack gravity/distribution from the sizing feedback loop.
            item.removeFromSuperview()
            item.translatesAutoresizingMaskIntoConstraints = true
            document.addSubview(item)
            if let picker = item as? NSPopUpButton {
                picker.cell?.lineBreakMode = .byTruncatingMiddle
            }
        }
        for case let button as NSButton in items where !(button is NSPopUpButton) {
            button.isBordered = true; button.bezelStyle = .rounded
            button.controlSize = .regular
            button.setContentHuggingPriority(.defaultHigh, for: .horizontal)
            button.font = .systemFont(ofSize: 13, weight: .medium)
            button.heightAnchor.constraint(greaterThanOrEqualToConstant: 28).isActive = true
            let label = button.title == "+" ? L("Zoom in") : (["−", "-"].contains(button.title) ? L("Zoom out") : button.title)
            button.toolTip = label
            button.setAccessibilityLabel(label)
            if ["+", "−", "-"].contains(button.title) {
                button.image = NSImage(systemSymbolName: button.title == "+" ? "plus" : "minus", accessibilityDescription: label)
                button.imagePosition = .imageOnly; button.title = ""
            }
        }
        viewport.drawsBackground = false
        viewport.hasHorizontalScroller = true; viewport.hasVerticalScroller = false
        viewport.scrollerStyle = .overlay; viewport.autohidesScrollers = true
        viewport.documentView = document
        interior.addSubview(viewport)
        for (button, action, label) in [(scrollLeft, #selector(scrollBackward), L("Scroll toolbar left")), (scrollRight, #selector(scrollForward), L("Scroll toolbar right"))] {
            button.target = self; button.action = action
            button.bezelStyle = .rounded; button.isBordered = true
            button.toolTip = label; button.setAccessibilityLabel(label)
            interior.addSubview(button)
        }
        addSubview(material)
        material.isHidden = true
        wantsLayer = true
        layer?.shadowColor = NSColor.shadowColor.cgColor
        layer?.shadowOpacity = 0; layer?.shadowRadius = 14
        layer?.shadowOffset = NSSize(width: 0, height: -3)
        setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        setAccessibilityRole(.toolbar)
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is unsupported") }
    private func measurements() -> [(NSView, NSSize)] {
        // Measure the visible controls directly: stack fittingSize can inherit a
        // previous narrow document frame and compress button cells to ellipses.
        return items.filter { !$0.isHidden }.map { item in
            var measured = item.intrinsicContentSize
            if let label = item as? NSTextField {
                label.textColor = .secondaryLabelColor
                label.setContentHuggingPriority(.defaultHigh, for: .horizontal)
                let font = label.font ?? NSFont.systemFont(ofSize: NSFont.systemFontSize(for: label.controlSize))
                let text = (label.stringValue as NSString).size(withAttributes: [.font: font])
                measured.width = ceil(text.width)
                measured.height = max(ceil(text.height), label.cell?.cellSize.height ?? 0)
            }
            if let button = item as? NSButton {
                let title = (button as? NSPopUpButton)?.selectedItem?.title ?? button.title
                let font = button.font ?? button.cell?.font ?? NSFont.systemFont(ofSize: NSFont.systemFontSize(for: button.controlSize))
                let textWidth = ceil((title as NSString).size(withAttributes: [.font: font]).width)
                if let picker = button as? NSPopUpButton {
                    // A popup's intrinsic size can retain a previous large menu
                    // width. Size the control from its current title instead.
                    let imageWidth = picker.selectedItem?.image.map { $0.size.width + 6 } ?? 0
                    measured.width = textWidth + imageWidth + 40
                    picker.toolTip = title
                } else {
                    let imageWidth = button.image?.size.width ?? 0
                    switch button.imagePosition {
                    case .imageOnly: measured.width = max(28, imageWidth + 12)
                    case .imageLeft, .imageRight, .imageLeading, .imageTrailing:
                        measured.width = max(28, textWidth + (imageWidth > 0 ? imageWidth + 6 : 0) + 32)
                    default: measured.width = max(28, max(textWidth, imageWidth) + 32)
                    }
                }
            } else if let segments = item as? NSSegmentedControl {
                let font = segments.font ?? NSFont.systemFont(ofSize: 13)
                let labelWidth = (0..<segments.segmentCount).reduce(CGFloat(0)) {
                    $0 + ceil(((segments.label(forSegment: $1) ?? "") as NSString).size(withAttributes: [.font: font]).width) + 24
                }
                measured.width = labelWidth
            }
            if !(item is NSControl) {
                // An unconstrained spacer has no content width. Never measure
                // its last stretched frame via fittingSize.
                measured.width = max(0, measured.width)
                measured.height = max(0, measured.height)
            }
            measured.width = max(0, measured.width)
            measured.height = max(28, measured.height)
            for constraint in item.constraints where constraint.isActive && constraint.firstAttribute == .width && constraint.secondItem == nil {
                if constraint.relation == .greaterThanOrEqual || constraint.relation == .equal {
                    measured.width = max(measured.width, constraint.constant)
                }
            }
            return (item, measured)
        }
    }
    private var contentSize: NSSize {
        let entries = measurements()
        return NSSize(width: entries.reduce(0) { $0 + $1.1.width } + CGFloat(max(0, entries.count - 1)) * 6,
                      height: entries.reduce(CGFloat(28)) { max($0, $1.1.height) })
    }
    override var intrinsicContentSize: NSSize {
        let size = contentSize
        return NSSize(width: size.width + 24, height: max(72, size.height + 32))
    }
    override func viewWillDraw() {
        let size = contentSize
        let visible = items.filter { !$0.isHidden }.map { ObjectIdentifier($0) }
        if size != measuredSize || visible != measuredViews {
            measuredSize = size; measuredViews = visible
            invalidateIntrinsicContentSize(); needsLayout = true
        }
        super.viewWillDraw()
    }
    override func layout() {
        super.layout()
        let content = contentSize
        let width = min(bounds.width, content.width + 24)
        material.frame = NSRect(x: (bounds.width - width) / 2, y: 0, width: width, height: max(28, content.height) + 16)
        interior.frame = material.bounds
        let overflow = content.width > max(0, width - 24)
        scrollLeft.isHidden = !overflow; scrollRight.isHidden = !overflow
        let inset: CGFloat = overflow ? 44 : 12
        viewport.frame = NSRect(x: inset, y: 8, width: max(0, width - 2 * inset), height: max(28, content.height))
        scrollLeft.frame = NSRect(x: 10, y: 8, width: 28, height: max(28, content.height))
        scrollRight.frame = NSRect(x: max(10, width - 38), y: 8, width: 28, height: max(28, content.height))
        let entries = measurements()
        document.frame = NSRect(origin: .zero, size: NSSize(width: content.width, height: max(content.height, viewport.contentSize.height)))
        var x: CGFloat = 0
        for (item, size) in entries {
            item.frame = NSRect(x: x, y: (document.bounds.height - size.height) / 2, width: size.width, height: size.height)
            x += size.width + 6
        }
        // A disappearing paging group or shorter label must not leave the
        // viewport scrolled beyond the new document's trailing edge.
        let origin = viewport.contentView.bounds.origin
        viewport.contentView.scroll(to: NSPoint(x: min(max(0, origin.x), max(0, content.width - viewport.contentSize.width)), y: 0))
        viewport.reflectScrolledClipView(viewport.contentView)
        material.wantsLayer = true
        material.layer?.cornerRadius = material.bounds.height / 2
        material.layer?.masksToBounds = true
        #if GLIM_NATIVE_GLASS
        if #available(macOS 26.0, *), let glass = material as? NSGlassEffectView { glass.cornerRadius = material.bounds.height / 2 }
        #endif
    }
    @objc private func scrollBackward() { scrollControls(by: -1) }
    @objc private func scrollForward() { scrollControls(by: 1) }
    private func scrollControls(by direction: CGFloat) {
        let clip = viewport.contentView
        let maximum = max(0, document.bounds.width - clip.bounds.width)
        clip.scroll(to: NSPoint(x: min(maximum, max(0, clip.bounds.origin.x + direction * clip.bounds.width * 0.8)), y: 0))
        viewport.reflectScrolledClipView(clip)
    }
    // The invisible activation strip never intercepts clicks or image drags.
    override func hitTest(_ point: NSPoint) -> NSView? {
        guard expanded, material.frame.contains(convert(point, from: superview)) else { return nil }
        return super.hitTest(point)
    }
    private var pointerInside: Bool {
        guard let window = window else { return false }
        return bounds.contains(convert(window.mouseLocationOutsideOfEventStream, from: nil))
    }
    private func scheduleShow() {
        collapseTask?.cancel(); collapseTask = nil
        guard !expanded, expandTask == nil, NSEvent.pressedMouseButtons == 0 else { return }
        let task = DispatchWorkItem { [weak self] in
            guard let self = self else { return }
            self.expandTask = nil
            guard self.pointerInside, NSEvent.pressedMouseButtons == 0 else { return }
            self.needsLayout = true; self.layoutSubtreeIfNeeded()
            self.expanded = true; self.material.isHidden = false
            self.layer?.shadowOpacity = 0.16
        }
        expandTask = task
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25, execute: task)
    }
    private func scheduleHide() {
        expandTask?.cancel(); expandTask = nil
        collapseTask?.cancel()
        let task = DispatchWorkItem { [weak self] in
            guard let self = self else { return }
            self.collapseTask = nil
            guard !self.pointerInside, NSEvent.pressedMouseButtons == 0 else { return }
            self.expanded = false; self.material.isHidden = true
            self.layer?.shadowOpacity = 0
        }
        collapseTask = task
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.7, execute: task)
    }
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if expanded && !pointerInside { scheduleHide() }
        if let area = hoverArea { removeTrackingArea(area) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .mouseMoved, .activeInKeyWindow, .inVisibleRect], owner: self)
        addTrackingArea(area); hoverArea = area
    }
    override func mouseEntered(with event: NSEvent) { scheduleShow() }
    override func mouseMoved(with event: NSEvent) { scheduleShow() }
    override func mouseExited(with event: NSEvent) { scheduleHide() }
    override func viewWillMove(toWindow newWindow: NSWindow?) {
        collapseTask?.cancel(); collapseTask = nil
        expandTask?.cancel(); expandTask = nil
        if let monitor = releaseMonitor { NSEvent.removeMonitor(monitor); releaseMonitor = nil }
        if newWindow == nil {
            expanded = false; material.isHidden = true; layer?.shadowOpacity = 0
        }
        super.viewWillMove(toWindow: newWindow)
        if newWindow != nil {
            releaseMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseUp, .rightMouseUp, .otherMouseUp]) { [weak self] event in
                // A release only permits hiding, never reveals controls at the
                // end of an image drag. A later ordinary move can reveal them.
                if let self = self, self.expanded, !self.pointerInside { self.scheduleHide() }
                return event
            }
        }
    }
    deinit {
        collapseTask?.cancel(); expandTask?.cancel()
        if let monitor = releaseMonitor { NSEvent.removeMonitor(monitor) }
    }
    func install(in host: NSView, bottomInset: CGFloat = 16, managesContextMenu: Bool = true) {
        translatesAutoresizingMaskIntoConstraints = false
        host.addSubview(self)
        keyboardShortcuts = ReaderKeyboardShortcuts(host: host, controls: controls)
        if managesContextMenu {
            contextMenu = ReaderContextMenu(host: host) { [weak self] _ in self?.controls.menu() }
        }
        let maximumWidth = widthAnchor.constraint(equalTo: host.widthAnchor, constant: -32)
        NSLayoutConstraint.activate([
            centerXAnchor.constraint(equalTo: host.centerXAnchor),
            bottomAnchor.constraint(equalTo: host.bottomAnchor, constant: -bottomInset),
            maximumWidth
        ])
    }
}
