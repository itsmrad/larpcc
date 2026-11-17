import SwiftUI
import LarpModel

// MARK: - General

struct GeneralPage: View {
    @ObservedObject var store: AppStore
    @Binding var appearance: String
    @AppStorage(PreferenceKey.terminal) private var terminal = TerminalChoice.terminal.rawValue

    var body: some View {
        Form {
            Section("System") {
                Picker(selection: $terminal) {
                    ForEach(TerminalChoice.allCases) { Text($0.rawValue).tag($0.rawValue) }
                } label: {
                    Text("Default terminal")
                    Text("Terminal used by Open in Terminal.")
                }
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

            Section("Larp") {
                LabeledContent {
                    HStack(spacing: 8) {
                        Button("Choose…") { store.chooseExecutable() }
                        Button("Use Sample Data") { store.useFixture() }
                            .disabled(store.isFixture && store.snapshot != nil)
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
            Section {
                if items.isEmpty {
                    Text(kind == .mcp ? "No MCP servers in the catalog." : "No skills in the catalog.")
                        .foregroundStyle(.secondary)
                }
                ForEach(items) { item in
                    LabeledContent {
                        Text("\(item.profileIDs.count) of \(snapshot.profiles.count) on")
                            .monospacedDigit()
                    } label: {
                        RowLabel(item.id, subtitle: item.subtitle) {
                            IconTile(symbol: kind == .mcp ? "server.rack" : "sparkles",
                                     color: kind == .mcp ? .indigo : .pink)
                        }
                    }
                }
            } header: {
                Text(kind == .mcp ? "Catalog" : "Library")
            }

            if !items.isEmpty && !snapshot.profiles.isEmpty {
                Section {
                    MatrixGrid(store: store, snapshot: snapshot, items: items, kind: kind)
                } header: {
                    Text("Profiles")
                } footer: {
                    Text(kind == .mcp
                         ? "Switches choose which profiles receive each server definition. LarpCC never starts a server."
                         : "Switches choose which profiles can use each skill.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
    }
}

/// Items × profiles grid of switches.
private struct MatrixGrid: View {
    @ObservedObject var store: AppStore
    let snapshot: Snapshot
    let items: [CatalogItem]
    let kind: CatalogKind
    private let column: CGFloat = 76

    var body: some View {
        ViewThatFits(in: .horizontal) {
            grid
            ScrollView(.horizontal) { grid.padding(.bottom, 6) }
        }
    }

    private var grid: some View {
        Grid(alignment: .leading, horizontalSpacing: 0, verticalSpacing: 0) {
            GridRow(alignment: .bottom) {
                Text(kind == .mcp ? "Server" : "Skill")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(minWidth: 150, alignment: .leading)
                    .padding(.bottom, 6)
                ForEach(snapshot.profiles) { profile in
                    VStack(spacing: 3) {
                        ProviderGlyph(provider: profile.provider, size: 12)
                        Text(profile.name)
                            .lineLimit(1)
                            .truncationMode(.tail)
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(width: column)
                    .padding(.bottom, 6)
                    .help(profile.name)
                }
            }

            ForEach(items) { item in
                Divider().gridCellUnsizedAxes(.horizontal)
                GridRow {
                    Text(item.id)
                        .lineLimit(1)
                        .frame(minWidth: 150, alignment: .leading)
                        .padding(.vertical, 7)
                    ForEach(snapshot.profiles) { profile in
                        Toggle("\(item.id) for \(profile.name)", isOn: Binding(
                            get: { item.profileIDs.contains(profile.id) },
                            set: { store.perform(.setCatalog(kind: kind, itemID: item.id, profileID: profile.id, enabled: $0)) }
                        ))
                        .labelsHidden()
                        .toggleStyle(.switch)
                        .controlSize(.mini)
                        .frame(width: column)
                    }
                }
            }
        }
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
            } header: {
                Text("Commands")
            } footer: {
                Text("Each alias is a command that starts its profile. LarpCC never edits your shell configuration or PATH.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            Section("Add Alias") {
                TextField(text: $newName, prompt: Text("claude-work")) {
                    Text("Command name")
                    Text("Lowercase letters, digits, - or _.")
                }
                .onSubmit(add)
                Picker("Profile", selection: $newProfileID) {
                    if newProfileID.isEmpty { Text("Choose…").tag("") }
                    ForEach(snapshot.profiles) { Text($0.name).tag($0.id) }
                }
                .onAppear { if newProfileID.isEmpty { newProfileID = snapshot.profiles.first?.id ?? "" } }
                HStack {
                    if !trimmed.isEmpty && !canAdd {
                        Text(nameTaken ? "That alias already exists." : (LarpNames.isValidAlias(trimmed) ? "Choose a profile." : "Use a new lowercase name, up to 32 characters. claude and codex are reserved."))
                            .font(.callout)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Add Alias", action: add)
                        .disabled(!canAdd)
                }
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
                        Button("Open") { store.perform(.open(profileID: profile.id, surface: .desktop)) }
                            .disabled(!profile.desktopEnabled)
                    } label: {
                        RowLabel(profile.name, subtitle: profile.desktopEnabled ? "\(profile.provider.desktopApp) · own data folder" : "Desktop app off for this profile") {
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
        let detail = [binding?.versionTitle ?? "Not detected", binding?.displayPath].compactMap { $0 }.joined(separator: " · ")
        return LabeledContent {
            HStack(spacing: 6) {
                StatusDot(color: binding?.stateColor ?? .secondary)
                Text(binding?.stateTitle ?? "No profiles")
            }
        } label: {
            RowLabel(provider.desktopApp, subtitle: detail) {
                IconTile(symbol: "macwindow", color: provider == .claude ? .orange : .teal)
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
                Label("Runs only the larp executable you choose", systemImage: "terminal")
                Label("Makes no network requests", systemImage: "wifi.slash")
                Label("Never reads sign-in tokens or keychain items", systemImage: "lock.shield")
                Label("No background activity or login items", systemImage: "moon.zzz")
            }
        }
        .formStyle(.grouped)
    }
}
