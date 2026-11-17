import SwiftUI
import LarpModel

@main
struct LarpCCApp: App {
    @StateObject private var store = AppStore(client: FixtureLarpClient())
    private var sampleAppearance: ColorScheme? {
        if CommandLine.arguments.contains("--appearance=dark") { return .dark }
        if CommandLine.arguments.contains("--appearance=light") { return .light }
        return nil
    }

    var body: some Scene {
        Window("LarpCC", id: "settings") {
            SettingsView(store: store)
                .frame(minWidth: 900, minHeight: 650)
                .preferredColorScheme(sampleAppearance)
        }
        .defaultSize(width: 1040, height: 760)
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
    private let client: any LarpClient
    init(client: any LarpClient) { self.client = client }

    func refresh() async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        do { snapshot = try await client.snapshot() }
        catch { message = "Could not refresh. Try again." }
    }
    func perform(_ action: LarpAction) {
        guard !busy else { return }
        busy = true
        Task {
            defer { busy = false }
            do {
                message = try await client.perform(action)
                snapshot = try await client.snapshot()
            } catch let error as ClientError { message = error.errorDescription }
            catch { message = "The action could not be completed. Refresh and try again." }
        }
    }
}
