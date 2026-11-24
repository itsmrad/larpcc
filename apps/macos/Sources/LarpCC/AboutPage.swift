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
                    VStack(alignment: .leading, spacing: 2) {
                        Text("LarpCC").font(.system(size: 15, weight: .semibold))
                        Text("Separate Claude and Codex profiles on one Mac.")
                            .foregroundStyle(.secondary)
                    }
                }
                .padding(.vertical, 4)
                ValueRow("Version", value: "\(version) · unsigned development build")
                ValueRow("Data source", value: store.isFixture ? "Sample data" : "Your larp executable")
            }
            Section("Privacy") {
                Label { Text("Runs only the larp executable you choose") } icon: { Image(systemName: "terminal").frame(width: 20) }
                Label { Text("Makes no network requests") } icon: { Image(systemName: "wifi.slash").frame(width: 20) }
                Label { Text("Never reads sign-in tokens or keychain items") } icon: { Image(systemName: "lock.shield").frame(width: 20) }
                Label { Text("No background activity or login items") } icon: { Image(systemName: "moon.zzz").frame(width: 20) }
            }
        }
        .themedFormStyle()
    }
}
