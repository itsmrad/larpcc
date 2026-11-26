import XCTest
@testable import LarpModel

final class LocalUsageBridgeTests: XCTestCase {
    private typealias Object = [String: Any]

    private func localExample(edit: (inout [Object]) -> Void = { _ in }) throws -> Data {
        let examples = try ProcessBridgeTests.examples()
        var envelope = try XCTUnwrap(examples.first {
            $0["command"] as? String == "usage" && ($0["data"] as? Object)?["requested_mode"] as? String == "settings"
        })
        var data = try XCTUnwrap(envelope["data"] as? Object)
        var rows = try XCTUnwrap(data["profiles"] as? [Object])
        edit(&rows)
        data["requested_mode"] = "local"
        data["profiles"] = rows
        envelope["data"] = data
        return try ProcessBridgeTests.bytes(envelope)
    }

    private func decode(_ bytes: Data) throws -> WireUsage {
        let envelope = try CLIContract.decode(bytes, exit: 0)
        guard case let .usage(usage) = envelope.data else { throw ClientError.invalidResponse }
        return usage
    }

    func testLocalUsesOneReadOnlyRequestAndLiveMakesNone() async throws {
        let transport = UsageTransport(bytes: try localExample())
        let client = ProcessLarpClient(transport: transport)
        do {
            _ = try await client.usage(source: .live)
            XCTFail("Live usage must be unavailable")
        } catch ClientError.commandUnavailable {}
        let before = await transport.calls
        XCTAssertTrue(before.isEmpty)

        let reports = try await client.usage(source: .local)
        let calls = await transport.calls
        XCTAssertEqual(calls, [["cc", "usage", "--local", "--json"]])
        XCTAssertEqual(Set(reports.keys), ["1", "2"])
        let claude = try XCTUnwrap(reports["1"])
        XCTAssertEqual(claude.fiveHour?.reason, .captureNotInstalled)
        XCTAssertNil(claude.lowestLeftPercent)
        XCTAssertTrue(claude.extra.isEmpty)
        XCTAssertEqual(claude.resetCredits?.state, .unsupported)

        let codex = try XCTUnwrap(reports["2"])
        XCTAssertEqual(codex.captureSource, .codexSessionLog)
        XCTAssertEqual(codex.fiveHour?.usedPercent, 30)
        XCTAssertEqual(codex.fiveHour?.pace?.elapsedPercent, 60)
        XCTAssertEqual(codex.fiveHour?.pace?.reservePercent, 30)
        XCTAssertEqual(codex.fiveHour?.pace?.lastsUntilReset, true)
        XCTAssertEqual(codex.extra.map(\.label), ["Other limit 1 5h", "Other limit 2 5h"])
        let extra = try XCTUnwrap(codex.extra.first?.window)
        let captured = try XCTUnwrap(extra.capturedAt)
        let rowCaptured = try XCTUnwrap(codex.capturedAt)
        let runsOut = try XCTUnwrap(extra.pace?.runsOutAt)
        XCTAssertEqual(rowCaptured.timeIntervalSince(captured), 7200)
        XCTAssertEqual(runsOut.timeIntervalSince(captured), 14400)
        XCTAssertEqual(codex.extra.last?.window.reason, .resetSinceCapture)
        XCTAssertNil(codex.extra.last?.window.leftPercent)
    }

    func testLocalUsageRefusalPreservesStableCode() async throws {
        let bytes = try ProcessBridgeTests.bytes([
            "schema": 1, "command": "usage", "ok": false,
            "error": ["code": "busy", "message": "Simulated registry diagnostic"]
        ])
        let client = ProcessLarpClient(transport: UsageTransport(bytes: bytes, status: 1))
        do {
            _ = try await client.usage(source: .local)
            XCTFail("Expected a usage refusal")
        } catch let error as ClientError {
            XCTAssertEqual(error, .refused(code: "busy"))
        }
    }

    func testLocalThroughSelectedExecutableRechecksIdentity() async throws {
        let fm = FileManager.default
        let dir = fm.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent(UUID().uuidString)
        try fm.createDirectory(at: dir, withIntermediateDirectories: false)
        defer { try? fm.removeItem(at: dir) }
        let url = dir.appendingPathComponent("larp")
        var bytes = Data([0xcf, 0xfa, 0xed, 0xfe])
        try bytes.write(to: url)
        try fm.setAttributes([.posixPermissions: 0o700], ofItemAtPath: url.path)
        let client = ProcessLarpClient(selected: try SelectedLarp(url: url))
        bytes.append(0)
        try bytes.write(to: url)
        do {
            _ = try await client.usage(source: .local)
            XCTFail("A changed executable must be rejected before launch")
        } catch ClientError.invalidExecutable {}
    }

    func testSchemaAndTypedBoundsRejectInvalidUsage() throws {
        for (field, value) in [("unrecognized", true), ("used_percent", 101), ("used_percent", 1.25),
                               ("used_percent", true), ("window_seconds", 0), ("window_seconds", 31_622_401),
                               ("window_seconds", NSNull()), ("reason", "unknown")] as [(String, Any)] {
            let bytes = try localExample { rows in
                var windows = rows[1]["windows"] as! Object
                var window = windows["five_hour"] as! Object
                window[field] = value
                windows["five_hour"] = window
                rows[1]["windows"] = windows
            }
            XCTAssertThrowsError(try decode(bytes), "Invalid \(field): \(value)")
        }

        let duplicate = try localExample { rows in
            let first = rows[0]
            rows.append(first)
        }
        XCTAssertThrowsError(try decode(duplicate).localReports())
        let liveRow = try localExample { $0[1]["mode"] = "live" }
        XCTAssertThrowsError(try decode(liveRow).localReports())
        let liveExample = try XCTUnwrap(ProcessBridgeTests.examples().first {
            $0["command"] as? String == "usage" && ($0["data"] as? Object)?["requested_mode"] as? String == "live"
        })
        XCTAssertThrowsError(try decode(ProcessBridgeTests.bytes(liveExample)).localReports())
    }

    func testReasonsMapWithoutStalePercentages() throws {
        let cases: [(UsageReason, String)] = [
            (.notCaptured, "Not available yet"), (.notReported, "Not reported"), (.unsupported, "Not supported"),
            (.unverifiedVersion, "Client version unverified"), (.resetSinceCapture, "Reset due"),
            (.resetTimeUnknown, "Reset time unknown"), (.captureNotInstalled, "Not available"),
            (.invalidRecord, "Invalid usage record"), (.liveDisabled, "Live usage disabled"),
            (.signInAgain, "Sign in again in the official client"), (.liveUnavailable, "Live usage unavailable")
        ]
        for (reason, text) in cases {
            let bytes = try localExample { rows in
                var windows = rows[1]["windows"] as! Object
                var window = windows["five_hour"] as! Object
                window["reason"] = reason.rawValue
                window["pace"] = NSNull()
                if reason != .resetTimeUnknown {
                    window["used_percent"] = NSNull()
                    window["captured_at"] = NSNull()
                }
                window["resets_at"] = reason == .resetSinceCapture ? "2026-11-21T11:00:00Z" as Any : NSNull()
                windows["five_hour"] = window
                rows[1]["windows"] = windows
            }
            let reports = try decode(bytes).localReports()
            let window = try XCTUnwrap(reports["2"]?.fiveHour)
            XCTAssertEqual(window.reason, reason)
            XCTAssertEqual(window.reason?.text, text)
            XCTAssertEqual(window.leftPercent, reason == .resetTimeUnknown ? 70 : nil)
            XCTAssertNil(window.pace)
        }
    }

    func testResetCreditsAndPlanMapUsingBundledExamples() throws {
        let examples = try ProcessBridgeTests.examples().filter { $0["command"] as? String == "usage" && $0["ok"] as? Bool == true }
        var states: Set<UsageResetCredits.State> = []
        for example in examples {
            let usage = try decode(ProcessBridgeTests.bytes(example))
            for row in usage.profiles {
                let credits = try XCTUnwrap(row.report.resetCredits)
                states.insert(credits.state)
                let expected: String
                switch credits.state {
                case .available: expected = "2 resets available"
                case .none: expected = "No resets available"
                case .notRecorded, .unsupported: expected = "Reset info not available"
                }
                XCTAssertEqual(ResetAvailability.text(credits: credits), expected)
            }
        }
        XCTAssertEqual(states, [.available, .none, .notRecorded, .unsupported])

        let bytes = try localExample { rows in
            rows[0]["plan"] = ["source": "user", "label": "Max 5x"]
            rows[1]["plan"] = ["source": "client_local", "label": "pro"]
            rows[1]["reset_credits"] = ["state": "available", "count": 1, "expires_state": "recorded",
                                       "expires_at": "2026-11-24T12:00:05Z"]
        }
        let reports = try decode(bytes).localReports()
        XCTAssertEqual(reports["1"]?.planLabel, "Max 5x")
        XCTAssertEqual(reports["2"]?.planLabel, "pro")
        let credits = try XCTUnwrap(reports["2"]?.resetCredits)
        XCTAssertEqual(credits.expiresState, .recorded)
        let expiry = try XCTUnwrap(credits.expiresAt)
        XCTAssertEqual(ResetAvailability.text(credits: credits),
                       "1 reset available · Expires \(expiry.formatted(date: .abbreviated, time: .shortened))")
    }
}

private actor UsageTransport: CLITransport {
    let bytes: Data
    let status: Int32
    private(set) var calls: [[String]] = []

    init(bytes: Data, status: Int32 = 0) {
        self.bytes = bytes
        self.status = status
    }

    func run(_ arguments: [String], mutation: Bool) async throws -> ProcessOutput {
        calls.append(arguments)
        guard !mutation else { throw ClientError.invalidResponse }
        return ProcessOutput(stdout: bytes, status: status)
    }
}
