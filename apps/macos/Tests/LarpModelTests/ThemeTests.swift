import XCTest
@testable import LarpModel

final class ThemeTests: XCTestCase {
    func testAutomaticFollowsTheRunningMacOS() {
        XCTAssertEqual(ThemeChoice.automatic.resolved(osMajor: 27), .tahoe)
        XCTAssertEqual(ThemeChoice.automatic.resolved(osMajor: 26), .tahoe)
        XCTAssertEqual(ThemeChoice.automatic.resolved(osMajor: 15), .sequoia)
        XCTAssertEqual(ThemeChoice.automatic.resolved(osMajor: 14), .sonoma)
    }

    func testExplicitChoicesAndTheTahoeRequirement() {
        XCTAssertEqual(ThemeChoice.sonoma.resolved(osMajor: 26), .sonoma)
        XCTAssertEqual(ThemeChoice.sequoia.resolved(osMajor: 14), .sequoia)
        XCTAssertEqual(ThemeChoice.tahoe.resolved(osMajor: 26), .tahoe)
        XCTAssertTrue(ThemeChoice.tahoe.isAvailable(osMajor: 26))
        XCTAssertFalse(ThemeChoice.tahoe.isAvailable(osMajor: 15))
        XCTAssertEqual(ThemeChoice.tahoe.resolved(osMajor: 15), .sequoia, "falls back to Automatic")
        XCTAssertEqual(ThemeChoice.tahoe.resolved(osMajor: 14), .sonoma)
        XCTAssertTrue(ThemeChoice.sonoma.isAvailable(osMajor: 14))
    }

    func testStoredValuesRoundTripThroughUserDefaults() {
        // The test runner's own defaults: a suite would leave a preferences file behind.
        let defaults = UserDefaults.standard
        let saved = defaults.object(forKey: ThemeChoice.key)
        defer { defaults.set(saved, forKey: ThemeChoice.key) }
        defaults.removeObject(forKey: ThemeChoice.key)
        XCTAssertEqual(ThemeChoice(stored: defaults.string(forKey: ThemeChoice.key)), .automatic, "unset")
        for choice in ThemeChoice.allCases {
            defaults.set(choice.rawValue, forKey: ThemeChoice.key)
            XCTAssertEqual(ThemeChoice(stored: defaults.string(forKey: ThemeChoice.key)), choice)
        }
        defaults.set("aqua", forKey: ThemeChoice.key)
        XCTAssertEqual(ThemeChoice(stored: defaults.string(forKey: ThemeChoice.key)), .automatic, "unknown value")
        XCTAssertEqual(ThemeChoice.allCases.map(\.title), ["Automatic", "Tahoe", "Sequoia", "Sonoma"])
    }
}
