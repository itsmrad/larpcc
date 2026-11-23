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
        case .general: return "appearance dark light executable larp data folder sample"
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
    @State private var confirmingStop = false
    @AppStorage(PreferenceKey.appearance) private var appearance = AppearanceMode.system.rawValue
    @Environment(\.theme) private var theme

    var body: some View {
        split
        .task { store.startRefresh(); store.scheduleLiveRefresh() }
        .confirmationDialog("Stop this change?", isPresented: $confirmingStop, titleVisibility: .visible) {
            Button("Stop", role: .destructive) { store.cancel() }
            Button("Keep Waiting", role: .cancel) {}
        } message: {
            Text("larp may already have changed some files. Stopping now leaves the outcome unknown until you refresh.")
        }
        .sheet(item: $store.sheet) { sheet in
            switch sheet {
            case .addProfile:
                AddProfileSheet(store: store) { name in
                    if let id = store.snapshot?.profiles.first(where: { $0.name == name })?.id { selection = .profile(id) }
                }
            case .addServer: AddServerSheet(store: store)
            case .addSkill: AddSkillSheet(store: store)
            }
        }
        .onAppear { (AppearanceMode(rawValue: appearance) ?? .system).apply(); showRequestedProfile() }
        .onChange(of: store.requestedProfileID) { _, _ in showRequestedProfile() }
        .onChange(of: appearance) { _, value in (AppearanceMode(rawValue: value) ?? .system).apply() }
        .onChange(of: store.snapshot?.profiles.map(\.id)) { _, ids in
            // Leave a profile page only once the profile is really gone (e.g. after Delete).
            if case let .profile(id) = selection, let ids, !ids.contains(id) { selection = .page(.general) }
            showRequestedProfile()
        }
        .onChange(of: selection) { old, _ in
            if historyMove { historyMove = false; return }
            if let old { backStack.append(old) }
            forwardStack.removeAll()
        }
    }

    // MARK: Sidebar

    /// Tahoe: the system split view with its floating glass sidebar. Sequoia and Sonoma: an
    /// edge-to-edge sidebar on the sidebar material with a hairline, as those releases drew it.
    @ViewBuilder
    private var split: some View {
        if theme.tokens.floatingSidebar {
            NavigationSplitView {
                sidebar
                    .navigationSplitViewColumnWidth(min: 210, ideal: 230, max: 300)
            } detail: {
                detailColumn
            }
        } else {
            HStack(spacing: 0) {
                sidebar
                    .scrollContentBackground(.hidden)
                    .frame(width: 230)
                    .background(SidebarMaterial().ignoresSafeArea())
                Divider().ignoresSafeArea()
                detailColumn.background(classicPageBackground.ignoresSafeArea())
            }
            .modifier(ClearToolbar())
        }
    }

    private var detailColumn: some View {
        detail
            // Minimum on the detail column, not the split view: a split-view
            // minimum makes the detail snap instead of resizing as the sidebar slides in.
            .frame(minWidth: 460, minHeight: 480)
            .navigationTitle(title)
            .toolbar { toolbar }
            .safeAreaInset(edge: .bottom, spacing: 0) { messageBar }
    }

    /// Shows the profile the menu bar asked for. Without metadata the request waits for it;
    /// a profile that no longer exists drops the request.
    private func showRequestedProfile() {
        guard let id = store.requestedProfileID, let profiles = store.snapshot?.profiles else { return }
        store.requestedProfileID = nil
        guard profiles.contains(where: { $0.id == id }) else { return }
        search = ""
        selection = .profile(id)
    }

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
                        ProfileSidebarRow(profile: profile, usage: store.usage[profile.id])
                            .tag(Selection.profile(profile.id))
                    }
                } header: {
                    HStack {
                        Text("Profiles")
                        Spacer()
                        Text("\(snapshot.enabledCount) on").monospacedDigit()
                        Button { store.sheet = .addProfile } label: { Image(systemName: "plus") }
                            .buttonStyle(.borderless)
                            .help("New Profile (⌘N)")
                            .accessibilityLabel("New Profile")
                            .disabled(store.busy)
                    }
                    .padding(.trailing, 10)
                    .accessibilityElement(children: .contain)
                }
            }
        }
        .listStyle(.sidebar)
        .modifier(SidebarDensity(size: theme.tokens.sidebarRowSize))
        .modifier(SidebarSearch(text: $search, inSidebarSplit: theme.tokens.floatingSidebar))
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
                        ProfilePage(store: store, snapshot: snapshot, profile: profile)
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
        if !theme.tokens.floatingSidebar {
            // Starts the navigation controls and title past the edge-to-edge sidebar.
            SidebarWidthSpacer()
        }
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
                Button("Cancel") { if store.mutating { confirmingStop = true } else { store.cancel() } }
                    .keyboardShortcut(.cancelAction)
            } else {
                if selection == .page(.mcp) || selection == .page(.skills) {
                    Button { store.sheet = selection == .page(.mcp) ? .addServer : .addSkill } label: {
                        Label(selection == .page(.mcp) ? "Add Server" : "Add Skill", systemImage: "plus")
                    }
                    .help(selection == .page(.mcp) ? "Add an MCP server" : "Add a skill folder")
                    .disabled(store.snapshot == nil)
                }
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
    let usage: UsageReport?
    var body: some View {
        HStack(spacing: 10) {
            // Not a Label: the sidebar restyles Label icons, which washes out the marks.
            ProviderGlyph(provider: profile.provider)
            Text(profile.name).lineLimit(1)
            Spacer(minLength: 4)
            UsageHint(report: usage)
            if profile.readiness != .off {
                StatusDot(color: profile.readiness.color)
            }
        }
        .opacity(profile.readiness == .off ? 0.55 : 1)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(profile.name), \(profile.provider.title), \(profile.readiness.title)")
    }
}

/// The pre-glass sidebar material, edge to edge under the title bar.
private struct SidebarMaterial: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = .sidebar
        view.blendingMode = .behindWindow
        view.state = .followsWindowActiveState
        return view
    }

    func updateNSView(_ nsView: NSVisualEffectView, context: Context) {}
}

/// The system search field sits in the sidebar only inside a split view; elsewhere LarpCC
/// puts a native search field at the top of the sidebar, as earlier releases did.
private struct SidebarSearch: ViewModifier {
    @Binding var text: String
    let inSidebarSplit: Bool

    func body(content: Content) -> some View {
        if inSidebarSplit {
            content.searchable(text: $text, placement: .sidebar, prompt: "Search")
        } else {
            content.safeAreaInset(edge: .top, spacing: 0) {
                SearchField(text: $text)
                    .padding(.horizontal, 10)
                    .padding(.top, 6)
                    .padding(.bottom, 4)
            }
        }
    }
}

private struct SearchField: NSViewRepresentable {
    @Binding var text: String

    func makeNSView(context: Context) -> NSSearchField {
        let field = NSSearchField()
        field.placeholderString = "Search"
        field.delegate = context.coordinator
        return field
    }

    func updateNSView(_ field: NSSearchField, context: Context) {
        if field.stringValue != text { field.stringValue = text }
    }

    func makeCoordinator() -> Coordinator { Coordinator(text: $text) }

    final class Coordinator: NSObject, NSSearchFieldDelegate {
        let text: Binding<String>
        init(text: Binding<String>) { self.text = text }
        func controlTextDidChange(_ notification: Notification) {
            text.wrappedValue = (notification.object as? NSSearchField)?.stringValue ?? ""
        }
    }
}

/// Lets the edge-to-edge sidebar run up under the title bar, as it did before Liquid Glass.
private struct ClearToolbar: ViewModifier {
    func body(content: Content) -> some View {
        if #available(macOS 15, *) {
            content.toolbarBackground(.hidden, for: .windowToolbar)
        } else {
            content
        }
    }
}

private struct SidebarWidthSpacer: ToolbarContent {
    var body: some ToolbarContent {
        if #available(macOS 26, *) {
            ToolbarItem(placement: .navigation) { spacer }.sharedBackgroundVisibility(.hidden)
        } else {
            ToolbarItem(placement: .navigation) { spacer }
        }
    }

    private var spacer: some View {
        Color.clear.frame(width: 140, height: 1).accessibilityHidden(true)
    }
}

/// The pre-Tahoe page background: light grey in light mode, as those releases drew windows.
private let classicPageBackground = Color(nsColor: NSColor(name: nil) { appearance in
    appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? .windowBackgroundColor : NSColor(white: 0.925, alpha: 1)
})

/// Sets the sidebar density only when the theme asks for one.
private struct SidebarDensity: ViewModifier {
    let size: SidebarRowSize?

    func body(content: Content) -> some View {
        if let size { content.environment(\.sidebarRowSize, size) } else { content }
    }
}
