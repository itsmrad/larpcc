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
    private var nameTaken: Bool { rows.contains { $0.alias == trimmed } }
    private var canAdd: Bool {
        LarpNames.isValidAlias(trimmed, shadow: shadow) && !nameTaken && snapshot.profiles.contains { $0.id == newProfileID }
    }

    var body: some View {
        Form {
            Section {
                if rows.isEmpty {
                    Text("No aliases yet.").foregroundStyle(.secondary)
                }
                ForEach(rows) { row in
                    LabeledContent {
                        HStack(spacing: 10) {
                            HStack(spacing: 4) {
                                ProviderGlyph(provider: row.profile.provider, size: 13)
                                Text(row.profile.name)
                            }
                            Button {
                                confirmation = Confirmation(title: "Remove “\(row.alias)”?",
                                    message: "Larp deletes the \(row.alias) command. The profile itself is unchanged.",
                                    button: "Remove Alias", action: .removeAlias(name: row.alias))
                            } label: {
                                Image(systemName: "minus.circle.fill")
                                    .foregroundStyle(.secondary)
                            }
                            .buttonStyle(.borderless)
                            .help("Remove alias \(row.alias)")
                            .accessibilityLabel("Remove alias \(row.alias)")
                        }
                    } label: {
                        Text(row.alias).font(.body.monospaced())
                    }
                }
            } header: {
                Text("Commands")
            } footer: {
                Text("Each alias is a command that starts its profile. LarpCC never edits your shell configuration or PATH.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            Section {
                TextField("Command name", text: $newName, prompt: Text("command-name"))
                    .onSubmit(add)
                Picker("Profile", selection: $newProfileID) {
                    if newProfileID.isEmpty { Text("Choose…").tag("") }
                    ForEach(snapshot.profiles) { Text($0.name).tag($0.id) }
                }
                .onAppear { if newProfileID.isEmpty { newProfileID = snapshot.profiles.first?.id ?? "" } }
                Toggle(isOn: $desktop) {
                    Text("Open the desktop app")
                    Text("The command opens the profile's desktop app instead of its command line.")
                }
                Toggle(isOn: $shadow) {
                    Text("Allow claude or codex")
                    Text("Lets the alias use a provider's own command name, ahead of it on PATH. Existing commands are never overwritten.")
                }
                HStack {
                    if let error {
                        Label(error, systemImage: "exclamationmark.triangle.fill")
                            .symbolRenderingMode(.multicolor)
                            .font(.callout)
                    } else if !trimmed.isEmpty && !canAdd {
                        Text(nameTaken ? "That alias already exists."
                             : LarpNames.isValidAlias(trimmed, shadow: shadow) ? "Choose a profile."
                             : LarpNames.isValidAlias(trimmed, shadow: true) ? "Turn on “Allow claude or codex” to use this name."
                             : "Use lowercase letters, digits, - or _, up to 32 characters.")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Add Alias", action: add)
                        .disabled(!canAdd)
                }
            } header: {
                Text("New Alias")
            }
        }
        .themedFormStyle()
        .confirming($confirmation, store: store)
    }

    private func add() {
        guard canAdd else { return }
        error = nil
        store.perform(.addAlias(name: trimmed, profileID: newProfileID, desktop: desktop, shadow: shadow)) { failure in
            if let failure { error = failure } else { newName = ""; shadow = false; desktop = false }
        }
    }
}
