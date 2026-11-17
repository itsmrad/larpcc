import XCTest
@testable import LarpModel

final class FixtureClientTests: XCTestCase {
    func testSampleSwitchesAreIndependentAndResetOnNewClient() async throws {
        let client = FixtureLarpClient()
        let original = try await client.snapshot()
        XCTAssertEqual(original.enabledCount, 4)
        XCTAssertEqual(original.profiles.map(\.name), original.profiles.map(\.name).sorted())
        _ = try await client.perform(.setSurface(profileID: "1", surface: .cli, enabled: false))
        let changed = try await client.snapshot()
        let profile = try XCTUnwrap(changed.profiles.first { $0.id == "1" })
        XCTAssertFalse(profile.cliEnabled)
        XCTAssertTrue(profile.desktopEnabled)
        XCTAssertEqual(changed.enabledCount, 4)
        let fresh = try await FixtureLarpClient().snapshot()
        XCTAssertEqual(fresh.profiles, original.profiles)
    }

    func testAssignmentsAndDeleteLeaveNoDanglingReferences() async throws {
        let client = FixtureLarpClient()
        _ = try await client.perform(.setCatalog(kind: .mcp, itemID: "local-tools", profileID: "2", enabled: true))
        _ = try await client.perform(.setSyncNewServers(true))
        let assigned = try await client.snapshot()
        XCTAssertTrue(try XCTUnwrap(assigned.servers.first { $0.id == "local-tools" }).profileIDs.contains("2"))
        XCTAssertTrue(assigned.syncNewServers)
        _ = try await client.perform(.delete(profileID: "2"))
        let deleted = try await client.snapshot()
        XCTAssertFalse(deleted.profiles.contains { $0.id == "2" })
        XCTAssertTrue((deleted.servers + deleted.skills).allSatisfy { !$0.profileIDs.contains("2") })
    }

    func testLaunchIsClosedAndMissingSelectionsRefuse() async throws {
        let client = FixtureLarpClient()
        for surface in [Surface.cli, .desktop] {
            do {
                _ = try await client.perform(.open(profileID: "1", surface: surface))
                XCTFail("Launch must refuse")
            } catch ClientError.qualificationPending {} 
        }
        do {
            _ = try await client.perform(.setSurface(profileID: "999", surface: .cli, enabled: true))
            XCTFail("Stale target must refuse")
        } catch ClientError.staleSelection {}
        let state = try await client.snapshot()
        XCTAssertEqual(state.profiles.count, 5)
    }

    func testReadyMeansSampleConfigurationOnlyAndDisabledIsOff() async throws {
        let client = FixtureLarpClient()
        let state = try await client.snapshot()
        XCTAssertEqual(state.profiles.first { $0.id == "1" }?.readiness, .ready)
        XCTAssertEqual(state.profiles.first { $0.id == "2" }?.readiness, .attention)
        XCTAssertEqual(state.profiles.first { $0.id == "5" }?.readiness, .off)
    }
}
