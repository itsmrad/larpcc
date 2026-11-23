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
            if mode != MenuBarMode.icon.rawValue, !rings.isEmpty,
               let image = Self.templateImage(rings, weight: theme.tokens.statusWeight) {
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

struct MenuBarPanel: View {
    @ObservedObject var store: AppStore
    /// Offscreen render for the debug snapshot hook: no scrolling, no focus.
    var isSnapshot = false
    var snapshotExpandedID: String? = nil
    var snapshotSelectedID: String? = nil
    @Environment(\.openWindow) private var openWindow
    @Environment(\.dismiss) private var dismiss
    @Environment(\.theme) private var theme
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
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
            list.padding(.horizontal, tokens.cardInset)
            VStack(spacing: 0) {
                // Classic menus: the footer items follow a separator instead of sitting in a card.
                if !tokens.footerInCard { Divider().padding(.horizontal, 7).padding(.vertical, 5) }
                footer
            }
            .padding(.horizontal, tokens.cardInset)
            .padding(.top, tokens.footerInCard ? tokens.cardInset : 0)
            .padding(.bottom, tokens.cardInset)
            .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { footerHeight = $0 }
        }
        .frame(width: 360)
        // The menu bar window keeps the size of its first layout, so it is resized
        // to the panel's ideal height whenever the content changes (top edge fixed).
        .fixedSize(horizontal: false, vertical: true)
        .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { panelHeight = $0 }
        .frame(maxHeight: .infinity, alignment: .top)
        .background {
            if tokens.panelMaterial && !isSnapshot {
                MenuBarMaterial().overlay(classicBacking).accessibilityHidden(true)
            } else {
                // The offscreen snapshot cannot draw the menu material; it uses a colour sampled from it.
                tokens.panelMaterial ? AnyView(menuMaterialStandIn.overlay(classicBacking)) : AnyView(panelBackground)
            }
        }
        .background {
            if !isSnapshot {
                MenuBarWindowSizer(height: min(panelHeight, maxPanelHeight)) { if screenHeight != $0 { screenHeight = $0 } }
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
                    .clipShape(RoundedRectangle(cornerRadius: tokens.cardRadius, style: .continuous))
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
            VStack(alignment: .leading, spacing: tokens.cardSpacing) {
                if groups.isEmpty {
                    Text(emptyMessage)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(tokens.cardPadding)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .panelCard(highlighted: false, glass: glass)
                }
                ForEach(groups, id: \.provider) { group in
                    Text(group.provider.title)
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, tokens.cardPadding)
                        .padding(.top, tokens.card == .flat ? 6 : 2)
                        .padding(.bottom, tokens.card == .flat ? 0 : -2)
                        .accessibilityAddTraits(.isHeader)
                    ForEach(Array(group.profiles.enumerated()), id: \.element.id) { index, profile in
                        // Flat cards are split by hairlines, like menu sections.
                        if tokens.card == .flat, index > 0 {
                            Divider().padding(.horizontal, tokens.cardPadding)
                        }
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
        .padding(tokens.footerInCard ? 4 : 0)
        .modifier(FooterCard(inCard: tokens.footerInCard, glass: glass))
    }

    private func menuRow(_ item: Item, title: String, shortcut: String, enabled: Bool = true) -> some View {
        let selected = highlighted == item
        // Classic menus highlight with the accent colour; Tahoe with a quiet fill.
        let accent = tokens.accentMenuHighlight && selected
        return Button { activate(item) } label: {
            HStack {
                Text(title)
                Spacer()
                Text(shortcut)
                    .foregroundStyle(accent ? AnyShapeStyle(Color(nsColor: .alternateSelectedControlTextColor))
                                            : AnyShapeStyle(.secondary))
                    .accessibilityHidden(true)
            }
            .font(.system(size: 13))
            .foregroundStyle(accent ? Color(nsColor: .alternateSelectedControlTextColor) : .primary)
            .padding(.horizontal, tokens.footerInCard ? 8 : 9)
            .frame(height: tokens.footerInCard ? 26 : 24)
            .opacity(enabled ? 1 : 0.45)
            .background {
                RoundedRectangle(cornerRadius: tokens.menuRowRadius, style: .continuous)
                    .fill(accent ? Color(nsColor: .selectedContentBackgroundColor)
                                 : Color.primary.opacity(selected ? 0.08 : 0))
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
    /// A profile card in the theme's style. Tahoe: a floating Liquid Glass card, concentric with
    /// the panel (opaque with a hairline without glass). Sequoia: a flat row. Sonoma: a widget tile.
    func panelCard(highlighted: Bool, glass: Bool) -> some View {
        modifier(PanelCard(highlighted: highlighted, glass: glass))
    }
}

private struct PanelCard: ViewModifier {
    let highlighted: Bool
    let glass: Bool
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.theme) private var theme

    func body(content: Content) -> some View {
        let tokens = theme.tokens
        let shape = RoundedRectangle(cornerRadius: tokens.cardRadius, style: .continuous)
        let strong = contrast == .increased
        let boxed = tokens.card != .flat
        let card = content
            .background {
                ZStack {
                    if boxed { shape.fill(tokens.card == .widget ? widgetFill : cardLift) }
                    // The selection sits above the fill, so it also shows on opaque cards.
                    if highlighted { shape.fill(Color.primary.opacity(0.08)) }
                }
                .shadow(color: .black.opacity(tokens.card == .widget ? 0.12 : 0), radius: 2, y: 1)
            }
            .overlay {
                if highlighted && (boxed || strong) {
                    shape.strokeBorder(Color.primary.opacity(strong ? 0.85 : 0.3), lineWidth: strong ? 2 : 1)
                } else if strong && boxed {
                    shape.strokeBorder(Color.primary.opacity(0.5), lineWidth: 1)
                }
            }
        if #available(macOS 26, *), glass, tokens.card == .glass {
            card.glassEffect(.regular, in: shape)
        } else {
            card.overlay(shape.strokeBorder(Color.primary.opacity(boxed ? 0.1 : 0), lineWidth: 0.5))
        }
    }
}

/// Tahoe keeps its footer items in a card of their own; classic menus list them plainly.
private struct FooterCard: ViewModifier {
    let inCard: Bool
    let glass: Bool

    func body(content: Content) -> some View {
        if inCard { content.panelCard(highlighted: false, glass: glass) } else { content }
    }
}

/// The classic menu material behind the Sequoia and Sonoma panels.
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

/// The menu material as sampled on macOS 26 (light and dark), for offscreen snapshots only.
private let menuMaterialStandIn = Color(nsColor: NSColor(name: nil) { appearance in
    isDark(appearance) ? NSColor(srgbRed: 0.353, green: 0.365, blue: 0.369, alpha: 1)
                       : NSColor(srgbRed: 0.886, green: 0.89, blue: 0.898, alpha: 1)
})

/// Darkens the classic menu material in dark mode so secondary text keeps 4.5:1 or better.
private let classicBacking = Color(nsColor: NSColor(name: nil) { appearance in
    NSColor.black.withAlphaComponent(isDark(appearance) ? 0.5 : 0)
})

/// The system red in light mode; a lighter red in dark mode, so a nearly-out percentage
/// reads at 4.5:1 or better on dark cards.
private let criticalRed = Color(nsColor: NSColor(name: nil) { appearance in
    isDark(appearance) ? NSColor(srgbRed: 1, green: 0.49, blue: 0.47, alpha: 1) : .systemRed
})

/// Sonoma widget tiles on the menu material.
private let widgetFill = Color(nsColor: NSColor(name: nil) { appearance in
    isDark(appearance) ? NSColor.white.withAlphaComponent(0.05) : NSColor.white.withAlphaComponent(0.6)
})

// MARK: - Profile card

/// The always-visible part of a card: ring, name, freshness and one meter line per usage window.
private struct MenuBarProfileSummary: View {
    let profile: Profile
    let report: UsageReport?
    let source: UsageSource
    let now: Date
    let expanded: Bool
    @Environment(\.theme) private var theme

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
            // Reset credits always show, whatever else the card has.
            Label(ResetAvailability.text(credits: report?.resetCredits), systemImage: "arrow.counterclockwise.circle")
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
        }
        .padding(theme.tokens.cardPadding)
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
        if kinds.contains(where: { kind in kind.window(in: report).map { UsagePace.isOutOfDate($0, now: now) } ?? false }) {
            return "\(UsagePace.outOfDate) · \(sourceTitle)"
        }
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
            Label(resetText(window), systemImage: "clock")
                .labelStyle(ResetLabelStyle())
                .foregroundStyle(.secondary)
                .frame(width: 58, alignment: .trailing)
        }
        .monospacedDigit()
    }

    /// Time to reset; a dash when unknown or already past (the subtitle then says the data is out of date).
    private func resetText(_ window: UsageWindow) -> String {
        guard let left = window.resetsAt?.timeIntervalSince(now), left > 0 else { return "—" }
        return UsageSpan.compact(left)
    }

    private var spokenSummary: String {
        var parts = [profile.name, profile.provider.title, subtitle]
        for kind in kinds {
            if let window = kind.window(in: report) {
                parts.append(MenuBarSummary.spokenRow(kind: kind, window: window,
                                                      capturedAt: report?.capturedAt, now: now))
            }
        }
        parts.append(ResetAvailability.text(credits: report?.resetCredits))
        return parts.joined(separator: ", ")
    }
}

/// Shown inside a card after a click or Space: pace and actions.
private struct MenuBarProfileDetail<Actions: View>: View {
    let report: UsageReport?
    let now: Date
    @ViewBuilder let actions: () -> Actions
    @Environment(\.theme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Divider()
            if !paces.isEmpty {
                Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 10, verticalSpacing: 4) {
                    ForEach(paces, id: \.kind) { line("\($0.kind.title) pace", $0.text) }
                }
                .font(.system(size: 11))
            }
            HStack(spacing: 6) { actions() }
        }
        .padding([.horizontal, .bottom], theme.tokens.cardPadding)
    }

    /// Pace as of the sample's capture time.
    private var paces: [(kind: UsageWindowKind, text: String)] {
        UsageWindowKind.allCases.compactMap { kind in
            kind.window(in: report)
                .flatMap { UsagePace.text($0, kind: kind, capturedAt: report?.capturedAt, now: now) }
                .map { (kind, $0) }
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
        case .critical: return criticalRed
        }
    }
}
