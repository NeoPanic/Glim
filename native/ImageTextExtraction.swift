import Cocoa
import Vision
import VisionKit

// OCR consumes the reader's bounded, already-oriented raster, never decodes a
// second full-resolution source. One shared queue bounds concurrent Vision work.
final class ImageTextExtraction: NSObject, NSWindowDelegate {
    private static let queue = DispatchQueue(label: "glim.image.text", qos: .userInitiated)
    private var request: VNRecognizeTextRequest?
    private var work: DispatchWorkItem?
    private var generation = 0
    private var panel: NSPanel?
    private let text = NSTextView()
    private let message = NSTextField(labelWithString: "")
    private let copyButton = NSButton(title: L("Copy All"), target: nil, action: nil)

    func extract(_ image: CGImage, in parent: NSWindow, extraControls: [NSView] = []) {
        cancel()
        generation += 1
        let generation = generation
        let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 620, height: 480),
                            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        panel.title = L("Image Text")
        panel.isReleasedWhenClosed = false
        panel.delegate = self
        panel.minSize = NSSize(width: 360, height: 240)
        self.panel = panel
        let root = NSView(); panel.contentView = root
        let scroll = NSScrollView(); scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true
        scroll.documentView = text
        text.isEditable = false; text.isSelectable = true; text.isRichText = false
        text.font = .systemFont(ofSize: 15)
        text.textContainerInset = NSSize(width: 16, height: 16)
        text.isVerticallyResizable = true; text.isHorizontallyResizable = false
        text.autoresizingMask = [.width]
        text.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        text.textContainer?.containerSize = NSSize(width: 620, height: CGFloat.greatestFiniteMagnitude)
        text.textContainer?.widthTracksTextView = true
        text.frame = NSRect(x: 0, y: 0, width: 620, height: 420)
        text.string = ""
        message.stringValue = L("Recognizing text…")
        message.textColor = .secondaryLabelColor
        copyButton.target = self; copyButton.action = #selector(copyAll)
        copyButton.bezelStyle = .rounded; copyButton.isEnabled = false
        let footer = NSStackView(views: [message] + extraControls + [copyButton]); footer.spacing = 12
        [scroll, footer].forEach { $0.translatesAutoresizingMaskIntoConstraints = false; root.addSubview($0) }
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: root.leadingAnchor), scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: root.topAnchor), scroll.bottomAnchor.constraint(equalTo: footer.topAnchor, constant: -12),
            footer.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 16), footer.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -16),
            footer.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -12)
        ])
        parent.addChildWindow(panel, ordered: .above)
        panel.setFrameOrigin(NSPoint(x: parent.frame.midX - panel.frame.width / 2, y: parent.frame.midY - panel.frame.height / 2))
        panel.makeKeyAndOrderFront(nil)
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = true
        if #available(macOS 13.0, *) { request.automaticallyDetectsLanguage = true }
        self.request = request
        let work = DispatchWorkItem { [weak self] in
            do {
                // Supported languages vary with OS and request revision.
                let supported: [String]
                if #available(macOS 12.0, *) {
                    supported = try request.supportedRecognitionLanguages()
                } else {
                    supported = try VNRecognizeTextRequest.supportedRecognitionLanguages(for: .accurate, revision: request.revision)
                }
                request.recognitionLanguages = ["zh-Hans", "zh-Hant", "en-US"].filter { supported.contains($0) }
                try VNImageRequestHandler(cgImage: image, options: [:]).perform([request])
                let result = (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }.joined(separator: "\n")
                DispatchQueue.main.async { [weak self] in
                    guard let self = self, self.generation == generation, self.panel != nil else { return }
                    self.request = nil; self.work = nil
                    self.text.string = result
                    self.message.stringValue = result.isEmpty ? L("No text found") : L("Select text to copy, or copy all")
                    self.copyButton.isEnabled = !result.isEmpty
                }
            } catch {
                DispatchQueue.main.async { [weak self] in
                    guard let self = self, self.generation == generation, self.panel != nil else { return }
                    self.request = nil; self.work = nil
                    self.message.stringValue = L("Text recognition failed")
                    self.text.string = error.localizedDescription
                }
            }
        }
        self.work = work
        Self.queue.async(execute: work)
    }
    @objc private func copyAll() {
        guard !text.string.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text.string, forType: .string)
    }
    func windowWillClose(_ notification: Notification) { cancel() }
    func cancel() {
        generation += 1
        request?.cancel(); request = nil
        work?.cancel(); work = nil
        if let panel = panel {
            self.panel = nil
            panel.delegate = nil
            panel.parent?.removeChildWindow(panel)
            panel.close()
        }
        text.string = ""
    }
    deinit { request?.cancel(); work?.cancel() }
}

// Let non-text hit tests reach the image's existing pan gesture.
@available(macOS 13.0, *)
private final class LiveTextSurface: NSView {
    let overlay = ImageAnalysisOverlayView(frame: .zero)
    override func hitTest(_ point: NSPoint) -> NSView? {
        if NSEvent.modifierFlags.contains(.option) { return nil }
        let local = convert(point, from: superview)
        guard bounds.contains(local) else { return nil }
        let target = overlay.convert(local, from: self)
        guard overlay.hasText(at: target) || overlay.hasInteractiveItem(at: target) || overlay.hasSupplementaryInterface(at: target) else { return nil }
        return super.hitTest(point)
    }
}

@available(macOS 13.0, *)
final class LiveImageText: NSObject, ImageAnalysisOverlayViewDelegate {
    private let surface = LiveTextSurface()
    private let analyzer = ImageAnalyzer()
    private var task: Task<Void, Never>?
    private var generation = 0
    private weak var imageView: NSImageView?

    init(imageView: NSImageView) {
        self.imageView = imageView
        super.init()
        surface.frame = imageView.bounds
        surface.autoresizingMask = [.width, .height]
        let overlay = surface.overlay
        overlay.frame = surface.bounds
        overlay.autoresizingMask = [.width, .height]
        overlay.delegate = self
        overlay.trackingImageView = imageView
        overlay.preferredInteractionTypes = .textSelection
        // Keep text selection available without the persistent Live Text badge.
        overlay.isSupplementaryInterfaceHidden = true
        surface.addSubview(overlay)
        imageView.addSubview(surface)
    }
    var hasSelection: Bool { surface.overlay.hasActiveTextSelection }
    func copySelection() -> Bool {
        guard hasSelection else { return false }
        if #available(macOS 14.0, *) {
            let value = surface.overlay.selectedText
            if !value.isEmpty {
                NSPasteboard.general.clearContents(); NSPasteboard.general.setString(value, forType: .string)
                return true
            }
        }
        NSApp.sendAction(#selector(NSText.copy(_:)), to: nil, from: surface.overlay)
        return true
    }
    func analyze(_ image: CGImage) {
        cancel()
        guard ImageAnalyzer.isSupported else { return }
        let generation = generation
        let analyzer = analyzer
        task = Task { @MainActor [weak self] in
            do {
                let analysis = try await analyzer.analyze(image, orientation: .up, configuration: ImageAnalyzer.Configuration(.text))
                guard !Task.isCancelled, let self = self, self.generation == generation else { return }
                self.surface.overlay.analysis = analysis
                self.surface.overlay.setContentsRectNeedsUpdate()
                self.task = nil
            } catch {
                // Extraction remains available when Live Text is unavailable.
            }
        }
    }
    func cancel() {
        generation += 1
        task?.cancel(); task = nil
        surface.overlay.analysis = nil
        surface.overlay.resetSelection()
    }
    func contentView(for overlayView: ImageAnalysisOverlayView) -> NSView? { imageView }
    func contentsRect(for overlayView: ImageAnalysisOverlayView) -> CGRect { surface.bounds }
    deinit { task?.cancel() }
}
