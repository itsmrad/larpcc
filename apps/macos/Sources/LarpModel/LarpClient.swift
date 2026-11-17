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
    public let aliases: [String]
    public var readiness: Readiness {
        if !cliEnabled && !desktopEnabled { return .off }
        return needsAttention ? .attention : .ready
    }
    public var folder: String { "<root>/profiles/\(id)" }
}

public enum CatalogKind: Sendable { case mcp, skill }

public struct CatalogItem: Identifiable, Sendable, Equatable {
    public let id: String
    public let subtitle: String
    public var profileIDs: Set<String>
}

public struct Snapshot: Sendable {
    public var profiles: [Profile]
    public var servers: [CatalogItem]
    public var skills: [CatalogItem]
    public var syncNewServers: Bool
    public var enabledCount: Int { profiles.filter { $0.cliEnabled || $0.desktopEnabled }.count }
}

public enum Surface: Sendable { case cli, desktop }
public enum LarpAction: Sendable {
    case setSurface(profileID: String, surface: Surface, enabled: Bool)
    case setCatalog(kind: CatalogKind, itemID: String, profileID: String, enabled: Bool)
    case setSyncNewServers(Bool)
    case open(profileID: String, surface: Surface)
    case delete(profileID: String)
}

public enum ClientError: Error, LocalizedError, Sendable {
    case staleSelection, commandUnavailable, qualificationPending
    public var errorDescription: String? {
        switch self {
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
    public init() {
        let profiles = [
            Profile(id: "1", name: "personal", provider: .claude, cliEnabled: true, desktopEnabled: true,
                    needsAttention: false, aliases: ["claude-personal"]),
            Profile(id: "2", name: "work", provider: .claude, cliEnabled: true, desktopEnabled: false,
                    needsAttention: true, aliases: ["work-agent", "review"]),
            Profile(id: "3", name: "studio", provider: .codex, cliEnabled: true, desktopEnabled: true,
                    needsAttention: false, aliases: ["studio"]),
            Profile(id: "4", name: "research", provider: .codex, cliEnabled: true, desktopEnabled: false,
                    needsAttention: true, aliases: []),
            Profile(id: "5", name: "sandbox", provider: .claude, cliEnabled: false, desktopEnabled: false,
                    needsAttention: false, aliases: [])
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
        }
        return "Sample updated in memory. No files were changed."
    }
}
