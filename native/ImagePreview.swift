import Cocoa
import ImageIO

// Image decoding may finish before the embedded view receives its first layout.
private final class ImagePreviewHost: NSView {
    var didLayout: (() -> Void)?
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        needsLayout = true
    }
    override func layout() {
        super.layout()
        didLayout?()
    }
}

// AppKit otherwise pins a document smaller than the viewport to its lower edge.
private final class CenteredImageClipView: NSClipView {
    override func constrainBoundsRect(_ proposedBounds: NSRect) -> NSRect {
        var bounds = super.constrainBoundsRect(proposedBounds)
        if let frame = documentView?.frame {
            if frame.width < bounds.width { bounds.origin.x = frame.midX - bounds.width / 2 }
            if frame.height < bounds.height { bounds.origin.y = frame.midY - bounds.height / 2 }
        }
        return bounds
    }
}

// Pan in clip-view coordinates so dragging follows the pointer at any zoom.
private final class PannableImageView: NSImageView {
    private var dragPoint: NSPoint?
    var allowsPan = false {
        didSet { window?.invalidateCursorRects(for: self) }
    }
    private var canPan: Bool {
        guard allowsPan, image != nil, let scroll = enclosingScrollView else { return false }
        let clip = scroll.contentView
        // Ignore subpixel overflow caused by fit/magnification rounding.
        let pixel = 1 / ((window?.backingScaleFactor ?? 1) * scroll.magnification)
        return bounds.width - clip.bounds.width > pixel || bounds.height - clip.bounds.height > pixel
    }
    override func resetCursorRects() {
        super.resetCursorRects()
        if canPan { addCursorRect(visibleRect, cursor: dragPoint == nil ? .openHand : .closedHand) }
    }
    override func mouseDown(with event: NSEvent) {
        guard canPan, let clip = enclosingScrollView?.contentView else { super.mouseDown(with: event); return }
        dragPoint = clip.convert(event.locationInWindow, from: nil)
        NSCursor.closedHand.set()
    }
    override func mouseDragged(with event: NSEvent) {
        guard let previous = dragPoint, let scroll = enclosingScrollView else { super.mouseDragged(with: event); return }
        let clip = scroll.contentView
        let point = clip.convert(event.locationInWindow, from: nil)
        var proposed = clip.bounds
        proposed.origin.x -= point.x - previous.x
        proposed.origin.y -= point.y - previous.y
        clip.scroll(to: clip.constrainBoundsRect(proposed).origin)
        scroll.reflectScrolledClipView(clip)
        dragPoint = clip.convert(event.locationInWindow, from: nil)
        NSCursor.closedHand.set()
    }
    override func mouseUp(with event: NSEvent) {
        guard dragPoint != nil else { super.mouseUp(with: event); return }
        dragPoint = nil
        window?.invalidateCursorRects(for: self)
        (canPan ? NSCursor.openHand : NSCursor.arrow).set()
    }
    override func viewWillMove(toWindow newWindow: NSWindow?) {
        dragPoint = nil
        super.viewWillMove(toWindow: newWindow)
    }
}

private final class ImageDecodeToken {
    private let lock = NSLock()
    private var stopped = false
    func cancel() { lock.lock(); stopped = true; lock.unlock() }
    var cancelled: Bool { lock.lock(); defer { lock.unlock() }; return stopped }
}

// ImageIO retains a file-backed source; only the requested page is decoded.
final class ImagePreview: NSObject {
    private static let decodeQueue = DispatchQueue(label: "glim.image.decode", qos: .userInitiated)
    private var decodeToken: ImageDecodeToken?
    let view: NSView = ImagePreviewHost()
    private let scroll = NSScrollView()
    private let imageView = PannableImageView()
    private let status = NSTextField(labelWithString: "")
    private let zoomLabel = NSTextField(labelWithString: "100.0%")
    private let zoomOutButton = NSButton(title: "−", target: nil, action: nil)
    private let zoomInButton = NSButton(title: "+", target: nil, action: nil)
    private let fitButton = NSButton(title: L("Fit image"), target: nil, action: nil)
    private var fitting = true
    private var requestedMagnification: CGFloat = 1
    private let loading = NSStackView()
    private let progress = NSProgressIndicator()
    private let loadingLabel = NSTextField(labelWithString: L("Loading preview…"))
    private var needsInitialFit = false
    private var fittedViewport = NSSize.zero
    private let previous = NSButton(title: L("Previous page"), target: nil, action: nil)
    private let next = NSButton(title: L("Next page"), target: nil, action: nil)
    private let resolution = NSButton(title: L("Actual size"), target: nil, action: nil)
    private let url: URL
    private let sourceSize: NSSize?
    private var page = 0
    private var count = 0
    private var original = false
    private var generation = 0
    private var task: DispatchWorkItem?
    private var closed = false
    private var svgPreview: HTMLPreview?
    init(url: URL, sourceSize: NSSize? = nil) {
        self.url = url; self.sourceSize = sourceSize
        super.init()
        if url.pathExtension.lowercased() == "svg" {
            let document = url.appendingPathExtension("glim-preview.html")
            let preview = HTMLPreview(document: document, contents: Self.svgDocument(url))
            svgPreview = preview
            preview.view.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(preview.view)
            NSLayoutConstraint.activate([
                preview.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
                preview.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
                preview.view.topAnchor.constraint(equalTo: view.topAnchor),
                preview.view.bottomAnchor.constraint(equalTo: view.bottomAnchor)
            ])
            installToolbar()
            status.isHidden = true; previous.isHidden = true; next.isHidden = true
            preview.didLoad = { [weak self] in self?.svgAction("status") }
            (view as? ImagePreviewHost)?.didLayout = { [weak self] in self?.svgAction("status") }
            return
        }
        let stack = NSStackView(); stack.orientation = .vertical; stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo: view.leadingAnchor), stack.trailingAnchor.constraint(equalTo: view.trailingAnchor), stack.topAnchor.constraint(equalTo: view.topAnchor), stack.bottomAnchor.constraint(equalTo: view.bottomAnchor)])
        imageView.imageScaling = .scaleAxesIndependently
        scroll.hasVerticalScroller = true; scroll.hasHorizontalScroller = true
        scroll.scrollerStyle = .overlay
        scroll.autohidesScrollers = true
        scroll.allowsMagnification = true; scroll.minMagnification = 0.00001; scroll.maxMagnification = 8
        scroll.contentView = CenteredImageClipView()
        scroll.documentView = imageView
        NotificationCenter.default.addObserver(self, selector: #selector(liveMagnificationEnded(_:)), name: NSScrollView.didEndLiveMagnifyNotification, object: scroll)
        stack.addArrangedSubview(scroll)
        scroll.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        scroll.heightAnchor.constraint(greaterThanOrEqualToConstant: 100).isActive = true
        loading.orientation = .vertical; loading.alignment = .centerX; loading.spacing = 8
        progress.style = .spinning; progress.controlSize = .small
        loadingLabel.textColor = .secondaryLabelColor
        loading.addArrangedSubview(progress); loading.addArrangedSubview(loadingLabel)
        loading.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(loading)
        NSLayoutConstraint.activate([
            loading.centerXAnchor.constraint(equalTo: scroll.centerXAnchor),
            loading.centerYAnchor.constraint(equalTo: scroll.centerYAnchor)
        ])
        (view as? ImagePreviewHost)?.didLayout = { [weak self] in self?.fitWhenReady() }
        installToolbar()
        load()
    }
    // The sanitized SVG is streamed through the existing local resource handler.
    // WebKit renders it as an image, without executing document scripts or links.
    private static func svgDocument(_ url: URL) -> String {
        var components = URLComponents()
        components.scheme = "glim-html"; components.host = "local"; components.path = url.path
        let labels = ["source": components.url!.absoluteString,
                      "out": L("Zoom out"), "in": L("Zoom in"),
                      "fit": L("Fit image"), "actual": L("Actual size"),
                      "loading": L("Loading preview…"), "error": L("Cannot open image")]
        let data = try! JSONSerialization.data(withJSONObject: labels)
        let configuration = String(data: data, encoding: .utf8)!.replacingOccurrences(of: "<", with: "\\u003c")
        return """
        <!doctype html><html><head><meta charset="utf-8">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src glim-html: data:; style-src 'unsafe-inline'; script-src 'unsafe-inline'">
        <style>
        :root { color-scheme: light dark; font: 13px -apple-system, system-ui; }
        * { box-sizing: border-box; }
        html, body { margin: 0; width: 100%; height: 100%; overflow: hidden; }
        body { display: flex; flex-direction: column; background: Canvas; color: CanvasText; }
        #viewport { flex: 1; min-height: 0; overflow: auto; }
        #stage { position: relative; }
        #viewport.pannable { cursor: grab; }
        #viewport.dragging { cursor: grabbing; }
        img { position: absolute; display: block; max-width: none; user-select: none; }
        </style></head><body>
        <div id="viewport" tabindex="0"><div id="stage"><img id="image" draggable="false" alt=""></div></div>
        <script>
        const labels = \(configuration);
        const viewport = document.getElementById('viewport'), stage = document.getElementById('stage');
        const image = document.getElementById('image');
        let scale = 1, fitting = true, ready = false, drag = null;
        viewport.addEventListener('pointerdown', event => {
            if (event.button !== 0 || !viewport.classList.contains('pannable') || event.target === viewport) return;
            drag = { id: event.pointerId, x: event.clientX, y: event.clientY, left: viewport.scrollLeft, top: viewport.scrollTop };
            viewport.setPointerCapture(event.pointerId); viewport.classList.add('dragging'); event.preventDefault();
        });
        viewport.addEventListener('pointermove', event => {
            if (!drag || drag.id !== event.pointerId) return;
            viewport.scrollLeft = drag.left - (event.clientX - drag.x);
            viewport.scrollTop = drag.top - (event.clientY - drag.y);
            event.preventDefault();
        });
        function endDrag(event) {
            if (!drag || drag.id !== event.pointerId) return;
            drag = null; viewport.classList.remove('dragging');
            if (viewport.hasPointerCapture(event.pointerId)) viewport.releasePointerCapture(event.pointerId);
        }
        ['pointerup', 'pointercancel', 'lostpointercapture'].forEach(name => viewport.addEventListener(name, endDrag));
        function apply(value) {
            if (!ready || !Number.isFinite(value) || value <= 0) return;
            const centerX = (viewport.scrollLeft + viewport.clientWidth / 2) / Math.max(stage.offsetWidth, 1);
            const centerY = (viewport.scrollTop + viewport.clientHeight / 2) / Math.max(stage.offsetHeight, 1);
            scale = value;
            const width = image.naturalWidth * scale, height = image.naturalHeight * scale;
            const stageWidth = Math.max(width, viewport.clientWidth), stageHeight = Math.max(height, viewport.clientHeight);
            stage.style.width = stageWidth + 'px'; stage.style.height = stageHeight + 'px';
            image.style.width = width + 'px'; image.style.height = height + 'px';
            image.style.left = Math.max(0, (stageWidth - width) / 2) + 'px';
            image.style.top = Math.max(0, (stageHeight - height) / 2) + 'px';
            viewport.scrollLeft = centerX * stageWidth - viewport.clientWidth / 2;
            viewport.scrollTop = centerY * stageHeight - viewport.clientHeight / 2;
            viewport.classList.toggle('pannable', !fitting && (viewport.scrollWidth - viewport.clientWidth > 1 || viewport.scrollHeight - viewport.clientHeight > 1));
        }
        function fit() {
            fitting = true;
            apply(Math.min(viewport.clientWidth / image.naturalWidth, viewport.clientHeight / image.naturalHeight, 1));
        }
        image.onload = () => { ready = image.naturalWidth > 0 && image.naturalHeight > 0; if (ready) fit(); };
        window.glimImageViewer = { command(name) {
            if (!ready) return null;
            if (name === 'fit') fit();
            else if (name === 'actual') { fitting = false; apply(1); }
            else if (name === 'in') { fitting = false; apply(scale * Math.SQRT2); }
            else if (name === 'out') { fitting = false; apply(scale / Math.SQRT2); }
            return scale;
        }};
        new ResizeObserver(() => { if (ready) { if (fitting) fit(); else apply(scale); } }).observe(viewport);
        image.src = labels.source;
        </script></body></html>
        """
    }
    private func installToolbar() {
        let toolbar = NSStackView()
        zoomLabel.alignment = .center
        zoomLabel.font = .monospacedDigitSystemFont(ofSize: 13, weight: .medium)
        zoomLabel.lineBreakMode = .byClipping
        zoomLabel.maximumNumberOfLines = 1
        zoomLabel.setContentCompressionResistancePriority(.required, for: .horizontal)
        zoomLabel.widthAnchor.constraint(greaterThanOrEqualToConstant: 68).isActive = true
        previous.target = self; previous.action = #selector(previousPage)
        next.target = self; next.action = #selector(nextPage)
        zoomOutButton.target = self; zoomOutButton.action = #selector(zoomOut)
        zoomInButton.target = self; zoomInButton.action = #selector(zoomIn)
        fitButton.target = self; fitButton.action = #selector(fitImage)
        resolution.target = self; resolution.action = #selector(actualSize)
        [previous, next, zoomOutButton, zoomLabel, zoomInButton, fitButton, resolution, status].forEach { toolbar.addArrangedSubview($0) }
        FloatingReaderToolbar(items: toolbar).install(in: view)
    }
    private func svgAction(_ command: String) {
        svgPreview?.imageCommand(command) { [weak self] scale in
            guard let self = self, !self.closed else { return }
            let ready = scale != nil
            [self.zoomOutButton, self.zoomInButton, self.fitButton, self.resolution].forEach { $0.isEnabled = ready }
            let text = scale.map { String(format: "%.1f%%", $0 * 100) } ?? L("Loading preview…")
            if self.zoomLabel.stringValue != text { self.zoomLabel.stringValue = text }
        }
    }
    private func changeZoom(_ factor: CGFloat) {
        guard imageView.image != nil else { return }
        fitting = false
        let value = min(scroll.maxMagnification, max(scroll.minMagnification, scroll.magnification * factor))
        let rect = imageView.visibleRect
        scroll.setMagnification(value, centeredAt: NSPoint(x: rect.midX, y: rect.midY))
        requestedMagnification = value; updateZoomLabel()
    }
    @objc private func liveMagnificationEnded(_ notification: Notification) {
        fitting = false; requestedMagnification = scroll.magnification; updateZoomLabel()
    }
    private func updateZoomLabel() {
        imageView.allowsPan = !fitting
        zoomLabel.stringValue = String(format: "%.1f%%", Double(scroll.magnification) * 100)
        imageView.window?.invalidateCursorRects(for: imageView)
    }
    @objc private func zoomOut() { if svgPreview != nil { svgAction("out") } else { changeZoom(1 / sqrt(2)) } }
    @objc private func zoomIn() { if svgPreview != nil { svgAction("in") } else { changeZoom(sqrt(2)) } }
    @objc private func fitImage() {
        if svgPreview != nil { svgAction("fit"); return }
        fitting = true; needsInitialFit = true; fitWhenReady()
    }
    @objc private func actualSize() {
        if svgPreview != nil { svgAction("actual"); return }
        fitting = false; requestedMagnification = 1
        if !original { original = true; load() } else { needsInitialFit = true; fitWhenReady() }
    }
    @objc private func previousPage() { if page > 0 { page -= 1; load() } }
    @objc private func nextPage() { if page + 1 < count { page += 1; load() } }
    private func controls(_ busy: Bool) {
        previous.isEnabled = !busy && page > 0; next.isEnabled = !busy && page + 1 < count
        resolution.isEnabled = !busy && imageView.image != nil
        previous.isHidden = count <= 1; next.isHidden = count <= 1
        [zoomOutButton, zoomInButton, fitButton].forEach { $0.isEnabled = !busy && imageView.image != nil }
    }
    private func fitWhenReady() {
        guard view.window != nil, imageView.image != nil else { return }
        let viewport = scroll.contentSize
        let size = imageView.frame.size
        guard viewport.width > 0, viewport.height > 0, size.width > 0, size.height > 0 else { return }
        guard needsInitialFit || (fitting && viewport != fittedViewport) else { return }
        fittedViewport = viewport
        // Never commit an almost-zero fit for an unattached/unlaid-out native view.
        needsInitialFit = false
        let fit = min(viewport.width / size.width, viewport.height / size.height, 1)
        scroll.magnification = fitting ? max(scroll.minMagnification, fit) : requestedMagnification
        updateZoomLabel()
        scroll.contentView.scroll(to: scroll.contentView.constrainBoundsRect(scroll.contentView.bounds).origin)
        scroll.reflectScrolledClipView(scroll.contentView)
        loading.isHidden = true; progress.stopAnimation(nil)
    }
    private func load() {
        decodeToken?.cancel(); task?.cancel(); generation += 1
        let token = ImageDecodeToken(); decodeToken = token
        let generation = generation, page = page, original = original, url = url, sourceSize = sourceSize
        needsInitialFit = false; imageView.image = nil
        loadingLabel.stringValue = L("Loading preview…")
        loading.isHidden = false; progress.startAnimation(nil)
        status.stringValue = L("Loading preview…"); controls(true)
        let work = DispatchWorkItem { [weak self] in
            guard !token.cancelled else { return }
            let source = CGImageSourceCreateWithURL(url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary)
            let count = source.map(CGImageSourceGetCount) ?? 0
            let props = source.flatMap { CGImageSourceCopyPropertiesAtIndex($0, page, nil) as? [CFString: Any] }
            let rasterWidth = (props?[kCGImagePropertyPixelWidth] as? NSNumber)?.doubleValue ?? 1
            let rasterHeight = (props?[kCGImagePropertyPixelHeight] as? NSNumber)?.doubleValue ?? 1
            let width = sourceSize.map { Double($0.width) } ?? rasterWidth
            let height = sourceSize.map { Double($0.height) } ?? rasterHeight
            let budgetEdge = max(1, floor(sqrt(Double(ReaderLimits.IMAGE_DECODED_BYTES) / 4)))
            let edge = min(max(rasterWidth, rasterHeight), budgetEdge, original ? budgetEdge : Double(ReaderLimits.IMAGE_PREVIEW_EDGE))
            guard !token.cancelled else { return }
            let image = source.flatMap { CGImageSourceCreateThumbnailAtIndex($0, page, [kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true, kCGImageSourceThumbnailMaxPixelSize: edge, kCGImageSourceShouldCacheImmediately: true, kCGImageSourceShouldAllowFloat: false] as CFDictionary) }
            guard !token.cancelled else { return }
            DispatchQueue.main.async { [weak self] in
                guard let self = self, !self.closed, self.generation == generation else { return }
                self.task = nil
                guard let image = image else {
                    self.status.stringValue = L("Cannot open image")
                    self.loadingLabel.stringValue = L("Cannot open image")
                    self.progress.stopAnimation(nil)
                    self.controls(false); return
                }
                self.count = count
                let scale = max(width, height) / Double(max(image.width, image.height))
                let size = sourceSize ?? NSSize(width: Double(image.width) * scale, height: Double(image.height) * scale)
                self.imageView.image = NSImage(cgImage: image, size: size)
                self.imageView.frame = NSRect(origin: .zero, size: size)
                self.needsInitialFit = true
                self.view.layoutSubtreeIfNeeded()
                self.fitWhenReady()
                self.status.stringValue = "\(page + 1) / \(count) · \(Int(width)) × \(Int(height))" + (original && max(width, height) > budgetEdge ? " · " + L("Scaled preview") : "")
                self.controls(false)
            }
        }
        task = work; Self.decodeQueue.async(execute: work)
    }
    func pause() { decodeToken?.cancel(); decodeToken = nil; task?.cancel(); task = nil; generation += 1; needsInitialFit = false; progress.stopAnimation(nil); imageView.image = nil }
    func resume() { if svgPreview != nil { return }; if !closed && task == nil && imageView.image == nil { load() } }
    func close() { NotificationCenter.default.removeObserver(self); closed = true; pause(); svgPreview?.close(); svgPreview = nil }
}
