import Cocoa

// One content-level capsule shared by the native readers. It overlays content
// rather than consuming document height; wide controls remain horizontally scrollable.
final class FloatingReaderToolbar: NSView {
    private let items: NSStackView
    private let material: NSView
    private let interior: NSView
    private var measuredSize = NSSize.zero
    private var controlWidths: [ObjectIdentifier: NSLayoutConstraint] = [:]
    private let viewport = NSScrollView()
    private var expanded = false
    private var hoverArea: NSTrackingArea?
    private var collapseTask: DispatchWorkItem?
    private var expandTask: DispatchWorkItem?
    private var releaseMonitor: Any?
    init(items: NSStackView) {
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
        items.detachesHiddenViews = true
        items.orientation = .horizontal
        items.alignment = .centerY
        items.distribution = .fill
        items.setHuggingPriority(.required, for: .horizontal)
        items.spacing = 6
        items.edgeInsets = NSEdgeInsets()
        items.translatesAutoresizingMaskIntoConstraints = true
        for item in items.arrangedSubviews {
            // Overflow belongs to the scroll viewport, never to individual
            // controls. Keep selectors and action titles at readable widths.
            item.setContentCompressionResistancePriority(.required, for: .horizontal)
            items.setVisibilityPriority(.mustHold, for: item)
            if let picker = item as? NSPopUpButton {
                picker.widthAnchor.constraint(greaterThanOrEqualToConstant: 160).isActive = true
            }
        }
        for case let button as NSButton in items.arrangedSubviews where !(button is NSPopUpButton) {
            button.isBordered = true; button.bezelStyle = .rounded
            button.controlSize = .regular
            button.setContentHuggingPriority(.required, for: .horizontal)
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
        viewport.documentView = items
        interior.addSubview(viewport); addSubview(material)
        material.isHidden = true
        wantsLayer = true
        layer?.shadowColor = NSColor.shadowColor.cgColor
        layer?.shadowOpacity = 0; layer?.shadowRadius = 14
        layer?.shadowOffset = NSSize(width: 0, height: -3)
        setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        setAccessibilityRole(.toolbar)
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is unsupported") }
    private var contentSize: NSSize {
        // Measure the visible controls directly: stack fittingSize can inherit a
        // previous narrow document frame and compress button cells to ellipses.
        let visible = items.arrangedSubviews.filter { !$0.isHidden }
        var width: CGFloat = 0, height: CGFloat = 28
        for item in visible {
            var measured = item.intrinsicContentSize
            if let label = item as? NSTextField {
                label.textColor = .secondaryLabelColor
                label.setContentHuggingPriority(.required, for: .horizontal)
            }
            if let button = item as? NSButton {
                let title = (button as? NSPopUpButton)?.selectedItem?.title ?? button.title
                let font = button.font ?? NSFont.systemFont(ofSize: 13)
                let textWidth = ceil((title as NSString).size(withAttributes: [.font: font]).width)
                measured.width = max(measured.width, max(button is NSPopUpButton ? 160 : 28, textWidth + 32))
            } else if let segments = item as? NSSegmentedControl {
                let font = segments.font ?? NSFont.systemFont(ofSize: 13)
                let labelWidth = (0..<segments.segmentCount).reduce(CGFloat(0)) {
                    $0 + ceil(((segments.label(forSegment: $1) ?? "") as NSString).size(withAttributes: [.font: font]).width) + 24
                }
                measured.width = max(measured.width, labelWidth)
            }
            if measured.width < 0 || measured.height < 0 { measured = item.fittingSize }
            for constraint in item.constraints where constraint.isActive && constraint.firstAttribute == .width && constraint.secondItem == nil {
                if let own = controlWidths[ObjectIdentifier(item)], constraint === own { continue }
                if constraint.relation == .greaterThanOrEqual || constraint.relation == .equal {
                    measured.width = max(measured.width, constraint.constant)
                }
            }
            if item is NSControl {
                let key = ObjectIdentifier(item)
                if let constraint = controlWidths[key] { constraint.constant = max(0, measured.width) }
                else {
                    let constraint = item.widthAnchor.constraint(greaterThanOrEqualToConstant: max(0, measured.width))
                    constraint.isActive = true; controlWidths[key] = constraint
                }
            }
            width += max(0, measured.width)
            height = max(height, measured.height)
        }
        width += CGFloat(max(0, visible.count - 1)) * items.spacing
        return NSSize(width: width, height: height)
    }
    override var intrinsicContentSize: NSSize {
        let size = contentSize
        return NSSize(width: size.width + 24, height: max(72, size.height + 32))
    }
    override func viewWillDraw() {
        let size = contentSize
        if size != measuredSize {
            measuredSize = size; invalidateIntrinsicContentSize(); needsLayout = true
        }
        super.viewWillDraw()
    }
    override func layout() {
        super.layout()
        let content = contentSize
        let width = min(bounds.width, content.width + 24)
        material.frame = NSRect(x: (bounds.width - width) / 2, y: 0, width: width, height: max(28, content.height) + 16)
        interior.frame = material.bounds
        viewport.frame = interior.bounds.insetBy(dx: 12, dy: 8)
        let size = contentSize
        items.frame = NSRect(origin: .zero, size: NSSize(width: size.width, height: max(size.height, viewport.bounds.height)))
        material.wantsLayer = true
        material.layer?.cornerRadius = material.bounds.height / 2
        material.layer?.masksToBounds = true
        #if GLIM_NATIVE_GLASS
        if #available(macOS 26.0, *), let glass = material as? NSGlassEffectView { glass.cornerRadius = material.bounds.height / 2 }
        #endif
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
    func install(in host: NSView, bottomInset: CGFloat = 16) {
        translatesAutoresizingMaskIntoConstraints = false
        host.addSubview(self)
        let maximumWidth = widthAnchor.constraint(equalTo: host.widthAnchor, constant: -32)
        NSLayoutConstraint.activate([
            centerXAnchor.constraint(equalTo: host.centerXAnchor),
            bottomAnchor.constraint(equalTo: host.bottomAnchor, constant: -bottomInset),
            maximumWidth
        ])
    }
}
