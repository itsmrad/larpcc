import SwiftUI
import LarpModel

// MARK: - General

struct GeneralPage: View {
    @ObservedObject var store: AppStore
    @Binding var appearance: String

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
                    Text(store.selectedPath ?? (store.isFixture ? "None — showing sample data that resets on restart." : "Checking the selected executable…"))
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

            Section("MCP Servers") {
                Toggle(isOn: Binding(
                    get: { store.snapshot?.syncNewServers ?? false },
                    set: { store.perform(.setSyncNewServers($0)) }
                )) {
                    Text("New MCP servers start on for every profile")
                    Text("When on, servers added to the catalog are switched on for every profile.")
                }
                .disabled(store.snapshot == nil)
            }
        }
        .formStyle(.grouped)
    }

    private var dataFolder: String {
        if store.isFixture { return "None (sample data)" }
        return store.snapshot?.dataFolder ?? "Not reported"
    }
}

// MARK: - MCP servers and skills

struct CatalogPage: View {
    @ObservedObject var store: AppStore
    let snapshot: Snapshot
    let kind: CatalogKind

    private var items: [CatalogItem] { kind == .mcp ? snapshot.servers : snapshot.skills }

    var body: some View {
        Form {
            if items.isEmpty {
                Section {
                    Text(kind == .mcp ? "No MCP servers in the catalog." : "No skills in the catalog.")
                        .foregroundStyle(.secondary)
                }
            }
            // One section per item: a header row, then a switch per profile.
            ForEach(items) { item in
                Section {
                    LabeledContent {
                        Text("\(item.profileIDs.count) of \(snapshot.profiles.count) on")
                            .monospacedDigit()
                    } label: {
                        RowLabel(item.id, subtitle: item.subtitle) {
                            IconTile(symbol: kind == .mcp ? "server.rack" : "sparkles",
                                     color: kind == .mcp ? .indigo : .pink)
                        }
                    }
                    ForEach(snapshot.profiles) { profile in
                        Toggle(isOn: Binding(
                            get: { item.profileIDs.contains(profile.id) },
                            set: { store.perform(.setCatalog(kind: kind, itemID: item.id, profileID: profile.id, enabled: $0)) }
                        )) {
                            Label { Text(profile.name) } icon: { ProviderGlyph(provider: profile.provider, size: 14) }
                        }
                        .foregroundStyle(profile.readiness == .off ? .secondary : .primary)
                        .accessibilityLabel("\(item.id) for \(profile.name)")
                    }
                } footer: {
                    if item.id == items.last?.id {
                        Text(kind == .mcp
                             ? "Switches choose which profiles receive each server definition. LarpCC never starts a server."
                             : "Switches choose which profiles can use each skill.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .formStyle(.grouped)
    }
}

// MARK: - Aliases

struct AliasesPage: View {
    @ObservedObject var store: AppStore
    let snapshot: Snapshot
    @State private var newName = ""
    @State private var newProfileID = ""

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
        LarpNames.isValidAlias(trimmed) && !nameTaken && snapshot.profiles.contains { $0.id == newProfileID }
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
                            Button { store.perform(.removeAlias(name: row.alias)) } label: {
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
                HStack(spacing: 8) {
                    TextField("Command name", text: $newName, prompt: Text("command-name"))
                        .labelsHidden()
                        .textFieldStyle(.roundedBorder)
                        .font(.body.monospaced())
                        .frame(maxWidth: 200)
                        .onSubmit(add)
                    Picker("Profile", selection: $newProfileID) {
                        if newProfileID.isEmpty { Text("Choose…").tag("") }
                        ForEach(snapshot.profiles) { Text($0.name).tag($0.id) }
                    }
                    .labelsHidden()
                    .fixedSize()
                    .onAppear { if newProfileID.isEmpty { newProfileID = snapshot.profiles.first?.id ?? "" } }
                    Spacer(minLength: 0)
                    Button("Add", action: add)
                        .disabled(!canAdd)
                }
            } header: {
                Text("Commands")
            } footer: {
                Text(!trimmed.isEmpty && !canAdd
                     ? (nameTaken ? "That alias already exists." : (LarpNames.isValidAlias(trimmed) ? "Choose a profile." : "Use a new lowercase name, up to 32 characters. claude and codex are reserved."))
                     : "Each alias is a command that starts its profile: lowercase letters, digits, - or _. LarpCC never edits your shell configuration or PATH.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }

    private func add() {
        guard canAdd else { return }
        store.perform(.addAlias(name: trimmed, profileID: newProfileID))
        newName = ""
    }
}

// MARK: - Desktop apps

struct DesktopAppsPage: View {
    @ObservedObject var store: AppStore
    let snapshot: Snapshot

    var body: some View {
        Form {
            Section("Applications") {
                ForEach(Provider.allCases, id: \.self) { provider in
                    appRow(provider)
                }
            }
            Section {
                if snapshot.profiles.isEmpty {
                    Text("No profiles.").foregroundStyle(.secondary)
                }
                ForEach(snapshot.profiles) { profile in
                    LabeledContent {
                        HStack(spacing: 10) {
                            if profile.desktopEnabled {
                                Button("Open") { store.perform(.open(profileID: profile.id, surface: .desktop)) }
                                    .controlSize(.small)
                            }
                            Toggle("Desktop app for \(profile.name)", isOn: Binding(
                                get: { profile.desktopEnabled },
                                set: { store.perform(.setSurface(profileID: profile.id, surface: .desktop, enabled: $0)) }
                            ))
                            .labelsHidden()
                            .toggleStyle(.switch)
                        }
                    } label: {
                        RowLabel(profile.name, subtitle: profile.desktopEnabled ? "Own \(profile.provider.desktopApp) data folder" : "Off") {
                            ProviderGlyph(provider: profile.provider)
                        }
                    }
                }
            } header: {
                Text("Profiles")
            } footer: {
                Text("Open requests go through larp. Launching stays unavailable until qualification is approved.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }

    private func appRow(_ provider: Provider) -> some View {
        let bindings = snapshot.profiles.filter { $0.provider == provider }.compactMap(\.desktopBinding)
        let binding = bindings.first { $0.state == .ok } ?? bindings.first
        let state = [binding?.stateTitle ?? "No profiles", binding?.detectedVersion].compactMap { $0 }.joined(separator: " · ")
        return LabeledContent {
            HStack(spacing: 6) {
                StatusDot(color: binding?.stateColor ?? .secondary)
                Text(state)
            }
        } label: {
            RowLabel(provider.desktopApp, subtitle: binding?.displayPath ?? "Not linked") {
                ProviderTile(provider: provider)
            }
        }
    }
}

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
        .formStyle(.grouped)
    }
}
