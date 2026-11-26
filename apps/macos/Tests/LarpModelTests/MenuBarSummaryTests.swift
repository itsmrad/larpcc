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
        let usage = try await FixtureLarpClient().usage(source: .local)
        XCTAssertEqual(ResetAvailability.text(credits: usage["1"]?.resetCredits), "2 resets available")
        XCTAssertEqual(ResetAvailability.text(credits: usage["2"]?.resetCredits), "Reset info not available")
        XCTAssertEqual(ResetAvailability.text(credits: usage["3"]?.resetCredits), "No resets available")
        XCTAssertEqual(ResetAvailability.text(credits: usage["4"]?.resetCredits), "Reset info not available", "no report")
    }

    func testPaceUsesPredictionAtWindowCapture() {
        let captured = Date(timeIntervalSinceReferenceDate: 800_000_000)
        let pace = UsagePrediction(elapsedPercent: 50, reservePercent: -10,
                                   runsOutAt: captured.addingTimeInterval(6000), lastsUntilReset: false)
        let window = UsageWindow(usedPercent: 60, resetsAt: captured.addingTimeInterval(9000),
                                 capturedAt: captured, pace: pace)
        XCTAssertEqual(UsagePace.text(window), "10% in deficit · Runs out in 1h 40m")
        let noPrediction = UsageWindow(usedPercent: 60, resetsAt: window.resetsAt, capturedAt: captured)
        XCTAssertNil(UsagePace.text(noPrediction))
    }

    func testFixtureShowsEachPaceState() async throws {
        let (_, usage) = try await sample()
        let rows = usage.values.flatMap { report in
            UsageWindowKind.allCases.compactMap { kind in
                kind.window(in: report).flatMap { UsagePace.text($0) }
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
        XCTAssertEqual(MenuBarSummary.spokenRow(title: "5-hour window", window: low, now: now),
                       "5-hour window, 9 percent left, resets in 41 minutes, almost out")
        let unknown = UsageWindow(usedPercent: 38, resetsAt: nil, reason: .resetTimeUnknown)
        XCTAssertEqual(MenuBarSummary.spokenRow(title: "5-hour window", window: unknown, now: now),
                       "5-hour window, 62 percent left, Reset time unknown")
        let reset = UsageWindow(usedPercent: nil, resetsAt: now, reason: .resetSinceCapture)
        XCTAssertEqual(MenuBarSummary.spokenRow(title: "Weekly window", window: reset, now: now),
                       "Weekly window, Reset due")
    }
}
