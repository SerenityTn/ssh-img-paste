import AppKit
import CoreGraphics

private struct Upload {
    let name: String
    let size: Int
    let profileID: String
}

private struct UploadCacheEntry {
    let uploads: [Upload]
    let ok: Bool
    let updatedAt: Date
    let error: String?
}

private struct CaptureRequest {
    let mode: String
    let profileID: String
}

// Menu-bar app. Left-click uploads the clipboard image (or a screenshot) via
// `~/bin/ssh-img-paste`; right-click (or Option-click) opens a menu to choose
// the active SSH destination, browse, and clean images uploaded there.
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var statusItem: NSStatusItem!
    private let scriptClient = ScriptClient()
    private let notificationPresenter = NotificationPresenter()
    private var profileManager: ProfileManagerWindowController?
    private var uploadCache: [String: UploadCacheEntry] = [:]
    private var loadingUploads = Set<String>()
    private let idleSymbol = "photo.on.rectangle.angled"

    func applicationDidFinishLaunching(_ notification: Notification) {
        notificationPresenter.prepare()
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let button = statusItem.button {
            button.image = symbol(idleSymbol)
            button.action = #selector(handleClick(_:))
            button.target = self
            button.sendAction(on: [.leftMouseUp, .rightMouseUp])
            button.toolTip = "SSH Image Paste — click to upload the clipboard image"
            button.setAccessibilityLabel("SSH Image Paste")
            button.setAccessibilityHelp("Upload the clipboard image, or open destination options with a secondary click")
        }
    }

    // MARK: - Click routing

    @objc private func handleClick(_ sender: Any?) {
        guard let event = NSApp.currentEvent else { runUpload(); return }
        if event.type == .rightMouseUp || event.modifierFlags.contains(.option) {
            showMenu()
        } else {
            runUpload()
        }
    }

    private func showMenu() {
        let menu = NSMenu()
        let (profiles, profilesOK) = listProfiles()
        let activeProfile = profiles.first { $0.isActive }
        let destinationName = destinationLabel(activeProfile)

        let destination = NSMenuItem(title: "Destination: \(destinationName)", action: nil, keyEquivalent: "")
        let destinationSubmenu = NSMenu()
        if !profilesOK {
            destinationSubmenu.addItem(disabled("Could not load SSH profiles"))
        } else if profiles.isEmpty {
            destinationSubmenu.addItem(disabled("No SSH profiles configured"))
            let add = NSMenuItem(title: "Add SSH Profile…", action: #selector(addProfile(_:)), keyEquivalent: "")
            add.target = self
            destinationSubmenu.addItem(add)
        } else {
            for profile in profiles {
                let item = NSMenuItem(title: "\(profile.label)  (\(profile.host))",
                                      action: #selector(selectProfile(_:)), keyEquivalent: "")
                item.target = self
                item.representedObject = profile.id
                item.state = profile.id == activeProfile?.id ? .on : .off
                destinationSubmenu.addItem(item)
            }
            if activeProfile == nil {
                destinationSubmenu.addItem(.separator())
                destinationSubmenu.addItem(disabled("No active destination selected"))
            }
        }
        destinationSubmenu.addItem(.separator())
        let manage = NSMenuItem(title: "Manage Profiles…", action: #selector(manageProfiles(_:)), keyEquivalent: "")
        manage.target = self
        destinationSubmenu.addItem(manage)
        destination.submenu = destinationSubmenu
        menu.addItem(destination)

        menu.addItem(.separator())

        let up = NSMenuItem(title: "Upload Clipboard Image / Screenshot → \(destinationName)",
                            action: #selector(runUploadFromMenu(_:)), keyEquivalent: "")
        up.target = self
        if let profileID = activeProfile?.id, !profileID.isEmpty {
            up.representedObject = profileID
        } else {
            up.action = nil
            up.isEnabled = false
        }
        menu.addItem(up)

        let region = NSMenuItem(title: "Capture Region → \(destinationName)…",
                                action: #selector(runCapture(_:)), keyEquivalent: "")
        region.target = self
        if let profileID = activeProfile?.id, !profileID.isEmpty {
            region.representedObject = CaptureRequest(mode: "region", profileID: profileID)
        } else {
            region.action = nil
            region.isEnabled = false
        }
        menu.addItem(region)

        let full = NSMenuItem(title: "Capture Full Screen → \(destinationName)",
                              action: #selector(runCapture(_:)), keyEquivalent: "")
        full.target = self
        if let profileID = activeProfile?.id, !profileID.isEmpty {
            full.representedObject = CaptureRequest(mode: "full", profileID: profileID)
        } else {
            full.action = nil
            full.isEnabled = false
        }
        menu.addItem(full)

        menu.addItem(.separator())

        // Uploaded-images section uses cached data only; SSH list runs from explicit Refresh.
        let activeID = activeProfile?.id
        let cache = activeID.flatMap { uploadCache[$0] }
        let isLoadingUploads = activeID.map { loadingUploads.contains($0) } ?? false
        let uploads = cache?.uploads ?? []
        let ok = cache?.ok ?? false
        let header = NSMenuItem(title: uploadsTitle(cache: cache, loading: isLoadingUploads, profile: activeProfile), action: nil, keyEquivalent: "")
        let sub = NSMenu()
        if activeProfile == nil {
            sub.addItem(disabled("No active destination selected"))
        } else if isLoadingUploads {
            sub.addItem(disabled("Loading uploaded images…"))
        } else if cache == nil {
            sub.addItem(disabled("Not loaded yet"))
        } else if !ok {
            sub.addItem(disabled(cache?.error ?? "Active destination unreachable"))
        } else if uploads.isEmpty {
            sub.addItem(disabled("No uploads"))
        } else {
            for u in uploads {
                let it = NSMenuItem(title: "\(u.name)   (\(humanSize(u.size)))",
                                    action: #selector(openImage(_:)), keyEquivalent: "")
                it.target = self
                it.representedObject = u
                sub.addItem(it)
            }
        }
        sub.addItem(.separator())
        let refresh = NSMenuItem(title: isLoadingUploads ? "Refreshing Uploaded Images…" : "Load/Refresh Uploaded Images…",
                                 action: #selector(refreshUploads(_:)), keyEquivalent: "")
        refresh.target = self
        refresh.representedObject = activeProfile
        refresh.isEnabled = activeProfile != nil && !isLoadingUploads
        sub.addItem(refresh)
        header.submenu = sub
        menu.addItem(header)

        if ok && !uploads.isEmpty, let profile = activeProfile {
            let clean = NSMenuItem(title: "Clean All Uploads (\(uploads.count))…",
                                   action: #selector(cleanUploads(_:)), keyEquivalent: "")
            clean.target = self
            clean.representedObject = profile
            menu.addItem(clean)
        }

        menu.addItem(.separator())
        let project = NSMenuItem(title: "SSH Image Paste on GitHub…", action: #selector(openProjectPage(_:)), keyEquivalent: "")
        project.target = self
        menu.addItem(project)
        let about = NSMenuItem(title: "About SSH Image Paste", action: #selector(showAbout(_:)), keyEquivalent: "")
        about.target = self
        menu.addItem(about)
        menu.addItem(.separator())
        let quit = NSMenuItem(title: "Quit SSH Image Paste", action: #selector(quit), keyEquivalent: "q")
        quit.target = self
        menu.addItem(quit)

        if let button = statusItem.button {
            menu.popUp(positioning: nil, at: NSPoint(x: 0, y: button.bounds.height + 5), in: button)
        }
    }

    // MARK: - Actions

    @objc private func runUpload() {
        setIcon("arrow.up.circle")
        runUploadScriptAsync([]) { [weak self] result in
            if result.uploadKind == .screenshot {
                self?.notificationPresenter.postCaptureSucceeded()
            }
            self?.flash(result.succeeded ? "checkmark.circle" : "exclamationmark.triangle")
        }
    }

    @objc private func runUploadFromMenu(_ sender: NSMenuItem) {
        guard let profileID = sender.representedObject as? String, !profileID.isEmpty else {
            flash("exclamationmark.triangle")
            return
        }
        setIcon("arrow.up.circle")
        runUploadScriptAsync(scriptArgs(profileID: profileID)) { [weak self] result in
            if result.uploadKind == .screenshot {
                self?.notificationPresenter.postCaptureSucceeded()
            }
            self?.flash(result.succeeded ? "checkmark.circle" : "exclamationmark.triangle")
        }
    }

    @objc private func runCapture(_ sender: NSMenuItem) {
        guard let request = sender.representedObject as? CaptureRequest, !request.profileID.isEmpty else {
            flash("exclamationmark.triangle")
            return
        }
        guard screenCapturePermissionGranted() else {
            flash("exclamationmark.triangle")
            return
        }
        setIcon("camera")
        runUploadScriptAsync(scriptArgs(profileID: request.profileID, command: request.mode)) { [weak self] result in
            if result.uploadKind == .screenshot {
                self?.notificationPresenter.postCaptureSucceeded()
            }
            self?.flash(result.succeeded ? "checkmark.circle" : "exclamationmark.triangle")
        }
    }

    @objc private func selectProfile(_ sender: NSMenuItem) {
        guard let id = sender.representedObject as? String else { return }
        setIcon("server.rack")
        runScriptAsync(["profile", "use", id]) { [weak self] ok in
            self?.flash(ok ? "checkmark.circle" : "exclamationmark.triangle")
        }
    }

    @objc private func manageProfiles(_ sender: NSMenuItem) {
        openProfileManager(addMode: false)
    }

    @objc private func addProfile(_ sender: NSMenuItem) {
        openProfileManager(addMode: true)
    }

    @objc private func refreshUploads(_ sender: NSMenuItem) {
        guard let profile = sender.representedObject as? SSHProfile else {
            flash("exclamationmark.triangle")
            return
        }
        loadUploads(for: profile, reopenMenu: true)
    }

    private func openProfileManager(addMode: Bool) {
        if profileManager == nil {
            profileManager = ProfileManagerWindowController(client: scriptClient) { [weak self] in
                self?.setIcon(self?.idleSymbol ?? "photo.on.rectangle.angled")
            }
        }
        profileManager?.show(addMode: addMode)
    }

    @objc private func openImage(_ sender: NSMenuItem) {
        guard let upload = sender.representedObject as? Upload else { return }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self = self else { return }
            let (out, status) = self.runScriptSync(self.scriptArgs(profileID: upload.profileID, command: "fetch", upload.name))
            let path = out.trimmingCharacters(in: .whitespacesAndNewlines)
            DispatchQueue.main.async {
                if status == 0, !path.isEmpty {
                    NSWorkspace.shared.open(URL(fileURLWithPath: path))
                } else {
                    self.flash("exclamationmark.triangle")
                }
            }
        }
    }

    @objc private func cleanUploads(_ sender: NSMenuItem) {
        guard let profile = sender.representedObject as? SSHProfile else {
            flash("exclamationmark.triangle")
            return
        }
        let alert = NSAlert()
        alert.messageText = "Delete all uploaded images on \(profile.label)?"
        alert.informativeText = "Destination host: \(profile.host)\n\nThis permanently removes every uploaded image from that remote folder."
        alert.alertStyle = .warning
        alert.addButton(withTitle: "Delete")
        alert.addButton(withTitle: "Cancel")
        NSApp.activate(ignoringOtherApps: true)
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        setIcon("trash")
        runScriptAsync(scriptArgs(profileID: profile.id, command: "clean")) { [weak self] ok in
            if ok { self?.uploadCache.removeValue(forKey: profile.id) }
            self?.flash(ok ? "checkmark.circle" : "exclamationmark.triangle")
        }
    }

    @objc private func quit() { NSApp.terminate(nil) }

    @objc private func openProjectPage(_ sender: Any?) {
        guard let url = URL(string: "https://github.com/SerenityTn/ssh-img-paste") else { return }
        NSWorkspace.shared.open(url)
    }

    @objc private func showAbout(_ sender: Any?) {
        NSApp.activate(ignoringOtherApps: true)
        NSApp.orderFrontStandardAboutPanel(sender)
    }

    private func screenCapturePermissionGranted() -> Bool {
        if CGPreflightScreenCaptureAccess() { return true }
        NSApp.activate(ignoringOtherApps: true)
        return CGRequestScreenCaptureAccess()
    }

    // MARK: - Script bridge

    private func runScriptSync(_ args: [String]) -> (String, Int32) {
        let result = scriptClient.runSync(args)
        return (result.stdout, result.status)
    }

    private func runScriptAsync(_ args: [String], onDone: @escaping (Bool) -> Void) {
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self = self else { return }
            let result = self.scriptClient.runSync(args)
            DispatchQueue.main.async { onDone(result.succeeded) }
        }
    }

    private func runUploadScriptAsync(_ args: [String], onDone: @escaping (ScriptResult) -> Void) {
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self = self else { return }
            let result = self.scriptClient.runSync(
                args,
                environmentOverrides: [
                    "SSH_IMG_PASTE_RESULT_FORMAT": "tsv",
                    "SSH_IMG_PASTE_SUPPRESS_SCREENSHOT_SUCCESS_NOTIFICATION": "1",
                ]
            )
            DispatchQueue.main.async { onDone(result) }
        }
    }

    private func scriptArgs(profileID: String?, command: String? = nil, _ rest: String...) -> [String] {
        var args: [String] = []
        if let profileID = profileID, !profileID.isEmpty {
            args += ["--profile", profileID]
        }
        if let command = command {
            args.append(command)
        }
        args += rest
        return args
    }

    private func loadUploads(for profile: SSHProfile, reopenMenu: Bool) {
        guard !loadingUploads.contains(profile.id) else { return }
        loadingUploads.insert(profile.id)
        setIcon("arrow.clockwise")
        scriptClient.runAsync(scriptArgs(profileID: profile.id, command: "list")) { [weak self] result in
            guard let self = self else { return }
            let parsed = result.succeeded ? self.parseUploads(result.stdout, profileID: profile.id) : []
            self.uploadCache[profile.id] = UploadCacheEntry(uploads: parsed,
                                                            ok: result.succeeded,
                                                            updatedAt: Date(),
                                                            error: result.succeeded ? nil : result.sanitizedError)
            self.loadingUploads.remove(profile.id)
            self.flash(result.succeeded ? "checkmark.circle" : "exclamationmark.triangle")
            if reopenMenu { self.showMenu() }
        }
    }

    private func parseUploads(_ output: String, profileID: String) -> [Upload] {
        var files: [Upload] = []
        for line in output.split(separator: "\n") {
            let parts = line.split(separator: "\t", maxSplits: 1)
            if parts.count == 2, let sz = Int(parts[0]) {
                files.append(Upload(name: String(parts[1]), size: sz, profileID: profileID))
            }
        }
        return files
    }

    private func listProfiles() -> ([SSHProfile], Bool) {
        let response = scriptClient.listProfiles()
        return (response.profiles, response.ok)
    }

    // MARK: - Helpers

    private func symbol(_ name: String) -> NSImage? {
        let img = NSImage(systemSymbolName: name, accessibilityDescription: "SSH Image Paste")
        img?.isTemplate = true
        return img
    }

    private func setIcon(_ name: String) { statusItem.button?.image = symbol(name) }

    private func flash(_ name: String) {
        setIcon(name)
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.3) { [weak self] in
            guard let self = self else { return }
            self.setIcon(self.idleSymbol)
        }
    }

    private func disabled(_ title: String) -> NSMenuItem {
        let m = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        m.isEnabled = false
        return m
    }

    private func humanSize(_ bytes: Int) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
    }

    private func destinationLabel(_ profile: SSHProfile?) -> String {
        profile?.label ?? "No active destination"
    }

    private func uploadsTitle(cache: UploadCacheEntry?, loading: Bool, profile: SSHProfile?) -> String {
        let destination = destinationLabel(profile)
        if loading { return "Uploaded Images on \(destination) (loading…)" }
        guard let cache = cache else { return "Uploaded Images on \(destination) (not loaded)" }
        guard cache.ok else { return "Uploaded Images on \(destination) (refresh failed)" }
        if cache.uploads.isEmpty { return "Uploaded Images on \(destination) (0)" }
        let total = cache.uploads.reduce(0) { $0 + $1.size }
        return "Uploaded Images on \(destination) (\(cache.uploads.count), \(humanSize(total)))"
    }
}

@main
struct SSHImagePasteApp {
    static func main() {
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.accessory)   // menu-bar only, no Dock icon
        app.run()
    }
}
