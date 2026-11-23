import XCTest
import SwiftUI
import AppKit
@testable import LarpCC
import LarpModel

/// The Settings window, the menu bar panel and the status item each get their theme from
/// `.themed()`, which observes the stored Theme setting. Changing the setting, as the
/// Theme picker does, must reach a view under `.themed()` at once, without a relaunch.
@MainActor
final class ThemePropagationTests: XCTestCase {
    private final class Seen { var themes: [Theme] = [] }

    private struct Probe: View {
        @Environment(\.theme) private var theme
        let seen: Seen
        var body: some View {
            Color.clear.frame(width: 4, height: 4)
                .onChange(of: theme, initial: true) { _, theme in seen.themes.append(theme) }
        }
    }

    private func settle(_ host: NSView) {
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
    }

    func testAThemeChangeReachesViewsUnderThemedWithoutARelaunch() {
        let defaults = UserDefaults.standard
        let saved = defaults.object(forKey: ThemeChoice.key)
        defer { defaults.set(saved, forKey: ThemeChoice.key) }
        defaults.removeObject(forKey: ThemeChoice.key)

        let seen = Seen()
        let host = NSHostingView(rootView: Probe(seen: seen).themed())
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 4, height: 4), styleMask: .borderless,
                              backing: .buffered, defer: false)
        window.contentView = host
        settle(host)
        let os = ThemePreference.osMajor
        XCTAssertEqual(seen.themes.last, ThemeChoice.automatic.resolved(osMajor: os))

        for choice in [ThemeChoice.tahoe, .sequoia, .sonoma, .automatic] {
            defaults.set(choice.rawValue, forKey: ThemeChoice.key)
            settle(host)
            XCTAssertEqual(seen.themes.last, choice.resolved(osMajor: os), "after choosing \(choice.title)")
        }
        XCTAssertTrue(seen.themes.contains(.sequoia) && seen.themes.contains(.sonoma))
        window.contentView = nil
    }

    func testTheMenuBarStatusWeightFollowsTheTheme() {
        XCTAssertEqual(Theme.tahoe.tokens.statusWeight, .medium)
        XCTAssertEqual(Theme.sequoia.tokens.statusWeight, .regular)
        XCTAssertEqual(Theme.sonoma.tokens.statusWeight, .regular)
    }
}
