import SwiftUI
import LarpModel

// MARK: - Preferences (UserDefaults; never sent anywhere)

enum UsagePreferences {
    static let sourceKey = "usageSource"
    static let consentKey = "usageLiveConsent"
    static let intervalKey = "usageLiveIntervalMinutes"
    static let intervals = [0, 5, 15, 30, 60]
    // The build has no qualified LIVE usage rows.
    static let liveQualified = false

    static func effectiveSource(stored: String, consented: Bool) -> UsageSource {
        liveQualified && stored == UsageSource.live.rawValue && consented ? .live : .local
    }

    /// Live counts only after the one-time confirmation.
    static var source: UsageSource {
        effectiveSource(stored: UserDefaults.standard.string(forKey: sourceKey) ?? UsageSource.local.rawValue,
                        consented: UserDefaults.standard.bool(forKey: consentKey))
    }
    static func supportedInterval(_ minutes: Int) -> Int {
        intervals.contains(minutes) ? minutes : 0
    }
    static var intervalMinutes: Int { supportedInterval(UserDefaults.standard.integer(forKey: intervalKey)) }

    static func liveUnavailable(isFixture: Bool) -> String {
        isFixture ? "Live usage isn't available in this build yet; sample data is shown."
                  : "Live usage isn't available in this build yet."
    }
}

// MARK: - General › Usage

struct UsageSettingsSection: View {
    @ObservedObject var store: AppStore
    @AppStorage(UsagePreferences.sourceKey) private var source = UsageSource.local.rawValue
    @AppStorage(UsagePreferences.consentKey) private var consented = false
    @AppStorage(UsagePreferences.intervalKey) private var interval = 0
    @State private var confirmingLive = false

    private var effectiveSource: UsageSource {
        UsagePreferences.effectiveSource(stored: source, consented: consented)
    }

    var body: some View {
        Section {
            Picker(selection: Binding(
                get: { effectiveSource },
                set: { choice in
                    guard choice != .live || UsagePreferences.liveQualified else { return }
                    if choice == .live && !consented { confirmingLive = true } else { select(choice) }
                }
            )) {
                Text(UsageSource.local.title).tag(UsageSource.local)
                if UsagePreferences.liveQualified {
                    Text(UsageSource.live.title).tag(UsageSource.live)
                }
            } label: {
                Text("Usage source")
                Text(effectiveSource == .live
                     ? UsagePreferences.liveUnavailable(isFixture: store.isFixture)
                     : "Local uses only data the official apps already saved on this Mac.")
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
            .pickerStyle(.segmented)
            if !UsagePreferences.liveQualified {
                LabeledContent("Live usage", value: "Disabled · Unverified")
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
            if effectiveSource == .live {
                Picker(selection: Binding(
                    get: { UsagePreferences.supportedInterval(interval) },
                    set: { interval = $0 }
                )) {
                    ForEach(UsagePreferences.intervals, id: \.self) { minutes in
                        Text(minutes == 0 ? "Off" : "Every \(minutes) min").tag(minutes)
                    }
                } label: {
                    Text("Refresh automatically")
                    Text("Off means usage updates only when you refresh.")
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
                LabeledContent {
                    Button("Refresh Now") { store.refreshUsage() }
                        .disabled(store.usageLoading || store.snapshot == nil)
                } label: {
                    Text("Live usage")
                    if !store.usageLoading, let error = store.usageError {
                        Text(error).foregroundStyle(SettingsTypography.supportingInk)
                    } else {
                        UsageUpdatedText(report: store.usage.values.first, loading: store.usageLoading, isFixture: store.isFixture)
                    }
                }
            } else if !store.usageLoading, let error = store.usageError {
                Text(error).foregroundStyle(SettingsTypography.supportingInk)
            }
        } header: {
            Text("Usage")
        }
        .onChange(of: interval) { _, _ in store.scheduleLiveRefresh() }
        .sheet(isPresented: $confirmingLive) {
            LiveUsageConsentSheet(isFixture: store.isFixture) {
                guard UsagePreferences.liveQualified else { return }
                consented = true
                select(.live)
            }
        }
    }

    private func select(_ choice: UsageSource) {
        source = choice.rawValue
        store.scheduleLiveRefresh()
        store.refreshUsage()
    }
}

/// One-time confirmation before Live usage. Cancel keeps Local.
struct LiveUsageConsentSheet: View {
    let isFixture: Bool
    let onEnable: () -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 12) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .symbolRenderingMode(.multicolor)
                    .font(.system(size: 28))
                Text("Turn on live usage?")
                    .font(.headline)
            }
            Text("LIVE usage asks the profile's qualified official client for limits using its stored sign-in. Provider restrictions are possible. LIVE remains unverified against real accounts.")
                .fixedSize(horizontal: false, vertical: true)
            Text(UsagePreferences.liveUnavailable(isFixture: isFixture))
                .font(.callout)
                .foregroundStyle(SettingsTypography.supportingInk)
                .fixedSize(horizontal: false, vertical: true)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Enable live usage") { onEnable(); dismiss() }
            }
        }
        .padding(20)
        .frame(width: 440)
    }
}

// MARK: - Profile page › Usage

struct UsageSection: View {
    @ObservedObject var store: AppStore
    let profile: Profile
    @State private var extrasExpanded = false

    var body: some View {
        let report = store.usage[profile.id]
        Section {
            if let label = report?.planLabel {
                LabeledContent("Plan", value: label)
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
            if let window = report?.fiveHour { UsageWindowRow(title: "5-hour window", window: window) }
            if let window = report?.weekly { UsageWindowRow(title: "Weekly window", window: window) }
            let extra = report?.extra ?? []
            let visibleExtras = extrasExpanded ? extra[...] : extra.prefix(2)
            ForEach(visibleExtras, id: \.label) { window in
                UsageWindowRow(title: window.title(source: report?.source), window: window.window)
            }
            if extra.count > 2 {
                Button(extrasExpanded ? "Show fewer usage windows" : "Show more usage windows") {
                    extrasExpanded.toggle()
                }
                .buttonStyle(.borderless)
            }
            if report == nil {
                if store.usageLoading {
                    Text("Reading usage…")
                        .foregroundStyle(SettingsTypography.supportingInk)
                } else if let error = store.usageError {
                    RowLabel(UsagePreferences.source == .live ? "Live usage unavailable" : "Usage unavailable",
                             subtitle: error) {
                        Image(systemName: "exclamationmark.circle")
                            .foregroundStyle(SettingsTypography.warningInk)
                    }
                } else {
                    Text("No usage windows saved for this profile yet.")
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
            }
            ResetAvailabilityRow(credits: report?.resetCredits)
        } header: {
            HStack(alignment: .firstTextBaseline) {
                Text("Usage")
                Spacer()
                if store.usageLoading {
                    ProgressView().controlSize(.mini).accessibilityLabel("Refreshing usage")
                } else {
                    Button { store.refreshUsage() } label: { Image(systemName: "arrow.clockwise") }
                        .buttonStyle(.borderless)
                        .foregroundStyle(SettingsTypography.supportingInk)
                        .help("Refresh usage")
                        .accessibilityLabel("Refresh usage")
                }
            }
        } footer: {
            if report != nil {
                UsageUpdatedText(report: report, loading: false, isFixture: store.isFixture)
                    .font(SettingsTypography.supporting)
            }
        }
    }
}

/// "Limit resets" row; always shown, with wording shared with the menu bar.
struct ResetAvailabilityRow: View {
    let credits: UsageResetCredits?
    var body: some View {
        LabeledContent {
            ResetCreditLabel(credits: credits)
                .monospacedDigit()
                .foregroundStyle(SettingsTypography.supportingInk)
        } label: {
            Text("Limit resets")
        }
    }
}

struct ResetCreditLabel: View {
    let credits: UsageResetCredits?

    var body: some View {
        let text = ResetAvailability.text(credits: credits)
        if credits?.state == .available, credits?.count != nil {
            Label {
                Text(text)
            } icon: {
                Image(systemName: "ticket").accessibilityHidden(true)
            }
        } else {
            Text(text)
        }
    }
}

private struct UsageWindowRow: View {
    let title: String
    let window: UsageWindow

    var body: some View {
        VStack(alignment: .leading, spacing: SettingsSpacing.inline) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: SettingsSpacing.tight) {
                    Text(title)
                        .font(SettingsTypography.heading)
                    if let reason = window.reason {
                        Text(reason.text)
                            .font(SettingsTypography.supporting)
                            .foregroundStyle(SettingsTypography.supportingInk)
                    } else if let resetsAt = window.resetsAt {
                        TimelineView(.everyMinute) { context in
                            Text(resetsAt > context.date ? "Resets in \(UsageTime.span(resetsAt.timeIntervalSince(context.date)))" : "Reset due")
                        }
                        .font(SettingsTypography.supporting)
                        .foregroundStyle(SettingsTypography.supportingInk)
                    }
                }
                Spacer()
                if let left = window.leftPercent {
                    Text("\(Int(left.rounded()))% left")
                        .monospacedDigit()
                        .foregroundStyle(left < 10 ? SettingsTypography.criticalInk : Color.primary)
                }
            }
            if let left = window.leftPercent {
                ProgressView(value: left, total: 100)
                    .tint(usageColor(left))
                    .accessibilityHidden(true)
            }
            if let pace = UsagePace.text(window) {
                Text(pace)
                    .font(SettingsTypography.supporting)
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
        }
        .padding(.vertical, SettingsSpacing.tight)
        .accessibilityElement(children: .combine)
    }
}

/// "Updated 3 minutes ago · Local"; the relative time updates itself.
struct UsageUpdatedText: View {
    let report: UsageReport?
    let loading: Bool
    let isFixture: Bool
    var body: some View {
        if loading {
            Text("Updating…").foregroundStyle(SettingsTypography.supportingInk)
        } else if let report, let captured = report.capturedAt {
            TimelineView(.everyMinute) { context in
                let age = context.date.timeIntervalSince(captured)
                Text("Updated \(age < 60 ? "just now" : UsageTime.span(age) + " ago") · \(isFixture ? "Sample" : report.source.title)")
            }
            .foregroundStyle(SettingsTypography.supportingInk)
        } else {
            Text("Not updated yet").foregroundStyle(SettingsTypography.supportingInk)
        }
    }
}

private func usageColor(_ leftPercent: Double) -> Color {
    switch UsageLevel(leftPercent: leftPercent) {
    case .critical: return .red
    case .low: return .orange
    case .normal: return .green
    }
}

/// Minute-precision spans such as "2 hr 13 min" or "3 days 1 hr".
enum UsageTime {
    private static let formatter: DateComponentsFormatter = {
        let f = DateComponentsFormatter()
        f.allowedUnits = [.day, .hour, .minute]
        f.unitsStyle = .short
        f.maximumUnitCount = 2
        return f
    }()
    static func span(_ seconds: TimeInterval) -> String {
        seconds < 60 ? "less than a minute" : formatter.string(from: seconds) ?? ""
    }
}
