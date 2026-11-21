import SwiftUI
import AppKit
import LarpModel

// MARK: - Preferences (UserDefaults; never sent anywhere)

enum MenuBarPreferences {
    static let shownKey = "menuBarShown"
    static let modeKey = "menuBarMode"
    static let profileKey = "menuBarProfileID"
    static let hideDockKey = "menuBarHideDock"

    static var shown: Bool { UserDefaults.standard.object(forKey: shownKey) as? Bool ?? true }
    /// Hiding the Dock icon only counts while the menu bar item is there to reopen the app.
    static var hidesDock: Bool { shown && UserDefaults.standard.bool(forKey: hideDockKey) }
}

// MARK: - Store helpers

extension AppStore {
    /// D14: opening the panel re-reads Local usage. Live waits for the user or the opt-in interval.
    func menuBarPanelOpened() {
        if snapshot == nil {
            // A metadata refresh also re-reads Local usage.
            if !busy { startRefresh() }
        } else if UsagePreferences.source == .local {
            refreshUsage()
        }
    }

    /// The panel's Refresh: reads usage from the current source, loading profiles first if needed.
    func menuBarRefresh() {
        if snapshot == nil {
            if !busy { startRefresh() }
        } else {
            refreshUsage()
        }
    }
}

// MARK: - Dock icon

/// Optional "Hide Dock icon when the window is closed". Only changes the activation policy.
@MainActor enum MenuBarDockPolicy {
    private static var observers: [NSObjectProtocol] = []

    static func install() {
        guard observers.isEmpty else { return }
        let center = NotificationCenter.default
        observers.append(center.addObserver(forName: NSWindow.willCloseNotification, object: nil, queue: .main) { note in
            guard isSettingsWindow(note.object) else { return }
            Task { @MainActor in
                if MenuBarPreferences.hidesDock { NSApp.setActivationPolicy(.accessory) }
            }
        })
        // However the window comes back (panel, relaunch), it brings the Dock icon with it.
        observers.append(center.addObserver(forName: NSWindow.didBecomeKeyNotification, object: nil, queue: .main) { note in
            guard isSettingsWindow(note.object) else { return }
            Task { @MainActor in showDockIcon() }
        })
    }

    static func showDockIcon() {
        if NSApp.activationPolicy() != .regular { NSApp.setActivationPolicy(.regular) }
    }

    nonisolated private static func isSettingsWindow(_ object: Any?) -> Bool {
        (object as? NSWindow)?.identifier?.rawValue.hasPrefix("settings") ?? false
    }
}

// MARK: - General › Menu Bar

struct MenuBarSettingsSection: View {
    @ObservedObject var store: AppStore
    @AppStorage(MenuBarPreferences.shownKey) private var shown = true
    @AppStorage(MenuBarPreferences.modeKey) private var mode = MenuBarMode.tightest.rawValue
    @AppStorage(MenuBarPreferences.profileKey) private var profileID = ""
    @AppStorage(MenuBarPreferences.hideDockKey) private var hideDock = false

    var body: some View {
        let profiles = store.snapshot?.profiles ?? []
        Section("Menu Bar") {
            Toggle(isOn: $shown) {
                Text("Show in menu bar")
                Text("Usage limits, resets and quick actions, one click away.")
            }
            Picker("Menu bar shows", selection: $mode) {
                ForEach(MenuBarMode.allCases, id: \.self) { Text($0.title).tag($0.rawValue) }
            }
            .disabled(!shown)
            if mode == MenuBarMode.profile.rawValue {
                Picker("Profile", selection: $profileID) {
                    if !profiles.contains(where: { $0.id == profileID }) {
                        Text("Choose a profile").tag(profileID)
                    }
                    ForEach(profiles) { profile in
                        Text(profile.name).tag(profile.id)
                    }
                }
                .disabled(!shown)
            }
            Toggle(isOn: $hideDock) {
                Text("Hide Dock icon when the window is closed")
                Text(shown ? "LarpCC stays reachable from the menu bar." : "Needs “Show in menu bar”.")
            }
            .disabled(!shown)
        }
        .onAppear { MenuBarDockPolicy.install() }
    }
}

// MARK: - Screenshot hook (debug builds only)

#if DEBUG
/// `--menubar-panel-snapshot=<abs path.png>`: renders the panel with sample data at 2x and exits.
/// Debug builds only; sample data only. It never runs larp or reads usage from anywhere else.
enum MenuBarSnapshot {
    private static let prefix = "--menubar-panel-snapshot="

    static func runIfRequested() {
        guard let arg = CommandLine.arguments.first(where: { $0.hasPrefix(prefix) }) else { return }
        let path = String(arg.dropFirst(prefix.count))
        guard path.hasPrefix("/"), path.lowercased().hasSuffix(".png") else { exit(2) }
        Task { @MainActor in exit(await render(to: path) ? 0 : 1) }
    }

    @MainActor private static func render(to path: String) async -> Bool {
        let store = AppStore(client: FixtureLarpClient())
        guard store.isFixture else { return false }
        await store.refresh()
        store.refreshUsage()
        while store.usageLoading { try? await Task.sleep(nanoseconds: 20_000_000) }

        AppearanceMode.launchOverride?.apply()
        let dark: Bool
        switch AppearanceMode.launchOverride {
        case .dark?: dark = true
        case .light?: dark = false
        default: dark = NSApp.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        }
        let renderer = ImageRenderer(content: MenuBarPanel(store: store, isSnapshot: true)
            .environment(\.colorScheme, dark ? .dark : .light))
        renderer.scale = 2
        var png: Data?
        let appearance = NSAppearance(named: dark ? .darkAqua : .aqua) ?? NSAppearance.currentDrawing()
        appearance.performAsCurrentDrawingAppearance {
            if let image = renderer.cgImage {
                png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
            }
        }
        guard let png else { return false }
        do { try png.write(to: URL(fileURLWithPath: path)); return true } catch { return false }
    }
}
#endif
