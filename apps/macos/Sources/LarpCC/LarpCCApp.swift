import SwiftUI
import LarpModel

@main
struct LarpCCApp: App {
    @StateObject private var store = AppStore(client: FixtureLarpClient())
    init() {
        if CommandLine.arguments.contains("--validate-contract-resource") {
            do { try ProcessLarpClient.validateBundledContract(); exit(0) }
            catch { exit(1) }
        }
    }
    var body: some Scene {
        Window("LarpCC", id: "settings") {
            SettingsView(store: store)
                .frame(minWidth: 760, minHeight: 520)
        }
        .defaultSize(width: 900, height: 680)
        .windowToolbarStyle(.unified)
        .commands {
            CommandGroup(replacing: .appSettings) {
                OpenSettingsWindow()
            }
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

@MainActor
final class AppStore: ObservableObject {
    @Published var snapshot: Snapshot?
    @Published var busy = false
    @Published var message: String?
    @Published private(set) var isFixture = true
    @Published private(set) var selectedPath: String?
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
    func useFixture() {
        cancel()
        isFixture = true; selectedPath = nil; snapshot = nil
        client = FixtureLarpClient()
        operation = Task { await refresh() }
    }
    func cancel() {
        generation += 1
        operation?.cancel(); operation = nil
        busy = false
        snapshot = nil
        message = "Cancelled. Refresh before choosing another action."
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
        } catch {
            guard generation == token else { return }
            snapshot = nil
            message = (error as? ClientError)?.errorDescription ?? "Could not refresh local metadata."
        }
    }
    func perform(_ action: LarpAction) {
        guard !busy, snapshot != nil else { return }
        busy = true
        generation += 1
        let token = generation
        operation = Task {
            defer { if generation == token { busy = false } }
            do {
                let result = try await client.perform(action)
                let value = try await client.snapshot()
                guard generation == token, !Task.isCancelled else { return }
                message = result; snapshot = value
            } catch {
                guard generation == token else { return }
                snapshot = nil
                message = (error as? ClientError)?.errorDescription ?? "The action could not be completed. Refresh and try again."
                if case ClientError.conflict = error {
                    // Refresh only; the old action is never replayed.
                    let value = try? await client.snapshot()
                    if generation == token, !Task.isCancelled { snapshot = value }
                }
            }
        }
    }
}
