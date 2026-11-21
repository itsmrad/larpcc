import XCTest
@testable import LarpModel

final class MenuBarSummaryTests: XCTestCase {
    // Sample left %: 1 personal 62/38, 2 work 9/30, 3 studio 88/60, 4 research none, 5 sandbox off.
    private func sample(disabling id: String? = nil) async throws -> ([Profile], [String: UsageReport]) {
        let client = FixtureLarpClient()
        if let id { _ = try await client.perform(.setSurface(profileID: id, surface: .cli, enabled: false)) }
        return (try await client.snapshot().profiles, try await client.usage(source: .local))
    }

    func testTightestPicksLowestWindowAcrossEnabledProfiles() async throws {
        let (profiles, usage) = try await sample()
        XCTAssertEqual(MenuBarSummary.tightestLeftPercent(profiles: profiles, usage: usage), 9, "work's 5-hour window")
        let (fewer, usage2) = try await sample(disabling: "2")
        XCTAssertEqual(MenuBarSummary.tightestLeftPercent(profiles: fewer, usage: usage2), 38, "personal's weekly window")
    }

    func testTightestIsNilWithoutData() async throws {
        let (profiles, usage) = try await sample()
        XCTAssertNil(MenuBarSummary.tightestLeftPercent(profiles: profiles, usage: [:]))
        XCTAssertNil(MenuBarSummary.tightestLeftPercent(profiles: [], usage: usage))
        let disabled = profiles.filter { !MenuBarSummary.isEnabled($0) }
        XCTAssertEqual(disabled.map(\.id), ["5"])
        XCTAssertNil(MenuBarSummary.tightestLeftPercent(profiles: disabled, usage: usage))
    }

    func testLabelTextPerMode() async throws {
        let (profiles, usage) = try await sample()
        func label(_ mode: MenuBarMode, _ id: String? = nil) -> String? {
            MenuBarSummary.labelText(mode: mode, chosenProfileID: id, profiles: profiles, usage: usage)
        }
        XCTAssertEqual(label(.tightest), "9%")
        XCTAssertEqual(label(.profile, "3"), "60%")
        XCTAssertEqual(label(.profile, "1"), "38%")
        XCTAssertNil(label(.profile, "4"), "no usage saved")
        XCTAssertNil(label(.profile, "missing"))
        XCTAssertNil(label(.profile, nil))
        XCTAssertNil(label(.icon))
        XCTAssertNil(MenuBarSummary.labelText(mode: .tightest, chosenProfileID: nil, profiles: profiles, usage: [:]))
        XCTAssertEqual(MenuBarMode(rawValue: "tightest"), .tightest)
    }

    func testSymbolFollowsLevel() {
        XCTAssertEqual(MenuBarSummary.symbolName(leftPercent: nil), "gauge.with.dots.needle.67percent")
        XCTAssertEqual(MenuBarSummary.symbolName(leftPercent: 62), "gauge.with.dots.needle.67percent")
        XCTAssertEqual(MenuBarSummary.symbolName(leftPercent: 20), "gauge.with.dots.needle.33percent")
        XCTAssertEqual(MenuBarSummary.symbolName(leftPercent: 9), "gauge.with.dots.needle.0percent")
        XCTAssertEqual(UsageLevel(leftPercent: 25), .normal)
        XCTAssertEqual(UsageLevel(leftPercent: 24.9), .low)
        XCTAssertEqual(UsageLevel(leftPercent: 9.9), .critical)
        XCTAssertEqual(UsageLevel.low.word, "Low")
        XCTAssertEqual(UsageLevel.critical.word, "Almost out")
        XCTAssertNil(UsageLevel.normal.word)
    }

    func testOrderedProfilesAreEnabledAndTightestFirst() async throws {
        let (profiles, usage) = try await sample()
        XCTAssertEqual(MenuBarSummary.orderedProfiles(profiles, usage: usage).map(\.id), ["2", "1", "3", "4"])
        XCTAssertEqual(MenuBarSummary.orderedProfiles(profiles, usage: [:]).map(\.name), ["personal", "research", "studio", "work"])
    }

    func testResetAvailabilityWording() async throws {
        XCTAssertEqual(ResetAvailability.text(credits: nil), "Reset info not available")
        XCTAssertEqual(ResetAvailability.text(credits: -1), "Reset info not available")
        XCTAssertEqual(ResetAvailability.text(credits: 0), "No resets available")
        XCTAssertEqual(ResetAvailability.text(credits: 1), "1 reset available")
        XCTAssertEqual(ResetAvailability.text(credits: 3), "3 resets available")
        let usage = try await FixtureLarpClient().usage(source: .local)
        XCTAssertEqual(ResetAvailability.text(credits: usage["1"]?.resetCredits), "2 resets available")
        XCTAssertEqual(ResetAvailability.text(credits: usage["2"]?.resetCredits), "Reset info not available")
        XCTAssertEqual(ResetAvailability.text(credits: usage["3"]?.resetCredits), "No resets available")
        XCTAssertEqual(ResetAvailability.text(credits: usage["4"]?.resetCredits), "Reset info not available", "no report")
    }

    func testPaceMath() {
        let now = Date(timeIntervalSinceReferenceDate: 800_000_000)
        let five = UsageWindowKind.fiveHour
        XCTAssertEqual(five.length, 18_000)
        XCTAssertEqual(UsageWindowKind.weekly.length, 604_800)
        let half = now.addingTimeInterval(2.5 * 3600)
        XCTAssertEqual(UsagePace.expectedLeftPercent(resetsAt: half, length: five.length, now: now)!, 50, accuracy: 0.001)
        XCTAssertNil(UsagePace.expectedLeftPercent(resetsAt: nil, length: five.length, now: now))
        XCTAssertEqual(UsagePace.expectedLeftPercent(resetsAt: now.addingTimeInterval(-60), length: five.length, now: now), 0)
        XCTAssertEqual(UsagePace.expectedLeftPercent(resetsAt: now.addingTimeInterval(9 * 3600), length: five.length, now: now), 100)

        XCTAssertTrue(UsagePace.isAhead(UsageWindow(usedPercent: 60, resetsAt: half), kind: five, now: now))
        XCTAssertFalse(UsagePace.isAhead(UsageWindow(usedPercent: 54, resetsAt: half), kind: five, now: now), "within margin")
        XCTAssertFalse(UsagePace.isAhead(UsageWindow(usedPercent: 30, resetsAt: half), kind: five, now: now))
        XCTAssertFalse(UsagePace.isAhead(UsageWindow(usedPercent: 99, resetsAt: nil), kind: five, now: now), "unknown reset")
        // Weekly: 3.5 of 7 days elapsed, 62% used.
        let midWeek = now.addingTimeInterval(3.5 * 86_400)
        XCTAssertTrue(UsagePace.isAhead(UsageWindow(usedPercent: 62, resetsAt: midWeek), kind: .weekly, now: now))
    }

    func testResetCountdownAndSpans() {
        let now = Date(timeIntervalSinceReferenceDate: 800_000_000)
        XCTAssertEqual(ResetCountdown.text(resetsAt: nil, now: now), "Reset time unknown")
        XCTAssertEqual(ResetCountdown.text(resetsAt: now, now: now), "Resetting now")
        XCTAssertEqual(ResetCountdown.text(resetsAt: now.addingTimeInterval(-5), now: now), "Resetting now")
        XCTAssertEqual(ResetCountdown.text(resetsAt: now.addingTimeInterval(2 * 3600 + 13 * 60), now: now), "Resets in 2h 13m")
        XCTAssertEqual(ResetCountdown.spoken(resetsAt: now.addingTimeInterval(2 * 3600 + 13 * 60), now: now), "resets in 2 hours 13 minutes")
        XCTAssertEqual(UsageSpan.compact(30), "1m")
        XCTAssertEqual(UsageSpan.compact(7200), "2h")
        XCTAssertEqual(UsageSpan.compact(3 * 86_400 + 3600 + 300), "3d 1h")
        XCTAssertEqual(UsageSpan.compact(4 * 60 + 30, roundingUp: false), "4m")
        XCTAssertEqual(UsageSpan.spoken(3600), "1 hour")
        XCTAssertEqual(UsageSpan.spoken(86_400 + 3600), "1 day 1 hour")
        XCTAssertEqual(UsageSpan.spoken(60), "1 minute")
    }

    func testSpokenRow() {
        let now = Date(timeIntervalSinceReferenceDate: 800_000_000)
        let low = UsageWindow(usedPercent: 91, resetsAt: now.addingTimeInterval(41 * 60))
        XCTAssertEqual(MenuBarSummary.spokenRow(kind: .fiveHour, window: low, now: now),
                       "5-hour window, 9 percent left, resets in 41 minutes, almost out")
        let ahead = UsageWindow(usedPercent: 62, resetsAt: now.addingTimeInterval(3.5 * 86_400))
        XCTAssertEqual(MenuBarSummary.spokenRow(kind: .weekly, window: ahead, now: now),
                       "Weekly window, 38 percent left, resets in 3 days 12 hours, faster than even pace")
        let unknown = UsageWindow(usedPercent: 38, resetsAt: nil)
        XCTAssertEqual(MenuBarSummary.spokenRow(kind: .fiveHour, window: unknown, now: now),
                       "5-hour window, 62 percent left, reset time unknown")
    }
}
