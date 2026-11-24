import SwiftUI
import LarpModel

enum OnboardingStep: String, CaseIterable {
    case welcome, larp, providers, desktop, profile, done

    var title: String {
        switch self {
        case .welcome: return "Welcome to LarpCC"
        case .larp: return "Choose larp"
        case .providers: return "Command-line tools"
        case .desktop: return "Desktop apps"
        case .profile: return "Create a profile"
        case .done: return "Setup complete"
        }
    }

    #if DEBUG
    static var snapshotValue: Self? {
        let prefix = "--onboarding-step="
        guard CommandLine.arguments.contains(where: { $0.hasPrefix("--settings-snapshot=") }),
              let argument = CommandLine.arguments.first(where: { $0.hasPrefix(prefix) }) else { return nil }
        return Self(rawValue: String(argument.dropFirst(prefix.count)))
    }
    #endif
}

struct OnboardingView: View {
    @ObservedObject var store: AppStore
    @State private var step: OnboardingStep
    @Environment(\.theme) private var theme
    @Environment(\.dismiss) private var dismiss
    @FocusState private var primaryFocused: Bool
    @State private var scan: ProviderDetection?
    @State private var loadedKey: ScanKey?
    @State private var scanError: String?
    @State private var bindingError: String?
    @State private var retry = 0
    @State private var binding: SetupBinding?
    @State private var addingProfile = false
    @State private var profileName: String?
    @State private var providerBindings: Set<Provider> = []
    @State private var desktopBindings: Set<Provider> = []

    init(store: AppStore, step: OnboardingStep = .welcome) {
        self.store = store
        _step = State(initialValue: step)
        #if DEBUG
        if step == .done, OnboardingStep.snapshotValue == .done, store.isFixture {
            _providerBindings = State(initialValue: [.claude])
            _desktopBindings = State(initialValue: [.codex])
            _profileName = State(initialValue: "claude-work")
        }
        #endif
    }

    private struct ScanKey: Equatable {
        let active: Bool
        let ready: Bool
        let busy: Bool
        let revision: String?
        let retry: Int
    }

    private var scanKey: ScanKey {
        ScanKey(active: step == .providers || step == .desktop,
                ready: store.snapshot != nil, busy: store.busy,
                revision: store.snapshot?.revision, retry: retry)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: SettingsSpacing.inset) {
            Text(step.title)
                .font(SettingsTypography.title)
                .accessibilityAddTraits(.isHeader)

            content
                .id(step)
                .frame(maxWidth: .infinity, alignment: .leading)
                .settingsAnimation(value: step)

            if let bindingError {
                supporting(bindingError).foregroundStyle(SettingsTypography.warningInk)
            }

            Divider()
            HStack {
                Button(step == .welcome ? "Set up later" : "Back", action: back)
                    .keyboardShortcut(.cancelAction)
                    .disabled(store.busy)
                Spacer()
                if step == .profile {
                    Button("Skip") { step = .done }
                        .disabled(store.busy)
                }
                Button(primaryTitle, action: advance)
                    .keyboardShortcut(.defaultAction)
                    .focused($primaryFocused)
                    .disabled(primaryDisabled)
            }
        }
        .padding(SettingsSpacing.inset * 2)
        .frame(width: 590)
        .interactiveDismissDisabled(store.busy)
        .defaultFocus($primaryFocused, true)
        .onChange(of: step) { _, _ in
            bindingError = nil
            primaryFocused = true
        }
        .onChange(of: store.selectedPath) { _, _ in
            scan = nil
            providerBindings.removeAll()
            desktopBindings.removeAll()
            profileName = nil
        }
        .task(id: scanKey) { await loadDetection() }
        .sheet(isPresented: $addingProfile) {
            AddProfileSheet(store: store) { name in
                profileName = name
                step = .done
            }
        }
        .sheet(item: $binding) { target in
            SetupBindingSheet(store: store, target: target) {
                if target.desktop { desktopBindings.insert(target.provider) }
                else { providerBindings.insert(target.provider) }
            }
        }
    }

    @ViewBuilder private var content: some View {
        switch step {
        case .welcome:
            supporting("LarpCC organizes your Claude Code and Codex profiles. Choose your local larp tool, then find installations you can connect.")
        case .larp:
            supporting("Choose a trusted larp build. LarpCC uses it to manage profiles and find installed tools.")
            if let path = store.selectedPath {
                pathLabel(path)
                Button("Choose another…") { store.chooseExecutable() }.disabled(store.busy)
            }
            if store.busy {
                ProgressView("Checking larp…").controlSize(.small)
            } else if store.selectedPath == nil, !store.isFixture, let message = store.message {
                supporting(message)
            }
        case .providers, .desktop:
            detectionContent
        case .profile:
            supporting("Profiles give each Claude Code or Codex setup a separate data folder. You can add a profile now or from Settings later.")
        case .done:
            let tools = providerBindings.union(scan?.executables.filter(\.bound).map(\.provider) ?? [])
            if tools.isEmpty && desktopBindings.isEmpty {
                supporting("You can connect installations from Settings at any time.")
            }
            ForEach(Provider.allCases, id: \.self) { provider in
                if tools.contains(provider) { Label("\(toolName(provider)) bound", systemImage: "checkmark.circle") }
                if desktopBindings.contains(provider) { Label("\(provider.desktopApp) bound", systemImage: "checkmark.circle") }
            }
            if let profileName { Label("Profile added: \(profileName)", systemImage: "checkmark.circle") }
        }
    }

    @ViewBuilder private var detectionContent: some View {
        if let scanError, loadedKey == scanKey {
            UnavailablePane(title: "Detection unavailable", symbol: "terminal",
                            message: scanError, retry: {
                Task {
                    await store.refresh()
                    retry += 1
                }
            })
            .frame(height: 170)
            if step == .providers, store.snapshot != nil {
                ForEach(Provider.allCases, id: \.self) { provider in
                    Button("Choose \(toolName(provider)) executable…") {
                        chooseProvider(provider)
                    }
                    .disabled(store.busy)
                }
            }
        } else if let scan, loadedKey == scanKey {
            supporting(step == .providers
                ? "Choose the tool larp should use for each provider."
                : "Desktop bindings are shared by a provider’s profiles. Launch requires separate qualification.")
            ScrollView {
                VStack(alignment: .leading, spacing: SettingsSpacing.inset) {
                    if step == .providers {
                        ForEach(Provider.allCases, id: \.self) { provider in
                            providerRows(provider, in: scan)
                        }
                    } else {
                        desktopRows(scan)
                    }
                }
                .padding(.trailing, SettingsSpacing.inline)
            }
            .frame(height: step == .providers ? 345 : 200)
            if scan.limited { supporting("The scan reached its limit. Some installations may be missing.") }
            if scan.skippedCount > 0 {
                supporting("Skipped protected locations (\(scan.skippedCount)).")
            }
        } else {
            LoadingPane(title: "Finding local installations…").frame(height: 210)
        }
    }

    private func providerRows(_ provider: Provider, in scan: ProviderDetection) -> some View {
        let candidates = scan.executables.filter { $0.provider == provider }
        return VStack(alignment: .leading, spacing: SettingsSpacing.inline) {
            HStack {
                Text(toolName(provider)).font(.headline).accessibilityAddTraits(.isHeader)
                Spacer()
                Button("Choose another…") { chooseProvider(provider) }
                    .accessibilityLabel("Choose another \(toolName(provider)) executable")
                    .disabled(store.busy)
            }
            if candidates.isEmpty { supporting("No installation found") }
            ForEach(candidates, id: \.path) { candidate in
                HStack(alignment: .top, spacing: SettingsSpacing.control) {
                    VStack(alignment: .leading, spacing: SettingsSpacing.tight) {
                        pathLabel(candidate.path)
                        supporting("\(installKind(candidate.install_kind)) · Version: \(candidate.version ?? "Unavailable")")
                        supporting("\(candidate.qualified ? "Qualified" : "Not qualified") · \(candidate.bound ? "Bound" : "Not bound")")
                        if !candidate.bindable {
                            supporting("Binding unavailable · \(candidate.bind_reason ?? "Trust check failed")")
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    if candidate.bindable && !candidate.bound {
                        Button("Bind…") {
                            binding = SetupBinding(provider: provider, path: candidate.path, desktop: false,
                                                   revision: loadedKey?.revision)
                        }
                        .accessibilityLabel("Bind \(candidate.path)")
                        .disabled(store.busy)
                    }
                }
            }
        }
        .padding(SettingsSpacing.control)
        .sectionBox(theme.tokens)
    }

    @ViewBuilder private func desktopRows(_ scan: ProviderDetection) -> some View {
        if scan.desktop_apps.isEmpty { supporting("No desktop apps found") }
        ForEach(scan.desktop_apps, id: \.path) { app in
            HStack(alignment: .top, spacing: SettingsSpacing.control) {
                VStack(alignment: .leading, spacing: SettingsSpacing.tight) {
                    Text(app.provider.desktopApp).font(SettingsTypography.heading)
                    pathLabel(app.path)
                    if let version = app.version { supporting("Version: \(version)") }
                    supporting("Code requirement: \(app.code_requirement.capitalized)")
                    if app.provider == .claude {
                        supporting("Binding and launch closed · Qualification pending")
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                if app.provider == .codex {
                    Button("Bind…") {
                        binding = SetupBinding(provider: app.provider, path: app.path, desktop: true,
                                               revision: loadedKey?.revision)
                    }
                    .accessibilityLabel("Bind \(app.provider.desktopApp) at \(app.path)")
                    .disabled(store.busy)
                }
            }
            .padding(SettingsSpacing.control)
            .sectionBox(theme.tokens)
        }
    }

    private func chooseProvider(_ provider: Provider) {
        let revision = store.snapshot?.revision
        guard let path = choosePath(title: "Choose \(toolName(provider)) executable") else { return }
        guard MCPInput.isAbsolutePath(path) else {
            bindingError = "Choose an executable with a valid absolute path."
            return
        }
        bindingError = nil
        binding = SetupBinding(provider: provider, path: path, desktop: false, revision: revision)
    }

    private func installKind(_ kind: String) -> String {
        switch kind {
        case "homebrew": return "Homebrew"
        case "npm", "bun": return kind
        default: return kind.capitalized
        }
    }

    private func toolName(_ provider: Provider) -> String {
        provider == .claude ? "Claude Code" : "Codex"
    }

    private func supporting(_ text: String) -> some View {
        Text(text)
            .font(SettingsTypography.supporting)
            .foregroundStyle(SettingsTypography.supportingInk)
    }

    private func pathLabel(_ path: String) -> some View {
        Text(path).lineLimit(1).truncationMode(.middle).help(path)
    }

    private var primaryTitle: String {
        switch step {
        case .larp: return store.selectedPath == nil ? "Choose…" : "Continue"
        case .profile: return "Add Profile…"
        case .done: return "Close"
        default: return "Continue"
        }
    }

    private var primaryDisabled: Bool {
        store.busy || (step == .profile && store.snapshot == nil)
    }

    private func advance() {
        switch step {
        case .welcome: step = .larp
        case .larp:
            if store.selectedPath == nil { store.chooseExecutable() }
            else { step = .providers }
        case .providers: step = .desktop
        case .desktop: step = .profile
        case .profile: addingProfile = true
        case .done: dismiss()
        }
    }

    private func back() {
        switch step {
        case .welcome: dismiss()
        case .larp: step = .welcome
        case .providers: step = .larp
        case .desktop: step = .providers
        case .profile: step = .desktop
        case .done: step = .profile
        }
    }

    private func loadDetection() async {
        let key = scanKey
        guard key.active else { return }
        scanError = nil
        loadedKey = nil
        guard !key.busy else { return }
        guard key.ready else {
            loadedKey = key
            scanError = "Refresh local metadata to try again."
            return
        }
        do {
            let result = try await store.detectProviders(revision: key.revision)
            guard !Task.isCancelled, scanKey == key else { return }
            loadedKey = key
            scan = result
        } catch {
            guard !Task.isCancelled, scanKey == key else { return }
            loadedKey = key
            scanError = (error as? ClientError)?.errorDescription ?? "Could not find local installations."
        }
    }
}
