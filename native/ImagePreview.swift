import Cocoa
import ImageIO
import Quartz

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
    private let imageView = NSImageView()
    private let status = NSTextField(labelWithString: "")
    private let loading = NSStackView()
    private let progress = NSProgressIndicator()
    private let loadingLabel = NSTextField(labelWithString: L("Loading preview…"))
    private var needsInitialFit = false
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
    private var quickLook: QLPreviewView?
    init(url: URL, sourceSize: NSSize? = nil) {
        self.url = url; self.sourceSize = sourceSize
        super.init()
        let stack = NSStackView(); stack.orientation = .vertical; stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo: view.leadingAnchor), stack.trailingAnchor.constraint(equalTo: view.trailingAnchor), stack.topAnchor.constraint(equalTo: view.topAnchor), stack.bottomAnchor.constraint(equalTo: view.bottomAnchor)])
        let toolbar = NSStackView()
        previous.target = self; previous.action = #selector(previousPage)
        next.target = self; next.action = #selector(nextPage)
        resolution.target = self; resolution.action = #selector(toggleResolution)
        [previous, status, next, resolution].forEach { toolbar.addArrangedSubview($0) }
        stack.addArrangedSubview(toolbar)
        imageView.imageScaling = .scaleAxesIndependently
        scroll.hasVerticalScroller = true; scroll.hasHorizontalScroller = true
        scroll.allowsMagnification = true; scroll.minMagnification = 0.00001; scroll.maxMagnification = 8
        scroll.documentView = imageView
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
        load()
    }
    @objc private func previousPage() { if page > 0 { page -= 1; load() } }
    @objc private func nextPage() { if page + 1 < count { page += 1; load() } }
    @objc private func toggleResolution() { original.toggle(); load() }
    private func controls(_ busy: Bool) {
        previous.isEnabled = !busy && page > 0; next.isEnabled = !busy && page + 1 < count
        resolution.isEnabled = !busy && quickLook == nil
        resolution.title = original ? L("Fit image") : L("Actual size")
    }
    private func fitWhenReady() {
        guard needsInitialFit, view.window != nil, imageView.image != nil else { return }
        let viewport = scroll.contentSize
        let size = imageView.frame.size
        guard viewport.width > 0, viewport.height > 0, size.width > 0, size.height > 0 else { return }
        // Never commit an almost-zero fit for an unattached/unlaid-out native view.
        needsInitialFit = false
        let fit = min(viewport.width / size.width, viewport.height / size.height, 1)
        scroll.magnification = original ? 1 : max(scroll.minMagnification, fit)
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
                    if source == nil, url.pathExtension.lowercased() == "svg", let preview = QLPreviewView(frame: self.scroll.bounds, style: .normal) {
                        preview.autoresizingMask = [.width, .height]; preview.autostarts = true
                        preview.previewItem = url as NSURL
                        self.quickLook = preview; self.scroll.contentView.addSubview(preview)
                        self.status.stringValue = L("Read-only preview")
                        self.loading.isHidden = true
                    } else {
                        self.status.stringValue = L("Cannot open image")
                        self.loadingLabel.stringValue = L("Cannot open image")
                    }
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
    func resume() { if !closed && task == nil && imageView.image == nil && quickLook == nil { load() } }
    func close() { closed = true; pause(); quickLook?.previewItem = nil; quickLook?.close(); quickLook = nil }
}
