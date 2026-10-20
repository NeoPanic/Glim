import Cocoa
import WebKit
import UniformTypeIdentifiers

// Serve the current editor snapshot at the document's URL, with streamed local
// resources. No temporary HTML files are written into the user's project.
private final class HTMLResources: NSObject, WKURLSchemeHandler {
    let document: URL
    private let directory: URL
    private let contents: Data?
    // Accessed only by the serial resource worker.
    private var readDirectory: URL?
    private let worker = DispatchQueue(label: "glim.html.resources", qos: .userInitiated)
    private var active: [ObjectIdentifier: WKURLSchemeTask] = [:]
    init(document: URL, contents: String?) {
        self.document = document
        directory = document.deletingLastPathComponent()
        self.contents = contents.map { Data($0.utf8) }
    }
    private func resourceDirectory() -> URL {
        if let directory = readDirectory { return directory }
        var candidate = directory.standardizedFileURL
        while candidate.path != "/" {
            // Worktrees use a .git file; ordinary checkouts use a directory.
            // Resolve this on the resource worker, never during native layout.
            if FileManager.default.fileExists(atPath: candidate.appendingPathComponent(".git").path) {
                let root = candidate.resolvingSymlinksInPath().standardizedFileURL
                readDirectory = root; return root
            }
            candidate.deleteLastPathComponent()
        }
        let root = directory.resolvingSymlinksInPath().standardizedFileURL
        readDirectory = root; return root
    }
    func webView(_ webView: WKWebView, start task: WKURLSchemeTask) {
        let id = ObjectIdentifier(task)
        active[id] = task
        let requestedURL = task.request.url
        worker.async { [weak self] in
            guard let self = self else { return }
            do {
                guard let url = requestedURL else { throw URLError(.badURL) }
                let path = URL(fileURLWithPath: url.path).standardizedFileURL
                let mainDocument = path.path == self.document.standardizedFileURL.path
                let file: FileHandle?
                let length: Int
                if mainDocument, let contents = self.contents {
                    file = nil; length = contents.count
                } else {
                    let resolved = path.resolvingSymlinksInPath()
                    let root = self.resourceDirectory().path
                    guard mainDocument || resolved.path.hasPrefix(root.hasSuffix("/") ? root : root + "/") else {
                        throw URLError(.noPermissionsToReadFile)
                    }
                    file = try FileHandle(forReadingFrom: resolved)
                    length = Int(clamping: try file!.seekToEnd())
                    try file!.seek(toOffset: 0)
                }
                defer { try? file?.close() }
                let mime = mainDocument ? "text/html" : (UTType(filenameExtension: path.pathExtension)?.preferredMIMEType ?? "application/octet-stream")
                let response = URLResponse(url: url, mimeType: mime, expectedContentLength: length,
                    textEncodingName: mime.hasPrefix("text/") ? "utf-8" : nil)
                var offset = 0
                let started = DispatchQueue.main.sync { () -> Bool in
                    guard let task = self.active[id] else { return false }
                    task.didReceive(response); return true
                }
                guard started else { return }
                while true {
                    let running = DispatchQueue.main.sync { self.active[id] != nil }
                    guard running else { return }
                    let data: Data
                    if let file = file {
                        data = try file.read(upToCount: ReaderLimits.IO_BUFFER_BYTES) ?? Data()
                    } else {
                        let contents = self.contents ?? Data()
                        let end = min(contents.count, offset + ReaderLimits.IO_BUFFER_BYTES)
                        data = contents.subdata(in: offset..<end); offset = end
                    }
                    if data.isEmpty { break }
                    // Deliver one chunk at a time so queued callbacks cannot retain
                    // a whole large resource. Cancellation is checked between chunks.
                    DispatchQueue.main.sync { self.active[id]?.didReceive(data) }
                }
                DispatchQueue.main.sync { self.active.removeValue(forKey: id)?.didFinish() }
            } catch {
                DispatchQueue.main.sync { self.active.removeValue(forKey: id)?.didFailWithError(error) }
            }
        }
    }
    func webView(_ webView: WKWebView, stop task: WKURLSchemeTask) {
        active.removeValue(forKey: ObjectIdentifier(task))
    }
    func close() { active.removeAll() }
}

final class HTMLPreview: NSObject, WKNavigationDelegate {
    let view = NSView()
    private let web: WKWebView
    private let resources: HTMLResources
    private let document: URL
    private let documentURL: URL
    private let loading = NSTextField(labelWithString: L("Loading preview…"))
    private var loaded = false
    private var anchor: String?
    var pendingNavigation: String?
    var didLoad: (() -> Void)?
    init(document: URL, contents: String?) {
        self.document = document.standardizedFileURL
        var components = URLComponents()
        components.scheme = "glim-html"; components.host = "local"; components.path = document.path
        documentURL = components.url!
        resources = HTMLResources(document: document, contents: contents)
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = true
        configuration.setURLSchemeHandler(resources, forURLScheme: "glim-html")
        web = WKWebView(frame: .zero, configuration: configuration)
        super.init()
        web.navigationDelegate = self
        web.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(web)
        loading.translatesAutoresizingMaskIntoConstraints = false
        loading.textColor = .secondaryLabelColor; view.addSubview(loading)
        NSLayoutConstraint.activate([
            web.leadingAnchor.constraint(equalTo: view.leadingAnchor), web.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            web.topAnchor.constraint(equalTo: view.topAnchor), web.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            loading.centerXAnchor.constraint(equalTo: view.centerXAnchor), loading.centerYAnchor.constraint(equalTo: view.centerYAnchor)
        ])
        web.load(URLRequest(url: documentURL))
    }
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        guard let url = action.request.url else { decisionHandler(.cancel); return }
        if action.navigationType == .linkActivated || action.targetFrame == nil {
            if url.scheme == "glim-html", url.path == document.path, let fragment = URLComponents(url: url, resolvingAgainstBaseURL: false)?.fragment {
                reveal(fragment); decisionHandler(.cancel); return
            }
            if url.scheme == "glim-html" {
                var file = URLComponents(url: URL(fileURLWithPath: url.path), resolvingAgainstBaseURL: false)!
                file.fragment = URLComponents(url: url, resolvingAgainstBaseURL: false)?.fragment
                pendingNavigation = file.url?.absoluteString
            } else if ["file", "http", "https", "mailto"].contains(url.scheme ?? "") {
                pendingNavigation = url.absoluteString
            }
            decisionHandler(.cancel); return
        }
        // Keep the preview tied to its source document; ordinary document links
        // are dispatched to Glim rather than silently replacing the editor's page.
        decisionHandler(action.targetFrame?.isMainFrame == false || url.path == documentURL.path ? .allow : .cancel)
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        loaded = true; loading.isHidden = true
        didLoad?()
        if let anchor = anchor { reveal(anchor) }
    }
    private func failed(_ error: Error) {
        guard (error as NSError).code != NSURLErrorCancelled else { return }
        loading.stringValue = error.localizedDescription; loading.isHidden = false
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { failed(error) }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { failed(error) }
    func reveal(_ anchor: String) {
        self.anchor = anchor
        guard loaded, let data = try? JSONSerialization.data(withJSONObject: [anchor]), let argument = String(data: data, encoding: .utf8) else { return }
        web.evaluateJavaScript("(()=>{const id=\(argument)[0];const target=document.getElementById(id)||document.getElementsByName(id)[0];if(target)target.scrollIntoView();else if(!id)window.scrollTo(0,0);})()", completionHandler: nil)
    }
    // Commands are provided by Glim's static image wrapper, never by user SVG.
    func imageCommand(_ command: String, completion: @escaping (Double?) -> Void) {
        guard loaded, let data = try? JSONSerialization.data(withJSONObject: [command]),
              let argument = String(data: data, encoding: .utf8) else { completion(nil); return }
        web.evaluateJavaScript("window.glimImageViewer?.command(\(argument)[0])") { result, _ in
            completion((result as? NSNumber)?.doubleValue)
        }
    }
    // Snapshot the rendered SVG image, excluding the viewer's controls and margins.
    func copyableImage(completion: @escaping (NSImage?) -> Void) {
        guard loaded else { completion(nil); return }
        web.evaluateJavaScript("(()=>{const r=document.getElementById('image')?.getBoundingClientRect();return r ? [r.x,r.y,r.width,r.height] : null})()") { [weak self] value, _ in
            guard let self = self, let values = value as? [Double], values.count == 4 else { completion(nil); return }
            let rect = NSRect(x: values[0], y: values[1], width: values[2], height: values[3]).intersection(self.web.bounds)
            guard !rect.isEmpty else { completion(nil); return }
            let configuration = WKSnapshotConfiguration()
            configuration.rect = rect
            self.web.takeSnapshot(with: configuration) { image, _ in completion(image) }
        }
    }
    func close() {
        web.stopLoading(); web.navigationDelegate = nil
        resources.close(); web.removeFromSuperview()
    }
}
