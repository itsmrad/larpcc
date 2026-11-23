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

    func testSessionPercentPerProvider() async throws {
        let (profiles, usage) = try await sample()
        func left(_ provider: Provider, _ id: String = "") -> Double? {
            MenuBarSummary.sessionLeftPercent(provider: provider, chosenProfileID: id, profiles: profiles, usage: usage)
        }
        XCTAssertEqual(left(.codex), 88)
        XCTAssertEqual(left(.claude), 9)
        XCTAssertEqual(left(.claude, "1"), 62)
        XCTAssertEqual(left(.claude, "3"), 9, "a selection from another provider is ignored")
        XCTAssertEqual(left(.codex, "missing"), 88)
        XCTAssertNil(left(.codex, "4"), "the chosen profile has no session data")
        XCTAssertNil(MenuBarSummary.sessionLeftPercent(provider: .codex, chosenProfileID: "", profiles: profiles, usage: [:]))

        let (fewer, usage2) = try await sample(disabling: "2")
        XCTAssertEqual(MenuBarSummary.sessionLeftPercent(provider: .claude, chosenProfileID: "2",
                                                         profiles: fewer, usage: usage2), 62)
        XCTAssertEqual(MenuBarMode(rawValue: "icon"), .icon)
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

    func testGroupsAreEnabledProvidersInConstraintOrder() async throws {
        let (profiles, usage) = try await sample()
        let groups = MenuBarSummary.groups(profiles, usage: usage)
        XCTAssertEqual(groups.map(\.provider), [.claude, .codex])
        XCTAssertEqual(groups.map { $0.profiles.map(\.id) }, [["2", "1"], ["3", "4"]])
        XCTAssertEqual(MenuBarSummary.groups(profiles.filter { $0.provider == .codex }, usage: usage).map(\.provider), [.codex])
        XCTAssertTrue(MenuBarSummary.groups(profiles.filter { !MenuBarSummary.isEnabled($0) }, usage: usage).isEmpty)
        XCTAssertEqual(MenuBarSummary.orderedProfiles(profiles, usage: usage, provider: .codex).map(\.id), ["3", "4"])
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

    func testWindowLengths() {
        XCTAssertEqual(UsageWindowKind.fiveHour.length, 18_000)
        XCTAssertEqual(UsageWindowKind.weekly.length, 604_800)
    }

    func testPaceWordingAndThresholds() {
        let now = Date(timeIntervalSinceReferenceDate: 800_000_000)
        let half = now.addingTimeInterval(2.5 * 3600)
        func pace(_ used: Double, resetsAt: Date?) -> String? {
            UsagePace.text(UsageWindow(usedPercent: used, resetsAt: resetsAt), kind: .fiveHour, capturedAt: now, now: now)
        }
        XCTAssertEqual(pace(30, resetsAt: half), "20% in reserve · Lasts until reset")
        XCTAssertEqual(pace(60, resetsAt: half), "10% in deficit · Runs out in 1h 40m")
        XCTAssertEqual(pace(45, resetsAt: half), "On pace · Lasts until reset")
        XCTAssertEqual(pace(55, resetsAt: half), "On pace · Runs out in 2h 3m")
        XCTAssertEqual(pace(50, resetsAt: half), "On pace · Lasts until reset")
        XCTAssertEqual(pace(0, resetsAt: half), "50% in reserve · Lasts until reset")
        XCTAssertEqual(pace(100, resetsAt: half), "50% in deficit · Runs out in 0m")
        XCTAssertNil(pace(99, resetsAt: nil))
        XCTAssertEqual(pace(99, resetsAt: now), UsagePace.outOfDate)
        XCTAssertNil(pace(1, resetsAt: now.addingTimeInterval(18_000)))
        XCTAssertNil(pace(1, resetsAt: now.addingTimeInterval(19_000)))
    }

    func testPaceIsTakenAtCaptureTimeAndExpires() {
        let now = Date(timeIntervalSinceReferenceDate: 800_000_000)
        let captured = now.addingTimeInterval(-3600)
        let window = UsageWindow(usedPercent: 30, resetsAt: captured.addingTimeInterval(2.5 * 3600))
        // At capture, 50% of the window was left and 70% of the quota: unchanged an hour later.
        XCTAssertEqual(UsagePace.text(window, kind: .fiveHour, capturedAt: captured, now: now),
                       "20% in reserve · Lasts until reset")
        XCTAssertEqual(UsagePace.text(window, kind: .fiveHour, capturedAt: captured, now: now.addingTimeInterval(1800)),
                       "20% in reserve · Lasts until reset")
        // Once the reset time passes, the sample no longer describes the window.
        let ended = now.addingTimeInterval(2 * 3600)
        XCTAssertTrue(UsagePace.isOutOfDate(window, now: ended))
        XCTAssertEqual(UsagePace.text(window, kind: .fiveHour, capturedAt: captured, now: ended), "Out of date — refresh")
        XCTAssertEqual(MenuBarSummary.spokenRow(kind: .fiveHour, window: window, capturedAt: captured, now: ended),
                       "5-hour window, 70 percent left, out of date, refresh")
        XCTAssertFalse(UsagePace.isOutOfDate(UsageWindow(usedPercent: 30, resetsAt: nil), now: ended))
    }

    func testNoPaceWithoutACaptureTime() {
        let now = Date(timeIntervalSinceReferenceDate: 800_000_000)
        let window = UsageWindow(usedPercent: 30, resetsAt: now.addingTimeInterval(2.5 * 3600))
        XCTAssertNil(UsagePace.text(window, kind: .fiveHour, capturedAt: nil, now: now), "no fallback to now")
        XCTAssertEqual(MenuBarSummary.spokenRow(kind: .fiveHour, window: window, capturedAt: nil, now: now),
                       "5-hour window, 70 percent left, resets in 2 hours 30 minutes")
        // An ended window still says so.
        let ended = UsageWindow(usedPercent: 30, resetsAt: now.addingTimeInterval(-60))
        XCTAssertEqual(UsagePace.text(ended, kind: .fiveHour, capturedAt: nil, now: now), "Out of date — refresh")
    }

    func testFixtureShowsEachPaceState() async throws {
        let (_, usage) = try await sample()
        let now = Date()
        let rows = usage.values.flatMap { report in
            UsageWindowKind.allCases.compactMap { kind in
                kind.window(in: report).flatMap { UsagePace.text($0, kind: kind, capturedAt: report.capturedAt, now: now) }
            }
        }
        XCTAssertTrue(rows.contains { $0.contains("in reserve · Lasts until reset") })
        XCTAssertTrue(rows.contains { $0.contains("in deficit · Runs out in") })
        XCTAssertTrue(rows.contains { $0.hasPrefix("On pace ·") })
    }

    func testResetCountdownAndSpans() {
        let now = Date(timeIntervalSinceReferenceDate: 800_000_000)
        XCTAssertEqual(ResetCountdown.spoken(resetsAt: nil, now: now), "reset time unknown")
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
        XCTAssertEqual(MenuBarSummary.spokenRow(kind: .fiveHour, window: low, capturedAt: now, now: now),
                       "5-hour window, 9 percent left, resets in 41 minutes, almost out, On pace, Runs out in 26 minutes")
        let ahead = UsageWindow(usedPercent: 62, resetsAt: now.addingTimeInterval(3.5 * 86_400))
        XCTAssertEqual(MenuBarSummary.spokenRow(kind: .weekly, window: ahead, capturedAt: now, now: now),
                       "Weekly window, 38 percent left, resets in 3 days 12 hours, 12 percent in deficit, Runs out in 2 days 3 hours")
        let unknown = UsageWindow(usedPercent: 38, resetsAt: nil)
        XCTAssertEqual(MenuBarSummary.spokenRow(kind: .fiveHour, window: unknown, capturedAt: nil, now: now),
                       "5-hour window, 62 percent left, reset time unknown")
    }
}
