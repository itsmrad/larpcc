import XCTest
@testable import LarpModel

final class EditingTests: XCTestCase {
    private func argv(_ action: LarpAction) async throws -> [String] {
        try ProcessLarpClient.arguments(action, snapshot: try await FixtureLarpClient().snapshot())
    }

    func testEveryEditingActionBuildsTheContractArgv() async throws {
        let cases: [(LarpAction, [String])] = [
            (.addProfile(name: "claude-work", provider: .claude), ["profile", "add", "claude-work", "--provider", "claude"]),
            (.addProfile(name: "codex-45", provider: .codex), ["profile", "add", "codex-45", "--provider", "codex"]),
            (.renameProfile(profileID: "1", newName: "home"), ["profile", "rename", "personal", "home"]),
            (.forget(profileID: "1"), ["profile", "forget", "personal", "--yes"]),
            (.recover(profileID: "1"), ["profile", "recover", "personal", "--yes"]),
            (.migrate, ["profile", "migrate"]),
            (.bindApp(provider: .claude, path: "/Applications/Claude.app", version: nil, replace: false),
             ["app", "bind", "claude", "/Applications/Claude.app"]),
            (.bindApp(provider: .codex, path: "/Applications/ChatGPT.app", version: "1.2.3", replace: true),
             ["app", "bind", "codex", "/Applications/ChatGPT.app", "--version", "1.2.3", "--replace"]),
            (.bindProvider(provider: .codex, path: "/opt/tools/codex", replace: false), ["provider", "bind", "codex", "/opt/tools/codex"]),
            (.bindProvider(provider: .claude, path: "/opt/tools/claude", replace: true), ["provider", "bind", "claude", "/opt/tools/claude", "--replace"]),
            (.addAlias(name: "claude", profileID: "1", desktop: false, shadow: true), ["alias", "add", "claude", "personal", "--shadow"]),
            (.addAlias(name: "chat", profileID: "1", desktop: true), ["alias", "add", "chat", "personal", "--desktop"]),
            (.addServer(name: "files", transport: .command(path: "/usr/local/bin/files-mcp", args: ["--stdio"], envRefs: ["FILES_TOKEN", "HOME_DIR"])),
             ["mcp", "add", "files", "--command", "/usr/local/bin/files-mcp", "--arg=--stdio", "--env-ref", "FILES_TOKEN", "--env-ref", "HOME_DIR"]),
            (.addServer(name: "docs", transport: .url("https://docs.example.com/mcp", headerRefs: [HeaderRef(header: "Authorization", envVar: "DOCS_TOKEN")])),
             ["mcp", "add", "docs", "--url", "https://docs.example.com/mcp", "--header-ref", "Authorization=DOCS_TOKEN"]),
            (.removeServer(name: "local-tools"), ["mcp", "remove", "local-tools"]),
            (.setServerForAll(name: "local-tools", enabled: true), ["mcp", "enable", "local-tools", "--all"]),
            (.setServerForAll(name: "local-tools", enabled: false), ["mcp", "disable", "local-tools", "--all"]),
            (.setSyncNewServers(false), ["mcp", "sync-all", "off"]),
            (.syncServers, ["mcp", "sync"]),
            (.addSkill(path: "/Users/me/skills/review", name: "review"), ["skills", "add", "/Users/me/skills/review", "--name", "review"]),
            (.removeSkill(name: "writing"), ["skills", "remove", "writing"]),
            (.syncSkills, ["skills", "sync"]),
            (.setCatalog(kind: .skill, itemID: "writing", profileID: "3", enabled: true), ["skills", "enable", "writing", "studio"])
        ]
        for (action, expected) in cases {
            let actual = try await argv(action)
            XCTAssertEqual(actual, expected)
        }
    }

    func testBadInputIsRefusedBeforeAnyProcess() async throws {
        let refused: [LarpAction] = [
            .addProfile(name: "claude", provider: .claude), .addProfile(name: "larp", provider: .codex),
            .addProfile(name: "Work", provider: .claude), .addProfile(name: "--yes", provider: .claude),
            .addProfile(name: String(repeating: "a", count: 33), provider: .claude),
            .renameProfile(profileID: "1", newName: "codex"), .renameProfile(profileID: "99", newName: "fine"),
            .addAlias(name: "claude", profileID: "1"), .addAlias(name: "codex", profileID: "1", desktop: true),
            .bindApp(provider: .claude, path: "Applications/Claude.app", version: nil, replace: false),
            .bindApp(provider: .claude, path: "/Applications/Claude", version: nil, replace: false),
            .bindApp(provider: .claude, path: "/Applications/Claude.app", version: "has space", replace: false),
            .bindProvider(provider: .codex, path: "codex", replace: false),
            .addServer(name: "x", transport: .command(path: "node server.js", args: [], envRefs: [])),
            .addServer(name: "x", transport: .command(path: "/bin/server", args: ["--token=abc"], envRefs: [])),
            .addServer(name: "x", transport: .command(path: "/bin/server", args: [], envRefs: ["sk-live-123=abc"])),
            .addServer(name: "x", transport: .command(path: "/bin/server", args: [], envRefs: ["TOKEN", "TOKEN"])),
            .addServer(name: "x", transport: .url("http://docs.example.com/mcp", headerRefs: [])),
            .addServer(name: "x", transport: .url("https://user:pass@docs.example.com/mcp", headerRefs: [])),
            .addServer(name: "x", transport: .url("https://docs.example.com/mcp?key=secret", headerRefs: [])),
            .addServer(name: "x", transport: .url("https://docs.example.com/api", headerRefs: [])),
            .addServer(name: "x", transport: .url("https://docs.example.com/mcp", headerRefs: [HeaderRef(header: "Authorization", envVar: "Bearer abc123")])),
            .addServer(name: "Bad Name", transport: .url("https://docs.example.com/mcp", headerRefs: [])),
            .addSkill(path: "relative/dir", name: "review"), .addSkill(path: "/tmp/review", name: "Review"),
            .removeServer(name: "missing"), .removeSkill(name: "missing"), .setServerForAll(name: "missing", enabled: true)
        ]
        for action in refused {
            do { _ = try await argv(action); XCTFail("\(action) must be refused") } catch {}
        }
        XCTAssertNil(MCPInput.problem(name: "docs", transport: .url("https://docs.example.com:8443/sse", headerRefs: [])))
        XCTAssertNotNil(MCPInput.problem(name: "docs", transport: .url("https://docs.example.com/mcp#frag", headerRefs: [])))
    }

    func testEachEditingCommandDecodesItsContractExample() async throws {
        let transport = try ExampleTransport()
        let client = ProcessLarpClient(transport: transport)
        // The frozen examples describe profile codex-45 (ID 2), server "example" and skill "example".
        let actions: [LarpAction] = [
            .addProfile(name: "claude-work", provider: .claude), .renameProfile(profileID: "2", newName: "codex-46"),
            .forget(profileID: "2"), .recover(profileID: "2"), .migrate,
            .bindApp(provider: .codex, path: "/Applications/ChatGPT.app", version: nil, replace: true),
            .bindProvider(provider: .codex, path: "/opt/codex", replace: false),
            .addAlias(name: "codex", profileID: "2", shadow: true), .removeAlias(name: "codex-45"),
            .addServer(name: "example", transport: .url("https://example.com/mcp", headerRefs: [])),
            .removeServer(name: "example"), .setServerForAll(name: "example", enabled: true), .syncServers,
            .addSkill(path: "/tmp/example", name: "example"), .removeSkill(name: "example"), .syncSkills
        ]
        for action in actions {
            _ = try await client.snapshot()
            _ = try await client.perform(action)
        }
        let calls = await transport.calls.filter { !["list", "matrix"].contains($0.dropFirst(2).first ?? "") && $0 != ["--help", "--json"] }
        XCTAssertEqual(calls.count, actions.count)
        for call in calls {
            XCTAssertEqual(Array(call.suffix(3)), ["--json", "--expected-revision", "7"])
        }
    }

    func testUnknownCommandAndErrorEnvelopesAreShownSafely() async throws {
        let server = LarpAction.addServer(name: "example", transport: .url("https://example.com/mcp", headerRefs: []))
        let bind = LarpAction.bindApp(provider: .codex, path: "/Applications/ChatGPT.app", version: nil, replace: false)
        let forget = LarpAction.forget(profileID: "2")
        let cases: [(String, String, Int32, Bool, Bool, LarpAction, ClientError)] = [
            // code, command, exit, help knows the command, revision moved, action, expected
            ("usage", "parse", 2, false, false, server, .unsupportedCommand),
            ("usage", "parse", 2, true, false, server, .refused(code: "usage")),
            ("unsupported", "mcp.add", 1, true, false, server, .unsupportedCommand),
            ("secret_literal", "mcp.add", 2, true, false, server, .refused(code: "secret_literal")),
            ("conflict", "mcp.add", 1, true, true, server, .conflict),
            ("conflict", "profile.forget", 1, true, false, forget, .blocked(command: "profile.forget")),
            ("conflict", "profile.forget", 1, true, true, forget, .conflict),
            ("conflict", "app.bind", 2, true, false, bind, .alreadyBound)
        ]
        for (code, command, status, known, moved, action, expected) in cases {
            let transport = try ExampleTransport(failing: ["schema": 1, "command": command, "ok": false,
                                                           "error": ["code": code, "message": "never shown"]],
                                                 status: status, knowsCommands: known, revisionMoves: moved)
            let client = ProcessLarpClient(transport: transport)
            _ = try await client.snapshot()
            do { _ = try await client.perform(action); XCTFail(code) } catch let error as ClientError {
                XCTAssertEqual(error, expected, "\(code) \(command)")
                XCTAssertFalse(error.errorDescription!.contains("never shown"))
            }
            let mutations = await transport.calls.filter { $0.contains("--expected-revision") && !["list", "matrix"].contains($0[2]) }
            XCTAssertEqual(mutations.count, 1, "never replayed")
        }
        XCTAssertNotEqual(ClientError.blocked(command: "profile.forget").errorDescription, ClientError.conflict.errorDescription)
    }

    func testDeleteWarningIsReadableAndUnknownCodesFallBack() async throws {
        let transport = try ExampleTransport()
        let client = ProcessLarpClient(transport: transport)
        _ = try await client.snapshot()
        let message = try await client.perform(.delete(profileID: "2"))
        XCTAssertFalse(message.contains("keychain_retained"))
        XCTAssertTrue(message.contains("Keychain items"))
        XCTAssertEqual(ProcessLarpClient.warningText(code: "new_code", message: "Safe text.\u{1b}[31m"), "Safe text.[31m")
    }

    func testCancelledMutationReportsUnknownOutcome() async throws {
        let transport = try ExampleTransport(hangMutations: true)
        let client = ProcessLarpClient(transport: transport)
        _ = try await client.snapshot()
        let task = Task { try await client.perform(.migrate) }
        try await Task.sleep(nanoseconds: 50_000_000)
        task.cancel()
        do { _ = try await task.value; XCTFail() } catch ClientError.outcomeUnknown {}
        let mutation = await transport.mutationFlags
        XCTAssertEqual(mutation, [false, false, false, false, false, false, true], "reads keep the timeout, mutations do not")
    }

    func testNamesRejectTrailingNewline() {
        XCTAssertFalse(LarpNames.isValid("work\n"))
        XCTAssertFalse(LarpNames.isValidProfile("work\n"))
        XCTAssertFalse(MCPInput.isEnvName("TOKEN\n"))
        XCTAssertFalse(MCPInput.isHeaderName("Authorization\n"))
        XCTAssertFalse(LarpNames.isVersionLabel("1.2\n"))
        XCTAssertTrue(LarpNames.isVersionLabel("1.2.3"))
    }

    func testSampleModeEditsInMemory() async throws {
        let client = FixtureLarpClient()
        _ = try await client.perform(.addProfile(name: "lab", provider: .codex))
        var state = try await client.snapshot()
        let lab = try XCTUnwrap(state.profiles.first { $0.name == "lab" })
        XCTAssertEqual(lab.id, "6")
        XCTAssertTrue(state.skills.allSatisfy { $0.profileIDs.contains("6") }, "new profiles inherit skills")
        XCTAssertFalse(state.servers.contains { $0.profileIDs.contains("6") }, "servers stay off without sync-all")
        do { _ = try await client.perform(.addProfile(name: "lab", provider: .claude)); XCTFail() } catch {}
        _ = try await client.perform(.renameProfile(profileID: "6", newName: "lab-two"))
        _ = try await client.perform(.addServer(name: "files", transport: .command(path: "/bin/files", args: ["--stdio"], envRefs: ["TOKEN"])))
        _ = try await client.perform(.setServerForAll(name: "files", enabled: true))
        _ = try await client.perform(.addSkill(path: "/tmp/review", name: "review"))
        do { _ = try await client.perform(.removeSkill(name: "review")); XCTFail() } catch ClientError.skillInUse {}
        do {
            _ = try await client.perform(.bindApp(provider: .claude, path: "/Applications/Claude.app", version: nil, replace: false))
            XCTFail()
        } catch ClientError.alreadyBound {}
        _ = try await client.perform(.bindApp(provider: .codex, path: "/Applications/ChatGPT.app", version: "1.0", replace: false))
        _ = try await client.perform(.addAlias(name: "claude", profileID: "1", shadow: true))
        state = try await client.snapshot()
        XCTAssertEqual(state.profiles.first { $0.id == "6" }?.name, "lab-two")
        XCTAssertEqual(state.servers.first { $0.id == "files" }?.profileIDs.count, state.profiles.count)
        XCTAssertEqual(state.profiles.first { $0.id == "3" }?.desktopBinding?.state, .ok)
        XCTAssertTrue(state.profiles.first { $0.id == "1" }?.aliases.contains("claude") == true)
        _ = try await client.perform(.removeServer(name: "files"))
        _ = try await client.perform(.forget(profileID: "6"))
        state = try await client.snapshot()
        XCTAssertFalse(state.servers.contains { $0.id == "files" })
        XCTAssertFalse((state.servers + state.skills).contains { $0.profileIDs.contains("6") })
    }
}

/// Answers each call with the frozen ok example for its command, or one fixed error for mutations.
private actor ExampleTransport: CLITransport {
    var calls = [[String]]()
    var mutationFlags = [Bool]()
    private let examples: [[String: Any]]
    private let failing: [String: Any]?
    private let status: Int32
    private let knowsCommands: Bool
    private let revisionMoves: Bool
    private let hangMutations: Bool
    private var failed = false
    init(failing: [String: Any]? = nil, status: Int32 = 1, knowsCommands: Bool = true,
         revisionMoves: Bool = false, hangMutations: Bool = false) throws {
        examples = try ProcessBridgeTests.examples(); self.failing = failing; self.status = status
        self.knowsCommands = knowsCommands; self.revisionMoves = revisionMoves; self.hangMutations = hangMutations
    }
    private func ok(_ command: String) throws -> ProcessOutput {
        var e = examples.first { $0["command"] as? String == command && $0["ok"] as? Bool == true }!
        if failed, revisionMoves, e["revision"] is String { e["revision"] = "8" }
        return ProcessOutput(stdout: try ProcessBridgeTests.bytes(e), status: 0)
    }
    func run(_ argv: [String], mutation: Bool) async throws -> ProcessOutput {
        calls.append(argv); mutationFlags.append(mutation)
        if argv.contains("--help"), argv.first == "cc" {
            // Per-command help: an envelope when the build knows the command, a parse error otherwise.
            if knowsCommands { return try ok("help") }
            return ProcessOutput(stdout: try ProcessBridgeTests.bytes(["schema": 1, "command": "parse", "ok": false,
                                                                       "error": ["code": "usage", "message": "x"]]), status: 2)
        }
        let command = argv.first == "--help" ? "help" : argv.dropFirst().prefix(2).joined(separator: ".")
        if mutation, hangMutations { try await Task.sleep(nanoseconds: 10_000_000_000) }
        if let failing, mutation {
            failed = true
            return ProcessOutput(stdout: try ProcessBridgeTests.bytes(failing), status: status)
        }
        return try ok(command)
    }
}
