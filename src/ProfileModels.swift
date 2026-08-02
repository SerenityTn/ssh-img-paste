import Foundation

struct ProfileManagerLoadState {
    private var createAfterLoad = false

    mutating func shouldStartCreateOnShow(addMode: Bool, isLoading: Bool, profilesEmpty: Bool) -> Bool {
        if isLoading {
            createAfterLoad = createAfterLoad || addMode
            return false
        }
        return addMode || profilesEmpty
    }

    mutating func shouldStartCreateAfterLoad(profilesEmpty: Bool) -> Bool {
        defer { createAfterLoad = false }
        return createAfterLoad || profilesEmpty
    }
}

struct VPSProfile: Equatable {
    let id: String
    let label: String
    let host: String
    let isActive: Bool

    var displayName: String { label.isEmpty ? id : label }

    static func parseList(_ output: String) -> [VPSProfile] {
        var profiles: [VPSProfile] = []
        for line in output.split(separator: "\n", omittingEmptySubsequences: true) {
            let parts = line.split(separator: "\t", maxSplits: 3, omittingEmptySubsequences: false)
            guard parts.count == 4 else { continue }
            let marker = String(parts[0])
            let id = String(parts[1]).trimmed
            let label = String(parts[2]).trimmed
            let host = String(parts[3]).trimmed
            guard VPSProfileDraft.isValidID(id), !label.isEmpty, VPSProfileDraft.isValidHost(host) else { continue }
            profiles.append(VPSProfile(id: id, label: label, host: host, isActive: marker == "*"))
        }
        return profiles
    }
}

enum ProfileKind: String, Equatable {
    case app
    case legacy
    case manual
    case env

    init(rawInspectValue: String) {
        switch rawInspectValue.trimmed.lowercased() {
        case "app": self = .app
        case "legacy": self = .legacy
        case "manual": self = .manual
        case "env": self = .env
        default: self = .app
        }
    }
}

struct ProfileDetails: Equatable {
    var id: String
    var label: String
    var host: String
    var remoteHome: String
    var remoteDir: String
    var shotMode: String
    var restoreSeconds: Int
    var kind: ProfileKind
    var path: String
    var editable: Bool
    var active: Bool

    var badges: [String] {
        var result: [String] = []
        if kind == .legacy || kind == .env { result.append("Legacy") }
        if kind == .manual || !editable { result.append("Manual") }
        return result
    }

    static func parseInspect(_ output: String) -> ProfileDetails? {
        var fields: [String: String] = [:]
        for line in output.split(separator: "\n", omittingEmptySubsequences: true) {
            let parts = line.split(separator: "\t", maxSplits: 1, omittingEmptySubsequences: false)
            guard parts.count == 2 else { continue }
            fields[String(parts[0]).trimmed] = String(parts[1]).trimmed
        }
        guard let id = fields["id"], VPSProfileDraft.isValidID(id),
              let label = fields["label"], !label.isEmpty,
              let host = fields["host"], VPSProfileDraft.isValidHost(host) else { return nil }
        let shotMode = fields["shot_mode"].flatMap { VPSProfileDraft.validShotModes.contains($0) ? $0 : nil } ?? "region"
        let seconds = Int(fields["restore_seconds"] ?? "") ?? 60
        return ProfileDetails(
            id: id,
            label: label,
            host: host,
            remoteHome: fields["remote_home"] ?? "",
            remoteDir: fields["remote_dir"] ?? "img-uploads",
            shotMode: shotMode,
            restoreSeconds: seconds,
            kind: ProfileKind(rawInspectValue: fields["kind"] ?? "app"),
            path: fields["path"] ?? "",
            editable: ProfileDetails.parseBool(fields["editable"], defaultValue: true),
            active: ProfileDetails.parseBool(fields["active"], defaultValue: false)
        )
    }

    private static func parseBool(_ value: String?, defaultValue: Bool) -> Bool {
        guard let v = value?.trimmed.lowercased() else { return defaultValue }
        return ["1", "true", "yes", "y"].contains(v)
    }
}

struct VPSProfileDraft: Equatable {
    static let validShotModes = ["region", "full"]

    var originalID: String?
    var id: String
    var label: String
    var host: String
    var remoteHome: String
    var remoteDir: String
    var shotMode: String
    var restoreSeconds: Int
    var editable: Bool

    var isNew: Bool { originalID == nil }

    init(originalID: String? = nil, id: String, label: String, host: String, remoteHome: String, remoteDir: String, shotMode: String = "region", restoreSeconds: Int = 60, editable: Bool = true) {
        self.originalID = originalID
        self.id = id
        self.label = label
        self.host = host
        self.remoteHome = remoteHome
        self.remoteDir = remoteDir
        self.shotMode = shotMode
        self.restoreSeconds = restoreSeconds
        self.editable = editable
    }

    init(details: ProfileDetails) {
        self.init(originalID: details.id, id: details.id, label: details.label, host: details.host, remoteHome: details.remoteHome, remoteDir: details.remoteDir, shotMode: details.shotMode, restoreSeconds: details.restoreSeconds, editable: details.editable)
    }

    static func empty() -> VPSProfileDraft {
        VPSProfileDraft(id: "", label: "", host: "", remoteHome: "", remoteDir: "img-uploads", shotMode: "region", restoreSeconds: 60)
    }

    var validationErrors: [String] {
        var errors: [String] = []
        if !Self.isValidID(id) { errors.append("Profile ID must start with a letter or number and contain only letters, numbers, _ or -.") }
        if Self.containsControl(id) || Self.containsControl(label) || Self.containsControl(host) || Self.containsControl(remoteHome) || Self.containsControl(remoteDir) { errors.append("Fields cannot contain tabs, newlines, or control characters.") }
        if label.trimmed.isEmpty { errors.append("Display name is required.") }
        if !Self.isValidHost(host) { errors.append("SSH host/alias is required and cannot contain whitespace or start with '-'.") }
        if !Self.isValidRemoteHome(remoteHome) { errors.append("Remote home must be a safe absolute path.") }
        if !Self.isValidRemotePath(remoteDir, allowRelative: true) { errors.append("Upload folder must be a safe path without quotes, traversal, or shell metacharacters.") }
        if !Self.validShotModes.contains(shotMode) { errors.append("Screenshot mode must be region or full.") }
        if restoreSeconds < 0 || restoreSeconds > 86400 { errors.append("Clipboard restore delay must be between 0 and 86400 seconds.") }
        if !editable { errors.append("Manual profiles are read-only in the app.") }
        return Array(Set(errors)).sorted()
    }

    var isValid: Bool { validationErrors.isEmpty }

    func changed(from original: VPSProfileDraft?) -> Bool {
        guard let original = original else { return true }
        return self != original
    }

    func createArguments() -> [String] {
        var args = ["profile", "create", id]
        appendMutationFlags(to: &args)
        return args
    }

    func updateArguments() -> [String] {
        let target = originalID ?? id
        var args = ["profile", "update", target]
        appendMutationFlags(to: &args)
        return args
    }

    func testArguments() -> [String] { ["profile", "test", originalID ?? id] }
    func useArguments() -> [String] { ["profile", "use", originalID ?? id] }

    static func renameArguments(oldID: String, newID: String) -> [String] { ["profile", "rename", oldID, newID] }
    static func deleteArguments(id: String, switchTo: String?) -> [String] {
        var args = ["profile", "delete", id]
        if let switchTo = switchTo, !switchTo.isEmpty { args += ["--switch-to", switchTo] }
        return args
    }

    private func appendMutationFlags(to args: inout [String]) {
        args += ["--label", label.trimmed]
        args += ["--host", host.trimmed]
        args += ["--remote-home", remoteHome.trimmed]
        args += ["--remote-dir", remoteDir.trimmed]
        args += ["--shot-mode", shotMode]
        args += ["--restore-seconds", String(restoreSeconds)]
    }

    static func isValidID(_ id: String) -> Bool {
        guard let first = id.unicodeScalars.first, Self.isASCIIAlphaNumeric(first) else { return false }
        return id.unicodeScalars.allSatisfy { Self.isASCIIAlphaNumeric($0) || $0 == "_" || $0 == "-" }
    }

    static func isValidHost(_ host: String) -> Bool {
        let h = host.trimmed
        return !h.isEmpty && !h.hasPrefix("-") && !h.contains(where: { $0.isWhitespace || $0.isNewline || $0.isControl }) && !containsShellMeta(h)
    }

    static func isValidRemotePath(_ path: String, allowRelative: Bool = false) -> Bool {
        let p = path.trimmed
        guard !p.isEmpty, !containsControl(p) else { return false }
        guard !containsShellMeta(p), containsOnlySafePathCharacters(p), !p.contains("//") else { return false }
        let comps = p.split(separator: "/", omittingEmptySubsequences: true)
        guard !comps.contains("..") else { return false }
        if allowRelative {
            return !p.hasPrefix("/") && !p.hasPrefix("-") && !p.hasPrefix(".")
        }
        return p.hasPrefix("/")
    }

    static func isValidRemoteHome(_ path: String) -> Bool {
        let p = path.trimmed
        return p.hasPrefix("/") && isValidRemotePath(p, allowRelative: false)
    }

    private static func isASCIIAlphaNumeric(_ scalar: UnicodeScalar) -> Bool {
        (scalar.value >= 48 && scalar.value <= 57) || (scalar.value >= 65 && scalar.value <= 90) || (scalar.value >= 97 && scalar.value <= 122)
    }

    private static func containsShellMeta(_ s: String) -> Bool {
        s.rangeOfCharacter(from: CharacterSet(charactersIn: "'\"`$;&|<>(){}[]*?\\")) != nil
    }

    private static func containsOnlySafePathCharacters(_ s: String) -> Bool {
        s.unicodeScalars.allSatisfy { scalar in
            isASCIIAlphaNumeric(scalar) || scalar == "." || scalar == "_" || scalar == "-" || scalar == "/"
        }
    }

    private static func containsControl(_ s: String) -> Bool { s.contains(where: { $0.isNewline || $0.isControl || $0 == "\t" }) }
}

private extension String {
    var trimmed: String { trimmingCharacters(in: .whitespacesAndNewlines) }
}

private extension Character {
    var isControl: Bool { unicodeScalars.allSatisfy { $0.value < 32 || $0.value == 127 } }
}
