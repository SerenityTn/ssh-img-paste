import AppKit

private final class ProfileManagerWindow: NSWindow {
    weak var profileController: ProfileManagerWindowController?

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard event.modifierFlags.intersection(.deviceIndependentFlagsMask).contains(.command),
              let chars = event.charactersIgnoringModifiers?.lowercased() else {
            return super.performKeyEquivalent(with: event)
        }
        if chars == "n" {
            profileController?.handleNewShortcut()
            return true
        }
        if chars == "s" {
            profileController?.handleSaveShortcut()
            return true
        }
        return super.performKeyEquivalent(with: event)
    }
}

final class ProfileManagerWindowController: NSWindowController, NSWindowDelegate, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    private let client: ScriptClient
    private let onMutation: () -> Void

    private var profiles: [VPSProfile] = []
    private var selectedID: String?
    private var originalDraft: VPSProfileDraft?
    private var draft = VPSProfileDraft.empty()
    private var currentDetails: ProfileDetails?
    private var isLoading = false
    private var isBusy = false
    private var loadGeneration = 0
    private var suppressSelectionChange = false
    private var detailsCache: [String: ProfileDetails] = [:]
    private var loadState = ProfileManagerLoadState()

    private let tableView = NSTableView()
    private let scrollView = NSScrollView()
    private let addButton = NSButton(title: "+", target: nil, action: nil)
    private let activeLabel = NSTextField(labelWithString: "")
    private let badgeLabel = NSTextField(labelWithString: "")
    private let idField = NSTextField()
    private let labelField = NSTextField()
    private let hostField = NSTextField()
    private let homeField = NSTextField()
    private let dirField = NSTextField()
    private let advancedButton = NSButton(title: "Advanced", target: nil, action: nil)
    private let advancedStack = NSStackView()
    private let shotMode = NSPopUpButton()
    private let restoreField = NSTextField()
    private let validationLabel = NSTextField(wrappingLabelWithString: "")
    private let statusLabel = NSTextField(wrappingLabelWithString: "")
    private let makeActiveButton = NSButton(title: "Make Active", target: nil, action: nil)
    private let testButton = NSButton(title: "Test Connection", target: nil, action: nil)
    private let revertButton = NSButton(title: "Revert", target: nil, action: nil)
    private let saveButton = NSButton(title: "Save Changes", target: nil, action: nil)
    private let openEditorButton = NSButton(title: "Open in Text Editor…", target: nil, action: nil)

    init(client: ScriptClient = ScriptClient(), onMutation: @escaping () -> Void) {
        self.client = client
        self.onMutation = onMutation
        let window = ProfileManagerWindow(contentRect: NSRect(x: 0, y: 0, width: 860, height: 560),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable],
                              backing: .buffered, defer: false)
        window.title = "Manage VPS Profiles"
        window.minSize = NSSize(width: 760, height: 500)
        super.init(window: window)
        window.profileController = self
        window.delegate = self
        buildUI()
        reloadProfiles(select: nil)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    func show(addMode: Bool = false) {
        showWindow(nil)
        window?.center()
        NSApp.activate(ignoringOtherApps: true)
        if loadState.shouldStartCreateOnShow(addMode: addMode, isLoading: isLoading, profilesEmpty: profiles.isEmpty) {
            startCreate(nil)
        }
    }

    // MARK: UI

    private func buildUI() {
        guard let content = window?.contentView else { return }
        let root = NSStackView()
        root.orientation = .horizontal
        root.spacing = 0
        root.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(root)
        NSLayoutConstraint.activate([
            root.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            root.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            root.topAnchor.constraint(equalTo: content.topAnchor),
            root.bottomAnchor.constraint(equalTo: content.bottomAnchor)
        ])

        let sidebar = NSStackView()
        sidebar.orientation = .vertical
        sidebar.alignment = .width
        sidebar.distribution = .fill
        sidebar.spacing = 8
        sidebar.edgeInsets = NSEdgeInsets(top: 12, left: 12, bottom: 12, right: 12)
        sidebar.widthAnchor.constraint(equalToConstant: 250).isActive = true

        let sidebarTitle = NSTextField(labelWithString: "Destinations")
        sidebarTitle.font = .boldSystemFont(ofSize: 13)
        addButton.target = self
        addButton.action = #selector(startCreate(_:))
        addButton.toolTip = "Add VPS Profile (⌘N)"
        addButton.setAccessibilityLabel("Add VPS Profile")
        addButton.setAccessibilityHelp("Create a new VPS image upload destination profile")
        let header = NSStackView(views: [sidebarTitle, spacer(), addButton])
        header.orientation = .horizontal
        sidebar.addArrangedSubview(header)

        tableView.addTableColumn(NSTableColumn(identifier: NSUserInterfaceItemIdentifier("profile")))
        tableView.headerView = nil
        tableView.delegate = self
        tableView.dataSource = self
        tableView.menu = sidebarMenu()
        tableView.target = self
        tableView.doubleAction = #selector(renameSelected(_:))
        scrollView.documentView = tableView
        scrollView.hasVerticalScroller = true
        sidebar.addArrangedSubview(scrollView)
        scrollView.heightAnchor.constraint(greaterThanOrEqualToConstant: 300).isActive = true

        let detail = NSStackView()
        detail.orientation = .vertical
        detail.alignment = .width
        detail.distribution = .fill
        detail.spacing = 10
        detail.edgeInsets = NSEdgeInsets(top: 16, left: 18, bottom: 14, right: 18)

        activeLabel.font = .boldSystemFont(ofSize: 13)
        badgeLabel.textColor = .secondaryLabelColor
        let titleRow = NSStackView(views: [activeLabel, spacer(), badgeLabel])
        titleRow.orientation = .horizontal
        detail.addArrangedSubview(titleRow)

        detail.addArrangedSubview(formRow("Profile ID", idField, help: "Required when creating. Immutable during normal edits; use Rename for existing profiles."))
        detail.addArrangedSubview(formRow("Display name", labelField))
        detail.addArrangedSubview(formRow("SSH host or alias", hostField, help: "Use ~/.ssh/config for the SSH user, port, identity file, and jump hosts. Keep passwords and private keys outside profile files."))
        detail.addArrangedSubview(formRow("Remote home", homeField))
        detail.addArrangedSubview(formRow("Upload folder", dirField))

        shotMode.addItems(withTitles: ["region", "full"])
        advancedButton.title = "Advanced Settings"
        advancedButton.setButtonType(.pushOnPushOff)
        advancedButton.isBordered = false
        advancedButton.image = NSImage(systemSymbolName: "chevron.right", accessibilityDescription: nil)
        advancedButton.imagePosition = .imageLeading
        advancedButton.alignment = .left
        advancedButton.contentTintColor = .secondaryLabelColor
        advancedButton.state = .off
        advancedButton.target = self
        advancedButton.action = #selector(toggleAdvanced(_:))
        detail.addArrangedSubview(formRow("", advancedButton))
        advancedStack.orientation = .vertical
        advancedStack.spacing = 10
        advancedStack.isHidden = true
        advancedStack.addArrangedSubview(formRow("Screenshot mode", shotMode))
        advancedStack.addArrangedSubview(formRow("Restore delay (seconds)", restoreField))
        detail.addArrangedSubview(advancedStack)

        validationLabel.textColor = .systemRed
        validationLabel.font = .systemFont(ofSize: 12)
        detail.addArrangedSubview(formRow("", validationLabel))

        statusLabel.textColor = .secondaryLabelColor
        statusLabel.font = .systemFont(ofSize: 12)
        detail.addArrangedSubview(formRow("", statusLabel))

        openEditorButton.target = self
        openEditorButton.action = #selector(openInTextEditor(_:))
        detail.addArrangedSubview(openEditorButton)

        let footer = NSStackView(views: [makeActiveButton, testButton, spacer(), revertButton, saveButton])
        footer.orientation = .horizontal
        footer.spacing = 8
        makeActiveButton.target = self; makeActiveButton.action = #selector(makeActive(_:))
        testButton.target = self; testButton.action = #selector(testConnection(_:))
        revertButton.target = self; revertButton.action = #selector(revertDraft(_:))
        saveButton.target = self; saveButton.action = #selector(saveDraft(_:))
        saveButton.keyEquivalent = "s"
        saveButton.keyEquivalentModifierMask = [.command]
        detail.addArrangedSubview(footer)

        root.addArrangedSubview(sidebar)
        let divider = NSBox(); divider.boxType = .separator; root.addArrangedSubview(divider)
        root.addArrangedSubview(detail)

        for field in [idField, labelField, hostField, homeField, dirField, restoreField] {
            field.delegate = self
            field.target = self
            field.action = #selector(fieldChanged(_:))
        }
        shotMode.target = self
        shotMode.action = #selector(fieldChanged(_:))

        let newItem = NSMenuItem(title: "New Profile", action: #selector(startCreate(_:)), keyEquivalent: "n")
        newItem.keyEquivalentModifierMask = [.command]
        newItem.target = self
        window?.contentView?.menu = NSMenu()
        window?.contentView?.menu?.addItem(newItem)
    }

    private func formRow(_ title: String, _ control: NSView, help: String? = nil) -> NSView {
        let row = NSStackView()
        row.orientation = .vertical
        row.spacing = 3
        let line = NSStackView()
        line.orientation = .horizontal
        line.spacing = 10
        let label = NSTextField(labelWithString: title)
        label.alignment = .right
        label.widthAnchor.constraint(equalToConstant: 145).isActive = true
        control.widthAnchor.constraint(greaterThanOrEqualToConstant: 320).isActive = true
        line.addArrangedSubview(label)
        line.addArrangedSubview(control)
        row.addArrangedSubview(line)
        if let help = help {
            let h = NSTextField(wrappingLabelWithString: help)
            h.textColor = .secondaryLabelColor
            h.font = .systemFont(ofSize: 11)
            h.translatesAutoresizingMaskIntoConstraints = false
            row.addArrangedSubview(h)
            h.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: 155).isActive = true
        }
        return row
    }

    private func spacer() -> NSView {
        let view = NSView()
        view.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return view
    }

    private func sidebarMenu() -> NSMenu {
        let menu = NSMenu()
        menu.delegate = self
        let dup = NSMenuItem(title: "Duplicate", action: #selector(duplicateSelected(_:)), keyEquivalent: "")
        dup.target = self
        menu.addItem(dup)
        let ren = NSMenuItem(title: "Rename…", action: #selector(renameSelected(_:)), keyEquivalent: "")
        ren.target = self
        menu.addItem(ren)
        let del = NSMenuItem(title: "Delete…", action: #selector(deleteSelected(_:)), keyEquivalent: "")
        del.target = self
        menu.addItem(del)
        return menu
    }

    func handleNewShortcut() { startCreate(nil) }

    func handleSaveShortcut() {
        if saveButton.isEnabled { saveDraft(nil) }
    }

    // MARK: Data

    private func reloadProfiles(select id: String?) {
        let generation = nextLoadGeneration()
        let selectedSnapshot = id ?? selectedID
        setLoading(true, message: "Loading profiles…")
        DispatchQueue.global(qos: .userInitiated).async { [client] in
            let response = client.listProfiles()
            var cache: [String: ProfileDetails] = [:]
            if response.ok {
                for profile in response.profiles {
                    if let details = client.inspectProfile(profile.id).details {
                        cache[profile.id] = details
                    }
                }
            }
            DispatchQueue.main.async { [weak self] in
                guard let self = self, self.loadGeneration == generation else { return }
                self.profiles = response.profiles
                self.detailsCache = cache
                self.tableView.reloadData()
                self.setLoading(false, message: response.ok ? "Profiles loaded." : (response.error ?? "Could not load profiles."))
                if self.loadState.shouldStartCreateAfterLoad(profilesEmpty: self.profiles.isEmpty) {
                    self.selectedID = nil
                    self.startCreate(nil)
                } else if let target = self.preferredSelection(explicit: id, snapshot: selectedSnapshot) {
                    self.restoreSelection(id: target)
                    self.loadProfile(target)
                } else {
                    self.selectedID = nil
                    self.startCreate(nil)
                }
            }
        }
    }

    private func nextLoadGeneration() -> Int {
        loadGeneration += 1
        return loadGeneration
    }

    private func preferredSelection(explicit id: String?, snapshot: String?) -> String? {
        if let id = id, profiles.contains(where: { $0.id == id }) { return id }
        if let snapshot = snapshot, profiles.contains(where: { $0.id == snapshot }) { return snapshot }
        return profiles.first?.id
    }

    private func restoreSelection(id: String) {
        guard let idx = profiles.firstIndex(where: { $0.id == id }) else { return }
        suppressSelectionChange = true
        tableView.selectRowIndexes(IndexSet(integer: idx), byExtendingSelection: false)
        suppressSelectionChange = false
    }

    private func loadProfile(_ id: String) {
        selectedID = id
        if let cached = detailsCache[id] {
            applyProfileDetails(cached)
            return
        }
        let generation = nextLoadGeneration()
        isLoading = true
        validateAndRefreshButtons()
        statusLabel.stringValue = "Inspecting profile…"
        DispatchQueue.global(qos: .userInitiated).async { [client] in
            let inspected = client.inspectProfile(id)
            DispatchQueue.main.async { [weak self] in
                guard let self = self, self.loadGeneration == generation, self.selectedID == id else { return }
                if let details = inspected.details {
                    self.detailsCache[id] = details
                    self.applyProfileDetails(details)
                } else if let summary = self.profiles.first(where: { $0.id == id }) {
                    self.currentDetails = nil
                    self.draft = VPSProfileDraft(originalID: summary.id, id: summary.id, label: summary.label, host: summary.host, remoteHome: "/home/user", remoteDir: "img-uploads")
                    self.originalDraft = self.draft
                    self.statusLabel.stringValue = inspected.result.sanitizedError
                    self.populateFields()
                    self.isLoading = false
                    self.validateAndRefreshButtons()
                }
            }
        }
    }

    private func applyProfileDetails(_ details: ProfileDetails) {
        isLoading = true
        currentDetails = details
        draft = VPSProfileDraft(details: details)
        originalDraft = draft
        statusLabel.stringValue = "Local config: \(details.path.isEmpty ? "unknown" : details.path)"
        populateFields()
        isLoading = false
        validateAndRefreshButtons()
    }

    private func populateFields() {
        idField.stringValue = draft.id
        labelField.stringValue = draft.label
        hostField.stringValue = draft.host
        homeField.stringValue = draft.remoteHome
        dirField.stringValue = draft.remoteDir
        shotMode.selectItem(withTitle: draft.shotMode)
        restoreField.stringValue = String(draft.restoreSeconds)
        idField.isEditable = draft.isNew
        let readOnly = !draft.editable
        [labelField, hostField, homeField, dirField, restoreField].forEach { $0.isEditable = !readOnly }
        shotMode.isEnabled = !readOnly
        openEditorButton.isHidden = !readOnly
        if draft.isNew {
            activeLabel.stringValue = "New VPS Profile"
            badgeLabel.stringValue = ""
        } else {
            activeLabel.stringValue = (currentDetails?.active == true ? "✓ Active Destination" : "Profile")
            badgeLabel.stringValue = currentDetails?.badges.joined(separator: "  ") ?? ""
        }
    }

    private func syncDraftFromFields() {
        guard !isLoading, !isBusy else { return }
        draft.id = idField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        draft.label = labelField.stringValue
        draft.host = hostField.stringValue
        draft.remoteHome = homeField.stringValue
        draft.remoteDir = dirField.stringValue
        draft.shotMode = shotMode.titleOfSelectedItem ?? "region"
        draft.restoreSeconds = Int(restoreField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)) ?? -1
    }

    private var isDirty: Bool { draft.changed(from: originalDraft) }

    private func validateAndRefreshButtons() {
        if isBusy {
            [makeActiveButton, testButton, revertButton, saveButton, addButton, openEditorButton].forEach { $0.isEnabled = false }
            return
        }
        syncDraftFromFields()
        let errors = draft.validationErrors
        validationLabel.stringValue = errors.joined(separator: "\n")
        let editable = draft.editable
        saveButton.isEnabled = editable && isDirty && errors.isEmpty
        revertButton.isEnabled = isDirty
        makeActiveButton.isEnabled = !draft.isNew && currentDetails?.active != true
        testButton.isEnabled = !draft.isNew && !(draft.originalID ?? "").isEmpty
        openEditorButton.isEnabled = !editable && !(currentDetails?.path ?? "").isEmpty
    }

    private func confirmDiscardIfNeeded() -> Bool {
        guard isDirty else { return true }
        let alert = NSAlert()
        alert.messageText = "Discard unsaved profile changes?"
        alert.informativeText = "Your edits have not been saved."
        alert.alertStyle = .warning
        alert.addButton(withTitle: "Discard")
        alert.addButton(withTitle: "Cancel")
        return alert.runModal() == .alertFirstButtonReturn
    }

    // MARK: Actions

    @objc private func fieldChanged(_ sender: Any?) { validateAndRefreshButtons() }

    @objc private func toggleAdvanced(_ sender: Any?) {
        let expanded = advancedButton.state == .on
        advancedStack.isHidden = !expanded
        advancedButton.image = NSImage(systemSymbolName: expanded ? "chevron.down" : "chevron.right", accessibilityDescription: nil)
        advancedButton.setAccessibilityValue(expanded ? "Expanded" : "Collapsed")
    }

    @objc private func startCreate(_ sender: Any?) {
        guard !isBusy else { return }
        guard confirmDiscardIfNeeded() else { return }
        selectedID = nil
        currentDetails = nil
        draft = VPSProfileDraft.empty()
        originalDraft = draft
        suppressSelectionChange = true
        tableView.deselectAll(nil)
        suppressSelectionChange = false
        populateFields()
        validateAndRefreshButtons()
        idField.becomeFirstResponder()
        statusLabel.stringValue = "Create a profile. Selection/creation will not upload anything."
    }

    @objc private func revertDraft(_ sender: Any?) {
        guard !isBusy else { return }
        if let original = originalDraft { draft = original } else { draft = VPSProfileDraft.empty() }
        populateFields()
        validateAndRefreshButtons()
    }

    @objc private func saveDraft(_ sender: Any?) {
        guard !isBusy else { return }
        syncDraftFromFields()
        guard draft.isValid, draft.editable else { validateAndRefreshButtons(); return }
        let savedID = draft.id
        setBusy(true, message: draft.isNew ? "Creating profile…" : "Saving profile…")
        let args = draft.isNew ? draft.createArguments() : draft.updateArguments()
        client.runAsync(args) { [weak self] result in
            guard let self = self else { return }
            self.setBusy(false, message: result.succeeded ? "Saved." : result.sanitizedError)
            if result.succeeded {
                self.onMutation()
                self.reloadProfiles(select: savedID)
            } else if result.status == 77 {
                self.statusLabel.stringValue = "This profile is manual/read-only and cannot be rewritten. Open it in a text editor instead."
                self.draft.editable = false
                self.populateFields()
                self.validateAndRefreshButtons()
            }
        }
    }

    @objc private func makeActive(_ sender: Any?) {
        guard !isBusy, !draft.isNew else { return }
        guard confirmDiscardIfNeeded() else { return }
        let id = draft.originalID ?? draft.id
        guard !id.isEmpty else { return }
        setBusy(true, message: "Switching active profile…")
        client.runAsync(["profile", "use", id]) { [weak self] result in
            guard let self = self else { return }
            self.setBusy(false, message: result.succeeded ? "Active profile changed." : result.sanitizedError)
            if result.succeeded { self.onMutation(); self.reloadProfiles(select: id) }
        }
    }

    @objc private func testConnection(_ sender: Any?) {
        guard !isBusy, !draft.isNew else { return }
        syncDraftFromFields()
        let testID = draft.originalID ?? ""
        guard !testID.isEmpty else { return }
        setBusy(true, message: "Testing \(testID)…")
        client.runAsync(["profile", "test", testID]) { [weak self] result in
            guard let self = self else { return }
            self.setBusy(false, message: result.succeeded ? "Connection OK." : result.sanitizedError)
            self.validateAndRefreshButtons()
        }
    }

    @objc private func openInTextEditor(_ sender: Any?) {
        guard !isBusy else { return }
        guard let path = currentDetails?.path, !path.isEmpty else { return }
        NSWorkspace.shared.open(URL(fileURLWithPath: path))
    }

    @objc private func duplicateSelected(_ sender: Any?) {
        guard !isBusy else { return }
        syncDraftFromFields()
        guard !draft.id.isEmpty else { return }
        guard confirmDiscardIfNeeded() else { return }
        let base = draft
        var copy = base
        copy.originalID = nil
        copy.id = uniqueDuplicateID(base.id)
        copy.label = "\(base.label) Copy"
        copy.editable = true
        selectedID = nil
        currentDetails = nil
        originalDraft = nil
        draft = copy
        suppressSelectionChange = true
        tableView.deselectAll(nil)
        suppressSelectionChange = false
        populateFields()
        validateAndRefreshButtons()
        idField.becomeFirstResponder()
    }

    private func uniqueDuplicateID(_ id: String) -> String {
        var n = 1
        var candidate = "\(id)-copy"
        let ids = Set(profiles.map { $0.id })
        while ids.contains(candidate) { n += 1; candidate = "\(id)-copy\(n)" }
        return candidate
    }

    @objc private func renameSelected(_ sender: Any?) {
        guard !isBusy else { return }
        guard let oldID = draft.originalID ?? selectedID else { return }
        guard confirmDiscardIfNeeded() else { return }
        let alert = NSAlert()
        alert.messageText = "Rename Profile"
        alert.informativeText = "Enter a new ID for \(oldID). The profile ID is used by CLI commands."
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 260, height: 24))
        field.stringValue = oldID
        alert.accessoryView = field
        alert.addButton(withTitle: "Rename")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let newID = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard VPSProfileDraft.isValidID(newID), newID != oldID else { statusLabel.stringValue = "Invalid or unchanged profile ID."; return }
        setBusy(true, message: "Renaming profile…")
        client.runAsync(VPSProfileDraft.renameArguments(oldID: oldID, newID: newID)) { [weak self] result in
            guard let self = self else { return }
            self.setBusy(false, message: result.succeeded ? "Renamed." : result.sanitizedError)
            if result.succeeded { self.onMutation(); self.reloadProfiles(select: newID) }
        }
    }

    @objc private func deleteSelected(_ sender: Any?) {
        guard !isBusy else { return }
        guard let id = draft.originalID ?? selectedID,
              let profile = profiles.first(where: { $0.id == id }) else { return }
        guard confirmDiscardIfNeeded() else { return }
        let detailSnapshot = currentDetails ?? detailsCache[id]
        guard profiles.count > 1 else {
            let alert = NSAlert()
            alert.messageText = "Cannot delete the last profile"
            alert.informativeText = "Create another profile before deleting \(profile.label)."
            alert.runModal()
            return
        }
        let replacement = chooseDeletionReplacementIfNeeded(for: profile)
        if profile.isActive && replacement == nil { return }
        let alert = NSAlert()
        alert.messageText = "Delete \(profile.label)?"
        let path = detailSnapshot?.path ?? "unknown"
        var text = "Profile label: \(profile.label)\nProfile ID: \(profile.id)\nHost: \(profile.host)\nLocal path: \(path)\n\nOnly the local profile config is deleted. Remote uploads are not deleted."
        if detailSnapshot?.kind == .legacy || detailSnapshot?.kind == .env { text += "\n\nLegacy default warning: deleting this may remove ~/.config/vps-img-paste.env and reveal a shadowed profiles/default.env." }
        alert.informativeText = text
        alert.alertStyle = .warning
        alert.addButton(withTitle: "Delete Local Profile")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        setBusy(true, message: "Deleting profile…")
        client.runAsync(VPSProfileDraft.deleteArguments(id: id, switchTo: replacement)) { [weak self] result in
            guard let self = self else { return }
            self.setBusy(false, message: result.succeeded ? "Deleted." : result.sanitizedError)
            if result.succeeded { self.onMutation(); self.reloadProfiles(select: replacement) }
        }
    }

    private func chooseDeletionReplacementIfNeeded(for profile: VPSProfile) -> String? {
        guard profile.isActive else { return nil }
        let choices = profiles.filter { $0.id != profile.id }
        guard !choices.isEmpty else { return nil }
        let alert = NSAlert()
        alert.messageText = "Choose replacement active profile"
        alert.informativeText = "Deleting the active profile requires switching to another destination in the same command."
        let popup = NSPopUpButton(frame: NSRect(x: 0, y: 0, width: 320, height: 26))
        for p in choices { popup.addItem(withTitle: "\(p.label) (\(p.id))"); popup.lastItem?.representedObject = p.id }
        alert.accessoryView = popup
        alert.addButton(withTitle: "Continue")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        return popup.selectedItem?.representedObject as? String
    }

    private func setBusy(_ busy: Bool, message: String) {
        isBusy = busy
        statusLabel.stringValue = message
        tableView.isEnabled = !busy && !isLoading
        [idField, labelField, hostField, homeField, dirField, restoreField].forEach { $0.isEnabled = !busy && !isLoading }
        shotMode.isEnabled = !busy && !isLoading && draft.editable
        advancedButton.isEnabled = !busy && !isLoading
        [makeActiveButton, testButton, revertButton, saveButton, addButton, openEditorButton].forEach { $0.isEnabled = !busy && !isLoading }
        if !busy { validateAndRefreshButtons() }
    }

    private func setLoading(_ loading: Bool, message: String) {
        isLoading = loading
        statusLabel.stringValue = message
        tableView.isEnabled = !loading && !isBusy
        [idField, labelField, hostField, homeField, dirField, restoreField].forEach { $0.isEnabled = !loading && !isBusy }
        shotMode.isEnabled = !loading && !isBusy && draft.editable
        advancedButton.isEnabled = !loading && !isBusy
        [makeActiveButton, testButton, revertButton, saveButton, addButton, openEditorButton].forEach { $0.isEnabled = !loading && !isBusy }
        if !loading { validateAndRefreshButtons() }
    }

    // MARK: Table

    func numberOfRows(in tableView: NSTableView) -> Int { profiles.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard row >= 0 && row < profiles.count else { return nil }
        let p = profiles[row]
        let cell = NSTableCellView()
        let badges = (detailsCache[p.id]?.badges ?? []).map { "[\($0)]" }.joined(separator: " ")
        let badgeSuffix = badges.isEmpty ? "" : "  \(badges)"
        let text = NSTextField(labelWithString: "\(p.isActive ? "✓ " : "")\(p.label)\(badgeSuffix)\n\(p.id) — \(p.host)")
        text.lineBreakMode = .byTruncatingTail
        text.maximumNumberOfLines = 2
        text.font = .systemFont(ofSize: 12)
        text.translatesAutoresizingMaskIntoConstraints = false
        cell.addSubview(text)
        NSLayoutConstraint.activate([
            text.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 6),
            text.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -6),
            text.centerYAnchor.constraint(equalTo: cell.centerYAnchor)
        ])
        return cell
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        guard !suppressSelectionChange else { return }
        guard !isBusy else {
            if let selectedID = selectedID, let idx = profiles.firstIndex(where: { $0.id == selectedID }) {
                suppressSelectionChange = true
                tableView.selectRowIndexes(IndexSet(integer: idx), byExtendingSelection: false)
                suppressSelectionChange = false
            }
            return
        }
        let row = tableView.selectedRow
        guard row >= 0 && row < profiles.count else { return }
        guard confirmDiscardIfNeeded() else {
            if let selectedID = selectedID, let idx = profiles.firstIndex(where: { $0.id == selectedID }) {
                suppressSelectionChange = true
                tableView.selectRowIndexes(IndexSet(integer: idx), byExtendingSelection: false)
                suppressSelectionChange = false
            }
            return
        }
        loadProfile(profiles[row].id)
    }

    func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat { 44 }

    func windowShouldClose(_ sender: NSWindow) -> Bool { confirmDiscardIfNeeded() }
}

extension ProfileManagerWindowController: NSTextFieldDelegate {
    func controlTextDidChange(_ obj: Notification) { validateAndRefreshButtons() }
}
