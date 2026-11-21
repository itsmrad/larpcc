import SwiftUI
import LarpModel

// MARK: - General

struct GeneralPage: View {
    @ObservedObject var store: AppStore
    @Binding var appearance: String
    @State private var binding: BindTarget?
    @State private var confirmation: Confirmation?

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
        .formStyle(.grouped)
        .bindSheet($binding, store: store)
        .confirming($confirmation, store: store)
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
    @State private var confirmation: Confirmation?

    private var items: [CatalogItem] { kind == .mcp ? snapshot.servers : snapshot.skills }
    private var noun: String { kind == .mcp ? "server" : "skill" }

    var body: some View {
        Form {
            Section {
                if kind == .mcp {
                    Toggle(isOn: Binding(
                        get: { snapshot.syncNewServers },
                        set: { on in
                            if on {
                                confirmation = Confirmation(title: "Turn on every server for every profile?",
                                    message: "Larp switches every current MCP server on for every active profile, and new servers and profiles start with them on. Turning this off later keeps the switches as they are.",
                                    button: "Turn On", action: .setSyncNewServers(true))
                            } else {
                                store.perform(.setSyncNewServers(false))
                            }
                        }
                    )) {
                        Text("New MCP servers start on for every profile")
                        Text("Turning this on also switches every current server on for every active profile.")
                    }
                }
                LabeledContent {
                    Button("Sync Now") { store.perform(kind == .mcp ? .syncServers : .syncSkills) }
                } label: {
                    Text(kind == .mcp ? "Server files" : "Skill links")
                    Text(kind == .mcp ? "Rewrites each profile's server files from these switches." : "Relinks each profile's skills from these switches.")
                }
            }

            if items.isEmpty {
                Section {
                    LabeledContent {
                        Button(kind == .mcp ? "Add Server…" : "Add Skill…") { store.sheet = kind == .mcp ? .addServer : .addSkill }
                    } label: {
                        Text(kind == .mcp ? "No MCP servers yet" : "No skills yet")
                        Text(kind == .mcp ? "Add a server by its program or URL." : "Add a folder that contains SKILL.md.")
                    }
                }
            }
            // One section per item: a header row, then a switch per profile.
            ForEach(items) { item in
                Section {
                    LabeledContent {
                        HStack(spacing: 10) {
                            Text("\(item.profileIDs.count) of \(snapshot.profiles.count) on")
                                .monospacedDigit()
                            menu(for: item)
                        }
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
                            Label {
                                Text(profile.name).accessibilityLabel("\(profile.name), \(item.id)")
                            } icon: {
                                ProviderGlyph(provider: profile.provider, size: 14)
                            }
                        }
                        .foregroundStyle(profile.readiness == .off ? .secondary : .primary)
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
        .confirming($confirmation, store: store)
    }

    private func menu(for item: CatalogItem) -> some View {
        Menu {
            if kind == .mcp {
                Button("Turn On for All Profiles") { store.perform(.setServerForAll(name: item.id, enabled: true)) }
                Button("Turn Off for All Profiles") { store.perform(.setServerForAll(name: item.id, enabled: false)) }
                Divider()
            }
            Button("Remove…", role: .destructive) {
                confirmation = kind == .mcp
                    ? Confirmation(title: "Remove “\(item.id)”?",
                                   message: "Larp deletes this server definition and its switches, and removes it from every profile's server files.",
                                   button: "Remove Server", action: .removeServer(name: item.id))
                    : Confirmation(title: "Remove “\(item.id)”?",
                                   message: item.profileIDs.isEmpty
                                       ? "Larp deletes this skill from its store."
                                       : "Larp refuses while any profile still uses this skill. Turn it off for every profile first.",
                                   button: "Remove Skill", action: .removeSkill(name: item.id))
            }
        } label: {
            Image(systemName: "ellipsis.circle")
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("\(noun.capitalized) actions")
        .accessibilityLabel("\(item.id) actions")
    }
}

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
        .formStyle(.grouped)
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

// MARK: - Desktop apps

struct DesktopAppsPage: View {
    @ObservedObject var store: AppStore
    let snapshot: Snapshot
    @State private var binding: BindTarget?

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
        .bindSheet($binding, store: store)
    }

    private func appRow(_ provider: Provider) -> some View {
        let bindings = snapshot.profiles.filter { $0.provider == provider }.compactMap(\.desktopBinding)
        let binding = bindings.first { $0.state == .ok } ?? bindings.first
        let state = [binding?.stateTitle ?? "No profiles", binding?.detectedVersion].compactMap { $0 }.joined(separator: " · ")
        return LabeledContent {
            HStack(spacing: 6) {
                StatusDot(color: binding?.stateColor ?? .secondary)
                Text(state)
                Button("Bind…") { self.binding = BindTarget(provider: provider, kind: .app) }
                    .padding(.leading, 6)
            }
        } label: {
            RowLabel(provider.desktopApp, subtitle: binding?.displayPath ?? "Not linked") {
                DesktopAppIcon(provider: provider)
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
