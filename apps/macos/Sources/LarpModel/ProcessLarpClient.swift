import Foundation

public actor ProcessLarpClient: LarpClient {
    private let transport: any CLITransport
    private var displayed: Snapshot?
    private var generation = 0
    private var verified = false
    // Packaging smoke check, without CLI execution or provider access.
    public static func validateBundledContract() throws {
        let bytes = Data(#"{"schema":1,"command":"version","ok":true,"revision":null,"data":{"version":"0.1.0"}}"#.utf8) + Data([10])
        _ = try CLIContract.decode(bytes, exit: 0)
    }
    public init(selected: SelectedLarp) { transport = ProcessTransport(selected: selected) }
    init(transport: any CLITransport) { self.transport = transport }

    private func request(_ path: [String], revision: String? = nil) async throws -> CLIEnvelope {
        var argv = ["cc"] + path + ["--json"]
        if let revision { argv += ["--expected-revision", revision] }
        let result = try await transport.run(argv)
        try Task.checkCancellation()
        let e = try CLIContract.decode(result.stdout, exit: result.status)
        let expected = path.prefix(2).joined(separator: ".")
        guard e.command == expected else { throw ClientError.invalidResponse }
        guard e.ok else {
            switch e.error?.code {
            case "conflict": throw ClientError.conflict
            case "qualification_pending": throw ClientError.qualificationPending
            case "unsupported": throw ClientError.commandUnavailable
            default: throw ClientError.operationRefused
            }
        }
        if let revision, path == ["mcp", "matrix"] || path == ["skills", "matrix"] || path == ["mcp", "list"] || path == ["skills", "list"] {
            guard e.revision == revision else { throw ClientError.conflict }
        }
        return e
    }
    public func snapshot() async throws -> Snapshot {
        generation += 1
        let token = generation
        displayed = nil
        if !verified {
            let helpOutput = try await transport.run(["--help", "--json"])
            let help = try CLIContract.decode(helpOutput.stdout, exit: helpOutput.status)
            guard help.command == "help", help.ok, case let .text(text) = help.data,
                  text.hasPrefix("Usage: larp cc") else { throw ClientError.invalidExecutable }
            verified = true
        }
        let p = try await request(["profile", "list"])
        guard case let .profiles(profiles) = p.data, let revision = p.revision else { throw ClientError.invalidResponse }
        let m = try await request(["mcp", "matrix"], revision: revision)
        let s = try await request(["skills", "matrix"], revision: revision)
        let list = try await request(["mcp", "list"], revision: revision)
        let skills = try await request(["skills", "list"], revision: revision)
        guard case let .matrix(mcp) = m.data, case let .matrix(skill) = s.data,
              case let .mcp(metadata) = list.data, case let .skills(skillMetadata) = skills.data else { throw ClientError.invalidResponse }
        let columns = profiles.map { Column(id: $0.id, name: $0.name, provider: $0.provider) }
        guard mcp.profiles == columns, skill.profiles == columns,
              mcp.rows.map(\.name) == metadata.servers.map(\.name), skill.rows.map(\.name) == skillMetadata.skills.map(\.name) else { throw ClientError.invalidResponse }
        let ids = Set(profiles.map(\.id))
        for switches in metadata.servers.map(\.switches) + skillMetadata.skills.map(\.switches) {
            guard switches.allSatisfy({ ids.contains($0.profile_id) }) else { throw ClientError.invalidResponse }
        }
        func catalog(_ matrix: Matrix) -> [CatalogItem] {
            matrix.rows.map { row in CatalogItem(id: row.name, subtitle: "Local metadata only", profileIDs: Set(zip(columns, row.enabled).filter { $0.1 }.map { $0.0.id })) }
        }
        let state = Snapshot(revision: revision, profiles: profiles.map {
            Profile(id: $0.id, name: $0.name, provider: Provider(rawValue: $0.provider)!, cliEnabled: $0.surfaces.cli.enabled,
                    desktopEnabled: $0.surfaces.desktop.enabled,
                    needsAttention: $0.state != "active" || $0.surfaces.cli.state != "ok" || $0.surfaces.desktop.state != "ok" || $0.desktop_binding.state != "ok" || $0.aliases.contains { $0.state != "ok" }, aliases: $0.aliases.map(\.name), displayFolder: $0.surfaces.cli.path,
                    desktopBinding: DesktopBinding(state: DesktopBinding.State(rawValue: $0.desktop_binding.state) ?? .invalid, displayPath: $0.desktop_binding.path,
                                                   detectedVersion: $0.desktop_binding.detected_version, recordedVersion: $0.desktop_binding.recorded_version))
        }, servers: catalog(mcp), skills: catalog(skill), syncNewServers: metadata.sync_all,
           dataFolder: profiles.first.flatMap { Self.dataFolder(cliPath: $0.surfaces.cli.path, id: $0.id) })
        guard generation == token else { throw ClientError.staleSelection }
        try Task.checkCancellation()
        displayed = state
        return state
    }
    /// Not wired until the `larp cc usage` contract merges.
    public func usage(source: UsageSource) async throws -> [String: UsageReport] {
        throw ClientError.commandUnavailable
    }
    /// Display-only: `<root>/profiles/<id>/cli` yields `<root>`. Never used as a path.
    static func dataFolder(cliPath: String, id: String) -> String? {
        let suffix = "/profiles/\(id)/cli"
        guard cliPath.hasSuffix(suffix), cliPath.count > suffix.count else { return nil }
        return String(cliPath.dropLast(suffix.count))
    }
    static func arguments(_ action: LarpAction, snapshot: Snapshot) throws -> [String] {
        func name(_ id: String) throws -> String {
            guard let p = snapshot.profiles.first(where: { $0.id == id }) else { throw ClientError.staleSelection }
            return p.name
        }
        switch action {
        case let .setSurface(id, surface, enabled): return ["profile", "surface", try name(id), surface == .cli ? "cli" : "desktop", enabled ? "on" : "off"]
        case let .setCatalog(kind, item, id, enabled):
            guard (kind == .mcp ? snapshot.servers : snapshot.skills).contains(where: { $0.id == item }) else { throw ClientError.staleSelection }
            return [kind == .mcp ? "mcp" : "skills", enabled ? "enable" : "disable", item, try name(id)]
        case let .setSyncNewServers(on): return ["mcp", "sync-all", on ? "on" : "off"]
        case let .open(id, surface):
            guard surface == .desktop else { throw ClientError.commandUnavailable }
            return ["app", "open", try name(id)]
        case let .delete(id): return ["profile", "delete", try name(id), "--yes"]
        case let .addAlias(alias, id):
            guard LarpNames.isValidAlias(alias) else { throw ClientError.invalidName }
            return ["alias", "add", alias, try name(id)]
        case let .removeAlias(alias):
            guard snapshot.profiles.contains(where: { $0.aliases.contains(alias) }) else { throw ClientError.staleSelection }
            return ["alias", "remove", alias]
        }
    }
    public func perform(_ action: LarpAction) async throws -> String {
        guard let state = displayed, let revision = state.revision else { throw ClientError.staleSelection }
        let args = try Self.arguments(action, snapshot: state)
        generation += 1
        let token = generation
        displayed = nil // One user intent consumes one snapshot; never retry after a conflict.
        let result = try await request(args, revision: revision)
        guard token == generation else { throw ClientError.staleSelection }
        if case let .receipt(r) = result.data {
            guard state.profiles.contains(where: { $0.id == r.profile_id && $0.provider.rawValue == r.provider }) else { throw ClientError.invalidResponse }
            return "Larp reports local app \(r.outcome). Authentication, readiness and isolation were not observed."
        }
        return "Larp updated local metadata. Authentication was not observed."
    }
}
