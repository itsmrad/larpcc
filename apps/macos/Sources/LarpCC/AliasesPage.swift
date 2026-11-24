import SwiftUI
import LarpModel

// MARK: - Aliases

struct AliasesPage: View {
    @ObservedObject var store: AppStore
    let snapshot: Snapshot
    @State private var newName = ""
    @State private var newProfileID = ""
    @State private var desktop = false
    @State private var shadow = false
    @State private var error: String?
    @State private var confirmation: Confirmation?

    private struct AliasRow: Identifiable {
        let alias: String
        let profile: Profile
        var id: String { alias }
    }
    private var rows: [AliasRow] {
        snapshot.profiles.flatMap { profile in profile.aliases.map { AliasRow(alias: $0, profile: profile) } }
            .sorted { $0.alias < $1.alias }
    }
    private var trimmed: String { newName.trimmingCharacters(in: .whitespaces) }
    private var addDisabledReason: String? {
        if snapshot.profiles.isEmpty { return "Create a profile before adding an alias." }
        if trimmed.isEmpty { return "Enter a command name." }
        if rows.contains(where: { $0.alias == trimmed }) { return "That alias already exists." }
        if !LarpNames.isValidAlias(trimmed, shadow: true) {
            return "Start with a lowercase letter. Use up to 32 lowercase letters, digits, hyphens or underscores. Reserved names are unavailable."
        }
        if !LarpNames.isValidAlias(trimmed, shadow: shadow) { return "Turn on “Allow shadowing” to use this name." }
        if !snapshot.profiles.contains(where: { $0.id == newProfileID }) { return "Choose a profile." }
        return nil
    }

    var body: some View {
        Form {
            Section {
                if rows.isEmpty {
                    UnavailablePane(title: "No aliases yet", symbol: "terminal",
                                    message: "Add an alias to give a profile its own command.")
                }
                ForEach(rows) { row in
                    LabeledContent {
                        HStack(spacing: SettingsSpacing.control) {
                            HStack(spacing: SettingsSpacing.tight) {
                                ProviderGlyph(provider: row.profile.provider, size: 13)
                                Text(row.profile.name).foregroundStyle(SettingsTypography.supportingInk)
                            }
                            Button("Remove") {
                                confirmation = Confirmation(title: "Remove “\(row.alias)”?",
                                    message: "Larp removes the command that points to “\(row.profile.name)”.",
                                    button: "Remove Alias", action: .removeAlias(name: row.alias))
                            }
                            .buttonStyle(.bordered)
                            .accessibilityLabel("Remove alias \(row.alias)")
                        }
                    } label: {
                        Text(row.alias).font(.body.monospaced())
                    }
                }
            } header: {
                Text("Commands")
            } footer: {
                Text("Aliases point to profiles. To run an alias by name, put Larp’s bin directory on your PATH.")
                    .font(.footnote)
                    .foregroundStyle(SettingsTypography.supportingInk)
            }

            Section {
                LabeledContent("Command name") {
                    TextField("Command name", text: $newName,
                              prompt: Text("command-name").foregroundColor(SettingsTypography.supportingInk))
                        .labelsHidden()
                        .textFieldStyle(.roundedBorder)
                        .multilineTextAlignment(.leading)
                        .font(.body.monospaced())
                        .frame(maxWidth: 280)
                        .onSubmit(add)
                        .onChange(of: newName) { error = nil }
                }
                Picker("Profile", selection: $newProfileID) {
                    if newProfileID.isEmpty { Text("Choose…").tag("") }
                    ForEach(snapshot.profiles) { Text($0.name).tag($0.id) }
                }
                .onAppear { if newProfileID.isEmpty { newProfileID = snapshot.profiles.first?.id ?? "" } }
                .onChange(of: snapshot.profiles.map(\.id)) { _, ids in
                    if !ids.contains(newProfileID) { newProfileID = "" }
                }
                Toggle(isOn: $desktop) {
                    Text("Target the desktop app")
                    Text("Launching requires an enabled desktop surface and a qualified app.")
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
                Toggle(isOn: $shadow) {
                    Text("Allow shadowing")
                    Text("Allows names like claude or codex. PATH order decides which command runs.")
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
                HStack {
                    if let error {
                        Label(error, systemImage: "exclamationmark.triangle.fill")
                            .font(SettingsTypography.supporting)
                            .foregroundStyle(SettingsTypography.criticalInk)
                    } else if let addDisabledReason, !trimmed.isEmpty || snapshot.profiles.isEmpty {
                        Text(addDisabledReason)
                            .font(SettingsTypography.supporting)
                            .foregroundStyle(SettingsTypography.supportingInk)
                    }
                    Spacer()
                    Button("Add Alias", action: add)
                        .disabledReason(addDisabledReason)
                }
            } header: {
                Text("New Alias")
            }
        }
        .themedFormStyle()
        .confirming($confirmation, store: store)
    }

    private func add() {
        guard addDisabledReason == nil else { return }
        error = nil
        store.perform(.addAlias(name: trimmed, profileID: newProfileID, desktop: desktop, shadow: shadow)) { failure in
            if let failure { error = failure } else { newName = ""; shadow = false; desktop = false }
        }
    }
}
