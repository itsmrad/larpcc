import SwiftUI
import AppKit
import Combine
import LarpModel

// MARK: - Preferences (UserDefaults; never sent anywhere)

enum MenuBarPreferences {
    static let shownKey = "menuBarShown"
    static let modeKey = "menuBarMode"
    static let codexProfileKey = "menuBarCodexProfileID"
    static let claudeProfileKey = "menuBarClaudeProfileID"
    static let hideDockKey = "menuBarHideDock"

    static var shown: Bool { UserDefaults.standard.object(forKey: shownKey) as? Bool ?? true }
    /// Hiding the Dock icon only counts while the menu bar item is there to reopen the app.
    static var hidesDock: Bool { shown && UserDefaults.standard.bool(forKey: hideDockKey) }
}

// MARK: - Store helpers

/// A menu bar window can hide while its SwiftUI view remains mounted.
struct UsageWindowVisibility: NSViewRepresentable {
    let store: AppStore
    let usageView: AppStore.UsageView

    func makeNSView(context: Context) -> Observer {
        Observer(store: store, usageView: usageView)
    }

    func updateNSView(_ view: Observer, context: Context) {}

    static func dismantleNSView(_ view: Observer, coordinator: ()) {
        view.stopObserving()
    }

    final class Observer: NSView {
        private weak var store: AppStore?
        private let usageView: AppStore.UsageView
        private var observation: AnyCancellable?

        init(store: AppStore, usageView: AppStore.UsageView) {
            self.store = store
            self.usageView = usageView
            super.init(frame: .zero)
        }

        required init?(coder: NSCoder) { return nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            observation = window.map { window in
                NotificationCenter.default.publisher(for: NSWindow.didChangeOcclusionStateNotification, object: window)
                    .sink { [weak self] _ in self?.updateVisibility() }
            }
            updateVisibility()
        }

        func stopObserving() {
            observation = nil
            store?.setUsageView(usageView, visible: false)
        }

        private func updateVisibility() {
            store?.setUsageView(usageView, visible: window?.occlusionState.contains(.visible) == true)
        }
    }
}

extension AppStore {
    /// D14: opening the panel re-reads Local usage. Live waits for the user or the opt-in interval.
    func menuBarPanelOpened() {
        guard UsagePreferences.source == .local else { return }
        if snapshot == nil {
            // A metadata refresh also re-reads Local usage.
            if !busy { startRefresh() }
        } else {
            refreshUsage()
        }
    }

    /// The panel's Refresh: reads usage from the current source, recovering profiles first if needed.
    func menuBarRefresh() {
        if snapshot == nil, !busy { startRefresh() }
        // A metadata refresh re-reads Local usage itself; Live usage is only read on request.
        if snapshot != nil || UsagePreferences.source == .live { refreshUsage() }
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
            MainActor.assumeIsolated {
                guard isSettingsWindow(note.object) else { return }
                if MenuBarPreferences.hidesDock { NSApp.setActivationPolicy(.accessory) }
            }
        })
        // However the window comes back (panel, relaunch), it brings the Dock icon with it.
        observers.append(center.addObserver(forName: NSWindow.didBecomeKeyNotification, object: nil, queue: .main) { note in
            MainActor.assumeIsolated {
                if isSettingsWindow(note.object) { showDockIcon() }
            }
        })
    }

    /// The status item's insertion binding. Removing the item from the menu bar (⌘-drag)
    /// brings the Dock icon back, so the app never runs without a way to reach it.
    static func insertion(_ shown: Binding<Bool>) -> Binding<Bool> {
        Binding(get: { shown.wrappedValue }, set: { value in
            shown.wrappedValue = value
            if !value { Task { @MainActor in showDockIcon() } }
        })
    }

    static func showDockIcon() {
        if NSApp.activationPolicy() != .regular { NSApp.setActivationPolicy(.regular) }
    }

    static func isSettingsWindow(_ object: Any?) -> Bool {
        (object as? NSWindow)?.identifier?.rawValue.hasPrefix("settings") ?? false
    }
}

/// SwiftUI quits an app whose only window closes. While the menu bar item is shown,
/// closing the window keeps LarpCC running so the panel (and "Hide Dock icon") still work.
@MainActor
final class MenuBarAppDelegate: NSObject, NSApplicationDelegate {
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        !MenuBarPreferences.shown
    }
}

// MARK: - General › Menu Bar

struct MenuBarSettingsSection: View {
    @ObservedObject var store: AppStore
    @AppStorage(MenuBarPreferences.shownKey) private var shown = true
    @AppStorage(MenuBarPreferences.modeKey) private var mode = MenuBarMode.tightest.rawValue
    @AppStorage(MenuBarPreferences.codexProfileKey) private var codexProfileID = ""
    @AppStorage(MenuBarPreferences.claudeProfileKey) private var claudeProfileID = ""
    @AppStorage(MenuBarPreferences.hideDockKey) private var hideDock = false

    var body: some View {
        let profiles = store.snapshot?.profiles ?? []
        Section("Menu Bar") {
            Toggle(isOn: $shown) {
                Text("Show in menu bar")
                Text("Usage limits, resets and quick actions, one click away.")
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
            Picker("Menu bar shows", selection: Binding(
                get: { MenuBarMode(rawValue: mode) ?? .tightest },
                set: { mode = $0.rawValue }
            )) {
                ForEach(MenuBarMode.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .disabled(!shown)
            if mode != MenuBarMode.icon.rawValue {
                profilePicker(.codex, choice: $codexProfileID, profiles: profiles)
                profilePicker(.claude, choice: $claudeProfileID, profiles: profiles)
            }
            Toggle(isOn: $hideDock) {
                Text("Hide Dock icon when the window is closed")
                Text(shown ? "LarpCC stays reachable from the menu bar." : "Needs “Show in menu bar”.")
            }
            .disabled(!shown)
        }
        .onAppear { MenuBarDockPolicy.install() }
    }

    private func profilePicker(_ provider: Provider, choice: Binding<String>, profiles: [Profile]) -> some View {
        let options = MenuBarSummary.orderedProfiles(profiles, usage: [:], provider: provider)
        let selection = Binding(
            get: { options.contains { $0.id == choice.wrappedValue } ? choice.wrappedValue : "" },
            set: { choice.wrappedValue = $0 }
        )
        return Picker(provider.title, selection: selection) {
            Text("Most constrained").tag("")
            ForEach(options) { profile in
                Text(profile.name).tag(profile.id)
            }
        }
        .disabled(!shown || options.isEmpty)
    }
}

// MARK: - Screenshot hook (debug builds only)

#if DEBUG
/// `--menubar-panel-snapshot=<abs path.png>`: renders the panel with sample data at 2x and exits.
/// `--menubar-panel-expanded=<profile id>` opens that profile's details.
/// `--menubar-panel-selected=<profile id>` draws that card selected.
/// `--menubar-panel-accessibility` renders with Increase Contrast and Reduce Transparency.
/// Debug builds only; sample data only. It never runs larp or reads usage from anywhere else.
enum MenuBarSnapshot {
    private static let prefix = "--menubar-panel-snapshot="
    private static let expandedPrefix = "--menubar-panel-expanded="

    /// `--menubar-screen-height=<pt>`: stands in for a short display when checking overflow.
    static var screenHeight: CGFloat? {
        CommandLine.arguments.first { $0.hasPrefix("--menubar-screen-height=") }
            .flatMap { Double($0.dropFirst("--menubar-screen-height=".count)) }.map { CGFloat($0) }
    }

    /// `--menubar-live-snapshots=<abs dir>`: whenever the running status item or panel receives
    /// a theme, it writes `<dir>/<n>-<view>-<theme>.png` from that live environment. This checks
    /// that a Settings theme change reaches the menu bar without a relaunch.
    @MainActor private static var liveCount = 0
    @MainActor static func recordLive(_ view: String, theme: Theme, image: () -> NSImage?) {
        let prefix = "--menubar-live-snapshots="
        guard let dir = CommandLine.arguments.first(where: { $0.hasPrefix(prefix) })?.dropFirst(prefix.count),
              dir.hasPrefix("/"), let tiff = image().flatMap({ $0.tiffRepresentation }),
              let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) else { return }
        liveCount += 1
        try? png.write(to: URL(fileURLWithPath: "\(dir)/\(liveCount)-\(view)-\(theme.rawValue).png"))
    }

    /// `--menubar-notice=<text>`: shows that text as the panel's notice, for overflow checks.
    static var notice: String? {
        CommandLine.arguments.first { $0.hasPrefix("--menubar-notice=") }.map { String($0.dropFirst("--menubar-notice=".count)) }
    }

    static func runIfRequested() {
        guard let arg = CommandLine.arguments.first(where: { $0.hasPrefix(prefix) }) else { return }
        let path = String(arg.dropFirst(prefix.count))
        guard path.hasPrefix("/"), path.lowercased().hasSuffix(".png") else { exit(2) }
        func value(_ prefix: String) -> String? {
            CommandLine.arguments.first { $0.hasPrefix(prefix) }.map { String($0.dropFirst(prefix.count)) }
        }
        let expanded = value(expandedPrefix), selected = value("--menubar-panel-selected=")
        Task { @MainActor in exit(await render(to: path, expanded: expanded, selected: selected) ? 0 : 1) }
    }

    /// `--menubar-status-snapshot=<abs path.png>`: the status item on a light and a dark bar.
    private static func statusPath() -> String? {
        let prefix = "--menubar-status-snapshot="
        return CommandLine.arguments.first { $0.hasPrefix(prefix) }.map { String($0.dropFirst(prefix.count)) }
    }

    @MainActor private static func renderStatus(_ store: AppStore, to path: String) -> Bool {
        let rings = MenuBarLabel.rings(store, codexProfileID: "", claudeProfileID: "")
        guard let image = MenuBarLabel.templateImage(rings, weight: ThemePreference.current.tokens.statusWeight) else { return false }
        func bar(_ dark: Bool) -> some View {
            Image(nsImage: image).renderingMode(.template)
                .foregroundStyle(dark ? Color.white : Color.black.opacity(0.85))
                .padding(.horizontal, 12).frame(height: 24)
                .background(dark ? Color(white: 0.16) : Color(white: 0.93))
        }
        let renderer = ImageRenderer(content: VStack(spacing: 0) { bar(false); bar(true) })
        renderer.scale = 2
        guard let cgImage = renderer.cgImage,
              let png = NSBitmapImageRep(cgImage: cgImage).representation(using: .png, properties: [:]) else { return false }
        return (try? png.write(to: URL(fileURLWithPath: path))) != nil
    }

    @MainActor private static func render(to path: String, expanded: String?, selected: String?) async -> Bool {
        let store = AppStore(client: FixtureLarpClient())
        guard store.isFixture else { return false }
        // Local usage follows the metadata refresh; Live (sample) usage is read once here.
        await store.refresh()
        if UsagePreferences.source == .live { store.refreshUsage() }
        for await loading in store.$usageLoading.values where !loading { break }
        if let statusPath = statusPath(), !renderStatus(store, to: statusPath) { return false }

        AppearanceMode.launchOverride?.apply()
        let dark: Bool
        switch AppearanceMode.launchOverride {
        case .dark?: dark = true
        case .light?: dark = false
        default: dark = NSApp.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        }
        let accessible = CommandLine.arguments.contains("--menubar-panel-accessibility")
        let renderer = ImageRenderer(content: MenuBarPanel(store: store, isSnapshot: true, snapshotExpandedID: expanded,
                                                            snapshotSelectedID: selected)
            .environment(\.colorScheme, dark ? .dark : .light)
            .environment(\._colorSchemeContrast, accessible ? .increased : .standard)
            .environment(\._accessibilityReduceTransparency, accessible)
            .environment(\.theme, ThemePreference.current))
        renderer.scale = 2
        var png: Data?
        let name: NSAppearance.Name = accessible ? (dark ? .accessibilityHighContrastDarkAqua : .accessibilityHighContrastAqua)
            : (dark ? .darkAqua : .aqua)
        let appearance = NSAppearance(named: name) ?? NSAppearance.currentDrawing()
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
