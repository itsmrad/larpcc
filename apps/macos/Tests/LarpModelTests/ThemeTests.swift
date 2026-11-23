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
        let suite = "larpcc.theme-tests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer {
            // removePersistentDomain leaves an empty plist behind; remove the file too.
            defaults.removePersistentDomain(forName: suite)
            let file = FileManager.default.homeDirectoryForCurrentUser
                .appendingPathComponent("Library/Preferences/\(suite).plist")
            try? FileManager.default.removeItem(at: file)
        }
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
