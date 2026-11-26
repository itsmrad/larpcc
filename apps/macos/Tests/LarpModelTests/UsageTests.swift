import XCTest
@testable import LarpModel

final class UsageTests: XCTestCase {
    func testWindowPercentLeftIsClampedAndLowestWins() {
        XCTAssertEqual(UsageWindow(usedPercent: 38, resetsAt: nil).leftPercent, 62)
        XCTAssertEqual(UsageWindow(usedPercent: 130, resetsAt: nil).leftPercent, 0)
        XCTAssertEqual(UsageWindow(usedPercent: -5, resetsAt: nil).leftPercent, 100)
        let report = UsageReport(source: .local, capturedAt: nil, fiveHour: UsageWindow(usedPercent: 91, resetsAt: nil),
                                 weekly: UsageWindow(usedPercent: 70, resetsAt: nil), resetCredits: nil)
        XCTAssertEqual(report.lowestLeftPercent, 9)
        XCTAssertNil(UsageReport(source: .live, capturedAt: nil, fiveHour: nil, weekly: nil, resetCredits: nil).lowestLeftPercent)
    }

    func testExtraWindowTitlesPreserveFixedLabels() {
        let window = UsageWindow(usedPercent: nil, resetsAt: nil)
        let model = UsageReport.ExtraWindow(label: "Sonnet weekly", window: window)
        XCTAssertEqual(model.title(source: .live), "Weekly · Sonnet")
        XCTAssertEqual(model.title(source: .local), "Sonnet weekly")

        for label in ["Other limit 1 weekly", "Other limit 2 60m", "Codex secondary"] {
            let extra = UsageReport.ExtraWindow(label: label, window: window)
            XCTAssertEqual(extra.title(source: .live), label)
        }
    }

    func testSampleUsageFollowsSourceAndExistingProfiles() async throws {
        let client = FixtureLarpClient()
        let local = try await client.usage(source: .local)
        XCTAssertEqual(Set(local.keys), ["1", "2", "3"])
        XCTAssertTrue(local.values.allSatisfy { $0.source == .local })
        XCTAssertEqual(local["1"]?.resetCredits?.count, 2)
        XCTAssertEqual(local["3"]?.resetCredits?.count, 0)
        _ = try await client.perform(.delete(profileID: "2"))
        let live = try await client.usage(source: .live)
        XCTAssertNil(live["2"], "deleted profiles have no usage")
        XCTAssertTrue(live.values.allSatisfy { $0.source == .live })
    }
}
