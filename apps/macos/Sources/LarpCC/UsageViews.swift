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
    static var intervalMinutes: Int { UserDefaults.standard.integer(forKey: intervalKey) }
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
                     ? "Live asks the provider through larp for current usage."
                     : "Local uses only data the official apps already saved on this Mac.")
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
            .pickerStyle(.segmented)
            if source == UsageSource.live.rawValue && consented {
                Picker(selection: $interval) {
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
                    UsageUpdatedText(report: store.usage.values.first, loading: store.usageLoading)
                }
            }
        } header: {
            Text("Usage")
        }
        .onChange(of: interval) { _, _ in store.scheduleLiveRefresh() }
        .sheet(isPresented: $confirmingLive) {
            LiveUsageConsentSheet {
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
            Text("Live mode reads each profile's sign-in token on this Mac to ask Anthropic/OpenAI for current usage. This may break the providers' terms and could lead to account restrictions. Local mode uses only data the official apps already saved.")
                .fixedSize(horizontal: false, vertical: true)
            Text("LarpCC itself never reads tokens; only larp does, and only while Live is on. You can switch back to Local in General at any time.")
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
            if let window = report?.fiveHour { UsageWindowRow(title: "5-hour window", window: window) }
            if let window = report?.weekly { UsageWindowRow(title: "Weekly window", window: window) }
            if report?.fiveHour == nil && report?.weekly == nil {
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
    let credits: Int?
    var body: some View {
        LabeledContent {
            Text(ResetAvailability.text(credits: credits)).monospacedDigit()
        } label: {
            Text("Limit resets")
            if let credits, credits > 0 {
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
        VStack(alignment: .leading, spacing: SettingsSpacing.tight) {
            HStack(alignment: .firstTextBaseline) {
                Text(title)
                    .font(SettingsTypography.heading)
                Spacer()
                Text("\(Int(window.leftPercent.rounded()))% left")
                    .monospacedDigit()
                    .foregroundStyle(window.leftPercent < 10 ? SettingsTypography.criticalInk : Color.primary)
            }
            ProgressView(value: window.leftPercent, total: 100)
                .tint(window.levelColor)
                .accessibilityHidden(true)
            if let resetsAt = window.resetsAt {
                TimelineView(.everyMinute) { context in
                    Text(resetsAt > context.date ? "Resets in \(UsageTime.span(resetsAt.timeIntervalSince(context.date)))" : "Reset time passed. Refresh usage.")
                }
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

extension UsageWindow {
    var levelColor: Color {
        switch leftPercent {
        case ..<10: return .red
        case ..<25: return .orange
        default: return .green
        }
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
