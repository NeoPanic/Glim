import Cocoa
import Quartz
import ImageIO
import UniformTypeIdentifiers

// Read only the currently exposed preview, without scraping application chrome.
final class PreviewCopy: NSObject {
    private weak var root: NSView?
    private let source: URL
    private let recognition = ImageTextExtraction()
    private var images: OfficeImageStream?
    private let queue = DispatchQueue(label: "glim.office.images", qos: .userInitiated)
    private var generation = 0
    private let nextImage = NSButton(title: L("Next image"), target: nil, action: nil)
    private let recognize = NSButton(title: L("Recognize Image Text"), target: nil, action: nil)
    var textProvider: ((@escaping (String) -> Void) -> Void)?
    var rasterProvider: ((@escaping (CGImage?) -> Void) -> Void)?
    init(root: NSView, source: URL) {
        self.root = root; self.source = source
        super.init()
        nextImage.target = self; nextImage.action = #selector(loadNextImage)
        recognize.target = self; recognize.action = #selector(recognizeImages)
    }
    func controls(includeRecognition: Bool = true) -> [NSView] {
        let copy = NSButton(title: L("Copy Preview Text"), target: self, action: #selector(copyText))
        return includeRecognition ? [copy, recognize] : [copy]
    }
    static func text(in root: NSView) -> String {
        var pending: [Any] = [root]
        var visited = Set<ObjectIdentifier>()
        var output: [String] = []
        while let value = pending.popLast() {
            if value is FloatingReaderToolbar { continue }
            guard let element = value as? NSAccessibilityProtocol,
                  visited.insert(ObjectIdentifier(element as AnyObject)).inserted else { continue }
            let role = element.accessibilityRole()
            if role == .button || role == .popUpButton || role == .menu || role == .scrollBar { continue }
            if let selected = element.accessibilitySelectedText(), !selected.isEmpty { return selected }
            if role == .staticText || role == .textArea || role == .textField || role == .cell {
                let value = element.accessibilityValue()
                let text = (value as? String) ?? (value as? NSAttributedString)?.string ?? (value as? NSNumber)?.stringValue
                if let text = text, !text.isEmpty { output.append(text); continue }
            }
            pending.append(contentsOf: (element.accessibilityChildren() ?? []).reversed())
        }
        return output.joined(separator: "\n")
    }
    @objc private func copyText() {
        let copy: (String) -> Void = { [weak self] value in
            guard let self = self, self.root?.window != nil else { return }
            if value.isEmpty { self.notice(L("No copyable text was found. Try image text recognition.")); return }
            NSPasteboard.general.clearContents(); NSPasteboard.general.setString(value, forType: .string)
        }
        if let textProvider = textProvider { textProvider(copy) }
        else if let root = root { copy(Self.text(in: root)) }
    }
    @objc private func recognizeImages() {
        guard let parent = root?.window else { return }
        generation += 1
        recognition.cancel()
        if let rasterProvider = rasterProvider {
            let generation = generation
            recognize.isEnabled = false
            rasterProvider { [weak self, weak parent] image in
                guard let self = self, let parent = parent, self.generation == generation else { return }
                self.recognize.isEnabled = true
                if let image = image { self.recognition.extract(image, in: parent) }
                else { self.notice(L("Image text recognition is unavailable for this preview.")) }
            }
            return
        }
        images?.cancel()
        images = OfficeImageStream(source: source)
        loadNextImage()
    }
    @objc private func loadNextImage() {
        guard let parent = root?.window, let images = images else { return }
        let generation = generation
        nextImage.isEnabled = false; recognize.isEnabled = false
        queue.async { [weak self, weak parent] in
            do {
                let found = try images.next()
                DispatchQueue.main.async { [weak self, weak parent] in
                    guard let self = self, let parent = parent, self.generation == generation else { return }
                    self.recognize.isEnabled = true
                    guard let (name, image) = found else {
                        self.notice(L("No more supported embedded images.")); return
                    }
                    self.nextImage.isEnabled = true
                    let label = NSTextField(labelWithString: name)
                    label.lineBreakMode = .byTruncatingMiddle
                    label.widthAnchor.constraint(lessThanOrEqualToConstant: 180).isActive = true
                    self.recognition.extract(image, in: parent, extraControls: [label, self.nextImage])
                }
            } catch {
                DispatchQueue.main.async { [weak self] in
                    guard let self = self, self.generation == generation else { return }
                    self.recognize.isEnabled = true; self.notice(error.localizedDescription)
                }
            }
        }
    }
    private func notice(_ message: String) {
        guard let parent = root?.window else { return }
        let alert = NSAlert(); alert.messageText = message
        alert.beginSheetModal(for: parent)
    }
    func cancel() {
        generation += 1; images?.cancel(); images = nil
        recognition.cancel(); recognize.isEnabled = true
    }
    deinit { images?.cancel() }
}

// Stream package names and extract only the requested image to a private file.
// No whole-package expansion, complete image array or total-file cutoff.
private final class OfficeImageStream {
    private let source: URL
    private var directory: URL?
    private var catalog: FileHandle?
    private var buffer = Data()
    private var enumerator: FileManager.DirectoryEnumerator?
    private var started = false
    private let lock = NSLock()
    private var stopped = false
    private var process: Process?
    init(source: URL) { self.source = source }
    func cancel() {
        lock.lock(); stopped = true
        if process?.isRunning == true { process?.terminate() }
        lock.unlock()
    }
    private func check() throws {
        lock.lock(); let stopped = stopped; lock.unlock()
        if stopped { throw CocoaError(.userCancelled) }
    }
    private func run(_ arguments: [String], output: URL) throws {
        try check()
        FileManager.default.createFile(atPath: output.path, contents: nil)
        let file = try FileHandle(forWritingTo: output); defer { try? file.close() }
        let task = Process(); task.executableURL = URL(fileURLWithPath: "/usr/bin/unzip")
        task.arguments = arguments; task.standardInput = FileHandle.nullDevice
        task.standardOutput = file; task.standardError = FileHandle.nullDevice
        lock.lock()
        if stopped { lock.unlock(); throw CocoaError(.userCancelled) }
        process = task
        do { try task.run(); lock.unlock() } catch { process = nil; lock.unlock(); throw error }
        task.waitUntilExit()
        lock.lock(); process = nil; lock.unlock()
        try check()
        guard task.terminationStatus == 0 else {
            throw NSError(domain: "Glim", code: 1, userInfo: [NSLocalizedDescriptionKey: L("Embedded image recognition requires an Office or iWork document package.")])
        }
    }
    private func prepare() throws {
        guard !started else { return }
        started = true
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("glim-office-images-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        self.directory = directory
        let values = try source.resourceValues(forKeys: [.isDirectoryKey])
        if values.isDirectory == true {
            enumerator = FileManager.default.enumerator(at: source, includingPropertiesForKeys: [.isRegularFileKey, .isSymbolicLinkKey], options: [.skipsHiddenFiles])
        } else {
            let listing = directory.appendingPathComponent("members.txt")
            try run(["-Z1", source.path], output: listing)
            catalog = try FileHandle(forReadingFrom: listing)
        }
    }
    private func member() throws -> String? {
        guard let catalog = catalog else { return nil }
        while true {
            try check()
            if let end = buffer.firstIndex(of: 10) {
                let line = buffer[..<end]; let name = String(decoding: line, as: UTF8.self)
                buffer.removeSubrange(...end)
                return name
            }
            let data = try catalog.read(upToCount: ReaderLimits.IO_BUFFER_BYTES) ?? Data()
            if data.isEmpty {
                if buffer.isEmpty { return nil }
                let name = String(decoding: buffer, as: UTF8.self); buffer.removeAll(); return name
            }
            buffer.append(data)
        }
    }
    func next() throws -> (String, CGImage)? {
        try prepare()
        while true {
            try check()
            let url: URL
            let name: String
            if let enumerator = enumerator {
                guard let candidate = enumerator.nextObject() as? URL else { return nil }
                let values = try candidate.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
                guard values.isRegularFile == true, values.isSymbolicLink != true,
                      UTType(filenameExtension: candidate.pathExtension)?.conforms(to: .image) == true else { continue }
                url = candidate; name = candidate.lastPathComponent
            } else {
                guard let member = try member(), let directory = directory else { return nil }
                let prefix = member.lowercased()
                guard ["word/media/", "ppt/media/", "xl/media/", "data/"].contains(where: { prefix.hasPrefix($0) }),
                      UTType(filenameExtension: (member as NSString).pathExtension)?.conforms(to: .image) == true else { continue }
                name = (member as NSString).lastPathComponent
                url = directory.appendingPathComponent("current-image")
                let pattern = member.reduce(into: "") { value, character in
                    if "\\*?[]".contains(character) { value.append("\\") }; value.append(character)
                }
                try run(["-p", source.path, pattern], output: url)
            }
            let edge = min(Double(ReaderLimits.IMAGE_PREVIEW_EDGE), floor(sqrt(Double(ReaderLimits.IMAGE_DECODED_BYTES) / 4)))
            if let image = autoreleasepool(invoking: { () -> CGImage? in
                guard let source = CGImageSourceCreateWithURL(url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary) else { return nil }
                return CGImageSourceCreateThumbnailAtIndex(source, 0, [kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true, kCGImageSourceThumbnailMaxPixelSize: edge, kCGImageSourceShouldCacheImmediately: true] as CFDictionary)
            }) { try check(); return (name, image) }
        }
    }
    deinit {
        cancel(); try? catalog?.close()
        if let directory = directory { try? FileManager.default.removeItem(at: directory) }
    }
}
