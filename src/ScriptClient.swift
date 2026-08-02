import Darwin
import Foundation

struct ScriptResult {
    let stdout: String
    let stderr: String
    let status: Int32

    var succeeded: Bool { status == 0 }
    var sanitizedError: String { ScriptClient.sanitize(stderr.isEmpty ? stdout : stderr) }
}

final class ScriptClient {
    static let defaultTimeout: TimeInterval = 120

    private let executableOverride: String?
    private let fileManager: FileManager

    init(executable: String? = nil, fileManager: FileManager = .default) {
        self.executableOverride = executable
        self.fileManager = fileManager
    }

    func scriptPath() -> String {
        if let override = executableOverride, !override.isEmpty { return override }
        let home = fileManager.homeDirectoryForCurrentUser.path
        let sourceInstalls = [
            "\(home)/bin/ssh-img-paste",
            "\(home)/bin/vps-img-paste",
        ]
        let homebrewInstalls = [
            "/opt/homebrew/bin/ssh-img-paste",
            "/usr/local/bin/ssh-img-paste",
            "/opt/homebrew/bin/vps-img-paste",
            "/usr/local/bin/vps-img-paste",
        ]
        let homeApplications = "\(home)/Applications/"
        let bundledInHome = Bundle.main.bundleURL.standardizedFileURL.path.hasPrefix(homeApplications)
        let detectedInstalls = bundledInHome ? sourceInstalls + homebrewInstalls : homebrewInstalls + sourceInstalls
        let environmentInstalls = [
            ProcessInfo.processInfo.environment["SSH_IMG_PASTE_BIN"],
            ProcessInfo.processInfo.environment["VPS_IMG_PASTE_BIN"],
        ].compactMap { $0 }
        let candidates = environmentInstalls + detectedInstalls
        for candidate in candidates where fileManager.isExecutableFile(atPath: candidate) { return candidate }
        return sourceInstalls[0]
    }

    func runSync(_ arguments: [String], timeout: TimeInterval = ScriptClient.defaultTimeout) -> ScriptResult {
        let task = Process()
        task.executableURL = URL(fileURLWithPath: scriptPath())
        task.arguments = arguments
        let tempRoot = fileManager.temporaryDirectory.appendingPathComponent("ssh-img-paste-script-\(UUID().uuidString)", isDirectory: true)
        let outURL = tempRoot.appendingPathComponent("stdout")
        let errURL = tempRoot.appendingPathComponent("stderr")
        do {
            try fileManager.createDirectory(at: tempRoot, withIntermediateDirectories: true)
            fileManager.createFile(atPath: outURL.path, contents: nil)
            fileManager.createFile(atPath: errURL.path, contents: nil)
        } catch {
            return ScriptResult(stdout: "", stderr: "Could not create temporary command output files: \(error.localizedDescription)", status: 127)
        }

        guard let out = FileHandle(forWritingAtPath: outURL.path),
              let err = FileHandle(forWritingAtPath: errURL.path) else {
            try? fileManager.removeItem(at: tempRoot)
            return ScriptResult(stdout: "", stderr: "Could not open temporary command output files.", status: 127)
        }
        defer { try? fileManager.removeItem(at: tempRoot) }

        let exitGroup = DispatchGroup()
        exitGroup.enter()
        task.terminationHandler = { _ in exitGroup.leave() }
        task.standardOutput = out
        task.standardError = err
        do {
            try task.run()
        } catch {
            try? out.close()
            try? err.close()
            return ScriptResult(stdout: "", stderr: "Could not start ssh-img-paste: \(error.localizedDescription)", status: 127)
        }
        let childPID = task.processIdentifier
        let timedOut = !waitForTaskExit(exitGroup, timeout: timeout)
        if timedOut {
            signalDirectChild(pid: childPID, signal: SIGTERM)
            if !waitForTaskExit(exitGroup, timeout: 2) {
                signalDirectChild(pid: childPID, signal: SIGINT)
                if !waitForTaskExit(exitGroup, timeout: 1) {
                    signalDirectChild(pid: childPID, signal: SIGKILL)
                    waitForConfirmedTaskExit(task, pid: childPID, exitGroup: exitGroup)
                }
            }
        }
        try? out.close()
        try? err.close()
        let outData = (try? Data(contentsOf: outURL)) ?? Data()
        let errData = (try? Data(contentsOf: errURL)) ?? Data()
        if timedOut {
            let timeoutMessage = "Command timed out after \(Int(timeout)) seconds."
            let stderr = String(data: errData, encoding: .utf8) ?? ""
            return ScriptResult(stdout: String(data: outData, encoding: .utf8) ?? "",
                                stderr: stderr.isEmpty ? timeoutMessage : stderr + "\n" + timeoutMessage,
                                status: 124)
        }
        return ScriptResult(stdout: String(data: outData, encoding: .utf8) ?? "",
                            stderr: String(data: errData, encoding: .utf8) ?? "",
                            status: task.terminationStatus)
    }

    private func waitForTaskExit(_ exitGroup: DispatchGroup, timeout: TimeInterval) -> Bool {
        exitGroup.wait(timeout: .now() + timeout) == .success
    }

    private func signalDirectChild(pid: pid_t, signal: Int32) {
        guard pid > 0 else { return }
        if Darwin.kill(pid, signal) != 0, errno != ESRCH {
            return
        }
    }

    private func waitForConfirmedTaskExit(_ task: Process, pid: pid_t, exitGroup: DispatchGroup) {
        while exitGroup.wait(timeout: .now() + 0.1) != .success {
            if !task.isRunning && !isProcessAlive(pid) { return }
        }
    }

    private func isProcessAlive(_ pid: pid_t) -> Bool {
        guard pid > 0 else { return false }
        if Darwin.kill(pid, 0) == 0 { return true }
        return errno != ESRCH
    }

    func runAsync(_ arguments: [String], timeout: TimeInterval = ScriptClient.defaultTimeout, completion: @escaping (ScriptResult) -> Void) {
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self = self else { return }
            let result = self.runSync(arguments, timeout: timeout)
            DispatchQueue.main.async { completion(result) }
        }
    }

    func listProfiles() -> (profiles: [VPSProfile], ok: Bool, error: String?) {
        let result = runSync(["profiles"])
        guard result.succeeded else { return ([], false, result.sanitizedError) }
        return (VPSProfile.parseList(result.stdout), true, nil)
    }

    func inspectProfile(_ id: String) -> (details: ProfileDetails?, result: ScriptResult) {
        let result = runSync(["profile", "inspect", id])
        guard result.succeeded else { return (nil, result) }
        return (ProfileDetails.parseInspect(result.stdout), result)
    }

    static func sanitize(_ text: String, maxLength: Int = 1200) -> String {
        let scalars = text.unicodeScalars.map { scalar -> Character in
            if scalar.value == 9 || scalar.value == 10 || scalar.value == 13 || scalar.value >= 32 { return Character(scalar) }
            return "�"
        }
        var s = String(scalars).trimmingCharacters(in: .whitespacesAndNewlines)
        while s.contains("\n\n\n") { s = s.replacingOccurrences(of: "\n\n\n", with: "\n\n") }
        if s.count > maxLength {
            let idx = s.index(s.startIndex, offsetBy: maxLength)
            s = String(s[..<idx]) + "…"
        }
        return s.isEmpty ? "The command failed." : s
    }
}
