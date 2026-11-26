import SwiftUI
import AppKit
import LarpModel

// MARK: - Status item

/// One ring per provider: the session left, with the provider mark inside and the percent beside it.
struct MenuBarLabel: View {
    @ObservedObject var store: AppStore
    @Environment(\.theme) private var theme
    @AppStorage(MenuBarPreferences.modeKey) private var mode = MenuBarMode.tightest.rawValue
    @AppStorage(MenuBarPreferences.codexProfileKey) private var codexProfileID = ""
    @AppStorage(MenuBarPreferences.claudeProfileKey) private var claudeProfileID = ""

    typealias Ring = (provider: Provider, left: Double)

    var body: some View {
        let profiles = store.snapshot?.profiles ?? []
        let rings = Self.rings(store, codexProfileID: codexProfileID, claudeProfileID: claudeProfileID)
        Group {
            if mode != MenuBarMode.icon.rawValue, !rings.isEmpty, let image = Self.templateImage(rings, weight: theme.tokens.statusWeight) {
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
        #if DEBUG
        .onChange(of: theme, initial: true) { _, theme in
            MenuBarSnapshot.recordLive("status", theme: theme) {
                Self.templateImage(Self.rings(store, codexProfileID: codexProfileID, claudeProfileID: claudeProfileID),
                                   weight: theme.tokens.statusWeight)
            }
        }
        #endif
    }

    @MainActor static func rings(_ store: AppStore, codexProfileID: String, claudeProfileID: String) -> [Ring] {
        [Provider.codex, .claude].compactMap { provider in
            let choice = provider == .codex ? codexProfileID : claudeProfileID
            return MenuBarSummary.sessionLeftPercent(provider: provider, chosenProfileID: choice,
                                                     profiles: store.snapshot?.profiles ?? [],
                                                     usage: store.usage).map { (provider, $0) }
        }
    }

    @MainActor static func templateImage(_ rings: [Ring], weight: Font.Weight) -> NSImage? {
        let content = HStack(spacing: 7) {
            ForEach(rings.indices, id: \.self) { index in
                HStack(spacing: 3) {
                    StatusRing(provider: rings[index].provider, left: rings[index].left)
                    Text("\(Int(rings[index].left.rounded()))%")
                        .font(.system(size: 12, weight: weight).monospacedDigit())
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
            // Inset by half the stroke so the ring is not clipped at the image edge.
            Circle().inset(by: 0.8).stroke(.black.opacity(0.28), lineWidth: 1.6)
            Circle()
                .inset(by: 0.8)
                .trim(from: 0, to: left / 100)
                .stroke(.black, style: StrokeStyle(lineWidth: 1.6, lineCap: .round))
                .rotationEffect(.degrees(-90))
            ProviderMark(provider: provider).fill(.black).frame(width: 7.5, height: 7.5)
        }
        .frame(width: 15, height: 15)
    }
}


// MARK: - Panel

/// The panel follows the theme's Settings window: profile cards are drawn like its grouped
/// form sections (`sectionBox`), and the footer like menu items.
private enum PanelMetrics {
    static let inset: CGFloat = 8
    /// Menu items sit 5 pt inside the panel.
    static let menuInset: CGFloat = 5
}

struct MenuBarPanel: View {
    @ObservedObject var store: AppStore
    /// Offscreen render for the debug snapshot hook: no scrolling, no focus.
    var isSnapshot = false
    var snapshotExpandedID: String? = nil
    var snapshotSelectedID: String? = nil
    @Environment(\.openWindow) private var openWindow
    @Environment(\.dismiss) private var dismiss
    @Environment(\.theme) private var theme
    @AppStorage(UsagePreferences.sourceKey) private var storedSource = UsageSource.local.rawValue
    @AppStorage(UsagePreferences.consentKey) private var consented = false
    @FocusState private var hasKeyboardFocus: Bool
    /// One selection for mouse and keyboard. Leaving a row keeps it, so Return still acts on it.
    @State private var selection: Item?
    @State private var expandedID: String?
    @State private var listHeight: CGFloat = 0
    @State private var headerHeight: CGFloat = 0
    @State private var footerHeight: CGFloat = 0
    @State private var panelHeight: CGFloat = 0
    /// The visible height of the screen the panel window is on, once the window exists.
    @State private var screenHeight: CGFloat?
    /// The panel's own fixed feedback. Shared notices (`store.message`) stay in Settings.
    @State private var launchFeedback: String?
    @State private var recoverAfterAction = false

    private enum Item: Hashable {
        case profile(String), openDesktop(String), showProfile(String), refresh, settings, quit
    }

    private var source: UsageSource {
        UsagePreferences.effectiveSource(stored: storedSource, consented: consented)
    }
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
    private var tokens: ThemeTokens { theme.tokens }
    private var highlighted: Item? {
        let item = isSnapshot ? snapshotSelectedID.map(Item.profile) : selection
        return item.flatMap { items.contains($0) ? $0 : nil }
    }

    var body: some View {
        PanelStack(maxHeight: stackLimit) {
            VStack(spacing: 0) {
                header
                notices
            }
            .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { headerHeight = $0 }
            .frame(minHeight: 0, maxHeight: .infinity, alignment: .top)
            .clipped()
            list.padding(.horizontal, PanelMetrics.inset)
            footer
                .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { footerHeight = $0 }
        }
        .frame(width: 360)
        // The menu bar window keeps the size of its first layout, so it is resized
        // to the panel's ideal height whenever the content changes (top edge fixed).
        .fixedSize(horizontal: false, vertical: true)
        .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { panelHeight = $0 }
        .frame(maxHeight: .infinity, alignment: .top)
        .background {
            // The Settings page's own translucent material, so the wallpaper shows through the
            // panel the same way (opaque with Reduce Transparency). Snapshots use a sampled colour.
            if isSnapshot {
                PanelSnapshotBackground(theme: theme)
            } else {
                MenuBarMaterial(material: tokens.panelMaterial).accessibilityHidden(true)
            }
        }
        .background {
            if !isSnapshot {
                // PanelStack already holds the panel to the screen limit, except for the footer's
                // own minimum; the window follows the measured height so the footer is never cut.
                MenuBarWindowSizer(height: panelHeight) { if screenHeight != $0 { screenHeight = $0 } }
            }
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
        #if DEBUG
        .onChange(of: theme, initial: true) { _, theme in
            guard !isSnapshot else { return }
            MenuBarSnapshot.recordLive("panel", theme: theme) {
                let renderer = ImageRenderer(content: MenuBarPanel(store: store, isSnapshot: true)
                    .environment(\.theme, theme))
                renderer.scale = 2
                return renderer.nsImage
            }
        }
        #endif
        .onAppear {
            guard !isSnapshot else { return }
            // Like a menu, every opening starts with nothing selected or expanded.
            selection = nil
            expandedID = nil
            launchFeedback = nil
            hasKeyboardFocus = true
            store.menuBarPanelOpened()
        }
        // A selection or an open card whose row has gone (list emptied, profile switched off) is dropped.
        .onChange(of: items) { _, items in
            if let selection, !items.contains(selection) { self.selection = nil }
        }
        .onChange(of: profiles.map(\.id)) { _, ids in
            if let expandedID, !ids.contains(expandedID) { self.expandedID = nil }
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
                Text(statusLine).foregroundStyle(SettingsTypography.supportingInk)
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
        if let feedback = isSnapshot ? snapshotNotice : launchFeedback {
            notice(feedback, symbol: "exclamationmark.triangle.fill", dismiss: { self.launchFeedback = nil })
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

    /// The cards at their natural height (never a sliver). `PanelStack` gives them what the
    /// header and footer leave; they are clipped to it until they are measured, then scroll.
    @ViewBuilder
    private var list: some View {
        let content = profileList
            .fixedSize(horizontal: false, vertical: true)
            .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { listHeight = $0 }
        if isSnapshot || listHeight <= maxListHeight {
            content.frame(minHeight: 0, maxHeight: .infinity, alignment: .top).clipped()
        } else {
            ScrollViewReader { proxy in
                ScrollView { content }
                    .scrollBounceBehavior(.basedOnSize)
                    .frame(idealHeight: listHeight)
                    .clipShape(RoundedRectangle(cornerRadius: tokens.sectionRadius, style: .continuous))
                    .onChange(of: selection) { _, item in
                        if let item { withAnimation(.easeOut(duration: 0.15)) { proxy.scrollTo(item) } }
                    }
            }
        }
    }

    /// 80% of the visible height of the panel's screen: the window's own screen once it
    /// exists, before that the screen under the pointer that opened it.
    private var maxPanelHeight: CGFloat {
        #if DEBUG
        if let height = MenuBarSnapshot.screenHeight { return height * 0.8 }
        #endif
        let pointerScreen = NSScreen.screens.first { $0.frame.contains(NSEvent.mouseLocation) }
        return (screenHeight ?? pointerScreen?.visibleFrame.height ?? NSScreen.main?.visibleFrame.height ?? 800) * 0.8
    }

    /// The panel's height limit. The offscreen snapshot has no screen and draws everything,
    /// unless a debug screen height asks it to check overflow.
    private var stackLimit: CGFloat {
        #if DEBUG
        if isSnapshot && MenuBarSnapshot.screenHeight == nil { return .infinity }
        #endif
        return maxPanelHeight
    }

    private var snapshotNotice: String? {
        #if DEBUG
        return MenuBarSnapshot.notice
        #else
        return nil
        #endif
    }

    /// What is left for the cards once the measured header, notices and footer are placed.
    private var maxListHeight: CGFloat {
        max(0, maxPanelHeight - headerHeight - footerHeight)
    }

    /// One card per profile, grouped under provider labels.
    private var profileList: some View {
        TimelineView(.everyMinute) { context in
            VStack(alignment: .leading, spacing: PanelMetrics.inset) {
                if groups.isEmpty {
                    Text(emptyMessage)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 8)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .panelCard(highlighted: false)
                }
                ForEach(groups, id: \.provider) { group in
                    // Section titles like the Settings form's ("System", "Usage").
                    Text(group.provider.title)
                        .font(.headline)
                        .padding(.horizontal, 10)
                        .padding(.top, 6)
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
                MenuBarProfileDetail(report: report) {
                    if profile.desktopEnabled {
                        detailButton(.openDesktop(profile.id), title: "Open \(profile.provider.desktopApp)",
                                     enabled: canOpenDesktop(profile))
                    }
                    detailButton(.showProfile(profile.id), title: "Show in Settings", enabled: true)
                }
            }
        }
        .panelCard(highlighted: highlighted == item)
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

    /// Plain menu items after a separator, as in any menu.
    private var footer: some View {
        VStack(spacing: 0) {
            Divider().padding(.horizontal, 10).padding(.top, PanelMetrics.inset).padding(.bottom, 5)
            menuRow(.refresh, title: store.usageLoading ? "Updating Usage…" : "Refresh", shortcut: "⌘R",
                    enabled: canRefresh)
            menuRow(.settings, title: "LarpCC Settings…", shortcut: "⌘,")
                .keyboardShortcut(",", modifiers: .command)
            menuRow(.quit, title: "Quit LarpCC", shortcut: "⌘Q")
                .keyboardShortcut("q", modifiers: .command)
        }
        .padding([.horizontal, .bottom], PanelMetrics.menuInset)
    }

    private func menuRow(_ item: Item, title: String, shortcut: String, enabled: Bool = true) -> some View {
        // The system menu highlight: the accent colour with light text.
        let selected = highlighted == item
        let text = Color(nsColor: .alternateSelectedControlTextColor)
        return Button { activate(item) } label: {
            HStack {
                Text(title)
                Spacer()
                Text(shortcut)
                    .foregroundStyle(selected ? AnyShapeStyle(text) : AnyShapeStyle(.secondary))
                    .accessibilityHidden(true)
            }
            .font(.system(size: 13))
            .foregroundStyle(selected ? text : .primary)
            .padding(.horizontal, 9)
            .frame(height: tokens.menuRowHeight)
            .opacity(enabled ? 1 : 0.45)
            .background {
                RoundedRectangle(cornerRadius: tokens.menuRowRadius, style: .continuous)
                    .fill(selected ? Color(nsColor: .selectedContentBackgroundColor) : .clear)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .onHover { hover(item, entered: $0) }
    }

    // MARK: Actions

    private func hover(_ item: Item, entered: Bool) {
        guard !isSnapshot, entered, items.contains(item) else { return }
        selection = item
    }

    private func moveHighlight(by step: Int) -> KeyPress.Result {
        let items = items
        let index = highlighted.flatMap { items.firstIndex(of: $0) } ?? (step > 0 ? -1 : items.count)
        selection = items[min(max(index + step, 0), items.count - 1)]
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
            launchFeedback = nil
            store.perform(.open(profileID: id, surface: .desktop)) { error in
                if error != nil { launchFeedback = "Couldn’t open \(profile.provider.desktopApp)." }
            }
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

/// Header, cards and footer in a column no taller than `maxHeight`, worked out in a single
/// layout pass. The footer always gets its full height, the header comes next, and the cards
/// get whatever is left.
private struct PanelStack: Layout {
    let maxHeight: CGFloat

    private func heights(width: CGFloat, subviews: Subviews) -> [CGFloat] {
        let ideal = subviews.map { $0.sizeThatFits(ProposedViewSize(width: width, height: nil)).height }
        let footer = ideal[2]
        let header = min(ideal[0], max(0, maxHeight - footer))
        let cards = min(ideal[1], max(0, maxHeight - footer - header))
        return [header, cards, footer]
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 360
        return CGSize(width: width, height: heights(width: width, subviews: subviews).reduce(0, +))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for (subview, height) in zip(subviews, heights(width: bounds.width, subviews: subviews)) {
            subview.place(at: CGPoint(x: bounds.minX, y: y), anchor: .topLeading,
                          proposal: ProposedViewSize(width: bounds.width, height: height))
            y += height
        }
    }
}

private struct MenuBarWindowSizer: NSViewRepresentable {
    let height: CGFloat
    /// Reports the visible height of the window's own screen.
    let onScreen: (CGFloat) -> Void

    func makeNSView(context: Context) -> NSView { NSView() }

    func updateNSView(_ view: NSView, context: Context) {
        let height = height, onScreen = onScreen
        DispatchQueue.main.async {
            guard let window = view.window else { return }
            if let screen = window.screen { onScreen(screen.visibleFrame.height) }
            guard height > 0, abs(window.frame.height - height) > 0.5 else { return }
            var frame = window.frame
            frame.origin.y += frame.height - height
            frame.size.height = height
            window.setFrame(frame, display: true)
        }
    }
}

// MARK: - Cards

private extension View {
    /// A profile card drawn like the theme's Settings form section (`sectionBox`).
    /// The selection is an outline, as Settings groups never tint.
    func panelCard(highlighted: Bool) -> some View {
        modifier(PanelCard(highlighted: highlighted))
    }
}

private struct PanelCard: ViewModifier {
    let highlighted: Bool
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.theme) private var theme

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: theme.tokens.sectionRadius, style: .continuous)
        let strong = contrast == .increased
        content
            .sectionBox(theme.tokens)
            .overlay {
                if highlighted {
                    shape.strokeBorder(Color.primary.opacity(strong ? 0.85 : 0.35), lineWidth: strong ? 2 : 1)
                } else if strong {
                    shape.strokeBorder(Color.primary.opacity(0.5), lineWidth: 1)
                }
            }
    }
}

/// Rows inside a card, split by separators inset like the Settings form's.
private struct CardRow<Content: View>: View {
    @ViewBuilder let content: () -> Content

    var body: some View {
        content()
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct CardSeparator: View {
    var body: some View { Divider().padding(.horizontal, 10) }
}

private struct MenuBarMaterial: NSViewRepresentable {
    let material: NSVisualEffectView.Material

    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = material
        view.blendingMode = .behindWindow
        view.state = .active
        return view
    }

    func updateNSView(_ nsView: NSVisualEffectView, context: Context) { nsView.material = material }
}

private func isDark(_ appearance: NSAppearance) -> Bool {
    let match = appearance.bestMatch(from: [.aqua, .darkAqua, .accessibilityHighContrastAqua,
                                            .accessibilityHighContrastDarkAqua])
    return match == .darkAqua || match == .accessibilityHighContrastDarkAqua
}

/// Offscreen snapshots cannot draw the material; they use its colour sampled over a desktop
/// on macOS 26: the Settings page (Tahoe) or the sidebar (Sequoia, Sonoma).
private struct PanelSnapshotBackground: View {
    let theme: Theme

    var body: some View {
        let tahoe = theme == .tahoe
        Color(nsColor: NSColor(name: nil) { appearance in
            switch (isDark(appearance), tahoe) {
            case (true, true): return NSColor(srgbRed: 0.106, green: 0.129, blue: 0.137, alpha: 1)
            case (false, true): return .white
            case (true, false): return NSColor(srgbRed: 0.157, green: 0.2, blue: 0.204, alpha: 1)
            case (false, false): return NSColor(srgbRed: 0.882, green: 0.906, blue: 0.91, alpha: 1)
            }
        })
    }
}

// MARK: - Profile card

/// The always-visible part of a card: ring, name, freshness and one meter line per usage window.
private struct MenuBarProfileSummary: View {
    let profile: Profile
    let report: UsageReport?
    let source: UsageSource
    let now: Date
    let expanded: Bool
    @Environment(\.theme) private var theme

    private var windows: [(title: String, window: UsageWindow)] {
        let primary: [(String, UsageWindow)] = UsageWindowKind.allCases.compactMap { kind in
            kind.window(in: report).map { (kind.title, $0) }
        }
        let extra = report?.extra ?? []
        return primary + extra.prefix(expanded ? extra.count : 2).map { ($0.title(source: report?.source), $0.window) }
    }

    var body: some View {
        // Rows like a Settings form section: title and description, then the meters, then credits.
        VStack(alignment: .leading, spacing: 0) {
            CardRow {
                HStack(spacing: 10) {
                    ProfileRing(provider: profile.provider, session: report?.fiveHour)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(profile.name).font(.system(size: 13)).lineLimit(1)
                        Text(subtitle).font(.system(size: theme.tokens.descriptionSize))
                            .foregroundStyle(SettingsTypography.supportingInk)
                            .monospacedDigit()
                    }
                    Spacer(minLength: 8)
                    if let label = report?.planLabel {
                        Text(label)
                            .font(.system(size: theme.tokens.descriptionSize))
                            .foregroundStyle(SettingsTypography.supportingInk)
                            .lineLimit(1)
                    }
                    Image(systemName: "chevron.down")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(.tertiary)
                        .rotationEffect(.degrees(expanded ? 180 : 0))
                        .accessibilityHidden(true)
                }
            }
            let windows = self.windows
            if !windows.isEmpty {
                CardSeparator()
                CardRow {
                    Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 6) {
                        ForEach(windows.indices, id: \.self) { index in
                            meterRow(windows[index].title, windows[index].window)
                        }
                    }
                    .font(.system(size: 11))
                }
            }
            CardSeparator()
            // Reset credits always show, whatever else the card has.
            CardRow {
                ResetCreditLabel(credits: report?.resetCredits)
                    .font(.system(size: 11))
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(spokenSummary)
        .accessibilityAddTraits(.isButton)
    }

    private var subtitle: String {
        guard let report else { return source == .local ? "No usage saved yet" : "Not refreshed yet" }
        if windows.isEmpty { return "No limits reported" }
        let sourceTitle = report.source.title
        guard let captured = report.capturedAt else { return sourceTitle }
        let age = now.timeIntervalSince(captured)
        if age < 60 { return "Updated just now · \(sourceTitle)" }
        return "Updated \(UsageSpan.compact(age, roundingUp: false)) ago · \(sourceTitle)"
    }

    @ViewBuilder
    private func meterRow(_ title: String, _ window: UsageWindow) -> some View {
        if let left = window.leftPercent {
            let level = UsageLevel(leftPercent: left)
            GridRow {
                Text(title)
                    .foregroundStyle(SettingsTypography.supportingInk)
                    .fixedSize(horizontal: false, vertical: true)
                UsageMeter(leftPercent: left, level: level)
                Text("\(Int(left.rounded()))%")
                    .fontWeight(.semibold)
                    .foregroundStyle(level.textTint ?? .primary)
                    .frame(width: 32, alignment: .trailing)
                if let resetsAt = window.resetsAt {
                    let seconds = resetsAt.timeIntervalSince(now)
                    Label(seconds > 0 ? UsageSpan.compact(seconds) : "Reset due", systemImage: "clock")
                        .labelStyle(ResetLabelStyle())
                        .foregroundStyle(SettingsTypography.supportingInk)
                        .frame(width: 58, alignment: .trailing)
                } else {
                    Color.clear.frame(width: 58, height: 0)
                }
            }
            .monospacedDigit()
            if let reason = window.reason {
                GridRow {
                    Color.clear.frame(width: 0, height: 0)
                    Text(reason.text)
                        .foregroundStyle(SettingsTypography.supportingInk)
                        .gridCellColumns(3)
                        .gridCellUnsizedAxes(.horizontal)
                }
            }
        } else {
            GridRow {
                Text(title)
                    .foregroundStyle(SettingsTypography.supportingInk)
                    .fixedSize(horizontal: true, vertical: false)
                Text(window.reason?.text ?? "Not available")
                    .foregroundStyle(SettingsTypography.supportingInk)
                    .gridCellColumns(3)
                    .gridCellUnsizedAxes(.horizontal)
            }
        }
    }

    private var spokenSummary: String {
        var parts = [profile.name, profile.provider.title, subtitle]
        if let label = report?.planLabel { parts.append(label) }
        for (title, window) in windows {
            parts.append(MenuBarSummary.spokenRow(title: title, window: window, now: now))
        }
        parts.append(ResetAvailability.text(credits: report?.resetCredits))
        return parts.joined(separator: ", ")
    }
}

/// Shown inside a card after a click or Space: pace and actions.
private struct MenuBarProfileDetail<Actions: View>: View {
    let report: UsageReport?
    @ViewBuilder let actions: () -> Actions

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            let paces = self.paces
            if !paces.isEmpty {
                CardSeparator()
                CardRow {
                    Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 10, verticalSpacing: 4) {
                        ForEach(paces.indices, id: \.self) { index in
                            line("\(paces[index].title) pace", paces[index].text)
                        }
                    }
                    .font(.system(size: 11))
                }
            }
            CardSeparator()
            CardRow { HStack(spacing: 6) { actions() } }
        }
    }

    /// Pace as of the sample's capture time.
    private var paces: [(title: String, text: String)] {
        let primary: [(String, String)] = UsageWindowKind.allCases.compactMap { kind in
            kind.window(in: report).flatMap { UsagePace.text($0) }.map { (kind.title, $0) }
        }
        return primary + (report?.extra ?? []).compactMap { extra in
            UsagePace.text(extra.window).map { (extra.title(source: report?.source), $0) }
        }
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
        ZStack {
            Circle().fill(Color.primary.opacity(0.07))
            Circle().strokeBorder(Color.primary.opacity(0.1), lineWidth: 2)
            if let left = session?.leftPercent {
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
    @Environment(\.theme) private var theme
    @Environment(\.colorScheme) private var colorScheme

    private var fill: Color {
        if level == .critical && theme != .tahoe && colorScheme == .dark {
            return Color(red: 1, green: 0.53, blue: 0.49)
        }
        return level.tint ?? Color.primary.opacity(0.7)
    }

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .leading) {
                Capsule().fill(Color.primary.opacity(0.12))
                Capsule()
                    .fill(fill)
                    .frame(width: max(leftPercent > 0 ? 4 : 0, proxy.size.width * leftPercent / 100))
            }
        }
        .frame(height: 4)
        .frame(maxWidth: .infinity)
        .accessibilityHidden(true)
    }
}

private extension UsageLevel {
    /// Calm by default; colour only when a limit is running low. For the meter and ring.
    var tint: Color? {
        switch self {
        case .normal: return nil
        case .low: return .orange
        case .critical: return criticalGraphicRed
        }
    }

    /// For the percentage text, which needs more contrast than the meter and ring.
    var textTint: Color? {
        switch self {
        case .normal: return nil
        case .low: return SettingsTypography.warningInk
        case .critical: return SettingsTypography.criticalInk
        }
    }
}

/// Meter and ring red, with a deeper shade on light cards.
private let criticalGraphicRed = Color(nsColor: NSColor(name: nil) { appearance in
    isDark(appearance) ? .systemRed : NSColor(srgbRed: 0.843, green: 0, blue: 0.082, alpha: 1)
})
