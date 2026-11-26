import XCTest
@testable import LarpModel

final class ProviderDetectionTests: XCTestCase {
    func testEmptyAndPartialDetectionPreserveVersionAndPrivacy() throws {
        let examples = try ProcessBridgeTests.examples().filter { $0["command"] as? String == "provider.detect" }
        let empty = try decode(XCTUnwrap(examples.first))
        XCTAssertTrue(empty.executables.isEmpty)
        XCTAssertTrue(empty.desktop_apps.isEmpty)
        XCTAssertEqual(empty.skippedCount, 1)
        XCTAssertFalse(empty.limited)

        var envelope = try XCTUnwrap(examples.last)
        var data = try XCTUnwrap(envelope["data"] as? [String: Any])
        data["limited"] = true
        data["desktop_apps"] = [[
            "provider": "codex", "path": "/Applications/ChatGPT.app", "resolved_path": "/Applications/ChatGPT.app",
            "bundle_id": NSNull(), "version": NSNull(), "code_requirement": "unavailable"
        ] as [String: Any]]
        envelope["data"] = data
        let partial = try decode(envelope)
        XCTAssertTrue(partial.limited)
        XCTAssertEqual(partial.desktop_apps.first?.code_requirement, "unavailable")
        let executable = try XCTUnwrap(partial.executables.first)
        XCTAssertTrue(executable.bindable)
        XCTAssertFalse(executable.qualified)
        XCTAssertFalse(executable.bound)
        XCTAssertNil(executable.version)
        XCTAssertEqual(executable.version_diagnostic, "version_probe_unavailable")

        var executables = try XCTUnwrap(data["executables"] as? [[String: Any]])
        executables[0]["version_diagnostic"] = "version_unapproved"
        data["executables"] = executables
        envelope["data"] = data
        XCTAssertThrowsError(try decode(envelope))

        executables[0]["version"] = "unapproved"
        data["executables"] = executables
        envelope["data"] = data
        let unapproved = try XCTUnwrap(decode(envelope).executables.first)
        XCTAssertEqual(unapproved.version, "unapproved")
        XCTAssertEqual(unapproved.version_diagnostic, "version_unapproved")
    }

    func testDetectionErrorEnvelopeHasNoData() throws {
        let error: [String: Any] = [
            "schema": 1, "command": "provider.detect", "ok": false,
            "error": ["code": "unsupported", "message": "Detection unavailable"]
        ]
        let decoded = try CLIContract.decode(ProcessBridgeTests.bytes(error), exit: 1)
        XCTAssertNil(decoded.data)
        XCTAssertEqual(decoded.error?.code, "unsupported")
    }

    func testFixtureDetectionReflectsExplicitBinding() async throws {
        let client = FixtureLarpClient()
        let before = try await client.detectProviders(revision: nil)
        let candidate = try XCTUnwrap(before.executables.first)
        XCTAssertFalse(candidate.bound)
        XCTAssertEqual(candidate.version, "unapproved")
        XCTAssertTrue(before.executables.contains { !$0.bindable })
        _ = try await client.perform(.bindProvider(provider: candidate.provider, path: candidate.path, replace: false))
        let after = try await client.detectProviders(revision: nil)
        XCTAssertTrue(after.executables.first?.bound == true)
    }

    private func decode(_ example: [String: Any]) throws -> ProviderDetection {
        let envelope = try CLIContract.decode(ProcessBridgeTests.bytes(example), exit: 0)
        XCTAssertEqual(envelope.revision, "0")
        guard case let .providerDetect(scan) = envelope.data else { throw ClientError.invalidResponse }
        return scan
    }
}
