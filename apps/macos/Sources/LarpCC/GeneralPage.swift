import SwiftUI
import LarpModel

// MARK: - General

struct GeneralPage: View {
    @ObservedObject var store: AppStore
    @Binding var appearance: String
    @AppStorage(ThemeChoice.key) private var theme = ThemeChoice.automatic.rawValue
    @State private var binding: BindTarget?
    @State private var confirmation: Confirmation?

    private var themeNote: String {
        if ThemePreference.launchOverride != nil { return "Set by a launch option for this session." }
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
                Picker(selection: $appearance) {
                    ForEach(AppearanceMode.allCases) { Text($0.title).tag($0.rawValue) }
                } label: {
                    Text("Appearance")
                    if AppearanceMode.launchOverride != nil {
                        Text("Set by a launch option for this session.")
                    }
                }
                .disabled(AppearanceMode.launchOverride != nil)
                Picker(selection: Binding(get: { ThemeChoice(stored: theme).rawValue }, set: { theme = $0 })) {
                    ForEach(ThemeChoice.allCases, id: \.self) { choice in
                        let available = choice.isAvailable(osMajor: ThemePreference.osMajor)
                        Text(available ? choice.title : "\(choice.title) — Requires macOS 26")
                            .tag(choice.rawValue)
                            .selectionDisabled(!available)
                    }
                } label: {
                    Text("Theme")
                    Text(themeNote)
                }
                .disabled(ThemePreference.launchOverride != nil)
            }

            UsageSettingsSection(store: store)
            MenuBarSettingsSection(store: store)

            Section("Command-line tool") {
                LabeledContent {
                    HStack(spacing: 8) {
                        if !store.isFixture || store.snapshot == nil {
                            Button("Use Sample Data") { store.useFixture() }
                        }
                        Button("Choose…") { store.chooseExecutable() }
                    }
                } label: {
                    Text("larp executable")
                    Text(store.selectedPath ?? executablePlaceholder)
                        .lineLimit(2)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                }
                LabeledContent {
                    Text(dataFolder)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                        .help(dataFolder)
                } label: {
                    Text("Data folder")
                    Text("Managed by larp. Read-only here.")
                }
            }

            Section {
                ForEach(Provider.allCases, id: \.self) { provider in
                    LabeledContent {
                        Button("Bind…") { binding = BindTarget(provider: provider, kind: .tool) }
                    } label: {
                        Text("\(provider.title) executable")
                        Text("The \(provider.rawValue) program larp runs for \(provider.title) profiles.")
                    }
                }
            } header: {
                Text("Provider Tools")
            }
            .disabled(store.snapshot == nil)

            Section {
                LabeledContent {
                    Button("Migrate…") {
                        confirmation = Confirmation(title: "Migrate profile folders?",
                            message: "Larp moves profiles to its current folder layout. Sessions should be closed first.",
                            button: "Migrate", action: .migrate)
                    }
                } label: {
                    Text("Folder layout")
                    Text("Moves older profiles to larp's current layout.")
                }
            } header: {
                Text("Maintenance")
            }
            .disabled(store.snapshot == nil)
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
