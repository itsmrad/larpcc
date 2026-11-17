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
    public let name: String
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
    /// Alias names follow the profile-name grammar. Provider command names need an
    /// explicit shadow flag that the GUI does not offer.
    public static func isValidAlias(_ name: String) -> Bool {
        CLIContract.name(name) && !["claude", "codex"].contains(name)
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
    case setSurface(profileID: String, surface: Surface, enabled: Bool)
    case setCatalog(kind: CatalogKind, itemID: String, profileID: String, enabled: Bool)
    case setSyncNewServers(Bool)
    case open(profileID: String, surface: Surface)
    case delete(profileID: String)
    case addAlias(name: String, profileID: String)
    case removeAlias(name: String)
}

public enum ClientError: Error, LocalizedError, Sendable {
    case staleSelection, commandUnavailable, qualificationPending, invalidResponse, invalidExecutable, timedOut, outputLimit, conflict, operationRefused, invalidName
    public var errorDescription: String? {
        switch self {
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
        switch action {
        case let .setSurface(id, surface, enabled):
            guard let index = state.profiles.firstIndex(where: { $0.id == id }) else { throw ClientError.staleSelection }
            switch surface {
            case .cli: state.profiles[index].cliEnabled = enabled
            case .desktop: state.profiles[index].desktopEnabled = enabled
            }
        case let .setCatalog(kind, itemID, profileID, enabled):
            guard state.profiles.contains(where: { $0.id == profileID }) else { throw ClientError.staleSelection }
            var items = kind == .mcp ? state.servers : state.skills
            guard let index = items.firstIndex(where: { $0.id == itemID }) else { throw ClientError.staleSelection }
            if enabled { items[index].profileIDs.insert(profileID) }
            else { items[index].profileIDs.remove(profileID) }
            if kind == .mcp { state.servers = items } else { state.skills = items }
        case let .setSyncNewServers(enabled): state.syncNewServers = enabled
        case let .open(id, _):
            guard state.profiles.contains(where: { $0.id == id }) else { throw ClientError.staleSelection }
            throw ClientError.qualificationPending
        case let .delete(id):
            guard state.profiles.contains(where: { $0.id == id }) else { throw ClientError.staleSelection }
            state.profiles.removeAll { $0.id == id }
            for index in state.servers.indices { state.servers[index].profileIDs.remove(id) }
            for index in state.skills.indices { state.skills[index].profileIDs.remove(id) }
        case let .addAlias(name, id):
            guard LarpNames.isValidAlias(name) else { throw ClientError.invalidName }
            guard let index = state.profiles.firstIndex(where: { $0.id == id }) else { throw ClientError.staleSelection }
            guard !state.profiles.contains(where: { $0.aliases.contains(name) }) else { throw ClientError.operationRefused }
            state.profiles[index].aliases = (state.profiles[index].aliases + [name]).sorted()
        case let .removeAlias(name):
            guard let index = state.profiles.firstIndex(where: { $0.aliases.contains(name) }) else { throw ClientError.staleSelection }
            state.profiles[index].aliases.removeAll { $0 == name }
        }
        return "Sample updated in memory. No files were changed."
    }
}
