import Darwin
import Foundation

func expect(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("FAIL: \(message)\n", stderr)
        exit(1)
    }
}

func testProfileTSVParser() {
    let rows = """
    *\tdefault\tProduction\tprod-ssh
    \tstaging\tStaging\tdeploy@staging
    """
    let parsed = SSHProfile.parseList(rows)
    expect(parsed.count == 2, "expected two well-formed profile rows")
    expect(parsed[0] == SSHProfile(id: "default", label: "Production", host: "prod-ssh", isActive: true), "active profile parse")
    expect(parsed[1].id == "staging" && !parsed[1].isActive, "inactive profile parse")
}

func testMalformedRowsAreIgnored() {
    let rows = """
    *\tdefault\tProduction\tprod-ssh
    bad\ttoo\tfew
    \tbad id\tLabel\thost
    \tmissinghost\tLabel\t
    \tbadhost\tLabel\t-oProxyCommand=evil
    \tgood_2\tLabel\talias
    """
    let parsed = SSHProfile.parseList(rows)
    expect(parsed.map { $0.id } == ["default", "good_2"], "malformed rows ignored")
}

func testManualInspectParserAndBadges() {
    let inspect = """
    id\tdefault
    label\tManual Prod
    host\tprod
    remote_home\t/home/me
    remote_dir\tpublic/uploads
    shot_mode\tfull
    restore_seconds\t42
    kind\tmanual
    path\t/Users/me/.config/ssh-img-paste/profiles/prod.env
    editable\tfalse
    active\ttrue
    """
    guard let details = ProfileDetails.parseInspect(inspect) else {
        fputs("FAIL: inspect parser returned nil\n", stderr)
        exit(1)
    }
    expect(details.id == "default", "inspect id")
    expect(details.shotMode == "full", "inspect shot mode")
    expect(details.restoreSeconds == 42, "inspect restore seconds")
    expect(details.kind == .manual && details.editable == false && details.active == true, "inspect flags")
    expect(details.badges == ["Manual"], "manual badge")
}

func testAppDefaultRemainsEditable() {
    let inspect = """
    id\tdefault
    label\tDefault App
    host\tprod
    remote_home\t/home/user
    remote_dir\timg-uploads
    kind\tapp
    editable\ttrue
    active\tfalse
    """
    guard let details = ProfileDetails.parseInspect(inspect) else {
        fputs("FAIL: inspect parser returned nil for app default\n", stderr)
        exit(1)
    }
    expect(details.kind == .app, "preserve app kind")
    expect(details.badges.isEmpty, "editable app profile has no badge")
}

func testDraftValidation() {
    var draft = SSHProfileDraft(id: "new-1", label: "New", host: "ssh-alias", remoteHome: "/home/user", remoteDir: "img-uploads", shotMode: "region", restoreSeconds: 60)
    expect(draft.isValid, "valid draft")
    draft.remoteHome = "/tmp/uploads"
    expect(draft.isValid, "absolute tmp upload home is valid")
    draft.remoteHome = "~/www"
    expect(!draft.isValid && draft.validationErrors.contains { $0.contains("absolute path") }, "reject tilde remote home")
    draft.remoteHome = "/home/user"
    draft.id = "-bad"
    expect(!draft.isValid && draft.validationErrors.contains { $0.contains("Profile ID") }, "invalid id")
    draft.id = "ok"
    draft.host = "bad host"
    expect(!draft.isValid && draft.validationErrors.contains { $0.contains("SSH host") }, "invalid host whitespace")
    draft.host = "host;bad"
    expect(!draft.isValid && draft.validationErrors.contains { $0.contains("SSH host") }, "invalid host semicolon")
    draft.host = "bad$(x)"
    expect(!draft.isValid && draft.validationErrors.contains { $0.contains("SSH host") }, "invalid host command substitution")
    draft.host = "host"
    draft.remoteDir = "../uploads"
    expect(!draft.isValid && draft.validationErrors.contains { $0.contains("Upload folder") }, "reject traversal")
    for badDir in ["~", "a//b", "a*", "a?", "{x}", "[x]"] {
        draft.remoteDir = badDir
        expect(!draft.isValid && draft.validationErrors.contains { $0.contains("Upload folder") }, "reject remote dir \(badDir)")
    }
    draft.remoteDir = "uploads"
    draft.id = "éclair"
    expect(!draft.isValid && draft.validationErrors.contains { $0.contains("Profile ID") }, "reject unicode profile id")
    draft.id = "ok"
    draft.remoteDir = "img-uploads"
    draft.restoreSeconds = -1
    expect(!draft.isValid && draft.validationErrors.contains { $0.contains("restore delay") }, "restore seconds range")
}

func testProfileManagerInitialLoadLifecycle() {
    var normalOpen = ProfileManagerLoadState()
    expect(!normalOpen.shouldStartCreateOnShow(addMode: false, isLoading: true, profilesEmpty: true), "normal open waits for the initial profile load")
    expect(!normalOpen.shouldStartCreateAfterLoad(profilesEmpty: false), "normal open selects a loaded profile instead of creating a draft")

    var requestedAdd = ProfileManagerLoadState()
    expect(!requestedAdd.shouldStartCreateOnShow(addMode: true, isLoading: true, profilesEmpty: true), "add request waits for the initial profile load")
    expect(requestedAdd.shouldStartCreateAfterLoad(profilesEmpty: false), "explicit add request survives the initial profile load")

    var emptyLoad = ProfileManagerLoadState()
    expect(!emptyLoad.shouldStartCreateOnShow(addMode: false, isLoading: true, profilesEmpty: true), "empty state is not assumed before loading finishes")
    expect(emptyLoad.shouldStartCreateAfterLoad(profilesEmpty: true), "an actually empty profile list opens the create form")

    let blank = SSHProfileDraft.empty()
    expect(!blank.changed(from: blank), "untouched new profile form is not dirty")
}

func testArgumentConstruction() {
    let draft = SSHProfileDraft(originalID: "prod", id: "prod", label: "Prod", host: "prod-host", remoteHome: "/home/prod", remoteDir: "uploads", shotMode: "full", restoreSeconds: 5)
    expect(draft.updateArguments() == ["profile", "update", "prod", "--label", "Prod", "--host", "prod-host", "--remote-home", "/home/prod", "--remote-dir", "uploads", "--shot-mode", "full", "--restore-seconds", "5"], "update args")
    let create = SSHProfileDraft(id: "new", label: "New", host: "alias", remoteHome: "/home/user", remoteDir: "u")
    expect(create.createArguments().prefix(3) == ["profile", "create", "new"], "create args prefix")
    expect(SSHProfileDraft.renameArguments(oldID: "old", newID: "new") == ["profile", "rename", "old", "new"], "rename args")
    expect(SSHProfileDraft.deleteArguments(id: "old", switchTo: "new") == ["profile", "delete", "old", "--switch-to", "new"], "delete switch args")
}

func testScriptClientHandlesLargeStdoutAndStderr() {
    let tempDir = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("ssh-img-paste-test-\(UUID().uuidString)", isDirectory: true)
    let script = tempDir.appendingPathComponent("mock-ssh-img-paste")
    do {
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tempDir) }
        try """
        #!/bin/sh
        /usr/bin/yes stdout-line | /usr/bin/head -n 200000
        /usr/bin/yes stderr-line | /usr/bin/head -n 200000 >&2
        exit 3
        """.write(to: script, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: script.path)
        let result = ScriptClient(executable: script.path).runSync([])
        expect(result.status == 3, "large-output script status")
        expect(result.stdout.contains("stdout-line"), "large stdout captured")
        expect(result.stderr.contains("stderr-line"), "large stderr captured")
        expect(result.sanitizedError.count <= 1201, "sanitized display remains bounded")
    } catch {
        fputs("FAIL: large-output ScriptClient test setup failed: \(error)\n", stderr)
        exit(1)
    }
}

func testScriptResultParsesOnlyStructuredUploadKind() {
    let screenshot = ScriptResult(
        stdout: "upload\tshot\t/srv/dev/dev-images/shot-1.png\n",
        stderr: "",
        status: 0
    )
    expect(screenshot.uploadKind == .screenshot, "structured screenshot outcome")

    let clipboard = ScriptResult(
        stdout: "upload\tclip\t/srv/dev/dev-images/clip-1.png\n",
        stderr: "",
        status: 0
    )
    expect(clipboard.uploadKind == .clipboardImage, "structured clipboard-image outcome")

    let craftedHumanOutput = ScriptResult(
        stdout: "✓ Uploaded (Image) → /srv/dev/clip.png [Uploaded (Screenshot)]\n",
        stderr: "",
        status: 0
    )
    expect(craftedHumanOutput.uploadKind == nil, "human-readable profile labels cannot classify uploads")
}

func testScriptClientPassesEnvironmentOverrides() {
    let tempDir = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("ssh-img-paste-env-test-\(UUID().uuidString)", isDirectory: true)
    let script = tempDir.appendingPathComponent("mock-ssh-img-paste")
    do {
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tempDir) }
        try """
        #!/bin/sh
        printf '%s' "${SSH_IMG_PASTE_SUPPRESS_NOTIFICATIONS:-missing}"
        """.write(to: script, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: script.path)
        let result = ScriptClient(executable: script.path).runSync(
            [],
            environmentOverrides: ["SSH_IMG_PASTE_SUPPRESS_NOTIFICATIONS": "1"]
        )
        expect(result.status == 0, "environment override script status")
        expect(result.stdout == "1", "environment override reaches child process")
    } catch {
        fputs("FAIL: ScriptClient environment override test setup failed: \(error)\n", stderr)
        exit(1)
    }
}

func expectProcessIsGone(pid: pid_t, _ message: String) {
    errno = 0
    let result = Darwin.kill(pid, 0)
    expect(result == -1 && errno == ESRCH, message)
}

func testScriptClientTimeoutTerminatesProcess() {
    let tempDir = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("ssh-img-paste-timeout-test-\(UUID().uuidString)", isDirectory: true)
    let script = tempDir.appendingPathComponent("mock-ssh-img-paste")
    let pidFile = tempDir.appendingPathComponent("child.pid")
    do {
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tempDir) }
        try """
        #!/bin/sh
        trap '' TERM INT
        echo $$ > \(pidFile.path)
        while :; do sleep 1; done
        """.write(to: script, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: script.path)
        let started = Date()
        let result = ScriptClient(executable: script.path).runSync([], timeout: 1)
        expect(result.status == 124, "timeout script status")
        expect(Date().timeIntervalSince(started) < 6, "timeout returned after bounded terminate/interrupt/kill attempts")
        expect(result.sanitizedError.contains("timed out"), "timeout error message")
        let pidText = try String(contentsOf: pidFile, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        guard let childPID = pid_t(pidText) else {
            fputs("FAIL: timeout child pid was not recorded\n", stderr)
            exit(1)
        }
        expectProcessIsGone(pid: childPID, "timeout direct child was killed before runSync returned")
    } catch {
        fputs("FAIL: timeout ScriptClient test setup failed: \(error)\n", stderr)
        exit(1)
    }
}

@main
struct ProfileModelsTestRunner {
    static func main() {
        testProfileTSVParser()
        testMalformedRowsAreIgnored()
        testManualInspectParserAndBadges()
        testAppDefaultRemainsEditable()
        testDraftValidation()
        testProfileManagerInitialLoadLifecycle()
        testArgumentConstruction()
        testScriptClientHandlesLargeStdoutAndStderr()
        testScriptResultParsesOnlyStructuredUploadKind()
        testScriptClientPassesEnvironmentOverrides()
        testScriptClientTimeoutTerminatesProcess()
        print("ProfileModelsTests: PASS")
    }
}
