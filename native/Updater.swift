import Cocoa
import Sparkle
import ObjectiveC.runtime

// One main-thread controller for the whole application, including all workspaces.
final class GlimUpdater: NSObject, SPUUserDriver, SPUUpdaterDelegate {
    var updater: SPUUpdater?
    var started = false
    var configured = false
    var phase = "disabled"
    var message = "Updates are not configured for this build."
    var lastError: NSError?
    var item: SUAppcastItem?
    var received: UInt64 = 0
    var expected: UInt64 = 0
    var progressKnown = false
    var attention: UInt64 = 0
    var installOnQuit = false
    var needsApproval = false
    var approved = false
    var quitApproved = false
    var exitPermitted = false
    var needsQuitApproval = false
    var suppressTermination = false
    private static weak var quitGuard: GlimUpdater?
    private static var guardedClass: AnyClass?
    var pendingCancel = false
    var choice: ((SPUUserUpdateChoice) -> Void)?
    var choiceInstalls = false
    var cancel: (() -> Void)?
    var immediateInstall: (() -> Void)?
    var delayedInstall: (() -> Void)?
    let host: Bundle

    init(host: Bundle = .main) {
        self.host = host
        super.init()
        guard Self.validIdentity(host.bundleIdentifier),
              host.object(forInfoDictionaryKey: "GlimUpdatesEnabled") as? Bool == true,
              host.object(forInfoDictionaryKey: "SURequireSignedFeed") as? Bool == true,
              host.object(forInfoDictionaryKey: "SUEnableSystemProfiling") as? Bool == false,
              host.object(forInfoDictionaryKey: "SUVerifyUpdateBeforeExtraction") as? Bool == true,
              host.object(forInfoDictionaryKey: "SUSignedFeedFailureExpirationInterval") as? Int == 0,
              let key = host.object(forInfoDictionaryKey: "SUPublicEDKey") as? String,
              Data(base64Encoded: key)?.count == 32,
              let feed = host.object(forInfoDictionaryKey: "SUFeedURL") as? String,
              Self.validFeed(feed),
              host.bundleURL.pathExtension == "app" else { return }
        let path = host.bundleURL.resolvingSymlinksInPath().path
        guard !path.hasPrefix("/Volumes/"), !path.contains("/AppTranslocation/") else {
            message = "Install Glim in Applications before updating."
            return
        }
        guard installTerminationGuard() else {
            message = "This build cannot safely coordinate application shutdown for updates."
            return
        }
        configured = true
        phase = "preparing"
        message = "Preparing a recovery backup…"
        updater = SPUUpdater(hostBundle: host, applicationBundle: host, userDriver: self, delegate: self)
    }
    // GPUI owns the NSApplication delegate. Add only the missing cancellation
    // hook, without replacing the delegate or bypassing its lifecycle methods.
    // Fail closed if a future GPUI version supplies a different quit contract.
    private func installTerminationGuard() -> Bool {
        guard let delegate = NSApp.delegate, let type = object_getClass(delegate) else { return false }
        if let existing = Self.guardedClass, existing === type {
            Self.quitGuard = self
            return true
        }
        #if !GLIM_UPDATER_TESTING
        guard NSStringFromClass(type) == "GPUIApplicationDelegate" else { return false }
        #endif
        let selector = NSSelectorFromString("applicationShouldTerminate:")
        guard !delegate.responds(to: selector) else { return false }
        let block: @convention(block) (AnyObject, NSApplication) -> UInt = { _, _ in
            Self.quitGuard?.terminationReply().rawValue ?? NSApplication.TerminateReply.terminateNow.rawValue
        }
        let implementation = imp_implementationWithBlock(block)
        guard class_addMethod(type, selector, implementation, "Q@:@") else {
            imp_removeBlock(implementation)
            return false
        }
        Self.guardedClass = type
        Self.quitGuard = self
        // Refresh AppKit's optional-delegate-method cache after adding the hook.
        NSApp.delegate = delegate
        return true
    }
    func terminationReply() -> NSApplication.TerminateReply {
        if suppressTermination { suppressTermination = false; return .terminateCancel }
        if approved || quitApproved { return .terminateNow }
        if installOnQuit || ["downloading", "extracting", "ready", "installing"].contains(phase) {
            needsQuitApproval = true
            attention &+= 1
            return .terminateCancel
        }
        return .terminateNow
    }
    func validateExit() -> Bool {
        exitPermitted = !NSWorkspace.shared.runningApplications.contains {
            $0.processIdentifier != ProcessInfo.processInfo.processIdentifier &&
            $0.bundleURL?.resolvingSymlinksInPath() == host.bundleURL.resolvingSymlinksInPath()
        }
        if !exitPermitted { message = "Quit other running copies of Glim before updating." }
        return exitPermitted
    }
    static func validIdentity(_ identity: String?) -> Bool {
        #if GLIM_UPDATER_TESTING
        if identity?.hasPrefix("io.github.neopanic.glim.updater-test.") == true { return true }
        #endif
        return identity == "io.github.jony4.glimpse"
    }
    static func validFeed(_ url: String) -> Bool {
        let feeds = ["arm64", "x86_64"].map { "https://github.com/glimapp/glim/releases/latest/download/appcast-macos-\($0).xml" }
        #if GLIM_UPDATER_TESTING
        if let value = URL(string: url), value.host == "127.0.0.1", value.scheme == "http" { return true }
        #endif
        return feeds.contains(url)
    }
    func start() {
        guard configured, !started, let updater = updater else { return }
        do {
            try updater.start()
            started = true; phase = "idle"; message = ""
        } catch { phase = "error"; message = error.localizedDescription }
    }
    var mode: String {
        guard let updater = updater else { return "manual" }
        if !updater.automaticallyChecksForUpdates { return "manual" }
        return updater.automaticallyDownloadsUpdates ? "automatic" : "notify"
    }
    func snapshot() -> [String: Any] {
        let date = updater?.lastUpdateCheckDate.map { ISO8601DateFormatter().string(from: $0) } ?? ""
        return ["enabled": configured, "phase": phase, "mode": mode,
            "current_version": host.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "",
            "current_build": host.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "",
            "version": item?.displayVersionString ?? "", "build": item?.versionString ?? "",
            "notes": String((item?.itemDescription ?? "").prefix(65536)),
            "published": item?.dateString ?? "", "message": message,
            "received": received, "expected": expected, "progress_known": progressKnown,
            "can_cancel": cancel != nil || choice != nil || installOnQuit,
            "can_download": phase == "available" && choice != nil && !choiceInstalls && item?.isInformationOnlyUpdate != true,
            "can_install": ["ready", "installing"].contains(phase) && (choice != nil || immediateInstall != nil || delayedInstall != nil || installOnQuit),
            "install_on_quit": installOnQuit, "needs_install_approval": needsApproval,
            "exit_requested": approved || quitApproved, "exit_permitted": exitPermitted,
            "needs_quit_approval": needsQuitApproval, "attention": attention, "bundle_path": host.bundleURL.path, "last_check": date]
    }
    func resolve(_ value: SPUUserUpdateChoice) {
        let reply = choice; choice = nil
        choiceInstalls = false; cancel = nil
        reply?(value)
    }
    func command(_ command: String) {
        precondition(Thread.isMainThread)
        guard configured else { return }
        switch command {
        case "start": start()
        case "check":
            if !started { start() }
            guard started else { return }
            attention &+= 1
            updater?.checkForUpdates()
        case "manual", "notify", "automatic":
            guard started else { return }
            if command == "manual" {
                updater?.automaticallyDownloadsUpdates = false
                updater?.automaticallyChecksForUpdates = false
            } else {
                updater?.automaticallyChecksForUpdates = true
                updater?.automaticallyDownloadsUpdates = command == "automatic"
            }
        case "download":
            guard phase == "available", !choiceInstalls, item?.isInformationOnlyUpdate != true else { return }
            phase = "downloading"; message = ""; resolve(.install)
        case "later":
            guard phase == "available", !choiceInstalls else { return }
            phase = "idle"; resolve(.dismiss)
        case "skip":
            guard phase == "available", !choiceInstalls else { return }
            phase = "idle"; resolve(.skip); item = nil
        case "cancel":
            needsApproval = false; approved = false; quitApproved = false
            if let cancellation = cancel {
                cancel = nil; cancellation(); phase = "cancelled"
            } else if choice != nil {
                resolve(.skip); installOnQuit = false; phase = "cancelled"
            } else if installOnQuit, updater?.canCheckForUpdates == true {
                // Reattach the user driver to cancel the staged installer, not
                // just dismiss its UI (which would still install on exit).
                pendingCancel = true; updater?.checkForUpdates()
            }
        case "install-on-quit":
            guard phase == "ready" else { return }
            installOnQuit = true
            // Keep the reply until quit or cancel, so cancellation remains real.
        case "validate-exit": _ = validateExit()
        case "quit-request-received": needsQuitApproval = false
        case "quit-approved": quitApproved = validateExit()
        case "install-approved":
            guard ["ready", "installing"].contains(phase) || needsApproval else { return }
            guard validateExit() else { needsApproval = false; return }
            approved = true; needsApproval = false
            if let handler = delayedInstall { delayedInstall = nil; handler() }
            else if let handler = immediateInstall { immediateInstall = nil; handler() }
            else if choice != nil { resolve(.install) }
            else { needsApproval = true; updater?.checkForUpdates() }
        case "withdraw-approval": approved = false; quitApproved = false; needsApproval = false; suppressTermination = true
        case "approval-denied": approved = false; quitApproved = false; needsApproval = false
        default: break
        }
    }
    func show(_ request: SPUUpdatePermissionRequest, reply: @escaping (SUUpdatePermissionResponse) -> Void) {
        // Official bundles define the defaults; an unexpected permission request
        // must never silently enable checks, downloads, or system profiling.
        reply(SUUpdatePermissionResponse(automaticUpdateChecks: false, automaticUpdateDownloading: false, sendSystemProfile: false))
    }
    func showUserInitiatedUpdateCheck(cancellation: @escaping () -> Void) {
        cancel = cancellation; phase = "checking"; message = ""; attention &+= 1
    }
    func showUpdateFound(with appcastItem: SUAppcastItem, state: SPUUserUpdateState, reply: @escaping (SPUUserUpdateChoice) -> Void) {
        item = appcastItem; expected = appcastItem.contentLength; cancel = nil; message = ""
        choice = reply; choiceInstalls = state.stage == .installing
        phase = choiceInstalls ? "ready" : "available"
        if state.userInitiated { attention &+= 1 }
        if pendingCancel { pendingCancel = false; resolve(.skip); installOnQuit = false; phase = "cancelled" }
        else if approved && choiceInstalls { resolve(.install) }
    }
    func showUpdateReleaseNotes(with downloadData: SPUDownloadData) {}
    func showUpdateReleaseNotesFailedToDownloadWithError(_ error: Error) { message = error.localizedDescription }
    func showUpdateNotFoundWithError(_ error: Error, acknowledgement: @escaping () -> Void) {
        cancel = nil; phase = "current"; item = nil; message = error.localizedDescription; acknowledgement()
    }
    func showUpdaterError(_ error: Error, acknowledgement: @escaping () -> Void) {
        lastError = error as NSError
        phase = "error"; message = error.localizedDescription; approved = false; needsApproval = false; acknowledgement()
    }
    func showDownloadInitiated(cancellation: @escaping () -> Void) {
        cancel = cancellation; received = 0; phase = "downloading"; progressKnown = true
    }
    func showDownloadDidReceiveExpectedContentLength(_ expectedContentLength: UInt64) { expected = expectedContentLength }
    func showDownloadDidReceiveData(ofLength length: UInt64) { received = received &+ length }
    func showDownloadDidStartExtractingUpdate() { cancel = nil; phase = "extracting" }
    func showExtractionReceivedProgress(_ progress: Double) {}
    func showReady(toInstallAndRelaunch reply: @escaping (SPUUserUpdateChoice) -> Void) {
        phase = "ready"; choice = reply; choiceInstalls = true; cancel = nil; installOnQuit = true
        if pendingCancel { pendingCancel = false; resolve(.skip); installOnQuit = false; phase = "cancelled" }
    }
    func showInstallingUpdate(withApplicationTerminated applicationTerminated: Bool, retryTerminatingApplication: @escaping () -> Void) {
        phase = "installing"
        if !applicationTerminated { delayedInstall = retryTerminatingApplication }
    }
    func showUpdateInstalledAndRelaunched(_ relaunched: Bool, acknowledgement: @escaping () -> Void) { acknowledgement() }
    func dismissUpdateInstallation() {
        cancel = nil; choice = nil; immediateInstall = nil; delayedInstall = nil
        installOnQuit = false; needsApproval = false; approved = false; quitApproved = false
        if !["error", "current", "cancelled"].contains(phase) { phase = "idle" }
    }
    func showUpdateInFocus() { attention &+= 1 }
    func updater(_ updater: SPUUpdater, didFindValidUpdate item: SUAppcastItem) { self.item = item }
    func updater(_ updater: SPUUpdater, willDownloadUpdate item: SUAppcastItem, with request: NSMutableURLRequest) {
        self.item = item; phase = "downloading"; expected = item.contentLength; received = 0; progressKnown = false
    }
    func updater(_ updater: SPUUpdater, didDownloadUpdate item: SUAppcastItem) {
        received = expected; phase = "extracting"
    }
    func updater(_ updater: SPUUpdater, didAbortWithError error: Error) {
        let value = error as NSError
        lastError = value
        if value.domain == SUSparkleErrorDomain && value.code == SUError.noUpdateError.rawValue {
            phase = "current"; message = error.localizedDescription; return
        }
        phase = "error"; message = error.localizedDescription; approved = false; needsApproval = false
    }
    func updater(_ updater: SPUUpdater, willInstallUpdateOnQuit item: SUAppcastItem, immediateInstallationBlock: @escaping () -> Void) -> Bool {
        self.item = item; immediateInstall = immediateInstallationBlock
        phase = "ready"; installOnQuit = true
        // Take control so the stored block can be used only after Rust's gate.
        return true
    }
    func updater(_ updater: SPUUpdater, shouldPostponeRelaunchForUpdate item: SUAppcastItem, untilInvokingBlock installHandler: @escaping () -> Void) -> Bool {
        if approved { return false }
        delayedInstall = installHandler; needsApproval = true; phase = "ready"; attention &+= 1
        return true
    }
}

@_cdecl("glim_updater_create")
func glimUpdaterCreate() -> UnsafeMutableRawPointer {
    precondition(Thread.isMainThread)
    return Unmanaged.passRetained(GlimUpdater()).toOpaque()
}
@_cdecl("glim_updater_snapshot")
func glimUpdaterSnapshot(_ handle: UnsafeMutableRawPointer) -> UnsafeMutablePointer<CChar>? {
    precondition(Thread.isMainThread)
    let value = Unmanaged<GlimUpdater>.fromOpaque(handle).takeUnretainedValue().snapshot()
    guard let data = try? JSONSerialization.data(withJSONObject: value), let string = String(data: data, encoding: .utf8) else { return nil }
    return strdup(string)
}
@_cdecl("glim_updater_command")
func glimUpdaterCommand(_ handle: UnsafeMutableRawPointer, _ command: UnsafePointer<CChar>) {
    Unmanaged<GlimUpdater>.fromOpaque(handle).takeUnretainedValue().command(String(cString: command))
}
@_cdecl("glim_updater_free_string")
func glimUpdaterFreeString(_ string: UnsafeMutablePointer<CChar>) { free(string) }
@_cdecl("glim_updater_release")
func glimUpdaterRelease(_ handle: UnsafeMutableRawPointer) {
    precondition(Thread.isMainThread)
    _ = Unmanaged<GlimUpdater>.fromOpaque(handle).takeRetainedValue()
}
