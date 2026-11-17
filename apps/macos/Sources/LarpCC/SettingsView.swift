import SwiftUI
import LarpModel

enum SettingsPage: String, CaseIterable, Identifiable {
    case general, mcp, skills, aliases, desktop, about
    var id: String { rawValue }
    var title: String {
        switch self {
        case .general: return "General"
        case .mcp: return "MCP Servers"
        case .skills: return "Skills"
        case .aliases: return "Aliases"
        case .desktop: return "Desktop Apps"
        case .about: return "About"
        }
    }
    var symbol: String {
        switch self {
        case .general: return "gearshape.fill"
        case .mcp: return "server.rack"
        case .skills: return "sparkles"
        case .aliases: return "terminal.fill"
        case .desktop: return "macwindow"
        case .about: return "info"
        }
    }
    var color: Color {
        switch self {
        case .general: return .gray
        case .mcp: return .indigo
        case .skills: return .pink
        case .aliases: return .orange
        case .desktop: return .blue
        case .about: return Color(white: 0.35)
        }
    }
    /// Extra words matched by the sidebar search.
    var keywords: String {
        switch self {
        case .general: return "terminal appearance dark light executable larp data folder sample"
        case .mcp: return "servers matrix catalog"
        case .skills: return "matrix catalog"
        case .aliases: return "commands launcher name"
        case .desktop: return "claude.app chatgpt.app version open"
        case .about: return "version privacy"
        }
    }
}

enum Selection: Hashable {
    case page(SettingsPage), profile(String)

    /// `--page=general|mcp|skills|aliases|desktop|about|profile-<id>` for fixture screenshots.
    static var launchValue: Selection? {
        guard let arg = CommandLine.arguments.first(where: { $0.hasPrefix("--page=") }) else { return nil }
        let value = String(arg.dropFirst("--page=".count))
        if value.hasPrefix("profile-") { return .profile(String(value.dropFirst("profile-".count))) }
        return SettingsPage(rawValue: value).map(Selection.page)
    }
}

struct SettingsView: View {
    @ObservedObject var store: AppStore
    @State private var selection: Selection? = Selection.launchValue ?? .page(.general)
    @State private var backStack: [Selection] = []
    @State private var forwardStack: [Selection] = []
    @State private var historyMove = false
    @State private var search = ""
    @AppStorage(PreferenceKey.appearance) private var appearance = AppearanceMode.system.rawValue

    var body: some View {
        NavigationSplitView {
            sidebar
                .navigationSplitViewColumnWidth(min: 210, ideal: 230, max: 300)
        } detail: {
            detail
                // Minimum on the detail column, not the split view: a split-view
                // minimum makes the detail snap instead of resizing as the sidebar slides in.
                .frame(minWidth: 460, minHeight: 480)
                .navigationTitle(title)
                .toolbar { toolbar }
                .safeAreaInset(edge: .bottom, spacing: 0) { messageBar }
        }
        .task { store.startRefresh() }
        .onAppear { (AppearanceMode(rawValue: appearance) ?? .system).apply() }
        .onChange(of: appearance) { _, value in (AppearanceMode(rawValue: value) ?? .system).apply() }
        .onChange(of: selection) { old, _ in
            if historyMove { historyMove = false; return }
            if let old { backStack.append(old) }
            forwardStack.removeAll()
        }
    }

    // MARK: Sidebar

    private var sidebar: some View {
        List(selection: $selection) {
            Section {
                ForEach(SettingsPage.allCases.filter { matches($0.title + " " + $0.keywords) }) { page in
                    Label {
                        Text(page.title)
                    } icon: {
                        IconTile(symbol: page.symbol, color: page.color)
                    }
                    .tag(Selection.page(page))
                }
            }
            if let snapshot = store.snapshot {
                Section {
                    ForEach(snapshot.profiles.filter { matches($0.name + " " + $0.provider.title + " " + $0.aliases.joined(separator: " ")) }) { profile in
                        ProfileSidebarRow(profile: profile)
                            .tag(Selection.profile(profile.id))
                    }
                } header: {
                    HStack {
                        Text("Profiles")
                        Spacer()
                        Text("\(snapshot.enabledCount) on").monospacedDigit()
                    }
                    .padding(.trailing, 10)
                }
            }
        }
        .listStyle(.sidebar)
        .searchable(text: $search, placement: .sidebar, prompt: "Search")
        .safeAreaInset(edge: .bottom, spacing: 0) {
            HStack(spacing: 6) {
                Image(systemName: store.isFixture ? "testtube.2" : "terminal")
                Text(store.isFixture ? "Sample data" : "Your larp executable")
                    .lineLimit(1)
            }
            .help(store.isFixture ? "Sample data: changes stay in memory and reset on restart." : "Using your larp executable")
            .font(.caption)
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .accessibilityElement(children: .combine)
        }
    }

    // MARK: Detail

    private var title: String {
        switch selection {
        case let .page(page): return page.title
        case let .profile(id): return store.snapshot?.profiles.first { $0.id == id }?.name ?? "Profile"
        case nil: return "LarpCC"
        }
    }

    @ViewBuilder
    private var detail: some View {
        Group {
            switch selection {
            case .page(.general): GeneralPage(store: store, appearance: $appearance)
            case .page(.about): AboutPage(store: store)
            case let .page(page):
                if let snapshot = store.snapshot {
                    switch page {
                    case .mcp: CatalogPage(store: store, snapshot: snapshot, kind: .mcp)
                    case .skills: CatalogPage(store: store, snapshot: snapshot, kind: .skill)
                    case .aliases: AliasesPage(store: store, snapshot: snapshot)
                    default: DesktopAppsPage(store: store, snapshot: snapshot)
                    }
                } else { metadataUnavailable }
            case let .profile(id):
                if let snapshot = store.snapshot {
                    if let profile = snapshot.profiles.first(where: { $0.id == id }) {
                        ProfilePage(store: store, snapshot: snapshot, profile: profile) {
                            selection = .page(.general)
                        }
                        .id(profile.id)
                    } else {
                        UnavailablePane(title: "Profile unavailable", symbol: "person.crop.circle.badge.questionmark",
                                        message: "This profile is no longer in the current snapshot.")
                    }
                } else { metadataUnavailable }
            case nil:
                UnavailablePane(title: "Nothing selected", symbol: "sidebar.left", message: "Choose a page or profile in the sidebar.")
            }
        }
        .disabled(store.busy)
    }

    private var metadataUnavailable: some View {
        Group {
            if store.busy {
                ProgressView("Loading local metadata…").frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                UnavailablePane(title: "Local metadata unavailable", symbol: "terminal",
                                message: "Choose a trusted larp executable or use sample data in General.")
            }
        }
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .navigation) {
            ControlGroup {
                Button { move(back: true) } label: { Label("Back", systemImage: "chevron.left") }
                    .keyboardShortcut("[", modifiers: .command)
                    .disabled(backStack.isEmpty)
                    .help("Back")
                Button { move(back: false) } label: { Label("Forward", systemImage: "chevron.right") }
                    .keyboardShortcut("]", modifiers: .command)
                    .disabled(forwardStack.isEmpty)
                    .help("Forward")
            }
            .controlGroupStyle(.navigation)
        }
        ToolbarItemGroup(placement: .primaryAction) {
            if store.busy {
                ProgressView().controlSize(.small)
                Button("Cancel") { store.cancel() }
                    .keyboardShortcut(.cancelAction)
            } else {
                Button { store.startRefresh() } label: { Label("Refresh", systemImage: "arrow.clockwise") }
                    .keyboardShortcut("r", modifiers: .command)
                    .help("Reload local metadata")
            }
        }
    }

    @ViewBuilder
    private var messageBar: some View {
        if let message = store.message {
            VStack(spacing: 0) {
                Divider()
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Image(systemName: "info.circle").foregroundStyle(.secondary)
                    Text(message)
                        .font(.callout)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .fixedSize(horizontal: false, vertical: true)
                    Button { store.message = nil } label: { Image(systemName: "xmark") }
                        .buttonStyle(.borderless)
                        .accessibilityLabel("Dismiss message")
                }
                .padding(.horizontal, 20)
                .padding(.vertical, 10)
            }
            .background(.bar)
        }
    }

    // MARK: Helpers

    private func matches(_ text: String) -> Bool {
        let query = search.trimmingCharacters(in: .whitespaces)
        return query.isEmpty || text.localizedCaseInsensitiveContains(query)
    }

    private func exists(_ target: Selection) -> Bool {
        guard case let .profile(id) = target else { return true }
        return store.snapshot?.profiles.contains { $0.id == id } ?? false
    }

    /// Steps through history, skipping profiles that no longer exist.
    private func move(back: Bool) {
        while let target = back ? backStack.popLast() : forwardStack.popLast() {
            guard exists(target), target != selection else { continue }
            if let current = selection {
                if back { forwardStack.append(current) } else { backStack.append(current) }
            }
            historyMove = true
            selection = target
            return
        }
    }
}

private struct ProfileSidebarRow: View {
    let profile: Profile
    var body: some View {
        HStack(spacing: 10) {
            // Not a Label: the sidebar restyles Label icons, which washes out the marks.
            ProviderGlyph(provider: profile.provider)
            Text(profile.name).lineLimit(1)
            Spacer(minLength: 4)
            if profile.readiness != .off {
                StatusDot(color: profile.readiness.color)
            }
        }
        .opacity(profile.readiness == .off ? 0.55 : 1)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(profile.name), \(profile.provider.title), \(profile.readiness.title)")
    }
}
