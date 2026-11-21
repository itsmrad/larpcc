import SwiftUI
import AppKit
import LarpModel

// MARK: - Status item

struct MenuBarLabel: View {
    @ObservedObject var store: AppStore
    @AppStorage(MenuBarPreferences.modeKey) private var mode = MenuBarMode.tightest.rawValue
    @AppStorage(MenuBarPreferences.codexProfileKey) private var codexProfileID = ""
    @AppStorage(MenuBarPreferences.claudeProfileKey) private var claudeProfileID = ""

    var body: some View {
        let profiles = store.snapshot?.profiles ?? []
        let lines: [(provider: Provider, text: String)] = [Provider.codex, .claude].compactMap { provider in
            let choice = provider == .codex ? codexProfileID : claudeProfileID
            return MenuBarSummary.statusText(provider: provider, chosenProfileID: choice,
                                             profiles: profiles, usage: store.usage).map { (provider, $0) }
        }
        Group {
            if mode != MenuBarMode.icon.rawValue, !lines.isEmpty, let image = templateImage(lines) {
                Image(nsImage: image)
            } else {
                Image(systemName: MenuBarSummary.symbolName(leftPercent:
                    MenuBarSummary.tightestLeftPercent(profiles: profiles, usage: store.usage)))
            }
        }
        .accessibilityLabel(mode == MenuBarMode.icon.rawValue || lines.isEmpty ? "LarpCC" :
            (["LarpCC"] + lines.map {
                "\($0.provider.title) session, \($0.text.dropFirst(3).dropLast()) percent left"
            }).joined(separator: ", "))
        .onAppear { MenuBarDockPolicy.install() }
    }

    @MainActor private func templateImage(_ lines: [(provider: Provider, text: String)]) -> NSImage? {
        let stacked = lines.count > 1
        let content = VStack(alignment: .leading, spacing: 0) {
            ForEach(lines.indices, id: \.self) { index in
                HStack(spacing: 3) {
                    ProviderMark(provider: lines[index].provider)
                        .fill(.black)
                        .frame(width: stacked ? 8 : 12, height: stacked ? 8 : 12)
                    Text(lines[index].text)
                        .font(.system(size: stacked ? 9 : 12, weight: .semibold).monospacedDigit())
                        .foregroundStyle(.black)
                }
                .frame(height: stacked ? 10 : 16)
            }
        }
        .padding(.horizontal, 1)
        .fixedSize()
        let renderer = ImageRenderer(content: content)
        renderer.scale = 2
        guard let bitmap = renderer.cgImage else { return nil }
        let image = NSImage(cgImage: bitmap, size: NSSize(width: CGFloat(bitmap.width) / 2,
                                                         height: CGFloat(bitmap.height) / 2))
        image.isTemplate = true
        return image
    }
}

// MARK: - Panel

struct MenuBarPanel: View {
    @ObservedObject var store: AppStore
    var snapshotTab: MenuBarTab? = nil
    @Environment(\.openWindow) private var openWindow
    @Environment(\.colorScheme) private var colorScheme
    @AppStorage(MenuBarPreferences.tabKey) private var storedTab = MenuBarTab.overview.rawValue
    @AppStorage(UsagePreferences.sourceKey) private var storedSource = UsageSource.local.rawValue
    @AppStorage(UsagePreferences.consentKey) private var consented = false
    @FocusState private var hasKeyboardFocus: Bool
    @State private var keyboardItem: Item?
    @State private var hoveredItem: Item?
    @State private var desktopProfileID: String?
    @State private var sectionsFrame: CGRect = .zero
    @State private var recoverAfterAction = false

    private enum Item: Hashable {
        case profile(String), desktop, refresh, settings, quit
    }

    private var source: UsageSource { storedSource == UsageSource.live.rawValue && consented ? .live : .local }
    private var allProfiles: [Profile] { store.snapshot?.profiles ?? [] }
    private var tabs: [MenuBarTab] { MenuBarSummary.tabs(profiles: allProfiles) }
    private var tab: MenuBarTab {
        if let snapshotTab { return snapshotTab }
        guard let saved = MenuBarTab(rawValue: storedTab), tabs.contains(saved) else { return .overview }
        return saved
    }
    private var profiles: [Profile] {
        MenuBarSummary.orderedProfiles(allProfiles, usage: store.usage, provider: tab.provider)
    }
    private var desktopProfile: Profile? {
        profiles.first { $0.id == desktopProfileID } ?? profiles.first
    }
    private var canOpenDesktop: Bool {
        desktopProfile?.desktopEnabled == true && !store.isFixture && !store.busy
    }
    private var canRefresh: Bool { !store.usageLoading && !store.busy }
    private var items: [Item] {
        var items = profiles.map { Item.profile($0.id) }
        if canOpenDesktop { items.append(.desktop) }
        if canRefresh { items.append(.refresh) }
        return items + [.settings, .quit]
    }
    private var highlighted: Item? {
        let candidate = hoveredItem ?? keyboardItem
        if let candidate, items.contains(candidate) { return candidate }
        return profiles.first.map { .profile($0.id) }
    }

    var body: some View {
        VStack(spacing: 0) {
            tabStrip
            notices
            Divider()
            sectionsArea
            Divider()
            footer
        }
        .frame(width: 332)
        .background {
            if snapshotTab != nil {
                Color(nsColor: .windowBackgroundColor)
            } else {
                MenuBarMaterial().accessibilityHidden(true)
            }
        }
        .focusable(snapshotTab == nil)
        .focused($hasKeyboardFocus)
        .focusEffectDisabled()
        .defaultFocus($hasKeyboardFocus, true)
        .onKeyPress(.downArrow) { moveHighlight(by: 1) }
        .onKeyPress(.upArrow) { moveHighlight(by: -1) }
        .onKeyPress(.leftArrow) { moveTab(by: -1) }
        .onKeyPress(.rightArrow) { moveTab(by: 1) }
        .onKeyPress(.tab, phases: .down) { press in
            moveTab(by: press.modifiers.contains(.shift) ? -1 : 1)
        }
        .onKeyPress(.return, phases: .down) { _ in
            activate(highlighted ?? .settings)
            return .handled
        }
        .onExitCommand(perform: closePanel)
        .onAppear {
            guard snapshotTab == nil else { return }
            hasKeyboardFocus = true
            store.menuBarPanelOpened()
        }
        .onChange(of: store.busy) { _, busy in
            guard !busy, recoverAfterAction else { return }
            recoverAfterAction = false
            // Recover the sections once after a failed launch clears the snapshot.
            if store.snapshot == nil { store.startRefresh() }
        }
    }

    private var tabStrip: some View {
        HStack(spacing: 5) {
            ForEach(tabs, id: \.self) { item in
                let selected = tab == item
                let highlighted = selected && item == .overview
                Button { selectTab(item) } label: {
                    VStack(spacing: 3) {
                        if let provider = item.provider {
                            ProviderGlyph(provider: provider, size: 16)
                        } else {
                            Image(systemName: "square.grid.2x2")
                                .frame(width: 20, height: 20)
                        }
                        Text(item.title)
                            .font(.caption.weight(selected ? .semibold : .medium))
                        if let provider = item.provider {
                            Capsule().fill(MenuBarColors.provider(provider, scheme: colorScheme))
                                .frame(width: 35, height: selected ? 3 : 2)
                        }
                    }
                    .frame(maxWidth: .infinity)
                    .frame(height: 47)
                    .foregroundStyle(highlighted ? MenuBarColors.selectedText : Color.primary)
                    .background {
                        if highlighted {
                            RoundedRectangle(cornerRadius: 6).fill(MenuBarColors.selection)
                        } else if selected {
                            RoundedRectangle(cornerRadius: 6).fill(.quaternary)
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(item.title)
                .accessibilityAddTraits(selected ? .isSelected : [])
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
    }

    @ViewBuilder
    private var notices: some View {
        if store.isFixture {
            notice("Sample data", symbol: "testtube.2")
        }
        if let error = store.usageError {
            notice(error, symbol: "exclamationmark.triangle.fill")
        }
        if let message = store.message, snapshotTab == nil {
            notice(message, symbol: "info.circle", dismiss: { store.message = nil })
        }
    }

    private func notice(_ text: String, symbol: String, dismiss: (() -> Void)? = nil) -> some View {
        HStack(spacing: 6) {
            Image(systemName: symbol).foregroundStyle(.secondary)
                .accessibilityHidden(true)
            Text(text)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)
                .help(text)
            if let dismiss {
                Button(action: dismiss) { Image(systemName: "xmark") }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("Dismiss message")
            }
        }
        .font(.caption)
        .padding(.horizontal, 14)
        .padding(.bottom, 7)
    }

    @ViewBuilder
    private var sectionsArea: some View {
        if snapshotTab != nil {
            sections
        } else {
            let height = min(max(sectionsFrame.height, 60), maxSectionsHeight)
            VStack(spacing: 0) {
                ScrollViewReader { proxy in
                    ScrollView {
                        sections.background {
                            GeometryReader { geometry in
                                Color.clear.preference(key: MenuBarSectionsFrame.self,
                                                       value: geometry.frame(in: .named("menuBarSections")))
                            }
                        }
                    }
                    .id(tab)
                    .coordinateSpace(name: "menuBarSections")
                    .scrollBounceBehavior(.basedOnSize)
                    .frame(height: height)
                    .onPreferenceChange(MenuBarSectionsFrame.self) { sectionsFrame = $0 }
                    .onChange(of: keyboardItem) { _, item in
                        if let item, case .profile = item { proxy.scrollTo(item) }
                    }
                }
                if sectionsFrame.height > height {
                    Image(systemName: "chevron.down")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.secondary)
                        .frame(height: 14)
                        .opacity(sectionsFrame.maxY > height + 1 ? 1 : 0)
                        .accessibilityHidden(true)
                }
            }
        }
    }

    private var maxSectionsHeight: CGFloat {
        let pointerScreen = NSScreen.screens.first { $0.frame.contains(NSEvent.mouseLocation) }
        let screen = NSApp.keyWindow?.screen ?? pointerScreen ?? NSScreen.main
        // Leave room for the tab strip, notices and the four footer rows.
        return max(120, (screen?.visibleFrame.height ?? 800) - 230)
    }

    private var sections: some View {
        VStack(spacing: 0) {
            if profiles.isEmpty {
                Text(emptyMessage)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(18)
                    .frame(maxWidth: .infinity)
            }
            ForEach(profiles) { profile in
                let item = Item.profile(profile.id)
                if profile.id != profiles.first?.id {
                    Divider().padding(.horizontal, 9).padding(.vertical, 3)
                }
                Button { activate(item) } label: {
                    MenuBarProfileSection(profile: profile, report: store.usage[profile.id],
                                          source: source, selected: highlighted == item)
                }
                .buttonStyle(.plain)
                .onHover { hover(item, entered: $0) }
                .id(item)
            }
        }
        .padding(.horizontal, 5)
        .padding(.vertical, 4)
    }

    private var emptyMessage: String {
        if store.busy || store.snapshot == nil && store.isFixture { return "Loading profiles…" }
        if store.snapshot == nil { return "Profiles are unavailable. Open Settings to choose larp or use sample data." }
        return "No profiles are switched on. Turn one on in Settings."
    }

    private func hover(_ item: Item, entered: Bool) {
        guard snapshotTab == nil else { return }
        if entered {
            guard items.contains(item) else { return }
            hoveredItem = item
            keyboardItem = item
            if case let .profile(id) = item { desktopProfileID = id }
        } else if hoveredItem == item {
            hoveredItem = nil
        }
    }

    private func moveHighlight(by step: Int) -> KeyPress.Result {
        let items = items
        let index = highlighted.flatMap { items.firstIndex(of: $0) } ?? (step > 0 ? -1 : items.count)
        let item = items[min(max(index + step, 0), items.count - 1)]
        hoveredItem = nil
        keyboardItem = item
        if case let .profile(id) = item { desktopProfileID = id }
        return .handled
    }

    private func moveTab(by step: Int) -> KeyPress.Result {
        let tabs = tabs
        let index = tabs.firstIndex(of: tab) ?? 0
        selectTab(tabs[(index + step + tabs.count) % tabs.count])
        return .handled
    }

    private func selectTab(_ tab: MenuBarTab) {
        if tab != self.tab {
            storedTab = tab.rawValue
            hoveredItem = nil
            keyboardItem = nil
            desktopProfileID = nil
        }
        hasKeyboardFocus = true
    }

    private var footer: some View {
        VStack(spacing: 0) {
            menuRow(.desktop, title: desktopProfile.map { "Open in \($0.provider.desktopApp)" } ?? "Open Desktop App",
                    enabled: canOpenDesktop)
                .accessibilityHint(desktopProfile?.name ?? "")
            menuRow(.refresh, title: refreshTitle, shortcut: "⌘R", enabled: canRefresh)
                .keyboardShortcut("r", modifiers: .command)
            menuRow(.settings, title: "Open LarpCC Settings…", shortcut: "⌘,")
                .keyboardShortcut(",", modifiers: .command)
            menuRow(.quit, title: "Quit LarpCC", shortcut: "⌘Q")
                .keyboardShortcut("q", modifiers: .command)
        }
        .padding(5)
    }

    private var refreshTitle: String {
        if store.usageLoading { return "Updating Usage…" }
        return "Refresh"
    }

    private func menuRow(_ item: Item, title: String, shortcut: String = "", enabled: Bool = true) -> some View {
        let selected = highlighted == item
        return Button { activate(item) } label: {
            HStack {
                Text(title)
                Spacer()
                Text(shortcut)
                    .foregroundStyle(selected ? MenuBarColors.selectedText : Color.secondary)
                    .accessibilityHidden(true)
            }
            .font(.callout)
            .foregroundStyle(selected ? MenuBarColors.selectedText : Color.primary)
            .padding(.horizontal, 9)
            .frame(height: 24)
            .opacity(enabled ? 1 : 0.48)
            .background {
                if selected { RoundedRectangle(cornerRadius: 5).fill(MenuBarColors.selection) }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .onHover { hover(item, entered: $0) }
    }

    private func activate(_ item: Item) {
        switch item {
        case .profile, .settings: openSettings()
        case .desktop:
            guard canOpenDesktop, let desktopProfile else { return }
            hasKeyboardFocus = true
            recoverAfterAction = true
            store.perform(.open(profileID: desktopProfile.id, surface: .desktop))
        case .refresh:
            if canRefresh {
                hasKeyboardFocus = true
                store.menuBarRefresh()
            }
        case .quit: NSApp.terminate(nil)
        }
    }

    private func openSettings() {
        closePanel()
        MenuBarDockPolicy.showDockIcon()
        openWindow(id: "settings")
        NSApp.activate()
    }

    private func closePanel() {
        if let window = NSApp.keyWindow, !MenuBarDockPolicy.isSettingsWindow(window) {
            window.close()
        }
    }
}

private struct MenuBarSectionsFrame: SwiftUI.PreferenceKey {
    static let defaultValue: CGRect = .zero
    static func reduce(value: inout CGRect, nextValue: () -> CGRect) { value = nextValue() }
}

private struct MenuBarMaterial: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = .menu
        view.blendingMode = .behindWindow
        view.state = .active
        return view
    }

    func updateNSView(_ nsView: NSVisualEffectView, context: Context) {}
}

// MARK: - Profile section

private struct MenuBarProfileSection: View {
    let profile: Profile
    let report: UsageReport?
    let source: UsageSource
    let selected: Bool

    private var secondary: Color { selected ? MenuBarColors.selectedText : .secondary }
    private var kinds: [UsageWindowKind] { UsageWindowKind.allCases.filter { $0.window(in: report) != nil } }
    private var resetCredits: String { ResetAvailability.text(credits: report?.resetCredits) }
    private var noUsage: String {
        if report != nil { return "No limits reported" }
        return source == .local ? "No usage saved yet" : "Not refreshed yet"
    }

    var body: some View {
        TimelineView(.everyMinute) { context in
            let updated = updatedText(now: context.date)
            VStack(alignment: .leading, spacing: 9) {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(alignment: .firstTextBaseline) {
                        Text(profile.name).font(.headline).lineLimit(1)
                        Spacer(minLength: 8)
                        Text(profile.provider.title).foregroundStyle(secondary)
                    }
                    Text(updated).foregroundStyle(secondary).monospacedDigit()
                }
                .font(.caption)
                if kinds.isEmpty {
                    Text(noUsage).font(.callout).foregroundStyle(secondary)
                }
                ForEach(kinds, id: \.self) { kind in
                    if let window = kind.window(in: report) {
                        MenuBarWindowRow(kind: kind, window: window, provider: profile.provider,
                                         now: context.date, selected: selected)
                    }
                }
                HStack(alignment: .firstTextBaseline) {
                    Text("Limit reset credits").fontWeight(.semibold)
                    Spacer(minLength: 8)
                    Text(resetCredits).foregroundStyle(secondary)
                }
                .font(.caption)
            }
            .foregroundStyle(selected ? MenuBarColors.selectedText : Color.primary)
            .padding(.horizontal, 12)
            .padding(.vertical, 9)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background {
                if selected { RoundedRectangle(cornerRadius: 6).fill(MenuBarColors.selection) }
            }
            .contentShape(Rectangle())
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(spokenSummary(updated: updatedText(now: context.date, spoken: true), now: context.date))
        }
    }

    private func updatedText(now: Date, spoken: Bool = false) -> String {
        let sourceTitle = (report?.source ?? source).title
        guard let captured = report?.capturedAt else { return sourceTitle }
        let age = now.timeIntervalSince(captured)
        if age < 60 { return "Updated just now" + (spoken ? ", " : " · ") + sourceTitle }
        let span = spoken ? UsageSpan.spoken(age) : UsageSpan.compact(age, roundingUp: false)
        return "Updated \(span) ago" + (spoken ? ", " : " · ") + sourceTitle
    }

    private func spokenSummary(updated: String, now: Date) -> String {
        var parts = [profile.name, profile.provider.title, updated]
        if kinds.isEmpty { parts.append(noUsage) }
        for kind in kinds {
            if let window = kind.window(in: report) {
                parts.append(MenuBarSummary.spokenRow(kind: kind, window: window, now: now))
            }
        }
        parts.append("Limit reset credits, \(resetCredits)")
        return parts.joined(separator: ", ")
    }
}

private struct MenuBarWindowRow: View {
    let kind: UsageWindowKind
    let window: UsageWindow
    let provider: Provider
    let now: Date
    let selected: Bool
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        let secondary = selected ? MenuBarColors.selectedText : Color.secondary
        let name = kind == .fiveHour ? "Session" : kind.title
        let tint = selected ? MenuBarColors.selectedText :
            MenuBarColors.provider(provider, scheme: colorScheme, critical: window.leftPercent < 10)
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline) {
                Text("\(name) \(Int(window.leftPercent.rounded()))% left")
                    .font(.callout.weight(.semibold))
                Spacer(minLength: 8)
                Text(ResetCountdown.text(resetsAt: window.resetsAt, now: now))
                    .font(.caption)
                    .foregroundStyle(secondary)
            }
            .monospacedDigit()
            MenuBarUsageBar(leftPercent: window.leftPercent,
                            pacePercent: UsagePace.expectedLeftPercent(resetsAt: window.resetsAt,
                                                                      length: kind.length, now: now),
                            tint: tint)
            if let pace = UsagePace.text(window, kind: kind, now: now) {
                Text(pace).font(.caption).foregroundStyle(secondary).monospacedDigit()
            }
        }
    }
}

private struct MenuBarUsageBar: View {
    let leftPercent: Double
    let pacePercent: Double?
    let tint: Color

    var body: some View {
        GeometryReader { proxy in
            let width = proxy.size.width
            ZStack(alignment: .leading) {
                Capsule().fill(tint.opacity(0.19))
                Capsule().fill(tint).frame(width: width * leftPercent / 100)
                if let pacePercent {
                    RoundedRectangle(cornerRadius: 1)
                        .fill(tint)
                        .overlay { RoundedRectangle(cornerRadius: 1).strokeBorder(Color.black, lineWidth: 0.75) }
                        .frame(width: 3, height: 10)
                        .offset(x: min(max(width * pacePercent / 100 - 1.5, 0), max(0, width - 3)))
                }
            }
        }
        .frame(height: 5)
        .padding(.vertical, 3)
    }
}

private enum MenuBarColors {
    static let selection = Color(nsColor: .selectedContentBackgroundColor)
    static let selectedText = Color(nsColor: .alternateSelectedControlTextColor)

    static func provider(_ provider: Provider, scheme: ColorScheme, critical: Bool = false) -> Color {
        let dark = scheme == .dark
        if critical {
            return dark ? Color(red: 1, green: 0.42, blue: 0.44) : Color(red: 0.72, green: 0.13, blue: 0.20)
        }
        switch provider {
        case .codex:
            return dark ? Color(red: 0.26, green: 0.79, blue: 0.75) : Color(red: 0.05, green: 0.43, blue: 0.41)
        case .claude:
            return dark ? Color(red: 0.98, green: 0.62, blue: 0.48) : Color(red: 0.63, green: 0.28, blue: 0.18)
        }
    }
}
