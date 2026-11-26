import SwiftUI
import LarpModel

@main
struct LarpCCApp: App {
    @StateObject private var store = AppStore(client: FixtureLarpClient())
    @AppStorage(MenuBarPreferences.shownKey) private var menuBarShown = true
    @NSApplicationDelegateAdaptor(MenuBarAppDelegate.self) private var menuBarDelegate
    init() {
        if CommandLine.arguments.contains("--validate-contract-resource") {
            do { try ProcessLarpClient.validateBundledContract(); exit(0) }
            catch { exit(1) }
        }
        #if DEBUG
        MenuBarSnapshot.runIfRequested()
        SettingsSnapshot.runIfRequested()
        #endif
    }
    @ViewBuilder
    private var settingsContent: some View {
        #if DEBUG
        if let step = OnboardingStep.snapshotValue, store.isFixture {
            OnboardingView(store: store, step: step)
                .task { await store.refresh() }
                .onAppear { (AppearanceMode.launchOverride ?? .system).apply() }
        } else {
            SettingsView(store: store)
        }
        #else
        SettingsView(store: store)
        #endif
    }

    var body: some Scene {
        Window("LarpCC", id: "settings") {
            settingsContent
                .themed()
                .onAppear { fitWindowOnScreen() }
        }
        .defaultSize(width: 900, height: 680)
        .defaultPosition(.center)
        .windowToolbarStyle(.unified)
        .commands {
            CommandGroup(replacing: .appSettings) {
                OpenSettingsWindow()
            }
            CommandGroup(replacing: .newItem) {
                NewProfileCommand(store: store)
            }
        }
        MenuBarExtra(isInserted: MenuBarDockPolicy.insertion($menuBarShown)) {
            MenuBarPanel(store: store)
                .themed()
        } label: {
            MenuBarLabel(store: store)
                .themed()
        }
        .menuBarExtraStyle(.window)
    }
}

/// Keeps the window fully on the screen it opens on, in case a restored or
/// default frame lands partly off-screen (e.g. after a display change).
@MainActor private func fitWindowOnScreen() {
    DispatchQueue.main.async {
        for window in NSApp.windows where window.isVisible && window.styleMask.contains(.titled) {
            guard let visible = (window.screen ?? NSScreen.main)?.visibleFrame,
                  !visible.contains(window.frame) else { continue }
            var frame = window.frame
            frame.size.width = min(frame.width, visible.width)
            frame.size.height = min(frame.height, visible.height)
            frame.origin.x = visible.midX - frame.width / 2
            frame.origin.y = visible.midY - frame.height / 2
            window.setFrame(frame, display: true)
        }
    }
}

private struct OpenSettingsWindow: View {
    @Environment(\.openWindow) private var openWindow
    var body: some View {
        Button("Settings…") { openWindow(id: "settings") }
            .keyboardShortcut(",", modifiers: .command)
    }
}

private struct NewProfileCommand: View {
    @ObservedObject var store: AppStore
    @Environment(\.openWindow) private var openWindow
    var body: some View {
        Button("New Profile…") {
            openWindow(id: "settings")
            NSApp.activate(ignoringOtherApps: true)
            store.sheet = .addProfile
        }
        .keyboardShortcut("n", modifiers: .command)
        .disabled(store.snapshot == nil || store.busy)
    }
}

/// App-wide sheets, opened from the menu bar, toolbar or sidebar.
enum AppSheet: String, Identifiable {
    case addProfile, addServer, addSkill, onboarding
    var id: String { rawValue }
}

@MainActor
final class AppStore: ObservableObject {
    @Published var sheet: AppSheet?
    private var offeredSetup = false

    func offerSetup() {
        #if DEBUG
        guard !CommandLine.arguments.contains(where: { $0.hasPrefix("--settings-snapshot=") }) else { return }
        #endif
        guard !offeredSetup, selectedPath == nil, sheet == nil else { return }
        offeredSetup = true
        sheet = .onboarding
    }

    func detectProviders(revision: String?) async throws -> ProviderDetection {
        guard let snapshot, snapshot.revision == revision else { throw ClientError.staleSelection }
        let token = generation
        let detection = try await client.detectProviders(revision: revision)
        guard generation == token else { throw ClientError.conflict }
        return detection
    }
    @Published var snapshot: Snapshot?
    @Published var busy = false
    @Published var message: String?
    /// A profile the menu bar asked Settings to show.
    @Published var requestedProfileID: String?
    @Published private(set) var isFixture = true
    @Published private(set) var selectedPath: String?
    /// Usage limits keyed by profile ID, from the last usage read.
    @Published private(set) var usage: [String: UsageReport] = [:]
    @Published private(set) var usageError: String?
    @Published private(set) var usageLoading = false
    private var usageTask: Task<Void, Never>?
    var modeTitle: String { isFixture ? "Sample mode" : "Local Larp CLI" }
    private var client: any LarpClient
    private var operation: Task<Void, Never>?
    private var generation = 0
    init(client: any LarpClient) { self.client = client }

    func chooseExecutable() {
        let panel = NSOpenPanel()
        panel.title = "Choose your local larp executable"
        panel.message = "Only choose a trusted Larp build. This executable can change managed profile folders."
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        cancel()
        isFixture = false
        snapshot = nil
        selectedPath = nil
        let token = generation
        busy = true
        operation = Task {
            do {
                let selected = try await Task.detached { try SelectedLarp(url: url) }.value
                guard generation == token, !Task.isCancelled else { return }
                client = ProcessLarpClient(selected: selected)
                busy = false
                await refresh(admitting: url.path)
            } catch {
                if generation == token { busy = false; message = ClientError.invalidExecutable.errorDescription }
            }
        }
    }
    func refreshUsage() {
        guard isFixture || selectedPath != nil else { return }
        usageTask?.cancel()
        usageLoading = true
        usageTask = Task {
            guard !Task.isCancelled else { return }
            defer {
                if !Task.isCancelled { clearUsageRefresh() }
            }
            do {
                let value = if let fixture = client as? FixtureLarpClient {
                    try await fixture.usage(source: .live)
                } else {
                    try await client.usage(source: .local)
                }
                guard !Task.isCancelled else { return }
                usage = value; usageError = nil
            } catch {
                guard !Task.isCancelled else { return }
                usage = [:]
                usageError = (error as? ClientError)?.errorDescription ?? "Usage is unavailable."
            }
        }
    }

    private func clearUsageRefresh() {
        usageTask = nil
        usageLoading = false
    }

    func useFixture() {
        cancel()
        isFixture = true; selectedPath = nil; snapshot = nil
        client = FixtureLarpClient()
        operation = Task { await refresh() }
    }
    /// True while a change (not a read) is running; Cancel then warns before stopping it.
    @Published private(set) var mutating = false

    func cancel() {
        generation += 1
        operation?.cancel(); operation = nil
        busy = false
        snapshot = nil
        usageTask?.cancel(); clearUsageRefresh()
        usage = [:]; usageError = nil
        message = mutating ? ClientError.outcomeUnknown.errorDescription : "Cancelled. Refresh before choosing another action."
        mutating = false
    }
    func startRefresh() { operation = Task { await refresh() } }
    func refresh(admitting path: String? = nil) async {
        guard !busy, isFixture || selectedPath != nil || path != nil else { return }
        busy = true
        generation += 1
        let token = generation
        defer { if generation == token { busy = false } }
        do {
            let value = try await client.snapshot()
            guard generation == token, !Task.isCancelled else { return }
            snapshot = value
            if let path { selectedPath = path }
            refreshUsage()
        } catch {
            guard generation == token else { return }
            snapshot = nil
            message = (error as? ClientError)?.errorDescription ?? "Could not refresh local metadata."
        }
    }
    /// Runs one user intent. `completion` gets nil on success or the safe error text;
    /// when given, the caller shows the error inline instead of the message bar.
    /// After a failure the snapshot is refreshed, but the action is never replayed.
    func perform(_ action: LarpAction, completion: ((String?) -> Void)? = nil) {
        guard !busy, snapshot != nil else { completion?("Refresh before trying again."); return }
        busy = true
        mutating = true
        generation += 1
        let token = generation
        operation = Task {
            defer { if generation == token { busy = false; mutating = false } }
            let result: String
            do {
                result = try await client.perform(action)
            } catch {
                guard generation == token else { return }
                let text = (error as? ClientError)?.errorDescription ?? "The action could not be completed. Refresh and try again."
                if let completion { completion(text) } else { message = text }
                mutating = false
                let value = try? await client.snapshot()
                if generation == token, !Task.isCancelled { snapshot = value }
                return
            }
            guard generation == token, !Task.isCancelled else { return }
            mutating = false
            // The change succeeded; a failed follow-up read is reported on its own.
            do {
                let value = try await client.snapshot()
                guard generation == token, !Task.isCancelled else { return }
                message = result; snapshot = value
            } catch {
                guard generation == token else { return }
                snapshot = nil
                message = result + " Refresh failed — refresh to see the current state."
            }
            completion?(nil)
        }
    }
}
