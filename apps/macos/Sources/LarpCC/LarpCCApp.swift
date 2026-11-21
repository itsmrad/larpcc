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
        #endif
    }
    var body: some Scene {
        Window("LarpCC", id: "settings") {
            SettingsView(store: store)
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
        } label: {
            MenuBarLabel(store: store)
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
    case addProfile, addServer, addSkill
    var id: String { rawValue }
}

@MainActor
final class AppStore: ObservableObject {
    @Published var sheet: AppSheet?
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
    private var liveRefresh: Task<Void, Never>?
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
                selectedPath = url.path
                busy = false
                await refresh()
            } catch {
                if generation == token { busy = false; message = ClientError.invalidExecutable.errorDescription }
            }
        }
    }
    /// Reads usage for every profile from the current source.
    func refreshUsage() {
        let source = UsagePreferences.source
        usageTask?.cancel()
        usageLoading = true
        usageTask = Task {
            do {
                let value = try await client.usage(source: source)
                guard !Task.isCancelled else { return }
                usage = value; usageError = nil
            } catch {
                guard !Task.isCancelled else { return }
                usage = [:]
                usageError = (error as? ClientError)?.errorDescription ?? "Usage is unavailable."
            }
            usageLoading = false
        }
    }

    /// Restarts the optional Live refresh. Nothing repeats in Local mode or when the interval is Off.
    func scheduleLiveRefresh() {
        liveRefresh?.cancel(); liveRefresh = nil
        let minutes = UsagePreferences.intervalMinutes
        guard UsagePreferences.source == .live, minutes > 0 else { return }
        liveRefresh = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: UInt64(minutes) * 60_000_000_000)
                guard !Task.isCancelled else { return }
                self?.refreshUsage()
            }
        }
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
        usageTask?.cancel(); usage = [:]; usageLoading = false
        message = mutating ? ClientError.outcomeUnknown.errorDescription : "Cancelled. Refresh before choosing another action."
        mutating = false
    }
    func startRefresh() { operation = Task { await refresh() } }
    func refresh() async {
        guard !busy, isFixture || selectedPath != nil else { return }
        busy = true
        generation += 1
        let token = generation
        defer { if generation == token { busy = false } }
        do {
            let value = try await client.snapshot()
            guard generation == token, !Task.isCancelled else { return }
            snapshot = value
            // Local usage is data already on this Mac, so it follows every refresh.
            // Live usage is only read on request or on the user's chosen interval.
            if UsagePreferences.source == .local { refreshUsage() }
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
