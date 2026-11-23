import SwiftUI
import AppKit
import LarpModel

// MARK: - Status item

/// One ring per provider: the session left, with the provider mark inside and the percent beside it.
struct MenuBarLabel: View {
    @ObservedObject var store: AppStore
    @AppStorage(MenuBarPreferences.modeKey) private var mode = MenuBarMode.tightest.rawValue
    @AppStorage(MenuBarPreferences.codexProfileKey) private var codexProfileID = ""
    @AppStorage(MenuBarPreferences.claudeProfileKey) private var claudeProfileID = ""

    typealias Ring = (provider: Provider, left: Double)

    var body: some View {
        let profiles = store.snapshot?.profiles ?? []
        let rings = Self.rings(store, codexProfileID: codexProfileID, claudeProfileID: claudeProfileID)
        Group {
            if mode != MenuBarMode.icon.rawValue, !rings.isEmpty, let image = Self.templateImage(rings) {
                Image(nsImage: image)
            } else {
                Image(systemName: MenuBarSummary.symbolName(leftPercent:
                    MenuBarSummary.tightestLeftPercent(profiles: profiles, usage: store.usage)))
            }
        }
        .accessibilityLabel(mode == MenuBarMode.icon.rawValue || rings.isEmpty ? "LarpCC" :
            (["LarpCC"] + rings.map { "\($0.provider.title) session, \(Int($0.left.rounded())) percent left" })
                .joined(separator: ", "))
        .onAppear { MenuBarDockPolicy.install() }
    }

    @MainActor static func rings(_ store: AppStore, codexProfileID: String, claudeProfileID: String) -> [Ring] {
        [Provider.codex, .claude].compactMap { provider in
            let choice = provider == .codex ? codexProfileID : claudeProfileID
            return MenuBarSummary.sessionLeftPercent(provider: provider, chosenProfileID: choice,
                                                     profiles: store.snapshot?.profiles ?? [],
                                                     usage: store.usage).map { (provider, $0) }
        }
    }

    @MainActor static func templateImage(_ rings: [Ring]) -> NSImage? {
        let content = HStack(spacing: 7) {
            ForEach(rings.indices, id: \.self) { index in
                HStack(spacing: 3) {
                    StatusRing(provider: rings[index].provider, left: rings[index].left)
                    Text("\(Int(rings[index].left.rounded()))%")
                        .font(.system(size: 12, weight: .medium).monospacedDigit())
                        .foregroundStyle(.black)
                }
            }
        }
        .frame(height: 16)
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

private struct StatusRing: View {
    let provider: Provider
    let left: Double

    var body: some View {
        ZStack {
            Circle().stroke(.black.opacity(0.28), lineWidth: 1.6)
            Circle()
                .trim(from: 0, to: left / 100)
                .stroke(.black, style: StrokeStyle(lineWidth: 1.6, lineCap: .round))
                .rotationEffect(.degrees(-90))
            ProviderMark(provider: provider).fill(.black).frame(width: 7.5, height: 7.5)
        }
        .frame(width: 15, height: 15)
    }
}


// MARK: - Panel

/// The panel window's corner radius on macOS 26 (measured from the menu bar window),
/// and the inset of the cards inside it. Cards are concentric: radius − inset.
private enum PanelMetrics {
    static let radius: CGFloat = 16
    static let inset: CGFloat = 8
    static let cardRadius = radius - inset
    /// Footer rows sit 4 pt inside their card.
    static let rowRadius = cardRadius - 4
}

struct MenuBarPanel: View {
    @ObservedObject var store: AppStore
    /// Offscreen render for the debug snapshot hook: no scrolling, no focus.
    var isSnapshot = false
    var snapshotExpandedID: String? = nil
    @Environment(\.openWindow) private var openWindow
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @AppStorage(UsagePreferences.sourceKey) private var storedSource = UsageSource.local.rawValue
    @AppStorage(UsagePreferences.consentKey) private var consented = false
    @FocusState private var hasKeyboardFocus: Bool
    @State private var keyboardItem: Item?
    @State private var hoveredItem: Item?
    @State private var expandedID: String?
    @State private var listHeight: CGFloat = 0
    @State private var panelHeight: CGFloat = 0
    @State private var recoverAfterAction = false

    private enum Item: Hashable {
        case profile(String), openDesktop(String), showProfile(String), refresh, settings, quit
    }

    private var source: UsageSource { storedSource == UsageSource.live.rawValue && consented ? .live : .local }
    private var allProfiles: [Profile] { store.snapshot?.profiles ?? [] }
    private var groups: [(provider: Provider, profiles: [Profile])] {
        MenuBarSummary.groups(allProfiles, usage: store.usage)
    }
    private var profiles: [Profile] { groups.flatMap(\.profiles) }
    private var expanded: String? { isSnapshot ? snapshotExpandedID : expandedID }
    private var canRefresh: Bool { !store.usageLoading && !store.busy }
    private func canOpenDesktop(_ profile: Profile) -> Bool {
        profile.desktopEnabled && !store.isFixture && !store.busy
    }
    private var items: [Item] {
        var items: [Item] = []
        for profile in profiles {
            items.append(.profile(profile.id))
            guard profile.id == expanded else { continue }
            if canOpenDesktop(profile) { items.append(.openDesktop(profile.id)) }
            items.append(.showProfile(profile.id))
        }
        if canRefresh { items.append(.refresh) }
        return items + [.settings, .quit]
    }
    /// Glass needs a live window; the offscreen snapshot and Reduce Transparency use opaque cards.
    private var glass: Bool { !isSnapshot && !reduceTransparency }
    private var highlighted: Item? {
        let candidate = hoveredItem ?? keyboardItem
        return candidate.flatMap { items.contains($0) ? $0 : nil }
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            notices
            VStack(spacing: PanelMetrics.inset) {
                list
                footer
            }
            .padding([.horizontal, .bottom], PanelMetrics.inset)
        }
        .frame(width: 360)
        // The menu bar window keeps the size of its first layout, so it is resized
        // to the panel's ideal height whenever the content changes (top edge fixed).
        .fixedSize(horizontal: false, vertical: true)
        .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { panelHeight = $0 }
        .frame(maxHeight: .infinity, alignment: .top)
        .background(panelBackground)
        .background {
            if !isSnapshot { MenuBarWindowSizer(height: panelHeight) }
        }
        .background {
            // ⌘R always lands here, even while Refresh is disabled, so it never
            // falls through to the Settings window's own ⌘R.
            Button("Refresh") { activate(.refresh) }
                .keyboardShortcut("r", modifiers: .command)
                .opacity(0)
                .accessibilityHidden(true)
        }
        .focusable(!isSnapshot)
        .focused($hasKeyboardFocus)
        .focusEffectDisabled()
        .defaultFocus($hasKeyboardFocus, true)
        .onKeyPress(.downArrow) { moveHighlight(by: 1) }
        .onKeyPress(.upArrow) { moveHighlight(by: -1) }
        .onKeyPress(.return, phases: .down) { _ in
            if let highlighted { activate(highlighted) }
            return .handled
        }
        .onKeyPress(.space, phases: .down) { _ in
            guard case let .profile(id)? = highlighted else { return .ignored }
            toggleDetails(for: id)
            return .handled
        }
        .onExitCommand(perform: closePanel)
        .onAppear {
            guard !isSnapshot else { return }
            // Like a menu, every opening starts with nothing highlighted or expanded.
            hoveredItem = nil
            keyboardItem = nil
            expandedID = nil
            hasKeyboardFocus = true
            store.menuBarPanelOpened()
        }
        .onChange(of: store.busy) { _, busy in
            guard !busy, recoverAfterAction else { return }
            recoverAfterAction = false
            // Recover the list once after a failed launch clears the snapshot.
            if store.snapshot == nil { store.startRefresh() }
        }
    }

    // MARK: Header

    private var header: some View {
        HStack(alignment: .center, spacing: 8) {
            VStack(alignment: .leading, spacing: 1) {
                Text("LarpCC").font(.system(size: 13, weight: .bold))
                Text(statusLine).foregroundStyle(.secondary)
            }
            Spacer(minLength: 8)
            if store.usageLoading || store.busy {
                ProgressView().controlSize(.small).accessibilityLabel("Updating")
            }
        }
        .font(.system(size: 13))
        .padding(.horizontal, 20)
        .padding(.top, 14)
        .padding(.bottom, 10)
        .accessibilityElement(children: .combine)
    }

    private var statusLine: String {
        let count = profiles.count
        var parts = [count == 0 ? "No profiles on" : count == 1 ? "1 profile" : "\(count) profiles"]
        parts.append(source == .live ? "Live usage" : "Local usage")
        if store.isFixture { parts.append("Sample data") }
        return parts.joined(separator: " · ")
    }

    @ViewBuilder
    private var notices: some View {
        if let error = store.usageError {
            notice(error, symbol: "exclamationmark.triangle.fill")
        }
        if let message = store.message, !isSnapshot {
            notice(message, symbol: "info.circle", dismiss: { store.message = nil })
        }
    }

    private func notice(_ text: String, symbol: String, dismiss: (() -> Void)? = nil) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Image(systemName: symbol).foregroundStyle(.secondary).accessibilityHidden(true)
            Text(text)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            if let dismiss {
                Button(action: dismiss) { Image(systemName: "xmark") }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("Dismiss message")
            }
        }
        .font(.system(size: 11))
        .padding(.horizontal, 20)
        .padding(.bottom, 10)
    }

    // MARK: Profiles

    /// Fits its content; scrolls only past ~80% of the screen. The list is laid out
    /// at its natural height first, so it can never collapse to a sliver.
    @ViewBuilder
    private var list: some View {
        let content = profileList
            .fixedSize(horizontal: false, vertical: true)
            .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { listHeight = $0 }
        if isSnapshot || listHeight <= maxListHeight {
            content
        } else {
            ScrollViewReader { proxy in
                ScrollView { content }
                    .scrollBounceBehavior(.basedOnSize)
                    .frame(height: maxListHeight)
                    .clipShape(RoundedRectangle(cornerRadius: PanelMetrics.cardRadius, style: .continuous))
                    .onChange(of: keyboardItem) { _, item in
                        if let item { withAnimation(.easeOut(duration: 0.15)) { proxy.scrollTo(item) } }
                    }
            }
        }
    }

    private var maxListHeight: CGFloat {
        let pointerScreen = NSScreen.screens.first { $0.frame.contains(NSEvent.mouseLocation) }
        let screen = NSApp.keyWindow?.screen ?? pointerScreen ?? NSScreen.main
        // 80% of the screen, less the header and the footer card.
        return max(160, (screen?.visibleFrame.height ?? 800) * 0.8 - 160)
    }

    /// One card per profile, grouped under provider labels.
    private var profileList: some View {
        TimelineView(.everyMinute) { context in
            VStack(alignment: .leading, spacing: PanelMetrics.inset) {
                if groups.isEmpty {
                    Text(emptyMessage)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(12)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .panelCard(highlighted: false, glass: glass)
                }
                ForEach(groups, id: \.provider) { group in
                    Text(group.provider.title)
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 12)
                        .padding(.top, 2)
                        .padding(.bottom, -2)
                        .accessibilityAddTraits(.isHeader)
                    ForEach(group.profiles) { profile in
                        profileCard(profile, now: context.date)
                    }
                }
            }
            .font(.system(size: 13))
        }
    }

    private var emptyMessage: String {
        if store.busy || store.snapshot == nil && store.isFixture { return "Loading profiles…" }
        if store.snapshot == nil { return "Profiles are unavailable. Open Settings to choose larp or use sample data." }
        return "No profiles are switched on. Turn one on in Settings."
    }

    private func profileCard(_ profile: Profile, now: Date) -> some View {
        let item = Item.profile(profile.id)
        let isExpanded = expanded == profile.id
        let report = store.usage[profile.id]
        return VStack(alignment: .leading, spacing: 0) {
            Button { toggleDetails(for: profile.id) } label: {
                MenuBarProfileSummary(profile: profile, report: report, source: source, now: now,
                                      expanded: isExpanded)
            }
            .buttonStyle(.plain)
            .onHover { hover(item, entered: $0) }
            .accessibilityHint(isExpanded ? "Hides details" : "Shows details")
            .accessibilityAction(named: Text("Show in Settings")) {
                activate(.showProfile(profile.id))
            }
            .id(item)
            if isExpanded {
                MenuBarProfileDetail(report: report, now: now) {
                    if profile.desktopEnabled {
                        detailButton(.openDesktop(profile.id), title: "Open \(profile.provider.desktopApp)",
                                     enabled: canOpenDesktop(profile))
                    }
                    detailButton(.showProfile(profile.id), title: "Show in Settings", enabled: true)
                }
            }
        }
        .panelCard(highlighted: highlighted == item, glass: glass)
    }

    private func detailButton(_ item: Item, title: String, enabled: Bool) -> some View {
        Button { activate(item) } label: {
            Text(title)
                .font(.system(size: 12))
                .padding(.horizontal, 11)
                .frame(height: 24)
                .background {
                    Capsule().fill(Color.primary.opacity(highlighted == item ? 0.16 : 0.08))
                }
                .contentShape(Capsule())
                .opacity(enabled ? 1 : 0.45)
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .onHover { hover(item, entered: $0) }
        .id(item)
    }

    // MARK: Footer

    private var footer: some View {
        VStack(spacing: 0) {
            menuRow(.refresh, title: store.usageLoading ? "Updating Usage…" : "Refresh", shortcut: "⌘R",
                    enabled: canRefresh)
            menuRow(.settings, title: "LarpCC Settings…", shortcut: "⌘,")
                .keyboardShortcut(",", modifiers: .command)
            menuRow(.quit, title: "Quit LarpCC", shortcut: "⌘Q")
                .keyboardShortcut("q", modifiers: .command)
        }
        .padding(4)
        .panelCard(highlighted: false, glass: glass)
    }

    private func menuRow(_ item: Item, title: String, shortcut: String, enabled: Bool = true) -> some View {
        Button { activate(item) } label: {
            HStack {
                Text(title)
                Spacer()
                Text(shortcut).foregroundStyle(.secondary).accessibilityHidden(true)
            }
            .font(.system(size: 13))
            .padding(.horizontal, 8)
            .frame(height: 26)
            .opacity(enabled ? 1 : 0.45)
            .background {
                RoundedRectangle(cornerRadius: PanelMetrics.rowRadius, style: .continuous)
                    .fill(Color.primary.opacity(highlighted == item ? 0.08 : 0))
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .onHover { hover(item, entered: $0) }
    }

    // MARK: Actions

    private func hover(_ item: Item, entered: Bool) {
        guard !isSnapshot else { return }
        if entered {
            guard items.contains(item) else { return }
            hoveredItem = item
            keyboardItem = item
        } else if hoveredItem == item {
            hoveredItem = nil
            keyboardItem = nil
        }
    }

    private func moveHighlight(by step: Int) -> KeyPress.Result {
        let items = items
        let index = highlighted.flatMap { items.firstIndex(of: $0) } ?? (step > 0 ? -1 : items.count)
        hoveredItem = nil
        keyboardItem = items[min(max(index + step, 0), items.count - 1)]
        return .handled
    }

    private func activate(_ item: Item) {
        hasKeyboardFocus = true
        switch item {
        case let .profile(id), let .showProfile(id):
            openSettings(profileID: id)
        case let .openDesktop(id):
            guard let profile = profiles.first(where: { $0.id == id }), canOpenDesktop(profile) else { return }
            recoverAfterAction = true
            store.perform(.open(profileID: id, surface: .desktop))
        case .refresh: if canRefresh { store.menuBarRefresh() }
        case .settings: openSettings(profileID: nil)
        case .quit: NSApp.terminate(nil)
        }
    }

    private func toggleDetails(for id: String) {
        hasKeyboardFocus = true
        expandedID = expandedID == id ? nil : id
    }

    private func openSettings(profileID: String?) {
        closePanel()
        MenuBarDockPolicy.showDockIcon()
        store.requestedProfileID = profileID
        openWindow(id: "settings")
        NSApp.activate()
    }

    /// Dismissing through SwiftUI keeps the menu bar item's open state in step, so the
    /// next click on the item opens the panel again (closing the window directly did not).
    private func closePanel() {
        guard !isSnapshot else { return }
        dismiss()
    }
}

private struct MenuBarWindowSizer: NSViewRepresentable {
    let height: CGFloat

    func makeNSView(context: Context) -> NSView { NSView() }

    func updateNSView(_ view: NSView, context: Context) {
        let height = height
        DispatchQueue.main.async {
            guard height > 0, let window = view.window, abs(window.frame.height - height) > 0.5 else { return }
            var frame = window.frame
            frame.origin.y += frame.height - height
            frame.size.height = height
            window.setFrame(frame, display: true)
        }
    }
}

// MARK: - Cards

private extension View {
    /// A floating card inset from the panel edge, like the Settings sidebar: Liquid Glass on
    /// macOS 26, an opaque fill with a hairline otherwise. Corners are concentric with the panel.
    func panelCard(highlighted: Bool, glass: Bool) -> some View {
        modifier(PanelCard(highlighted: highlighted, glass: glass))
    }
}

private struct PanelCard: ViewModifier {
    let highlighted: Bool
    let glass: Bool
    @Environment(\.colorSchemeContrast) private var contrast

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: PanelMetrics.cardRadius, style: .continuous)
        let card = content
            .background(shape.fill(cardLift))
            .background(shape.fill(Color.primary.opacity(highlighted ? 0.07 : 0)))
            .overlay {
                if contrast == .increased {
                    shape.strokeBorder(Color.primary.opacity(0.5), lineWidth: 1)
                }
            }
        if #available(macOS 26, *), glass {
            card.glassEffect(.regular, in: shape)
        } else {
            card.overlay(shape.strokeBorder(Color.primary.opacity(0.1), lineWidth: 0.5))
        }
    }
}

private func isDark(_ appearance: NSAppearance) -> Bool {
    let match = appearance.bestMatch(from: [.aqua, .darkAqua, .accessibilityHighContrastAqua,
                                            .accessibilityHighContrastDarkAqua])
    return match == .darkAqua || match == .accessibilityHighContrastDarkAqua
}

/// Behind the cards: the dark window background, or a soft grey in light mode
/// (the light window background is nearly white and would swallow the cards).
private let panelBackground = Color(nsColor: NSColor(name: nil) { appearance in
    isDark(appearance) ? .windowBackgroundColor : NSColor(white: 0.925, alpha: 1)
})

/// Cards sit a step lighter than the panel in both appearances, like the Settings sidebar.
private let cardLift = Color(nsColor: NSColor(name: nil) { appearance in
    NSColor.white.withAlphaComponent(isDark(appearance) ? 0.08 : 0.8)
})

// MARK: - Profile card

/// The always-visible part of a card: ring, name, freshness and one meter line per usage window.
private struct MenuBarProfileSummary: View {
    let profile: Profile
    let report: UsageReport?
    let source: UsageSource
    let now: Date
    let expanded: Bool

    private var kinds: [UsageWindowKind] { UsageWindowKind.allCases.filter { $0.window(in: report) != nil } }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 10) {
                ProfileRing(provider: profile.provider, session: report?.fiveHour)
                VStack(alignment: .leading, spacing: 1) {
                    Text(profile.name).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                    Text(subtitle).font(.system(size: 11)).foregroundStyle(.secondary).monospacedDigit()
                }
                Spacer(minLength: 8)
                Image(systemName: "chevron.down")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(.tertiary)
                    .rotationEffect(.degrees(expanded ? 180 : 0))
                    .accessibilityHidden(true)
            }
            if !kinds.isEmpty {
                Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 5) {
                    ForEach(kinds, id: \.self) { kind in
                        if let window = kind.window(in: report) {
                            meterRow(kind, window)
                        }
                    }
                }
                .font(.system(size: 11))
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(spokenSummary)
        .accessibilityAddTraits(.isButton)
    }

    private var subtitle: String {
        guard let report else { return source == .local ? "No usage saved yet" : "Not refreshed yet" }
        if kinds.isEmpty { return "No limits reported" }
        let sourceTitle = report.source.title
        guard let captured = report.capturedAt else { return sourceTitle }
        let age = now.timeIntervalSince(captured)
        if age < 60 { return "Updated just now · \(sourceTitle)" }
        return "Updated \(UsageSpan.compact(age, roundingUp: false)) ago · \(sourceTitle)"
    }

    private func meterRow(_ kind: UsageWindowKind, _ window: UsageWindow) -> some View {
        let level = UsageLevel(leftPercent: window.leftPercent)
        return GridRow {
            Text(kind.title).foregroundStyle(.secondary)
            UsageMeter(leftPercent: window.leftPercent, level: level)
            Text("\(Int(window.leftPercent.rounded()))%")
                .fontWeight(.semibold)
                .foregroundStyle(level.tint ?? .primary)
                .frame(width: 32, alignment: .trailing)
            // Fixed widths keep every card's meter the same length.
            Label(resetText(window), systemImage: "arrow.clockwise")
                .labelStyle(ResetLabelStyle())
                .foregroundStyle(.secondary)
                .frame(width: 58, alignment: .trailing)
        }
        .monospacedDigit()
    }

    private func resetText(_ window: UsageWindow) -> String {
        guard let resetsAt = window.resetsAt else { return "—" }
        let left = resetsAt.timeIntervalSince(now)
        return left > 0 ? UsageSpan.compact(left) : "now"
    }

    private var spokenSummary: String {
        var parts = [profile.name, profile.provider.title, subtitle]
        for kind in kinds {
            if let window = kind.window(in: report) {
                parts.append(MenuBarSummary.spokenRow(kind: kind, window: window, now: now))
            }
        }
        return parts.joined(separator: ", ")
    }
}

/// Shown inside a card after a click or Space: pace, reset credits and actions.
private struct MenuBarProfileDetail<Actions: View>: View {
    let report: UsageReport?
    let now: Date
    @ViewBuilder let actions: () -> Actions

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Divider()
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 10, verticalSpacing: 4) {
                ForEach(UsageWindowKind.allCases, id: \.self) { kind in
                    if let window = kind.window(in: report), let pace = UsagePace.text(window, kind: kind, now: now) {
                        line("\(kind.title) pace", pace)
                    }
                }
                line("Limit resets", ResetAvailability.text(credits: report?.resetCredits))
            }
            .font(.system(size: 11))
            HStack(spacing: 6) { actions() }
        }
        .padding([.horizontal, .bottom], 12)
    }

    private func line(_ label: String, _ value: String) -> some View {
        GridRow {
            Text(label).foregroundStyle(.secondary)
            Text(value).monospacedDigit().fixedSize(horizontal: false, vertical: true)
        }
        .accessibilityElement(children: .combine)
    }
}

/// The card's mark inside a ring of the session left, echoing the status item.
private struct ProfileRing: View {
    let provider: Provider
    let session: UsageWindow?

    var body: some View {
        let left = session?.leftPercent ?? 0
        ZStack {
            Circle().fill(Color.primary.opacity(0.07))
            Circle().strokeBorder(Color.primary.opacity(0.1), lineWidth: 2)
            if session != nil {
                Circle()
                    .inset(by: 1)
                    .trim(from: 0, to: left / 100)
                    .stroke(UsageLevel(leftPercent: left).tint ?? Color.primary.opacity(0.7),
                            style: StrokeStyle(lineWidth: 2, lineCap: .round))
                    .rotationEffect(.degrees(-90))
            }
            // Explicit colour: glass vibrancy washes out thin shapes drawn in .primary.
            ProviderMark(provider: provider).fill(Color(nsColor: .labelColor)).frame(width: 13, height: 13)
        }
        .frame(width: 30, height: 30)
        .accessibilityHidden(true)
    }
}

private struct ResetLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 2) {
            configuration.icon.font(.system(size: 8, weight: .semibold)).accessibilityHidden(true)
            configuration.title
        }
    }
}

private struct UsageMeter: View {
    let leftPercent: Double
    let level: UsageLevel

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .leading) {
                Capsule().fill(Color.primary.opacity(0.12))
                Capsule()
                    .fill(level.tint ?? Color.primary.opacity(0.7))
                    .frame(width: max(leftPercent > 0 ? 4 : 0, proxy.size.width * leftPercent / 100))
            }
        }
        .frame(height: 4)
        .frame(maxWidth: .infinity)
        .accessibilityHidden(true)
    }
}

private extension UsageLevel {
    /// Calm by default; colour only when a limit is running low.
    var tint: Color? {
        switch self {
        case .normal: return nil
        case .low: return .orange
        case .critical: return .red
        }
    }
}
