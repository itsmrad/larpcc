import SwiftUI
import LarpModel

// MARK: - General › Usage

struct UsageSettingsSection: View {
    @ObservedObject var store: AppStore

    var body: some View {
        Section("Usage") {
            LabeledContent {
                Text("Local")
            } label: {
                Text("Usage source")
                Text("Local uses only data the official apps already saved on this Mac.")
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
            LabeledContent("Live usage", value: "Disabled · Unverified")
                .foregroundStyle(SettingsTypography.supportingInk)
            if !store.usageLoading, let error = store.usageError {
                Text(error).foregroundStyle(SettingsTypography.supportingInk)
            }
        }
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
                    RowLabel("Usage unavailable",
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
