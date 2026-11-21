import Foundation

// Presentation values, not a CLI wire contract. Paths are display-only.
public enum Provider: String, Sendable, CaseIterable {
    case claude, codex
    public var title: String { rawValue.capitalized }
    public var symbol: String { self == .claude ? "sun.max" : "circle.hexagongrid" }
}

public enum Readiness: String, Sendable {
    case ready, attention, off
}

public struct Profile: Identifiable, Sendable, Equatable {
    public let id: String
    public var name: String
    public let provider: Provider
    public var cliEnabled: Bool
    public var desktopEnabled: Bool
    public let needsAttention: Bool
    public var aliases: [String]
    public var readiness: Readiness {
        if !cliEnabled && !desktopEnabled { return .off }
        return needsAttention ? .attention : .ready
    }
    public var displayFolder: String? = nil
    public var folder: String { displayFolder ?? "<root>/profiles/\(id)" }
    public var desktopBinding: DesktopBinding? = nil
    /// An interrupted delete left this profile in Larp's recovery state.
    public var recoverable = false
}

// Display-only desktop app binding metadata reported by Larp. Never a launch path.
public struct DesktopBinding: Sendable, Equatable {
    public enum State: String, Sendable { case unbound, ok, invalid }
    public let state: State
    public let displayPath: String?
    public let detectedVersion: String?
    public let recordedVersion: String?
}

public enum LarpNames {
    static let providerCommands = ["claude", "codex"]
    /// Profile, skill and MCP server names: `[a-z][a-z0-9_-]{0,31}`, not `larp` or a device name.
    public static func isValid(_ name: String) -> Bool { CLIContract.name(name) }
    /// Profiles also may not take a provider command name.
    public static func isValidProfile(_ name: String) -> Bool {
        CLIContract.name(name) && !providerCommands.contains(name)
    }
    /// A user's own desktop-app version note for `app bind --version`.
    public static func isVersionLabel(_ value: String) -> Bool {
        value.range(of: "^[A-Za-z0-9][A-Za-z0-9._+-]{0,31}\\z", options: .regularExpression) != nil
    }
    /// Alias names follow the profile-name grammar; `claude` and `codex` need `--shadow`.
    public static func isValidAlias(_ name: String, shadow: Bool = false) -> Bool {
        CLIContract.name(name) && (shadow || !providerCommands.contains(name))
    }
}

/// MCP server transport. Values are references only: env-var names, never secret literals.
public enum MCPTransport: Sendable, Equatable {
    case command(path: String, args: [String], envRefs: [String])
    case url(String, headerRefs: [HeaderRef])
}

public struct HeaderRef: Sendable, Equatable {
    public let header: String
    public let envVar: String
    public init(header: String, envVar: String) { self.header = header; self.envVar = envVar }
}

/// Mirrors the CLI's `mcp add` input rules so bad input is refused before any process runs.
public enum MCPInput {
    /// The only arguments the CLI accepts until a reviewed adapter amendment.
    public static let allowedArgs = ["--stdio", "--transport=stdio"]
    public static let maxRefs = 16

    public static func isEnvName(_ value: String) -> Bool {
        value.range(of: "^[A-Za-z_][A-Za-z0-9_]{0,63}\\z", options: .regularExpression) != nil
    }
    public static func isHeaderName(_ value: String) -> Bool {
        value.range(of: "^[A-Za-z0-9][A-Za-z0-9-]{0,63}\\z", options: .regularExpression) != nil
    }
    /// An absolute local path with no control characters.
    public static func isAbsolutePath(_ value: String) -> Bool {
        value.hasPrefix("/") && value.count <= 1024 && !value.unicodeScalars.contains { $0.properties.generalCategory == .control }
    }
    /// `https://host[:port]/mcp` or `/sse`, with no credentials, query or fragment.
    public static func isURL(_ value: String) -> Bool {
        guard let c = URLComponents(string: value), c.scheme == "https", let host = c.host, !host.isEmpty,
              !host.contains("%"), c.user == nil, c.password == nil, c.query == nil, c.fragment == nil,
              ["/mcp", "/sse"].contains(c.percentEncodedPath) else { return false }
        return true
    }
    /// A user-facing reason the input is refused, or nil when it is valid.
    public static func problem(name: String, transport: MCPTransport) -> String? {
        guard LarpNames.isValid(name) else { return "Use a lowercase name: letters, digits, - or _, up to 32 characters." }
        switch transport {
        case let .command(path, args, envRefs):
            guard isAbsolutePath(path) else { return "Choose the server program by its full path, starting with /." }
            guard args.allSatisfy(allowedArgs.contains), Set(args).count == args.count else { return "Only --stdio or --transport=stdio are allowed as arguments." }
            guard envRefs.count <= maxRefs, envRefs.allSatisfy(isEnvName) else { return "Environment references must be variable names like API_TOKEN." }
            guard Set(envRefs).count == envRefs.count else { return "Each environment reference may appear once." }
        case let .url(url, headers):
            guard isURL(url) else { return "Use an https URL ending in /mcp or /sse, without credentials, query or fragment." }
            guard headers.count <= maxRefs, headers.allSatisfy({ isHeaderName($0.header) && isEnvName($0.envVar) }) else {
                return "Headers need a header name and an environment variable name."
            }
            guard Set(headers.map { $0.header.lowercased() }).count == headers.count else { return "Each header may appear once." }
        }
        return nil
    }
}

public enum CatalogKind: Sendable { case mcp, skill }

public struct CatalogItem: Identifiable, Sendable, Equatable {
    public let id: String
    public let subtitle: String
    public var profileIDs: Set<String>
}

public struct Snapshot: Sendable {
    public var revision: String? = nil
    public var profiles: [Profile]
    public var servers: [CatalogItem]
    public var skills: [CatalogItem]
    public var syncNewServers: Bool
    /// Display-only data folder derived from Larp metadata; nil when unknown.
    public var dataFolder: String? = nil
    public var enabledCount: Int { profiles.filter { $0.cliEnabled || $0.desktopEnabled }.count }
}

public enum Surface: Sendable { case cli, desktop }
public enum LarpAction: Sendable {
    case addProfile(name: String, provider: Provider)
    case renameProfile(profileID: String, newName: String)
    case setSurface(profileID: String, surface: Surface, enabled: Bool)
    case delete(profileID: String)
    case forget(profileID: String)
    case recover(profileID: String)
    case migrate
    case open(profileID: String, surface: Surface)
    case bindApp(provider: Provider, path: String, version: String?, replace: Bool)
    case bindProvider(provider: Provider, path: String, replace: Bool)
    case addAlias(name: String, profileID: String, desktop: Bool = false, shadow: Bool = false)
    case removeAlias(name: String)
    case addServer(name: String, transport: MCPTransport)
    case removeServer(name: String)
    case setServerForAll(name: String, enabled: Bool)
    case setSyncNewServers(Bool)
    case syncServers
    case addSkill(path: String, name: String)
    case removeSkill(name: String)
    case syncSkills
    case setCatalog(kind: CatalogKind, itemID: String, profileID: String, enabled: Bool)
}

public enum ClientError: Error, LocalizedError, Sendable, Equatable {
    case staleSelection, commandUnavailable, qualificationPending, invalidResponse, invalidExecutable, timedOut, outputLimit, conflict, operationRefused, invalidName
    case invalidInput, unsupportedCommand, alreadyBound, skillInUse, outcomeUnknown
    /// A `conflict` with an unchanged revision: the registry state blocks the command.
    case blocked(command: String)
    /// A CLI error envelope, described by its stable code (the message is never shown).
    case refused(code: String)
    public var errorDescription: String? {
        switch self {
        case .invalidInput: return "Check the entered values. Nothing was changed."
        case .unsupportedCommand: return "This action is not supported by this larp yet. Nothing was changed."
        case .outcomeUnknown: return "Outcome unknown — refresh to see the current state."
        case let .blocked(command):
            switch command {
            case "profile.forget": return "Larp can't forget this profile yet. Remove its command names and shared items first."
            case "skills.remove": return "Turn this skill off for every profile before removing it."
            default: return "Larp refused because existing items depend on this. Nothing was changed."
            }
        case .alreadyBound: return "An app is already bound. Turn on Replace to change it."
        case .skillInUse: return "Turn this skill off for every profile before removing it."
        case let .refused(code):
            switch code {
            case "busy": return "The profile is in use. Quit its sessions and try again."
            case "not_found": return "Larp couldn't find that item. Refresh and try again."
            case "unsafe_path": return "Larp refused an unsafe path."
            case "drift": return "Files changed outside larp. Repair them before trying again."
            case "secret_literal": return "Larp refused a value that may be a secret. Use an environment variable name instead."
            case "secret_reference": return "Larp can't use one of the referenced variables."
            case "too_large": return "The result would exceed larp's size limit."
            case "recovery_required": return "Larp needs to recover an interrupted operation first."
            case "io": return "Larp couldn't read or write its local files."
            case "corrupt": return "Larp's local metadata looks damaged."
            case "newer_schema": return "This data was written by a newer larp."
            case "exhausted": return "Larp has no IDs left for new profiles."
            case "confirmation_required": return "Larp needs confirmation for this action."
            case "usage": return "Larp rejected the request's arguments. Nothing was changed."
            default: return "Larp refused the operation. Check local metadata and recovery state."
            }
        case .invalidName: return "Use a new lowercase name: letters, digits, - or _, up to 32 characters."
        case .invalidResponse: return "Larp returned an unsupported or invalid response."
        case .invalidExecutable: return "Select a local executable named larp. The selected file must remain unchanged."
        case .timedOut: return "Larp timed out. Refresh before trying again."
        case .outputLimit: return "Larp exceeded the safe output limit."
        case .conflict: return "Registry changed. Review the refreshed snapshot and choose the action again."
        case .operationRefused: return "Larp refused the operation. Check local metadata and recovery state."
        case .staleSelection: return "This selection is no longer available. Refresh and try again."
        case .commandUnavailable: return "The Larp command is unavailable. Sample mode remains available."
        case .qualificationPending: return "Native launch qualification is pending. No app was opened."
        }
    }
}

public protocol LarpClient: Sendable {
    func snapshot() async throws -> Snapshot
    func perform(_ action: LarpAction) async throws -> String
}

// Sample mode has no filesystem, process, provider or network access.
public actor FixtureLarpClient: LarpClient {
    private var state: Snapshot
    private static let claudeApp = DesktopBinding(state: .ok, displayPath: "/Applications/Claude.app", detectedVersion: nil, recordedVersion: nil)
    private static let chatGPTApp = DesktopBinding(state: .unbound, displayPath: nil, detectedVersion: nil, recordedVersion: nil)
    public init() {
        let profiles = [
            Profile(id: "1", name: "personal", provider: .claude, cliEnabled: true, desktopEnabled: true,
                    needsAttention: false, aliases: ["claude-personal"], desktopBinding: Self.claudeApp),
            Profile(id: "2", name: "work", provider: .claude, cliEnabled: true, desktopEnabled: false,
                    needsAttention: true, aliases: ["work-agent", "review"], desktopBinding: Self.claudeApp),
            Profile(id: "3", name: "studio", provider: .codex, cliEnabled: true, desktopEnabled: true,
                    needsAttention: false, aliases: ["studio"], desktopBinding: Self.chatGPTApp),
            Profile(id: "4", name: "research", provider: .codex, cliEnabled: true, desktopEnabled: false,
                    needsAttention: true, aliases: [], desktopBinding: Self.chatGPTApp),
            Profile(id: "5", name: "sandbox", provider: .claude, cliEnabled: false, desktopEnabled: false,
                    needsAttention: false, aliases: [], desktopBinding: Self.claudeApp)
        ].sorted { $0.name < $1.name }
        state = Snapshot(profiles: profiles, servers: [
            CatalogItem(id: "documentation", subtitle: "HTTP · definition only", profileIDs: ["1", "2", "3", "4"]),
            CatalogItem(id: "local-tools", subtitle: "Standard IO · definition only", profileIDs: ["1", "3"]),
            CatalogItem(id: "project-search", subtitle: "Standard IO · definition only", profileIDs: ["2"])
        ], skills: [
            CatalogItem(id: "code-review", subtitle: "Shared skill · sample", profileIDs: ["1", "2", "3"]),
            CatalogItem(id: "writing", subtitle: "Shared skill · sample", profileIDs: ["1", "4"])
        ], syncNewServers: false)
    }
    public func snapshot() async throws -> Snapshot { state }

    public func perform(_ action: LarpAction) async throws -> String {
        func index(_ id: String) throws -> Int {
            guard let i = state.profiles.firstIndex(where: { $0.id == id }) else { throw ClientError.staleSelection }
            return i
        }
        func remove(_ id: String) {
            state.profiles.removeAll { $0.id == id }
            for i in state.servers.indices { state.servers[i].profileIDs.remove(id) }
            for i in state.skills.indices { state.skills[i].profileIDs.remove(id) }
        }
        func unusedName(_ name: String) throws {
            guard !state.profiles.contains(where: { $0.name == name }) else { throw ClientError.refused(code: "conflict") }
        }
        switch action {
        case let .addProfile(name, provider):
            guard LarpNames.isValidProfile(name) else { throw ClientError.invalidName }
            try unusedName(name)
            let id = String((state.profiles.compactMap { UInt64($0.id) }.max() ?? 0) + 1)
            let binding = state.profiles.first { $0.provider == provider }?.desktopBinding
                ?? DesktopBinding(state: .unbound, displayPath: nil, detectedVersion: nil, recordedVersion: nil)
            state.profiles.append(Profile(id: id, name: name, provider: provider, cliEnabled: true, desktopEnabled: false,
                                          needsAttention: false, aliases: [], desktopBinding: binding))
            state.profiles.sort { $0.name < $1.name }
            // New profiles inherit every stored skill, and every server when sync-all is on.
            for i in state.skills.indices { state.skills[i].profileIDs.insert(id) }
            if state.syncNewServers { for i in state.servers.indices { state.servers[i].profileIDs.insert(id) } }
        case let .renameProfile(id, newName):
            guard LarpNames.isValidProfile(newName) else { throw ClientError.invalidName }
            let i = try index(id)
            try unusedName(newName)
            state.profiles[i].name = newName
            state.profiles.sort { $0.name < $1.name }
        case let .setSurface(id, surface, enabled):
            let i = try index(id)
            switch surface {
            case .cli: state.profiles[i].cliEnabled = enabled
            case .desktop: state.profiles[i].desktopEnabled = enabled
            }
        case let .delete(id), let .forget(id):
            _ = try index(id)
            remove(id)
        case let .recover(id):
            let i = try index(id)
            state.profiles[i].recoverable = false
        case .migrate, .syncServers, .syncSkills, .bindProvider:
            break
        case let .open(id, _):
            _ = try index(id)
            throw ClientError.qualificationPending
        case let .bindApp(provider, path, version, replace):
            guard MCPInput.isAbsolutePath(path), path.hasSuffix(".app") else { throw ClientError.invalidInput }
            let current = state.profiles.first { $0.provider == provider }?.desktopBinding
            if current?.state == .ok, !replace { throw ClientError.alreadyBound }
            let binding = DesktopBinding(state: .ok, displayPath: path, detectedVersion: nil, recordedVersion: version)
            for i in state.profiles.indices where state.profiles[i].provider == provider { state.profiles[i].desktopBinding = binding }
        case let .addAlias(name, id, _, shadow):
            guard LarpNames.isValidAlias(name, shadow: shadow) else { throw ClientError.invalidName }
            let i = try index(id)
            guard !state.profiles.contains(where: { $0.aliases.contains(name) }) else { throw ClientError.operationRefused }
            state.profiles[i].aliases = (state.profiles[i].aliases + [name]).sorted()
        case let .removeAlias(name):
            guard let i = state.profiles.firstIndex(where: { $0.aliases.contains(name) }) else { throw ClientError.staleSelection }
            state.profiles[i].aliases.removeAll { $0 == name }
        case let .addServer(name, transport):
            guard MCPInput.problem(name: name, transport: transport) == nil else { throw ClientError.invalidInput }
            guard !state.servers.contains(where: { $0.id == name }) else { throw ClientError.refused(code: "conflict") }
            let subtitle: String
            switch transport {
            case let .command(_, _, envRefs): subtitle = "Standard IO" + (envRefs.isEmpty ? "" : " · \(envRefs.count) env ref\(envRefs.count == 1 ? "" : "s")")
            case let .url(_, headers): subtitle = "HTTP" + (headers.isEmpty ? "" : " · \(headers.count) header ref\(headers.count == 1 ? "" : "s")")
            }
            // New definitions start off unless sync-all is on.
            let on = state.syncNewServers ? Set(state.profiles.filter(\.cliEnabled).map(\.id)) : []
            state.servers.append(CatalogItem(id: name, subtitle: subtitle, profileIDs: on))
            state.servers.sort { $0.id < $1.id }
        case let .removeServer(name):
            guard state.servers.contains(where: { $0.id == name }) else { throw ClientError.staleSelection }
            state.servers.removeAll { $0.id == name }
        case let .setServerForAll(name, enabled):
            guard let i = state.servers.firstIndex(where: { $0.id == name }) else { throw ClientError.staleSelection }
            state.servers[i].profileIDs = enabled ? Set(state.profiles.map(\.id)) : []
        case let .setSyncNewServers(enabled):
            state.syncNewServers = enabled
            // Turning it on enables every current server for every active CLI profile.
            if enabled {
                let ids = Set(state.profiles.filter(\.cliEnabled).map(\.id))
                for i in state.servers.indices { state.servers[i].profileIDs.formUnion(ids) }
            }
        case let .addSkill(path, name):
            guard LarpNames.isValid(name), MCPInput.isAbsolutePath(path) else { throw ClientError.invalidInput }
            guard !state.skills.contains(where: { $0.id == name }) else { throw ClientError.refused(code: "conflict") }
            // New skills start on for every active CLI profile.
            state.skills.append(CatalogItem(id: name, subtitle: "Shared skill · sample",
                                            profileIDs: Set(state.profiles.filter(\.cliEnabled).map(\.id))))
            state.skills.sort { $0.id < $1.id }
        case let .removeSkill(name):
            guard let i = state.skills.firstIndex(where: { $0.id == name }) else { throw ClientError.staleSelection }
            guard state.skills[i].profileIDs.isEmpty else { throw ClientError.skillInUse }
            state.skills.remove(at: i)
        case let .setCatalog(kind, itemID, profileID, enabled):
            _ = try index(profileID)
            var items = kind == .mcp ? state.servers : state.skills
            guard let i = items.firstIndex(where: { $0.id == itemID }) else { throw ClientError.staleSelection }
            if enabled { items[i].profileIDs.insert(profileID) } else { items[i].profileIDs.remove(profileID) }
            if kind == .mcp { state.servers = items } else { state.skills = items }
        }
        return "Sample updated in memory. No files were changed."
    }
}
