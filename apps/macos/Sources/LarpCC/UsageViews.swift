import SwiftUI
import LarpModel

// MARK: - Preferences (UserDefaults; never sent anywhere)

enum UsagePreferences {
    static let sourceKey = "usageSource"
    static let consentKey = "usageLiveConsent"
    static let intervalKey = "usageLiveIntervalMinutes"
    static let intervals = [0, 5, 15, 30, 60]

    /// Live counts only after the one-time confirmation.
    static var source: UsageSource {
        let stored = UsageSource(rawValue: UserDefaults.standard.string(forKey: sourceKey) ?? "") ?? .local
        return stored == .live && UserDefaults.standard.bool(forKey: consentKey) ? .live : .local
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

    var body: some View {
        Section {
            Picker(selection: Binding(
                get: { source == UsageSource.live.rawValue && consented ? UsageSource.live : .local },
                set: { choice in
                    if choice == .live && !consented { confirmingLive = true } else { select(choice) }
                }
            )) {
                ForEach(UsageSource.allCases, id: \.self) { Text($0.title).tag($0) }
            } label: {
                Text("Usage source")
                Text(source == UsageSource.live.rawValue && consented
                     ? UsagePreferences.liveUnavailable(isFixture: store.isFixture)
                     : "Local uses only data the official apps already saved on this Mac.")
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
            .pickerStyle(.segmented)
            if source == UsageSource.live.rawValue && consented {
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
                        UsageUpdatedText(report: store.usage.values.first, loading: store.usageLoading)
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
            Text("LIVE usage reads this profile's sign-in token from its local credential store and sends it to the provider's usage endpoint. This may break the provider's terms of service, and LarpCC can't rule out account restrictions. LIVE is unverified against real accounts. LOCAL mode, the default, never reads tokens and makes no network requests.")
                .fixedSize(horizontal: false, vertical: true)
            Text("For Claude profiles, macOS will ask whether larp may read the Claude Code sign-in item in your Keychain. Allow lets it read once. Always Allow lets larp read that item from now on without asking. Deny keeps LIVE from working for that profile. LarpCC never chooses for you.")
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
            ForEach(extra.indices, id: \.self) { index in
                UsageWindowRow(title: extra[index].label, window: extra[index].window)
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
                UsageUpdatedText(report: report, loading: false)
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
            Text(ResetAvailability.text(credits: credits))
                .monospacedDigit()
                .foregroundStyle(SettingsTypography.supportingInk)
        } label: {
            Text("Limit resets")
            if credits?.state == .available {
                Text("Each one resets a window early.")
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
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
    var body: some View {
        if loading {
            Text("Updating…").foregroundStyle(SettingsTypography.supportingInk)
        } else if let report, let captured = report.capturedAt {
            TimelineView(.everyMinute) { context in
                let age = context.date.timeIntervalSince(captured)
                Text("Updated \(age < 60 ? "just now" : UsageTime.span(age) + " ago") · \(report.source.title)")
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
