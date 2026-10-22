import Cocoa
import WebKit
import Quartz

// Preserve Word layout by default; reflow is an explicit reading option.
final class WordPreview: NSObject, WKNavigationDelegate {
    let view = NSView()
    private let body = NSView()
    private let web: WKWebView
    private let source: URL
    private let reading: URL?
    private var readingFailed = false
    private var readingMode = false
    private let mode = NSButton(title: L("Reading mode"), target: nil, action: nil)
    private let zoomOut = NSButton(title: "−", target: nil, action: nil)
    private let zoomIn = NSButton(title: "+", target: nil, action: nil)
    private let reset = NSButton(title: L("Reset zoom"), target: nil, action: nil)
    private let zoomLabel = NSTextField(labelWithString: "100%")
    private let loading = NSTextField(labelWithString: L("Loading preview…"))
    private var original: QLPreviewView?
    private var percent = 100
    private var loaded = false
    private var closed = false
    private var copyMonitor: Any?
    private var copyGeneration = 0
    init(source: URL, imagePages: Bool = false, reading: URL?, extraControls: [NSView]) {
        self.source = source; self.reading = reading
        self.readingMode = imagePages && reading != nil
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = false
        web = WKWebView(frame: .zero, configuration: configuration)
        super.init()
        body.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(body)
        body.wantsLayer = true; body.layer?.masksToBounds = true
        web.navigationDelegate = self
        web.translatesAutoresizingMaskIntoConstraints = false; body.addSubview(web)
        loading.textColor = .secondaryLabelColor; loading.maximumNumberOfLines = 3
        loading.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(loading)
        NSLayoutConstraint.activate([
            body.leadingAnchor.constraint(equalTo: view.leadingAnchor), body.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            body.topAnchor.constraint(equalTo: view.topAnchor), body.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            web.leadingAnchor.constraint(equalTo: body.leadingAnchor), web.trailingAnchor.constraint(equalTo: body.trailingAnchor),
            web.topAnchor.constraint(equalTo: body.topAnchor), web.bottomAnchor.constraint(equalTo: body.bottomAnchor),
            loading.centerXAnchor.constraint(equalTo: body.centerXAnchor), loading.centerYAnchor.constraint(equalTo: body.centerYAnchor),
            loading.widthAnchor.constraint(lessThanOrEqualTo: body.widthAnchor, constant: -48)
        ])
        mode.target = self; mode.action = #selector(toggleMode)
        mode.isEnabled = reading != nil
        zoomOut.target = self; zoomOut.action = #selector(decreaseZoom)
        zoomIn.target = self; zoomIn.action = #selector(increaseZoom)
        reset.target = self; reset.action = #selector(resetZoom)
        zoomLabel.alignment = .center; zoomLabel.font = .monospacedDigitSystemFont(ofSize: 13, weight: .medium)
        zoomLabel.widthAnchor.constraint(greaterThanOrEqualToConstant: 60).isActive = true
        let controls = ReaderControls([
            ReaderControl("word-mode", mode),
            ReaderControl("zoom-out", zoomOut), ReaderControl("zoom-status", zoomLabel),
            ReaderControl("zoom-in", zoomIn), ReaderControl("reset-zoom", reset)
        ] + extraControls.enumerated().map { ReaderControl("document-action-\($0.offset)", $0.element) })
        FloatingReaderToolbar(controls: controls).install(in: view)
        if let reading = reading { web.loadFileURL(reading, allowingReadAccessTo: reading.deletingLastPathComponent()) }
        showMode()
        copyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self = self, !self.closed, !self.web.isHiddenOrHasHiddenAncestor,
                  event.window === self.view.window,
                  let responder = event.window?.firstResponder as? NSView,
                  responder === self.web || responder.isDescendant(of: self.web),
                  event.charactersIgnoringModifiers?.lowercased() == "c" else { return event }
            let modifiers = event.modifierFlags.intersection([.command, .control, .option, .shift])
            guard modifiers == .command || modifiers == .control else { return event }
            self.copyGeneration += 1
            let generation = self.copyGeneration
            let changeCount = NSPasteboard.general.changeCount
            self.web.evaluateJavaScript("window.getSelection()?.toString() || ''") { [weak self] value, _ in
                guard let self = self, !self.closed, self.copyGeneration == generation,
                      !self.web.isHiddenOrHasHiddenAncestor,
                      NSPasteboard.general.changeCount == changeCount,
                      let text = value as? String, !text.isEmpty else { return }
                NSPasteboard.general.clearContents(); NSPasteboard.general.setString(text, forType: .string)
            }
            return nil
        }
    }
    private func showMode() {
        guard !closed else { return }
        let showingReading = readingMode && reading != nil && !readingFailed
        mode.title = L(showingReading ? "Original layout" : "Reading mode")
        mode.isEnabled = reading != nil && !readingFailed
        web.isHidden = !showingReading
        loading.isHidden = !showingReading || loaded
        original?.isHidden = showingReading
        [zoomOut, zoomLabel, zoomIn, reset].forEach { $0.isHidden = !showingReading }
        if !showingReading && original == nil {
            guard let preview = QLPreviewView(frame: .zero, style: .normal) else {
                loading.stringValue = L("Document preview is unavailable"); loading.isHidden = false; return
            }
            preview.translatesAutoresizingMaskIntoConstraints = false; preview.autostarts = true
            preview.shouldCloseWithWindow = false; preview.wantsLayer = true
            preview.layer?.cornerRadius = 0; preview.layer?.masksToBounds = true
            body.addSubview(preview)
            NSLayoutConstraint.activate([
                preview.leadingAnchor.constraint(equalTo: body.leadingAnchor), preview.trailingAnchor.constraint(equalTo: body.trailingAnchor),
                preview.topAnchor.constraint(equalTo: body.topAnchor), preview.bottomAnchor.constraint(equalTo: body.bottomAnchor)
            ])
            view.layoutSubtreeIfNeeded(); body.layoutSubtreeIfNeeded()
            preview.previewItem = source as NSURL; original = preview
        }
        updateZoom()
    }
    @objc private func toggleMode() { readingMode.toggle(); showMode() }
    @objc private func increaseZoom() { percent = min(ReaderLimits.WORD_ZOOM_MAX_PERCENT, percent + ReaderLimits.WORD_ZOOM_STEP_PERCENT); updateZoom() }
    @objc private func decreaseZoom() { percent = max(ReaderLimits.WORD_ZOOM_MIN_PERCENT, percent - ReaderLimits.WORD_ZOOM_STEP_PERCENT); updateZoom() }
    @objc private func resetZoom() { percent = 100; updateZoom() }
    private func updateZoom() {
        web.pageZoom = Double(percent) / 100
        zoomLabel.stringValue = "\(percent)%"
        zoomOut.isEnabled = percent > ReaderLimits.WORD_ZOOM_MIN_PERCENT
        zoomIn.isEnabled = percent < ReaderLimits.WORD_ZOOM_MAX_PERCENT
        reset.isEnabled = percent != 100
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        guard !closed else { return }; loaded = true; showMode()
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { failed() }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { failed() }
    private func failed() {
        guard !closed else { return }
        readingFailed = true; showMode()
    }
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        guard let url = action.request.url else { decisionHandler(.cancel); return }
        if action.navigationType == .linkActivated {
            if url.isFileURL && url.standardizedFileURL.path == reading?.standardizedFileURL.path {
                decisionHandler(.allow); return
            }
            if ["http", "https", "mailto", "file"].contains(url.scheme?.lowercased() ?? "") { NSWorkspace.shared.open(url) }
            decisionHandler(.cancel); return
        }
        let allowed = url.isFileURL && url.standardizedFileURL.path == reading?.standardizedFileURL.path
        decisionHandler(allowed ? .allow : .cancel)
    }
    func copyText(_ completion: @escaping (String) -> Void) {
        guard readingMode && reading != nil && !readingFailed else { completion(PreviewCopy.text(in: body)); return }
        web.evaluateJavaScript("window.getSelection()?.toString() || document.body.innerText") { value, _ in completion(value as? String ?? "") }
    }
    func recognizeVisiblePage(_ completion: @escaping (CGImage?) -> Void) {
        guard readingMode && reading != nil && !readingFailed else { completion(nil); return }
        let configuration = WKSnapshotConfiguration()
        configuration.rect = web.bounds
        configuration.snapshotWidth = NSNumber(value: min(Double(ReaderLimits.IMAGE_PREVIEW_EDGE), Double(web.bounds.width)))
        web.takeSnapshot(with: configuration) { image, _ in
            completion(image?.cgImage(forProposedRect: nil, context: nil, hints: nil))
        }
    }
    deinit { if let monitor = copyMonitor { NSEvent.removeMonitor(monitor) } }
    func close() {
        if let monitor = copyMonitor { NSEvent.removeMonitor(monitor); copyMonitor = nil }
        closed = true; web.navigationDelegate = nil; web.stopLoading()
        original?.previewItem = nil; original?.close(); original = nil
    }
}
