import XCTest
@testable import LarpModel

final class ProcessBridgeTests: XCTestCase {
    static func examples() throws -> [[String: Any]] {
        let url = Bundle.module.url(forResource: "cli-v1.examples", withExtension: "json")!
        return try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as! [[String: Any]]
    }
    static func bytes(_ object: [String: Any]) throws -> Data {
        var bytes = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]); bytes.append(10); return bytes
    }
    func testEveryFrozenExampleAndErrorVariant() throws {
        let prospectiveCommands: Set<String> = ["status.account", "host.list", "host.show", "host.check", "host.catalog.list",
                                                 "settings.usage", "settings.usage.live", "settings.usage.interval",
                                                 "provider.evidence", "provider.detect"]
        var checkedProspective: Set<String> = []
        for (index, e) in try Self.examples().enumerated() {
            if e["ok"] as? Bool == true, let command = e["command"] as? String,
               prospectiveCommands.contains(command) {
                let bytes = try Self.bytes(e)
                XCTAssertThrowsError(try CLIContract.decode(bytes, exit: 0), "Prospective command \(command) must fail closed until app support lands")
                checkedProspective.insert(command)
                continue
            }
            do {
                let result = try CLIContract.decode(Self.bytes(e), exit: e["ok"] as? Bool == true ? 0 : 1)
                XCTAssertEqual(result.command, e["command"] as? String)
            } catch {
                XCTFail("Frozen example index \(index), command \(e["command"] as? String ?? "missing") failed decoding")
            }
        }
        XCTAssertEqual(checkedProspective, prospectiveCommands)
    }
    func testCopiesMatchMergedContract() throws {
        // The workflow also compares these bytes to the repository contract.
        XCTAssertGreaterThan(try Self.examples().count, 35)
    }
    func testMalformedFramingSchemaAndExitStatus() throws {
        let e = try Self.examples()[0], valid = try Self.bytes(e)
        for data in [Data(), Data("{}\n".utf8), Data(valid.dropLast()), valid + valid,
                     Data("{\n".utf8), Data(repeating: 32, count: CLIContract.limit + 1), Data([0xff,10]), valid + Data([10])] {
            XCTAssertThrowsError(try CLIContract.decode(data, exit: 0))
        }
        for major in [2, true, "1"] as [Any] {
            var object = e; object["schema"] = major
            XCTAssertThrowsError(try CLIContract.decode(Self.bytes(object), exit: 0))
        }
        XCTAssertThrowsError(try CLIContract.decode(valid, exit: 1))
        XCTAssertThrowsError(try CLIContract.decode(valid, exit: 3))
        var unknown = e; unknown["account"] = "must never display"
        XCTAssertThrowsError(try CLIContract.decode(Self.bytes(unknown), exit: 0))
    }
    func testCanonicalU64AndReservedNames() throws {
        for id in ["0", "01", "-1", "18446744073709551616", "1e3", " 1", ""] { XCTAssertFalse(CLIContract.decimal(id, nonzero: true)) }
        XCTAssertTrue(CLIContract.decimal("18446744073709551615", nonzero: true))
        XCTAssertTrue(CLIContract.decimal("9007199254740993", nonzero: true))
        for name in ["larp", "con", "com1", "lpt9", "../home", "--yes"] { XCTAssertFalse(CLIContract.name(name)) }
        for id: Any in ["0", "01", "18446744073709551616", 2] {
            var object = try Self.examples()[1]; object["data"] = modifying(object["data"], "id", id)
            XCTAssertThrowsError(try CLIContract.decode(Self.bytes(object), exit: 0))
        }
        var object = try Self.examples()[0]; object["revision"] = "18446744073709551616"
        XCTAssertThrowsError(try CLIContract.decode(Self.bytes(object), exit: 0))
    }
    private func modifying(_ object: Any?, _ key: String, _ value: Any) -> [String: Any] {
        var d = object as! [String: Any]; d[key] = value; return d
    }
    func testMatrixDimensionsDuplicatesAndAliasCrossReference() throws {
        var matrix = try Self.examples()[5]
        matrix["data"] = modifying(matrix["data"], "rows", [["name": "example", "enabled": []]])
        XCTAssertThrowsError(try CLIContract.decode(Self.bytes(matrix), exit: 0))
        var profile = try Self.examples()[1]
        var p = profile["data"] as! [String: Any]
        var a = (p["aliases"] as! [[String: Any]])[0]; a["profile_id"] = "3"
        p["aliases"] = [a]; profile["data"] = p
        XCTAssertThrowsError(try CLIContract.decode(Self.bytes(profile), exit: 0))
        var list = try Self.examples()[0]; list["data"] = ["profiles": [p, p]]
        XCTAssertThrowsError(try CLIContract.decode(Self.bytes(list), exit: 0))
        p["provider"] = "unknown"; profile["data"] = p
        XCTAssertThrowsError(try CLIContract.decode(Self.bytes(profile), exit: 0))
    }
    func testEmptyListsNullBindingsAndUnsafePathsDecodeSafely() throws {
        for command in ["profile.list", "alias.list", "provider.list", "mcp.list", "skills.list", "mcp.matrix", "skills.matrix"] {
            var e = try Self.examples().first { $0["command"] as? String == command }!
            var data = e["data"] as! [String: Any]
            for (key, value) in data where value is [Any] { data[key] = [Any]() }
            e["data"] = data
            _ = try CLIContract.decode(Self.bytes(e), exit: 0)
        }
        var e = try Self.examples()[1], p = e["data"] as! [String: Any]
        var surfaces = p["surfaces"] as! [String: Any]
        surfaces["cli"] = ["enabled": true, "path": "\\xff; $(bad)", "state": "unsafe"]
        p["surfaces"] = surfaces; e["data"] = p
        _ = try CLIContract.decode(Self.bytes(e), exit: 0)
    }
    func testExactActionArgumentsNeverUseDisplayPaths() async throws {
        let snapshot = try await FixtureLarpClient().snapshot()
        let cases: [(LarpAction, [String])] = [
            (.delete(profileID: "1"), ["profile", "delete", "personal", "--yes"]),
            (.setSurface(profileID: "1", surface: .desktop, enabled: true), ["profile", "surface", "personal", "desktop", "on"]),
            (.setSurface(profileID: "1", surface: .cli, enabled: false), ["profile", "surface", "personal", "cli", "off"]),
            (.setCatalog(kind: .mcp, itemID: "local-tools", profileID: "1", enabled: true), ["mcp", "enable", "local-tools", "personal"]),
            (.setCatalog(kind: .skill, itemID: "writing", profileID: "1", enabled: false), ["skills", "disable", "writing", "personal"]),
            (.setSyncNewServers(true), ["mcp", "sync-all", "on"]),
            (.open(profileID: "1", surface: .desktop), ["app", "open", "personal"]),
            (.addAlias(name: "claude-work", profileID: "1"), ["alias", "add", "claude-work", "personal"]),
            (.removeAlias(name: "claude-personal"), ["alias", "remove", "claude-personal"])
        ]
        for (action, argv) in cases { XCTAssertEqual(try ProcessLarpClient.arguments(action, snapshot: snapshot), argv) }
        XCTAssertThrowsError(try ProcessLarpClient.arguments(.open(profileID: "1", surface: .cli), snapshot: snapshot))
        for alias in ["claude", "codex", "larp", "Bad", "--yes", "a b", ""] {
            XCTAssertThrowsError(try ProcessLarpClient.arguments(.addAlias(name: alias, profileID: "1"), snapshot: snapshot))
        }
        XCTAssertThrowsError(try ProcessLarpClient.arguments(.removeAlias(name: "missing"), snapshot: snapshot))
        XCTAssertEqual(ProcessLarpClient.dataFolder(cliPath: "/fixture/root/profiles/2/cli", id: "2"), "/fixture/root")
        XCTAssertNil(ProcessLarpClient.dataFolder(cliPath: "/elsewhere/2/cli", id: "2"))
    }
    func testCoherentSnapshotAllActionRevisionsAndConflictConsumesIntent() async throws {
        let transport = try RecordingTransport()
        let client = ProcessLarpClient(transport: transport)
        _ = try await client.snapshot()
        let actions: [LarpAction] = [.setSurface(profileID: "2", surface: .desktop, enabled: true),
            .setCatalog(kind: .mcp, itemID: "example", profileID: "2", enabled: true),
            .setCatalog(kind: .skill, itemID: "example", profileID: "2", enabled: false), .setSyncNewServers(false),
            .open(profileID: "2", surface: .desktop), .addAlias(name: "codex-work", profileID: "2"),
            .removeAlias(name: "codex-45"), .delete(profileID: "2")]
        for action in actions {
            _ = try await client.perform(action)
            _ = try await client.snapshot()
        }
        await transport.renameAndReuseName()
        do { _ = try await client.perform(.delete(profileID: "2")); XCTFail() } catch ClientError.conflict {}
        do { _ = try await client.perform(.delete(profileID: "2")); XCTFail() } catch ClientError.staleSelection {}
        let refreshed = try await client.snapshot()
        XCTAssertEqual(refreshed.profiles.first?.id, "3")
        do { _ = try await client.perform(.delete(profileID: "2")); XCTFail() } catch ClientError.staleSelection {}
        _ = try await client.perform(.delete(profileID: "3")) // renewed intent only
        let calls = await transport.calls
        for argv in calls where argv != ["--help", "--json"] && argv != ["cc", "profile", "list", "--json"] {
            XCTAssertTrue(argv.contains("--json")); XCTAssertTrue(argv.contains("--expected-revision"))
            XCTAssertTrue(["7", "8"].contains(argv.last ?? ""))
        }
        let replacementDeletes = await transport.replacementDeletes
        XCTAssertEqual(replacementDeletes, 1)
    }
    func testMixedRevisionAndDanglingSwitchRefuse() async throws {
        for mode in ["mixed", "dangling", "matrix-columns"] {
            let transport = try RecordingTransport(mode: mode)
            do { _ = try await ProcessLarpClient(transport: transport).snapshot(); XCTFail(mode) } catch {}
        }
    }
    func testStaleAsyncSnapshotCannotOverwriteNewerSnapshot() async throws {
        let transport = try RecordingTransport(mode: "delay")
        let client = ProcessLarpClient(transport: transport)
        let old = Task { try await client.snapshot() }
        try await Task.sleep(nanoseconds: 20_000_000)
        _ = try await client.snapshot()
        do { _ = try await old.value; XCTFail() } catch ClientError.staleSelection {}
        _ = try await client.perform(.delete(profileID: "2"))
    }
    private func script(_ body: String) throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("synthetic-cli")
        try Data(("#!/bin/sh\n" + body).utf8).write(to: url)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: url.path)
        return url
    }
    func testTransportExactArgvNoShellExpansion() async throws {
        let url = try script("printf '%s\\n' \"$@\"\n")
        let result = try await ProcessRunner.run(url: url, arguments: ["$(touch /never)", "a b", ";exit 4"])
        XCTAssertEqual(String(data: result.stdout, encoding: .utf8), "$(touch /never)\na b\n;exit 4\n")
    }
    func testTimeoutAndCancellationReapDirectChild() async throws {
        let url = try script("exec /bin/sleep 10\n")
        let start = Date()
        do { _ = try await ProcessRunner.run(url: url, arguments: [], timeout: 0.05); XCTFail() } catch ClientError.timedOut {}
        let task = Task { try await ProcessRunner.run(url: url, arguments: []) }
        try await Task.sleep(nanoseconds: 20_000_000); task.cancel()
        do { _ = try await task.value; XCTFail() } catch is CancellationError {}
        XCTAssertLessThan(Date().timeIntervalSince(start), 3)
    }
    func testStdoutCapAndStderrSaturationDoNotDeadlock() async throws {
        for redirect in ["", " >&2"] {
            let chunk = String(repeating: "x", count: 8192)
            let url = try script("i=0; while [ $i -lt 140 ]; do printf '%s' '\(chunk)'\(redirect); i=$((i+1)); done\n")
            do { _ = try await ProcessRunner.run(url: url, arguments: [], timeout: 5); XCTFail() } catch ClientError.outputLimit {}
        }
        let url = try script("printf 'safe json\\n'; printf 'SECRET' >&2\n")
        do { _ = try await ProcessRunner.run(url: url, arguments: []); XCTFail() } catch ClientError.invalidResponse {}
    }
    func testSelectionIdentityRejectsReplacement() throws {
        let scriptURL = try script("exit 0")
        let url = scriptURL.deletingLastPathComponent().appendingPathComponent("larp")
        var data = Data([0xcf, 0xfa, 0xed, 0xfe])
        try data.write(to: url)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: url.path)
        let selected = try SelectedLarp(url: url)
        try selected.recheck()
        data.append(0)
        try data.write(to: url)
        XCTAssertThrowsError(try selected.recheck())
    }
    func testProductionSelectionRejectsScriptsDirectoriesAndWrongNames() throws {
        let url = try script("exit 0\n")
        XCTAssertThrowsError(try SelectedLarp(url: url))
        let larp = url.deletingLastPathComponent().appendingPathComponent("larp")
        try FileManager.default.copyItem(at: url, to: larp)
        XCTAssertThrowsError(try SelectedLarp(url: larp))
        XCTAssertThrowsError(try SelectedLarp(url: url.deletingLastPathComponent()))
    }
}

private actor RecordingTransport: CLITransport {
    var calls = [[String]]()
    var replacementDeletes = 0
    private var reused = false
    private let mode: String
    private var delayed = false
    private let examples: [[String: Any]]
    init(mode: String = "") throws { examples = try ProcessBridgeTests.examples(); self.mode = mode }
    func renameAndReuseName() { reused = true }
    func run(_ argv: [String], mutation: Bool) async throws -> ProcessOutput {
        calls.append(argv)
        let command = argv.first == "--help" ? "help" : argv.dropFirst().prefix(2).joined(separator: ".")
        if mode == "delay", command == "profile.list", !delayed {
            delayed = true; try await Task.sleep(nanoseconds: 150_000_000)
        }
        // 1 initial + 8 per-action refreshes precede the stale delete.
        if reused, command == "profile.delete", calls.filter({ $0.prefix(3) == ["cc", "profile", "list"] }).count <= 9 {
            let error: [String: Any] = ["schema": 1, "command": command, "ok": false, "error": ["code": "conflict", "message": "secret not displayed"]]
            return ProcessOutput(stdout: try ProcessBridgeTests.bytes(error), status: 1)
        }
        if reused, command == "profile.delete" { replacementDeletes += 1 }
        var e = examples.first { $0["command"] as? String == command && $0["ok"] as? Bool == true }!
        if mode == "mixed", command == "mcp.matrix" { e["revision"] = "8" }
        if mode == "dangling", command == "mcp.list" {
            var data = e["data"] as! [String: Any], servers = data["servers"] as! [[String: Any]]
            servers[0]["switches"] = [["profile_id": "99", "enabled": true]]; data["servers"] = servers; e["data"] = data
        }
        if mode == "matrix-columns", command == "skills.matrix" {
            var data = e["data"] as! [String: Any], columns = data["profiles"] as! [[String: Any]]
            columns[0]["name"] = "other"; data["profiles"] = columns; e["data"] = data
        }
        if reused, e["revision"] is String { e["revision"] = "8" }
        var bytes = try ProcessBridgeTests.bytes(e)
        if reused {
            // Rename ID 2 away; a new ID 3 now owns codex-45. Update all coherent refs.
            bytes = Data(String(data: bytes, encoding: .utf8)!.replacingOccurrences(of: "\"2\"", with: "\"3\"").utf8)
        }
        return ProcessOutput(stdout: bytes, status: 0)
    }
}
