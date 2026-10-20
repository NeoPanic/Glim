import Cocoa
import WebKit
import Quartz

// Reflow is the default reading surface; Quick Look retains the original layout.
final class WordPreview: NSObject, WKNavigationDelegate {
    let view = NSView()
    private let body = NSView()
    private let web: WKWebView
    private let source: URL
    private let reading: URL?
    private var readingFailed = false
    private let zoomOut = NSButton(title: "−", target: nil, action: nil)
    private let zoomIn = NSButton(title: "+", target: nil, action: nil)
    private let reset = NSButton(title: L("Reset zoom"), target: nil, action: nil)
    private let zoomLabel = NSTextField(labelWithString: "100%")
    private let loading = NSTextField(labelWithString: L("Loading preview…"))
    private var original: QLPreviewView?
    private var percent = 100
    private var loaded = false
    private var closed = false
    init(source: URL, reading: URL?, extraControls: [NSView]) {
        self.source = source; self.reading = reading
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
        zoomOut.target = self; zoomOut.action = #selector(decreaseZoom)
        zoomIn.target = self; zoomIn.action = #selector(increaseZoom)
        reset.target = self; reset.action = #selector(resetZoom)
        zoomLabel.alignment = .center; zoomLabel.font = .monospacedDigitSystemFont(ofSize: 13, weight: .medium)
        zoomLabel.widthAnchor.constraint(greaterThanOrEqualToConstant: 60).isActive = true
        let controls = ReaderControls([
            ReaderControl("zoom-out", zoomOut), ReaderControl("zoom-status", zoomLabel),
            ReaderControl("zoom-in", zoomIn), ReaderControl("reset-zoom", reset)
        ] + extraControls.enumerated().map { ReaderControl("document-action-\($0.offset)", $0.element) })
        FloatingReaderToolbar(controls: controls).install(in: view)
        if let reading = reading { web.loadFileURL(reading, allowingReadAccessTo: reading.deletingLastPathComponent()) }
        showMode()
    }
    private func showMode() {
        guard !closed else { return }
        let showingReading = reading != nil && !readingFailed
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
        guard !closed else { return }; loaded = true; loading.isHidden = true; updateZoom()
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
        guard reading != nil && !readingFailed else { completion(PreviewCopy.text(in: body)); return }
        web.evaluateJavaScript("window.getSelection()?.toString() || document.body.innerText") { value, _ in completion(value as? String ?? "") }
    }
    func recognizeVisiblePage(_ completion: @escaping (CGImage?) -> Void) {
        guard reading != nil && !readingFailed else { completion(nil); return }
        let configuration = WKSnapshotConfiguration()
        configuration.rect = web.bounds
        configuration.snapshotWidth = NSNumber(value: min(Double(ReaderLimits.IMAGE_PREVIEW_EDGE), Double(web.bounds.width)))
        web.takeSnapshot(with: configuration) { image, _ in
            completion(image?.cgImage(forProposedRect: nil, context: nil, hints: nil))
        }
    }
    func close() {
        closed = true; web.navigationDelegate = nil; web.stopLoading()
        original?.previewItem = nil; original?.close(); original = nil
    }
}
