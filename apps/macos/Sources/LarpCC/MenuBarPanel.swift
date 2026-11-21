import SwiftUI
import AppKit
import LarpModel

// MARK: - Status item label

/// Template glyph plus "62%"; glyph only in Icon mode or when there is no data.
struct MenuBarLabel: View {
    @ObservedObject var store: AppStore
    @AppStorage(MenuBarPreferences.modeKey) private var mode = MenuBarMode.tightest.rawValue
    @AppStorage(MenuBarPreferences.profileKey) private var profileID = ""

    var body: some View {
        let selected = MenuBarMode(rawValue: mode) ?? .tightest
        let profiles = store.snapshot?.profiles ?? []
        let text = MenuBarSummary.labelText(mode: selected, chosenProfileID: profileID, profiles: profiles, usage: store.usage)
        // Icon mode still lets the needle drop when any enabled profile runs low.
        let symbolLeft = MenuBarSummary.leftPercent(mode: selected == .icon ? .tightest : selected,
                                                    chosenProfileID: profileID, profiles: profiles, usage: store.usage)
        HStack(spacing: 3) {
            Image(systemName: MenuBarSummary.symbolName(leftPercent: symbolLeft))
            if let text { Text(text).monospacedDigit() }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(text.map { "LarpCC, \($0) left" } ?? "LarpCC")
        .onAppear { MenuBarDockPolicy.install() }
    }
}

// MARK: - Panel

struct MenuBarPanel: View {
    @ObservedObject var store: AppStore
    /// Rendered off-screen for `--menubar-panel-snapshot`: no scrolling, focus or side effects.
    var isSnapshot = false
    @Environment(\.openWindow) private var openWindow
    @AppStorage(UsagePreferences.sourceKey) private var storedSource = UsageSource.local.rawValue
    @AppStorage(UsagePreferences.consentKey) private var consented = false
    @FocusState private var focusedProfile: String?
    @State private var cardsHeight: CGFloat = 0

    private var source: UsageSource { storedSource == UsageSource.live.rawValue && consented ? .live : .local }
    private var profiles: [Profile] { MenuBarSummary.orderedProfiles(store.snapshot?.profiles ?? [], usage: store.usage) }

    var body: some View {
        VStack(spacing: 0) {
            header
            notices
            cardsArea
            Divider()
            footer
        }
        .frame(width: 352)
        .background {
            if isSnapshot { Color(nsColor: .windowBackgroundColor) }
        }
        .background {
            // Esc closes the panel and Return opens Settings, wherever focus is
            // (Space still presses a focused button).
            ZStack {
                Button(action: closePanel) { EmptyView() }
                    .keyboardShortcut(.cancelAction)
                Button(action: openSettings) { EmptyView() }
                    .keyboardShortcut(.defaultAction)
            }
            .frame(width: 0, height: 0)
            .opacity(0)
            .accessibilityHidden(true)
        }
        .onAppear { if !isSnapshot { store.menuBarPanelOpened() } }
    }

    // MARK: Header

    private var header: some View {
        HStack(spacing: 6) {
            Text("LarpCC").font(.headline)
            MenuBarBadge(text: source.title)
            if store.isFixture { MenuBarBadge(text: "Sample") }
            Spacer(minLength: 8)
            if store.usageLoading || store.busy {
                ProgressView().controlSize(.small)
                    .accessibilityLabel("Updating usage")
            } else {
                Button { store.menuBarRefresh() } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(MenuBarIconButtonStyle())
                .help(source == .live ? "Refresh live usage" : "Re-read saved usage")
                .accessibilityLabel(source == .live ? "Refresh live usage" : "Re-read saved usage")
            }
        }
        .padding(.horizontal, 14)
        .padding(.top, 12)
        .padding(.bottom, 10)
    }

    @ViewBuilder
    private var notices: some View {
        if let error = store.usageError {
            MenuBarNotice(symbol: "exclamationmark.triangle.fill", text: error)
        }
        if let message = store.message, !isSnapshot {
            MenuBarNotice(symbol: "info.circle", text: message) { store.message = nil }
        }
    }

    // MARK: Cards

    @ViewBuilder
    private var cardsArea: some View {
        if isSnapshot {
            cards
        } else {
            ScrollView {
                cards.background(GeometryReader { proxy in
                    Color.clear.preference(key: MenuBarCardsHeight.self, value: proxy.size.height)
                })
            }
            .scrollBounceBehavior(.basedOnSize)
            .frame(height: min(max(cardsHeight, 60), maxCardsHeight))
            .onPreferenceChange(MenuBarCardsHeight.self) { cardsHeight = $0 }
        }
    }

    /// Leaves room for the header, footer and menu bar on small screens.
    private var maxCardsHeight: CGFloat { max(240, (NSScreen.main?.visibleFrame.height ?? 800) - 200) }

    private var cards: some View {
        VStack(spacing: 8) {
            if profiles.isEmpty {
                emptyState
            }
            ForEach(profiles) { profile in
                MenuBarProfileCard(store: store, profile: profile, report: store.usage[profile.id],
                                   source: source, focused: focusedProfile == profile.id)
                    .focusable(!isSnapshot)
                    .focused($focusedProfile, equals: profile.id)
                    .focusEffectDisabled()
            }
        }
        .padding(.horizontal, 10)
        .padding(.bottom, 10)
        .onKeyPress(.downArrow) { moveFocus(by: 1) }
        .onKeyPress(.upArrow) { moveFocus(by: -1) }
    }

    private var emptyState: some View {
        let text: String
        if store.busy || store.snapshot == nil && store.isFixture {
            text = "Loading profiles…"
        } else if store.snapshot == nil {
            text = "Profiles are unavailable. Open Settings to choose larp or use sample data."
        } else {
            text = "No profiles are switched on. Turn one on in Settings."
        }
        return Text(text)
            .font(.callout)
            .foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 18)
    }

    private func moveFocus(by step: Int) -> KeyPress.Result {
        let ids = profiles.map(\.id)
        guard !ids.isEmpty else { return .ignored }
        guard let current = focusedProfile, let index = ids.firstIndex(of: current) else {
            focusedProfile = step > 0 ? ids.first : ids.last
            return .handled
        }
        focusedProfile = ids[min(max(index + step, 0), ids.count - 1)]
        return .handled
    }

    // MARK: Footer

    private var footer: some View {
        VStack(spacing: 0) {
            MenuBarMenuRow(title: source == .live ? "Refresh" : "Re-read Usage", shortcut: "⌘R") { store.menuBarRefresh() }
                .keyboardShortcut("r", modifiers: .command)
                .disabled(store.usageLoading || store.busy)
            MenuBarMenuRow(title: "Open LarpCC Settings…", shortcut: "⌘,", action: openSettings)
                .keyboardShortcut(",", modifiers: .command)
            MenuBarMenuRow(title: "Quit LarpCC", shortcut: "⌘Q") { NSApp.terminate(nil) }
                .keyboardShortcut("q", modifiers: .command)
        }
        .padding(5)
    }

    private func openSettings() {
        MenuBarDockPolicy.showDockIcon()
        openWindow(id: "settings")
        NSApp.activate()
    }

    private func closePanel() {
        NSApp.keyWindow?.close()
    }
}

private struct MenuBarCardsHeight: SwiftUI.PreferenceKey {
    static let defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

// MARK: - Profile card

struct MenuBarProfileCard: View {
    @ObservedObject var store: AppStore
    let profile: Profile
    let report: UsageReport?
    let source: UsageSource
    let focused: Bool

    private var level: UsageLevel { report?.lowestLeftPercent.map(UsageLevel.init(leftPercent:)) ?? .normal }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            header
            if let report {
                let kinds = UsageWindowKind.allCases.filter { $0.window(in: report) != nil }
                if kinds.isEmpty {
                    Text("No limits reported").font(.callout).foregroundStyle(.secondary)
                }
                ForEach(kinds, id: \.self) { kind in
                    if let window = kind.window(in: report) {
                        MenuBarWindowRow(kind: kind, window: window)
                    }
                }
            } else {
                Text(source == .local ? "No usage saved yet" : "Not refreshed yet")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Divider()
            HStack(alignment: .firstTextBaseline) {
                Label(ResetAvailability.text(credits: report?.resetCredits), systemImage: "arrow.counterclockwise.circle")
                    .accessibilityLabel(ResetAvailability.text(credits: report?.resetCredits))
                Spacer(minLength: 8)
                if let report {
                    MenuBarUpdatedText(report: report)
                }
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(.quinary))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(borderStyle, lineWidth: focused ? 2 : 1)
        )
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(profile.name), \(profile.provider.title)")
    }

    private var borderStyle: AnyShapeStyle {
        if focused { return AnyShapeStyle(Color.accentColor) }
        if level != .normal { return AnyShapeStyle(level.color.opacity(0.45)) }
        return AnyShapeStyle(Color(nsColor: .separatorColor).opacity(0.6))
    }

    private var desktopAvailable: Bool {
        profile.desktopEnabled && !store.isFixture && !store.busy && store.snapshot != nil
    }

    private var desktopHelp: String {
        let open = "Open \(profile.provider.desktopApp)"
        if !profile.desktopEnabled { return "\(open): desktop app is off" }
        if store.isFixture { return "\(open): not available with sample data" }
        return open
    }

    private var header: some View {
        HStack(spacing: 10) {
            ProviderTile(provider: profile.provider, size: 28)
            VStack(alignment: .leading, spacing: 1) {
                Text(profile.name)
                    .font(.system(size: 13, weight: .semibold))
                    .lineLimit(1)
                Text(profile.provider.title)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 6)
            // Opening in Terminal has no larp command yet, and sample mode never launches,
            // so those stay disabled rather than failing (a failed action clears the snapshot).
            Button {} label: { Image(systemName: "terminal") }
                .buttonStyle(MenuBarIconButtonStyle())
                .disabled(true)
                .help(profile.cliEnabled ? "Open in Terminal: not available yet" : "Open in Terminal: command line is off")
                .accessibilityLabel("Open \(profile.name) in Terminal, not available")
            Button { store.perform(.open(profileID: profile.id, surface: .desktop)) } label: {
                Image(systemName: "macwindow")
            }
            .buttonStyle(MenuBarIconButtonStyle())
            .disabled(!desktopAvailable)
            .help(desktopHelp)
            .accessibilityLabel("Open \(profile.name) in \(profile.provider.desktopApp)" + (desktopAvailable ? "" : ", not available"))
        }
    }
}

/// One window: label, % left, a capsule bar of % left with a pace tick, and the reset countdown.
struct MenuBarWindowRow: View {
    let kind: UsageWindowKind
    let window: UsageWindow

    var body: some View {
        TimelineView(.everyMinute) { context in
            let now = context.date
            let level = UsageLevel(leftPercent: window.leftPercent)
            VStack(alignment: .leading, spacing: 5) {
                HStack(alignment: .firstTextBaseline) {
                    Text(kind.title).font(.callout.weight(.medium))
                    Spacer()
                    Text("\(Int(window.leftPercent.rounded()))% left" + (level.word.map { " · \($0)" } ?? ""))
                        .font(.callout)
                        .monospacedDigit()
                        .foregroundStyle(level == .normal ? AnyShapeStyle(.secondary) : AnyShapeStyle(level.color))
                }
                MenuBarUsageBar(leftPercent: window.leftPercent,
                                pacePercent: UsagePace.expectedLeftPercent(resetsAt: window.resetsAt, length: kind.length, now: now),
                                tint: window.levelColor)
                HStack(alignment: .firstTextBaseline) {
                    Text(ResetCountdown.text(resetsAt: window.resetsAt, now: now))
                        .monospacedDigit()
                    Spacer()
                    if UsagePace.isAhead(window, kind: kind, now: now) {
                        Text("Ahead of pace")
                    }
                }
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(MenuBarSummary.spokenRow(kind: kind, window: window, now: now))
        }
    }
}

/// Capsule of percent left. The tick marks what would be left on an even pace.
struct MenuBarUsageBar: View {
    let leftPercent: Double
    let pacePercent: Double?
    let tint: Color

    var body: some View {
        GeometryReader { proxy in
            let width = proxy.size.width
            ZStack(alignment: .leading) {
                Capsule().fill(.quaternary)
                Capsule().fill(tint)
                    .frame(width: leftPercent > 0 ? max(6, width * leftPercent / 100) : 0)
                if let pacePercent {
                    RoundedRectangle(cornerRadius: 0.75)
                        .fill(Color.primary.opacity(0.6))
                        .frame(width: 1.5, height: 10)
                        .offset(x: min(max(width * pacePercent / 100 - 0.75, 0), width - 1.5))
                }
            }
        }
        .frame(height: 6)
        .padding(.vertical, 2)
        .accessibilityHidden(true)
    }
}

/// "Updated 4m ago · Local", updating itself every minute.
struct MenuBarUpdatedText: View {
    let report: UsageReport
    var body: some View {
        if let captured = report.capturedAt {
            TimelineView(.everyMinute) { context in
                let age = context.date.timeIntervalSince(captured)
                Text("Updated \(age < 60 ? "just now" : UsageSpan.compact(age, roundingUp: false) + " ago") · \(report.source.title)")
                    .monospacedDigit()
            }
        } else {
            Text(report.source.title)
        }
    }
}

// MARK: - Small pieces

extension UsageLevel {
    var color: Color {
        switch self {
        case .normal: return .primary
        case .low: return .orange
        case .critical: return .red
        }
    }
}

struct MenuBarBadge: View {
    let text: String
    var body: some View {
        Text(text)
            .font(.caption2.weight(.semibold))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(Capsule().fill(.quaternary))
    }
}

/// One-line notice with fixed safe text (never raw CLI output).
struct MenuBarNotice: View {
    let symbol: String
    let text: String
    var onDismiss: (() -> Void)? = nil
    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: symbol).foregroundStyle(.secondary)
            Text(text)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: .infinity, alignment: .leading)
                .help(text)
            if let onDismiss {
                Button(action: onDismiss) { Image(systemName: "xmark") }
                    .buttonStyle(MenuBarIconButtonStyle())
                    .accessibilityLabel("Dismiss message")
            }
        }
        .font(.caption)
        .padding(.horizontal, 14)
        .padding(.bottom, 8)
        .accessibilityElement(children: .combine)
    }
}

/// Compact SwiftUI-drawn icon button (also renders in the snapshot).
struct MenuBarIconButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View { IconBody(configuration: configuration) }

    private struct IconBody: View {
        let configuration: ButtonStyleConfiguration
        @Environment(\.isEnabled) private var isEnabled
        @State private var hovering = false
        var body: some View {
            configuration.label
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(isEnabled ? AnyShapeStyle(.secondary) : AnyShapeStyle(.quaternary))
                .frame(width: 24, height: 22)
                .background(
                    RoundedRectangle(cornerRadius: 5, style: .continuous)
                        .fill(Color.primary.opacity(configuration.isPressed ? 0.16 : hovering && isEnabled ? 0.08 : 0))
                )
                .contentShape(Rectangle())
                .onHover { hovering = $0 }
        }
    }
}

/// Full-width, menu-style row with its shortcut on the right.
struct MenuBarMenuRow: View {
    let title: String
    let shortcut: String
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            HStack {
                Text(title)
                Spacer()
                Text(shortcut).foregroundStyle(.secondary).accessibilityHidden(true)
            }
        }
        .buttonStyle(MenuBarRowStyle())
    }

    private struct MenuBarRowStyle: ButtonStyle {
        func makeBody(configuration: Configuration) -> some View { RowBody(configuration: configuration) }
        private struct RowBody: View {
            let configuration: ButtonStyleConfiguration
            @Environment(\.isEnabled) private var isEnabled
            @State private var hovering = false
            var body: some View {
                configuration.label
                    .font(.system(size: 13))
                    .foregroundStyle(isEnabled ? AnyShapeStyle(.primary) : AnyShapeStyle(.tertiary))
                    .padding(.horizontal, 9)
                    .frame(height: 24)
                    .background(
                        RoundedRectangle(cornerRadius: 5, style: .continuous)
                            .fill(Color.primary.opacity(configuration.isPressed ? 0.14 : hovering && isEnabled ? 0.08 : 0))
                    )
                    .contentShape(Rectangle())
                    .onHover { hovering = $0 }
            }
        }
    }
}
