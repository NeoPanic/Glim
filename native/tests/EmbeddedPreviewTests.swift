import Cocoa
import PDFKit

@main
struct EmbeddedPreviewTests {
    static func pump() {
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.15))
    }
    static func create(_ path: URL) throws -> UnsafeMutableRawPointer {
        let data = try JSONSerialization.data(withJSONObject: ["files": [path.path], "shuffle": false])
        return data.withUnsafeBytes { glimPreviewCreate($0.bindMemory(to: UInt8.self).baseAddress!, data.count) }
    }
    static func controller(_ handle: UnsafeMutableRawPointer) -> NativePreviewController {
        Unmanaged<NativePreviewController>.fromOpaque(handle).takeUnretainedValue()
    }
    static func main() throws {
        _ = NSApplication.shared
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        // The shared capsule measures visible controls, even after a narrow layout.
        let short = NSButton(title: "SQL", target: nil, action: nil)
        let long = NSButton(title: "A deliberately long reader action label", target: nil, action: nil)
        let stack = NSStackView(views: [short, long])
        let capsule = FloatingReaderToolbar(items: stack)
        let fullWidth = capsule.intrinsicContentSize.width
        capsule.frame = NSRect(x: 0, y: 0, width: 180, height: 72)
        capsule.layoutSubtreeIfNeeded()
        precondition(long.frame.width > 180)
        long.isHidden = true
        let shortWidth = capsule.intrinsicContentSize.width
        precondition(shortWidth < fullWidth)
        short.title = "A much longer SQL action"
        precondition(capsule.intrinsicContentSize.width > shortWidth)
        long.isHidden = false
        precondition(capsule.intrinsicContentSize.width > fullWidth)

        let pdfURL = directory.appendingPathComponent("two pages.pdf")
        let document = PDFDocument()
        let image = NSImage(size: NSSize(width: 300, height: 400), flipped: false) { rect in
            NSColor.white.setFill(); rect.fill(); return true
        }
        document.insert(PDFPage(image: image)!, at: 0)
        document.insert(PDFPage(image: image)!, at: 1)
        precondition(document.write(to: pdfURL))

        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 900, height: 600),
            styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let gpui = NSView(frame: window.contentView!.bounds)
        window.contentView!.addSubview(gpui)
        window.setAccessibilityChildren([gpui])
        let parent = Unmanaged.passUnretained(gpui).toOpaque()
        let visibleWindows = NSApp.windows.filter { $0.isVisible }.count

        let pdf = try create(pdfURL)
        weak var pdfOwner = controller(pdf)
        glimPreviewLayout(pdf, parent, 20, 30, 600, 400)
        pump()
        precondition(controller(pdf).host.superview === gpui.superview)
        precondition(controller(pdf).host.frame == NSRect(x: 20, y: 170, width: 600, height: 400))
        precondition(controller(pdf).pdfView.document?.pageCount == 2)
        controller(pdf).next()
        precondition(controller(pdf).pdfView.document?.index(for: controller(pdf).pdfView.currentPage!) == 1)
        glimPreviewHide(pdf)
        precondition(controller(pdf).host.isHidden)
        precondition(window.accessibilityChildren()?.count == 1)
        precondition(window.accessibilityChildren()?.first as? NSView === gpui)
        glimPreviewLayout(pdf, parent, 40, 60, 700, 450)
        precondition(!controller(pdf).host.isHidden)
        precondition(window.accessibilityChildren()?.count == 2)
        precondition(controller(pdf).pdfView.document?.index(for: controller(pdf).pdfView.currentPage!) == 1)
        glimPreviewRelease(pdf)
        pump()
        precondition(pdfOwner == nil)
        precondition(window.contentView!.subviews.count == 1)
        precondition(window.accessibilityChildren()?.count == 1)

        // A tab closed before its queued load must neither create UI nor retain its owner.
        let pending = try create(pdfURL)
        weak var pendingOwner = controller(pending)
        glimPreviewRelease(pending)
        pump()
        precondition(pendingOwner == nil)

        // Loading before the first layout still autoplays once shown. Hiding
        // pauses without losing the item, and returning never restarts playback.
        let audioURL = directory.appendingPathComponent("audio.wav")
        var wave = Data()
        func bytes(_ text: String) { wave.append(contentsOf: text.utf8) }
        func little(_ value: UInt32, count: Int = 4) {
            for index in 0..<count { wave.append(UInt8((value >> (index * 8)) & 255)) }
        }
        bytes("RIFF"); little(160036); bytes("WAVEfmt "); little(16)
        little(1, count: 2); little(1, count: 2); little(8000); little(16000)
        little(2, count: 2); little(16, count: 2); bytes("data"); little(160000)
        wave.append(Data(repeating: 0, count: 160000)); try wave.write(to: audioURL)
        let audio = try create(audioURL)
        pump()
        precondition(controller(audio).pendingPlayback)
        precondition(controller(audio).host.isHidden)
        glimPreviewLayout(audio, parent, 0, 0, 600, 400)
        precondition(!controller(audio).pendingPlayback)
        pump()
        let item = controller(audio).player.currentItem
        precondition(item != nil)
        glimPreviewHide(audio)
        precondition(controller(audio).player.rate == 0)
        glimPreviewLayout(audio, parent, 0, 0, 700, 500)
        precondition(controller(audio).player.currentItem === item)
        precondition(controller(audio).player.rate == 0)
        controller(audio).host.layoutSubtreeIfNeeded()
        precondition(abs(controller(audio).playerView.frame.width - 700) < 1)
        weak var audioOwner = controller(audio)
        glimPreviewRelease(audio)
        pump()
        precondition(audioOwner == nil)
        precondition(window.accessibilityChildren()?.count == 1)

        // Update session restoration must not start media in the background.
        let restoredData = try JSONSerialization.data(withJSONObject: ["files": [audioURL.path], "shuffle": false, "autoplay": false])
        let restoredAudio = restoredData.withUnsafeBytes { glimPreviewCreate($0.bindMemory(to: UInt8.self).baseAddress!, restoredData.count) }
        glimPreviewLayout(restoredAudio, parent, 0, 0, 700, 500)
        pump()
        precondition(controller(restoredAudio).player.currentItem != nil)
        precondition(controller(restoredAudio).player.rate == 0)
        precondition(!controller(restoredAudio).pendingPlayback)
        glimPreviewRelease(restoredAudio)

        // A failed preview stays inside the same tab instead of terminating the app.
        let invalidURL = directory.appendingPathComponent("invalid.pdf")
        try Data("invalid".utf8).write(to: invalidURL)
        let invalid = try create(invalidURL)
        glimPreviewLayout(invalid, parent, 0, 0, 600, 400)
        pump()
        precondition(controller(invalid).host.subviews.contains { $0 is NSTextField })
        glimPreviewRelease(invalid)
        precondition(NSApp.windows.filter { $0.isVisible }.count == visibleWindows)
        print("Embedded preview: layout, page retention, accessibility, playback lifecycle, release, cancelled load and error isolation passed")
    }
}
