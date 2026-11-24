import SwiftUI
import LarpModel

// MARK: - About

struct AboutPage: View {
    @ObservedObject var store: AppStore
    private var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "development"
    }

    var body: some View {
        Form {
            Section {
                HStack(spacing: 14) {
                    IconTile(symbol: "person.2.fill", color: .accentColor, size: 44)
                    VStack(alignment: .leading, spacing: SettingsSpacing.tight) {
                        Text("LarpCC").font(.system(size: 15, weight: .semibold))
                        Text("Separate Claude and Codex profiles on one Mac.")
                            .foregroundStyle(SettingsTypography.supportingInk)
                    }
                }
                .padding(.vertical, SettingsSpacing.tight)
                LabeledContent("Version") {
                    Text("\(version) · unsigned development build")
                        .foregroundStyle(SettingsTypography.supportingInk)
                        .textSelection(.enabled)
                }
                LabeledContent("Data source") {
                    Text(store.isFixture ? "Sample data" : "Your larp executable")
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
            }
            Section("Privacy") {
                privacyRow("Local usage is the default", symbol: "wifi.slash",
                           detail: "Reads only usage data already saved on this Mac. No network requests or sign-in token reads in Local mode.")
                privacyRow("Live usage is opt-in and off by default", symbol: "lock.shield",
                           detail: "When enabled and used, Larp reads a profile's sign-in token in memory for one usage request to that provider's endpoint. It never stores the token.")
                privacyRow("Live usage is not available in this build", symbol: "exclamationmark.triangle",
                           detail: "Unverified against real accounts. Live usage may violate provider terms and could lead to account restrictions.")
                privacyRow("You choose the larp executable", symbol: "terminal",
                           detail: "Profile operations run through your selected executable. Sample data stays in memory and resets on restart.")
                privacyRow("Menu bar activity", symbol: "menubar.rectangle",
                           detail: "LarpCC can stay active in the menu bar when Settings is closed. It does not install a login item.")
            }
        }
        .themedFormStyle()
    }

    private func privacyRow(_ title: String, symbol: String, detail: String) -> some View {
        Label {
            VStack(alignment: .leading, spacing: SettingsSpacing.tight) {
                Text(title)
                Text(detail)
                    .font(SettingsTypography.supporting)
                    .foregroundStyle(SettingsTypography.supportingInk)
                    .fixedSize(horizontal: false, vertical: true)
            }
        } icon: {
            Image(systemName: symbol)
                .frame(width: SettingsSpacing.page)
                .accessibilityHidden(true)
        }
        .accessibilityElement(children: .combine)
    }
}
