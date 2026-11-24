import SwiftUI
import LarpModel

// MARK: - General

struct GeneralPage: View {
    @ObservedObject var store: AppStore
    @Binding var appearance: String
    @AppStorage(ThemeChoice.key) private var theme = ThemeChoice.automatic.rawValue
    @State private var binding: BindTarget?
    @State private var confirmation: Confirmation?

    private var appearanceOverrideReason: String? {
        AppearanceMode.launchOverride == nil ? nil
            : "Quit and relaunch without --appearance to change this setting."
    }

    private var themeOverrideReason: String? {
        ThemePreference.launchOverride == nil ? nil
            : "Quit and relaunch without --theme to change this setting."
    }

    private var themeNote: String {
        if let themeOverrideReason { return themeOverrideReason }
        let native = Theme.native(osMajor: ThemePreference.osMajor).title
        return "Automatic matches this Mac (\(native)). Restyles the sidebar, forms and menu bar panel; "
            + "window frames, title bars and the menu bar follow macOS."
    }

    private var executablePlaceholder: String {
        if store.isFixture { return "None — showing sample data that resets on restart." }
        return store.busy ? "Checking the selected executable…" : "No executable selected."
    }

    var body: some View {
        Form {
            Section("System") {
                Picker(selection: Binding(
                    get: { AppearanceMode.launchOverride?.rawValue ?? appearance },
                    set: { appearance = $0 }
                )) {
                    ForEach(AppearanceMode.allCases) { Text($0.title).tag($0.rawValue) }
                } label: {
                    Text("Appearance")
                    if let appearanceOverrideReason {
                        Text(appearanceOverrideReason)
                            .foregroundStyle(SettingsTypography.supportingInk)
                    }
                }
                .disabledReason(appearanceOverrideReason)
                Picker(selection: Binding(
                    get: { ThemePreference.launchOverride?.rawValue ?? ThemeChoice(stored: theme).rawValue },
                    set: { theme = $0 }
                )) {
                    ForEach(ThemeChoice.allCases, id: \.self) { choice in
                        let available = choice.isAvailable(osMajor: ThemePreference.osMajor)
                        Text(available ? choice.title : "\(choice.title) — Requires macOS 26")
                            .tag(choice.rawValue)
                            .selectionDisabled(!available)
                    }
                } label: {
                    Text("Theme")
                    Text(themeNote)
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
                .disabledReason(themeOverrideReason)
            }

            UsageSettingsSection(store: store)
            MenuBarSettingsSection(store: store)

            Section("Command-line tool") {
                LabeledContent {
                    HStack(spacing: SettingsSpacing.inline) {
                        if !store.isFixture || store.snapshot == nil {
                            Button("Use Sample Data") { store.useFixture() }
                        }
                        Button("Choose…") { store.chooseExecutable() }
                    }
                } label: {
                    Text("larp executable")
                    Text(store.selectedPath ?? executablePlaceholder)
                        .foregroundStyle(SettingsTypography.supportingInk)
                        .lineLimit(2)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                }
                LabeledContent {
                    Text(dataFolder)
                        .foregroundStyle(SettingsTypography.supportingInk)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                        .help(dataFolder)
                } label: {
                    Text("Data folder")
                    Text("Managed by larp. Read-only here.")
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
            }

            Section {
                ForEach(Provider.allCases, id: \.self) { provider in
                    LabeledContent {
                        Button("Bind…") { binding = BindTarget(provider: provider, kind: .tool) }
                            .disabledReason(store.snapshot == nil ? "Load sample data or choose a larp executable to manage provider tools." : nil)
                    } label: {
                        Text("\(provider.title) executable")
                        Text("The \(provider.rawValue) program larp runs for \(provider.title) profiles.")
                            .foregroundStyle(SettingsTypography.supportingInk)
                    }
                }
            } header: {
                Text("Provider Tools")
            }

            Section {
                LabeledContent {
                    Button("Migrate…") {
                        confirmation = Confirmation(title: "Migrate profile folders?",
                            message: "Larp moves profiles to its current folder layout. Sessions should be closed first.",
                            button: "Migrate", action: .migrate)
                    }
                    .disabledReason(store.snapshot == nil ? "Load sample data or choose a larp executable before migrating profiles." : nil)
                } label: {
                    Text("Folder layout")
                    Text("Moves older profiles to larp's current layout.")
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
            } header: {
                Text("Maintenance")
            }
        }
        .themedFormStyle()
        .bindSheet($binding, store: store)
        .confirming($confirmation, store: store)
    }

    private var dataFolder: String {
        if store.isFixture { return "None (sample data)" }
        return store.snapshot?.dataFolder ?? "Not reported"
    }
}
