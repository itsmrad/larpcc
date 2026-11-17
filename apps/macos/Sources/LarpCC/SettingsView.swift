import SwiftUI
import LarpModel

enum SettingsPage: String, CaseIterable, Identifiable {
    case general = "General", mcp = "MCP Servers", skills = "Skills"
    case aliases = "Aliases", desktop = "Desktop Apps", about = "About"
    var id: String { rawValue }
    var symbol: String {
        switch self {
        case .general: return "gearshape.fill"
        case .mcp: return "server.rack"
        case .skills: return "sparkles"
        case .aliases: return "terminal.fill"
        case .desktop: return "macwindow"
        case .about: return "info.circle.fill"
        }
    }
    var color: Color {
        switch self {
        case .general: return .gray
        case .mcp: return .indigo
        case .skills: return .purple
        case .aliases: return .orange
        case .desktop: return .blue
        case .about: return .teal
        }
    }
}

private enum Selection: Hashable { case page(SettingsPage), profile(String) }

struct SettingsView: View {
    @ObservedObject var store: AppStore
    @State private var selection: Selection? = .profile("1")
    @State private var search = ""
    @State private var deleting: Profile?

    var body: some View {
        NavigationSplitView {
            VStack(spacing: 0) {
                HStack(spacing: 7) {
                    Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                    TextField("Search settings", text: $search).textFieldStyle(.plain)
                        .accessibilityLabel("Search settings and profiles")
                    if !search.isEmpty {
                        Button { search = "" } label: { Image(systemName: "xmark.circle.fill") }
                            .buttonStyle(.plain).accessibilityLabel("Clear search")
                    }
                }
                .padding(9).background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 9))
                .padding(.horizontal, 14).padding(.vertical, 12)
                List(selection: $selection) {
                    Section {
                        ForEach(SettingsPage.allCases.filter { matches($0.rawValue) }) { page in
                            Label { Text(page.rawValue) } icon: {
                                Image(systemName: page.symbol).font(.system(size: 13, weight: .semibold))
                                    .foregroundStyle(.white).frame(width: 26, height: 26)
                                    .background(page.color.gradient, in: RoundedRectangle(cornerRadius: 6))
                            }
                            .padding(.vertical, 3).tag(Selection.page(page))
                        }
                    }
                    Section {
                        ForEach((store.snapshot?.profiles ?? []).filter { matches($0.name + " " + $0.provider.rawValue) }) { profile in
                            HStack(spacing: 9) {
                                ProviderGlyph(provider: profile.provider, size: 26)
                                Text(profile.name).lineLimit(1)
                                Spacer()
                                Circle().fill(statusColor(profile.readiness)).frame(width: 7, height: 7)
                                    .accessibilityLabel("Sample \(profile.readiness.rawValue)")
                            }
                            .padding(.vertical, 3).tag(Selection.profile(profile.id))
                        }
                    } header: {
                        HStack {
                            Text("Profiles")
                            Spacer()
                            Text("\(store.snapshot?.enabledCount ?? 0) on")
                        }
                    }
                }
                .listStyle(.sidebar)
                VStack(alignment: .leading, spacing: 4) {
                    Label("Sample mode", systemImage: "testtube.2").font(.callout.weight(.medium))
                    Text("Changes stay in memory.").font(.caption).foregroundStyle(.secondary)
                }.frame(maxWidth: .infinity, alignment: .leading).padding(16)
            }
            .navigationSplitViewColumnWidth(min: 240, ideal: 260, max: 280)
        } detail: {
            ScrollView {
                VStack(alignment: .leading, spacing: 22) {
                    if let snapshot = store.snapshot {
                        switch selection {
                        case let .profile(id):
                            if let profile = snapshot.profiles.first(where: { $0.id == id }) {
                                profileDetail(profile, snapshot: snapshot)
                            } else { emptySelection }
                        case let .page(page): pageDetail(page, snapshot: snapshot)
                        case nil: emptySelection
                        }
                    } else {
                        ProgressView("Loading sample profiles…").frame(maxWidth: .infinity)
                    }
                }
                .frame(maxWidth: 760, alignment: .leading).padding(32)
                .frame(maxWidth: .infinity)
            }
            .background(Color(nsColor: .windowBackgroundColor))
            .navigationTitle("LarpCC")
            .toolbar {
                ToolbarItem {
                    Label("Sample mode", systemImage: "testtube.2").foregroundStyle(.secondary)
                }
            }
            .safeAreaInset(edge: .bottom) {
                if let message = store.message {
                    HStack(alignment: .top) {
                        Text(message).font(.callout).frame(maxWidth: .infinity, alignment: .leading)
                        Button { store.message = nil } label: { Image(systemName: "xmark") }
                            .buttonStyle(.plain).accessibilityLabel("Dismiss message")
                    }.padding(14).background(.regularMaterial)
                }
            }
        }
        .disabled(store.busy)
        .task { await store.refresh() }
        .sheet(item: $deleting) { profile in
            VStack(alignment: .leading, spacing: 18) {
                Text("Delete \(profile.name)?").font(.title2.bold())
                Text("This removes the sample profile and its sample assignments from memory. No folder or alias on disk will be removed.")
                    .fixedSize(horizontal: false, vertical: true)
                HStack {
                    Spacer()
                    Button("Cancel") { deleting = nil }.keyboardShortcut(.cancelAction)
                    Button("Delete sample profile", role: .destructive) {
                        store.perform(.delete(profileID: profile.id))
                        deleting = nil
                        selection = .page(.general)
                    }
                }
            }.padding(24).frame(width: 420)
        }
    }

    private var emptySelection: some View {
        ContentUnavailableView("Select a setting or profile", systemImage: "sidebar.left")
    }
    private func matches(_ text: String) -> Bool {
        search.isEmpty || text.localizedCaseInsensitiveContains(search)
    }
    private func statusColor(_ status: Readiness) -> Color {
        switch status { case .ready: return .green; case .attention: return .yellow; case .off: return .gray }
    }
    private func heading(_ title: String, subtitle: String) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(title).font(.system(size: 26, weight: .bold))
            Text(subtitle).foregroundStyle(.secondary)
        }
    }

    private func profileDetail(_ profile: Profile, snapshot: Snapshot) -> some View {
        VStack(alignment: .leading, spacing: 22) {
            Card {
                HStack(spacing: 14) {
                    ProviderGlyph(provider: profile.provider, size: 48)
                    heading(profile.name, subtitle: "\(profile.provider.rawValue) · authentication unknown")
                    Spacer(minLength: 12)
                    Button { Task { await store.refresh() } } label: {
                        Image(systemName: "arrow.clockwise")
                    }.help("Refresh sample profile").accessibilityLabel("Refresh profile")
                    Toggle("CLI enabled", isOn: surfaceBinding(profile, .cli))
                        .labelsHidden().toggleStyle(.switch).help("Enable sample CLI surface")
                }
            }
            Card {
                ValueRow("Provider", value: profile.provider.title)
                Divider()
                ValueRow("Command aliases", value: profile.aliases.isEmpty ? "None" : profile.aliases.joined(separator: ", "))
                Divider()
                ValueRow("Folder", value: profile.folder)
                Divider()
                ValueRow("Version", value: "Not detected · qualification pending")
                Divider()
                ValueRow("Surfaces", value: "CLI \(profile.cliEnabled ? "on" : "off") · Desktop \(profile.desktopEnabled ? "on" : "off")")
            }
            if profile.needsAttention {
                Label("Sample folder or binding needs attention. Authentication is still unknown.", systemImage: "exclamationmark.triangle")
                    .font(.callout).foregroundStyle(.secondary)
            }
            catalogCard("MCP Servers", items: snapshot.servers, profile: profile, kind: .mcp)
            catalogCard("Skills", items: snapshot.skills, profile: profile, kind: .skill)
            Card {
                HStack {
                    Button("Open in Terminal", systemImage: "terminal") {
                        store.perform(.open(profileID: profile.id, surface: .cli))
                    }
                    Button(profile.provider == .claude ? "Open Claude.app" : "Open ChatGPT.app", systemImage: "macwindow") {
                        store.perform(.open(profileID: profile.id, surface: .desktop))
                    }
                    Spacer()
                }
                Text("Sample actions only. Native launches remain unavailable pending qualification.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Button("Delete Profile…", role: .destructive) { deleting = profile }
                .foregroundStyle(.red)
        }
    }

    private func catalogCard(_ title: String, items: [CatalogItem], profile: Profile, kind: CatalogKind) -> some View {
        VStack(alignment: .leading, spacing: 9) {
            Text(title).font(.headline)
            Card {
                ForEach(Array(items.enumerated()), id: \.element.id) { index, item in
                    if index > 0 { Divider() }
                    Toggle(isOn: catalogBinding(item, profile: profile, kind: kind)) {
                        VStack(alignment: .leading, spacing: 3) {
                            Text(item.id)
                            Text(item.subtitle).font(.caption).foregroundStyle(.secondary)
                        }
                    }.toggleStyle(.switch)
                }
                if items.isEmpty { Text("No entries").foregroundStyle(.secondary) }
            }
        }
    }
    private func surfaceBinding(_ profile: Profile, _ surface: Surface) -> Binding<Bool> {
        Binding(get: { surface == .cli ? profile.cliEnabled : profile.desktopEnabled }, set: {
            store.perform(.setSurface(profileID: profile.id, surface: surface, enabled: $0))
        })
    }
    private func catalogBinding(_ item: CatalogItem, profile: Profile, kind: CatalogKind) -> Binding<Bool> {
        Binding(get: { item.profileIDs.contains(profile.id) }, set: {
            store.perform(.setCatalog(kind: kind, itemID: item.id, profileID: profile.id, enabled: $0))
        })
    }

    @ViewBuilder
    private func pageDetail(_ page: SettingsPage, snapshot: Snapshot) -> some View {
        heading(page.rawValue, subtitle: page == .mcp ? "Choose which profiles receive each server definition." : "Local profile settings · sample mode")
        switch page {
        case .general:
            Card {
                ValueRow("Data source", value: "In-memory samples")
                Divider()
                ValueRow("Profiles enabled", value: "\(snapshot.enabledCount) of \(snapshot.profiles.count)")
                Divider()
                ValueRow("Authentication", value: "Unknown · never observed")
            }
            Card {
                Text("Your profiles, deliberately separate.").font(.headline)
                Text("Each profile has its own CLI and desktop folders. Aliases can use explicit custom names. Launch qualification remains pending.")
                    .foregroundStyle(.secondary)
                Text("All controls here edit sample data only. Restarting restores the samples.")
                    .font(.callout).foregroundStyle(.secondary)
            }
        case .mcp:
            matrix(snapshot.servers, snapshot: snapshot, kind: .mcp)
            Card {
                Toggle("Sync new servers to all profiles", isOn: Binding(
                    get: { snapshot.syncNewServers },
                    set: { store.perform(.setSyncNewServers($0)) }
                )).toggleStyle(.switch)
                Text("Sample preference only. Server definitions are never executed.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        case .skills:
            matrix(snapshot.skills, snapshot: snapshot, kind: .skill)
            Text("Shared definitions with explicit per-profile assignments. Samples never read or copy skill files.")
                .font(.callout).foregroundStyle(.secondary)
        case .aliases:
            Card {
                ForEach(snapshot.profiles) { profile in
                    ValueRow(profile.name, value: profile.aliases.isEmpty ? "No aliases" : profile.aliases.joined(separator: ", "))
                    if profile.id != snapshot.profiles.last?.id { Divider() }
                }
                if snapshot.profiles.isEmpty { Text("No profiles") }
            }
            Text("Aliases shown are examples. No launchers or shell settings are installed.")
                .font(.callout).foregroundStyle(.secondary)
        case .desktop:
            Card {
                ForEach(snapshot.profiles) { profile in
                    Toggle(isOn: surfaceBinding(profile, .desktop)) {
                        HStack {
                            ProviderGlyph(provider: profile.provider, size: 28)
                            VStack(alignment: .leading, spacing: 3) {
                                Text(profile.name)
                                Text("\(profile.folder)/desktop").font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }.toggleStyle(.switch)
                    if profile.id != snapshot.profiles.last?.id { Divider() }
                }
                if snapshot.profiles.isEmpty { Text("No profiles") }
            }
            Text("Sample desktop switches do not authorize a launch. Official, unmodified provider applications require separate qualification.")
                .font(.callout).foregroundStyle(.secondary)
        case .about:
            Card {
                Text("LarpCC").font(.title2.bold())
                Text("Native profile settings for Larp.")
                Divider()
                ValueRow("Build", value: "Development · unsigned")
                ValueRow("Mode", value: "Sample data only")
                Text("No provider access, authentication inspection, background jobs or network requests.")
                    .font(.callout).foregroundStyle(.secondary)
            }
        }
    }

    private func matrix(_ items: [CatalogItem], snapshot: Snapshot, kind: CatalogKind) -> some View {
        ScrollView(.horizontal) {
            Grid(alignment: .leading, horizontalSpacing: 22, verticalSpacing: 16) {
                GridRow {
                    Text(kind == .mcp ? "Server" : "Skill").font(.caption.weight(.semibold))
                    ForEach(snapshot.profiles) { profile in
                        Text(profile.name).font(.caption.weight(.semibold)).frame(minWidth: 66)
                    }
                }.foregroundStyle(.secondary)
                ForEach(items) { item in
                    GridRow {
                        VStack(alignment: .leading, spacing: 3) {
                            Text(item.id).fontWeight(.medium)
                            Text(item.subtitle).font(.caption).foregroundStyle(.secondary)
                        }.frame(minWidth: 170, alignment: .leading)
                        ForEach(snapshot.profiles) { profile in
                            Toggle("\(item.id) for \(profile.name)", isOn: catalogBinding(item, profile: profile, kind: kind))
                                .labelsHidden().toggleStyle(.checkbox)
                                .frame(minWidth: 66)
                        }
                    }
                }
            }.padding(18)
        }
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(.primary.opacity(0.07)))
    }
}

struct ProviderGlyph: View {
    let provider: Provider
    let size: CGFloat
    var body: some View {
        Image(systemName: provider.symbol)
            .font(.system(size: size * 0.48, weight: .medium))
            .foregroundStyle(provider == .claude ? Color.orange : Color.primary)
            .frame(width: size, height: size)
            .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: size * 0.24))
            .accessibilityLabel(provider.title)
    }
}

struct Card<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 12) { content }
            .padding(16).frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(.primary.opacity(0.07)))
    }
}

struct ValueRow: View {
    let title: String
    let value: String
    init(_ title: String, value: String) { self.title = title; self.value = value }
    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 20) {
            Text(title).frame(width: 118, alignment: .leading)
            Spacer(minLength: 0)
            Text(value).foregroundStyle(.secondary).multilineTextAlignment(.trailing).textSelection(.enabled)
        }.font(.callout).padding(.vertical, 2)
    }
}
