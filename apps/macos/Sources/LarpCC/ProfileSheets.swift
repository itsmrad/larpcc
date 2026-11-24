import SwiftUI
import LarpModel

// MARK: - Profiles

private let nameRule = "Start with a lowercase letter. Use up to 32 lowercase letters, digits, hyphens or underscores. The names larp, claude and codex are reserved."

struct AddProfileSheet: View {
    @ObservedObject var store: AppStore
    var onAdded: (String) -> Void = { _ in }
    @State private var name = ""
    @State private var provider = Provider.claude
    @State private var error: String?
    @FocusState private var nameFocused: Bool
    @Environment(\.dismiss) private var dismiss

    private var taken: Bool { store.snapshot?.profiles.contains { $0.name == trimmed(name) } ?? false }
    private var valid: Bool { LarpNames.isValidProfile(trimmed(name)) && !taken }

    var body: some View {
        EditSheet(title: "New Profile", confirm: "Add Profile", canConfirm: valid && !store.busy, error: $error, onConfirm: add) {
            Section {
                TextField("Name", text: $name, prompt: Text("claude-work"))
                    .focused($nameFocused)
                Picker("Provider", selection: $provider) {
                    ForEach(Provider.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
            } header: {
                Text("New Profile")
            } footer: {
                Text(taken ? "A profile with that name already exists." : nameRule)
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
        }
        .defaultFocus($nameFocused, true)
    }

    private func add() {
        let new = trimmed(name)
        error = nil
        store.perform(.addProfile(name: new, provider: provider)) { failure in
            if let failure { error = failure } else { dismiss(); onAdded(new) }
        }
    }
}

struct RenameProfileSheet: View {
    @ObservedObject var store: AppStore
    let profile: Profile
    @State private var name: String
    @State private var error: String?
    @FocusState private var nameFocused: Bool
    @Environment(\.dismiss) private var dismiss

    init(store: AppStore, profile: Profile) {
        self.store = store; self.profile = profile
        _name = State(initialValue: profile.name)
    }
    private var taken: Bool { store.snapshot?.profiles.contains { $0.name == trimmed(name) && $0.id != profile.id } ?? false }
    private var valid: Bool { LarpNames.isValidProfile(trimmed(name)) && !taken && trimmed(name) != profile.name }

    var body: some View {
        EditSheet(title: "Rename Profile", confirm: "Rename", canConfirm: valid && !store.busy, error: $error) {
            submit(store, .renameProfile(profileID: profile.id, newName: trimmed(name)), error: $error, dismiss: dismiss)
        } content: {
            Section {
                TextField("New name", text: $name)
                    .focused($nameFocused)
            } header: {
                Text("Rename “\(profile.name)”")
            } footer: {
                Text(taken ? "A profile with that name already exists." : "\(nameRule) Existing aliases stay with this profile.")
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
        }
        .defaultFocus($nameFocused, true)
    }
}

/// Binds a provider's desktop app (`app bind`) or command-line executable (`provider bind`).
struct BindSheet: View {
    enum Kind { case app, tool }
    @ObservedObject var store: AppStore
    let provider: Provider
    let kind: Kind
    @State private var path = ""
    @State private var version = ""
    @State private var replace = false
    @State private var error: String?
    @Environment(\.dismiss) private var dismiss

    private var label: String { kind == .app ? provider.desktopApp : "\(provider.rawValue) executable" }
    private var versionValid: Bool {
        version.isEmpty || LarpNames.isVersionLabel(version)
    }
    private var valid: Bool {
        MCPInput.isAbsolutePath(path) && (kind == .tool || path.hasSuffix(".app")) && versionValid
    }

    var body: some View {
        EditSheet(title: "Bind \(label)", confirm: "Bind", canConfirm: valid && !store.busy, error: $error) {
            let action: LarpAction = kind == .app
                ? .bindApp(provider: provider, path: path, version: version.isEmpty ? nil : version, replace: replace)
                : .bindProvider(provider: provider, path: path, replace: replace)
            submit(store, action, error: $error, dismiss: dismiss)
        } content: {
            Section {
                LabeledContent {
                    Button("Choose…") {
                        if let chosen = choosePath(title: "Choose \(label)", application: kind == .app) { path = chosen }
                    }
                } label: {
                    Text(kind == .app ? "Application" : "Executable")
                    Text(path.isEmpty ? "None chosen" : path)
                        .truncationMode(.middle)
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
                if kind == .app {
                    TextField("Version label", text: $version, prompt: Text("Optional, e.g. 1.2.3"))
                }
                Toggle(isOn: $replace) {
                    Text("Replace existing binding")
                    Text("Required when \(provider.title) already has one.")
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
            } header: {
                Text(kind == .app ? "\(provider.desktopApp) for \(provider.title) profiles" : "\(provider.title) command-line tool")
            } footer: {
                Text(kind == .app
                     ? "This binding is shared by all \(provider.title) profiles. The optional version label is a note. Launching requires separate qualification."
                     : "Larp uses the chosen executable for all \(provider.title) profiles.")
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
        }
    }
}
