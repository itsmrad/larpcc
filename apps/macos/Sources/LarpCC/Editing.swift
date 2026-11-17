import SwiftUI
import AppKit
import UniformTypeIdentifiers
import LarpModel

// MARK: - Shared sheet chrome

/// Form sheet with a title, inline error and Cancel / confirm buttons.
/// The confirm action runs one larp command; the sheet closes only on success.
struct EditSheet<Content: View>: View {
    let title: String
    let confirm: String
    let canConfirm: Bool
    @Binding var error: String?
    let onConfirm: () -> Void
    @ViewBuilder let content: Content
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            Form {
                content
                if let error {
                    Section {
                        Label {
                            Text(error)
                        } icon: {
                            Image(systemName: "exclamationmark.triangle.fill").symbolRenderingMode(.multicolor)
                        }
                    }
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(confirm, action: onConfirm)
                    .keyboardShortcut(.defaultAction)
                    .disabled(!canConfirm)
            }
            .padding(.horizontal, 20)
            .padding(.bottom, 16)
        }
        .frame(width: 460)
        .navigationTitle(title)
    }
}

/// Runs an action from a sheet: closes it on success, shows the error inline otherwise.
@MainActor
func submit(_ store: AppStore, _ action: LarpAction, error: Binding<String?>, dismiss: DismissAction) {
    error.wrappedValue = nil
    store.perform(action) { failure in
        if let failure { error.wrappedValue = failure } else { dismiss() }
    }
}

/// Asks for one local path; returns nil if cancelled.
@MainActor
func choosePath(title: String, folder: Bool = false, application: Bool = false) -> String? {
    let panel = NSOpenPanel()
    panel.title = title
    panel.canChooseDirectories = folder
    panel.canChooseFiles = !folder
    panel.allowsMultipleSelection = false
    if application { panel.allowedContentTypes = [.application] }
    return panel.runModal() == .OK ? panel.url?.path : nil
}

private let nameRule = "Lowercase letters, digits, - or _, up to 32 characters, starting with a letter."

private func trimmed(_ value: String) -> String { value.trimmingCharacters(in: .whitespacesAndNewlines) }

// MARK: - Profiles

struct AddProfileSheet: View {
    @ObservedObject var store: AppStore
    var onAdded: (String) -> Void = { _ in }
    @State private var name = ""
    @State private var provider = Provider.claude
    @State private var error: String?
    @Environment(\.dismiss) private var dismiss

    private var taken: Bool { store.snapshot?.profiles.contains { $0.name == trimmed(name) } ?? false }
    private var valid: Bool { LarpNames.isValidProfile(trimmed(name)) && !taken }

    var body: some View {
        EditSheet(title: "New Profile", confirm: "Add Profile", canConfirm: valid, error: $error, onConfirm: add) {
            Section {
                TextField("Name", text: $name, prompt: Text("claude-work"))
                Picker("Provider", selection: $provider) {
                    ForEach(Provider.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
            } header: {
                Text("New Profile")
            } footer: {
                Text(taken ? "A profile with that name already exists."
                     : !trimmed(name).isEmpty && !valid ? "\(nameRule) Not larp, claude or codex." : nameRule)
                    .foregroundStyle(.secondary)
            }
        }
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
    @Environment(\.dismiss) private var dismiss

    init(store: AppStore, profile: Profile) {
        self.store = store; self.profile = profile
        _name = State(initialValue: profile.name)
    }
    private var taken: Bool { store.snapshot?.profiles.contains { $0.name == trimmed(name) && $0.id != profile.id } ?? false }
    private var valid: Bool { LarpNames.isValidProfile(trimmed(name)) && !taken && trimmed(name) != profile.name }

    var body: some View {
        EditSheet(title: "Rename Profile", confirm: "Rename", canConfirm: valid, error: $error) {
            submit(store, .renameProfile(profileID: profile.id, newName: trimmed(name)), error: $error, dismiss: dismiss)
        } content: {
            Section {
                TextField("New name", text: $name)
            } header: {
                Text("Rename “\(profile.name)”")
            } footer: {
                Text(taken ? "A profile with that name already exists." : "\(nameRule) Aliases keep pointing at this profile.")
                    .foregroundStyle(.secondary)
            }
        }
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
        version.isEmpty || version.range(of: "^[A-Za-z0-9][A-Za-z0-9._+-]{0,31}$", options: .regularExpression) != nil
    }
    private var valid: Bool {
        MCPInput.isAbsolutePath(path) && (kind == .tool || path.hasSuffix(".app")) && versionValid
    }

    var body: some View {
        EditSheet(title: "Bind \(label)", confirm: "Bind", canConfirm: valid, error: $error) {
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
                }
                if kind == .app {
                    TextField("Version label", text: $version, prompt: Text("Optional, e.g. 1.2.3"))
                }
                Toggle(isOn: $replace) {
                    Text("Replace existing binding")
                    Text("Required when \(provider.title) already has one.")
                }
            } header: {
                Text(kind == .app ? "\(provider.desktopApp) for \(provider.title) profiles" : "\(provider.title) command-line tool")
            } footer: {
                Text(kind == .app
                     ? "The version label is your own note; larp doesn't verify it. Binding doesn't qualify launching."
                     : "Larp runs this executable for \(provider.title) profiles. It is never discovered from PATH.")
                    .foregroundStyle(.secondary)
            }
        }
    }
}

// MARK: - MCP servers

struct AddServerSheet: View {
    enum Kind: String, CaseIterable { case command = "Command", url = "URL" }
    @ObservedObject var store: AppStore
    @State private var name = ""
    @State private var kind = Kind.command
    @State private var path = ""
    @State private var stdioArg = ""
    @State private var envRefs: [RefField] = []
    @State private var url = ""
    @State private var headers: [HeaderField] = []
    @State private var error: String?
    @Environment(\.dismiss) private var dismiss

    struct HeaderField: Identifiable { let id = UUID(); var header = ""; var envVar = "" }
    struct RefField: Identifiable { let id = UUID(); var name = "" }

    private var transport: MCPTransport {
        switch kind {
        case .command:
            return .command(path: path, args: stdioArg.isEmpty ? [] : [stdioArg], envRefs: envRefs.map { trimmed($0.name) })
        case .url:
            return .url(trimmed(url), headerRefs: headers.map { HeaderRef(header: trimmed($0.header), envVar: trimmed($0.envVar)) })
        }
    }
    private var problem: String? {
        if store.snapshot?.servers.contains(where: { $0.id == trimmed(name) }) == true { return "A server with that name already exists." }
        return MCPInput.problem(name: trimmed(name), transport: transport)
    }

    var body: some View {
        EditSheet(title: "Add MCP Server", confirm: "Add", canConfirm: problem == nil, error: $error) {
            submit(store, .addServer(name: trimmed(name), transport: transport), error: $error, dismiss: dismiss)
        } content: {
            Section {
                TextField("Name", text: $name, prompt: Text("files"))
                Picker("Connects with", selection: $kind) {
                    ForEach(Kind.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
            } header: {
                Text("New MCP Server")
            }
            switch kind {
            case .command: commandFields
            case .url: urlFields
            }
            Section {
                EmptyView()
            } footer: {
                Text(trimmed(name).isEmpty ? "References are names only. Larp never stores secret values, and LarpCC never starts a server."
                     : problem ?? "References are names only. Larp never stores secret values, and LarpCC never starts a server.")
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var commandFields: some View {
        Section {
            LabeledContent {
                Button("Choose…") { if let chosen = choosePath(title: "Choose the server program") { path = chosen } }
            } label: {
                Text("Program")
                Text(path.isEmpty ? "None chosen" : path).truncationMode(.middle)
            }
            Picker("Argument", selection: $stdioArg) {
                Text("None").tag("")
                ForEach(MCPInput.allowedArgs, id: \.self) { Text($0).tag($0) }
            }
            ForEach($envRefs) { $field in
                HStack(spacing: 8) {
                    Text("Environment")
                    Spacer()
                    TextField("Environment variable", text: $field.name, prompt: Text("API_TOKEN"))
                        .labelsHidden()
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 180)
                    removeButton { envRefs.removeAll { $0.id == field.id } }
                }
            }
            Button("Add Environment Reference") { envRefs.append(RefField()) }
                .disabled(envRefs.count >= MCPInput.maxRefs)
        } header: {
            Text("Command")
        }
    }

    private var urlFields: some View {
        Section {
            TextField("Address", text: $url, prompt: Text("https://example.com/mcp"))
            ForEach($headers) { $field in
                HStack(spacing: 8) {
                    TextField("Header", text: $field.header, prompt: Text("Authorization"))
                        .labelsHidden()
                        .textFieldStyle(.roundedBorder)
                    Text("from").foregroundStyle(.secondary)
                    TextField("Environment variable", text: $field.envVar, prompt: Text("API_TOKEN"))
                        .labelsHidden()
                        .textFieldStyle(.roundedBorder)
                    removeButton { headers.removeAll { $0.id == field.id } }
                }
            }
            Button("Add Header Reference") { headers.append(HeaderField()) }
                .disabled(headers.count >= MCPInput.maxRefs)
        } header: {
            Text("URL")
        }
    }

    private func removeButton(_ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: "minus.circle.fill").foregroundStyle(.secondary)
        }
        .buttonStyle(.borderless)
        .accessibilityLabel("Remove reference")
    }
}

// MARK: - Skills

struct AddSkillSheet: View {
    @ObservedObject var store: AppStore
    @State private var path = ""
    @State private var name = ""
    @State private var error: String?
    @Environment(\.dismiss) private var dismiss

    private var taken: Bool { store.snapshot?.skills.contains { $0.id == trimmed(name) } ?? false }
    private var valid: Bool { MCPInput.isAbsolutePath(path) && LarpNames.isValid(trimmed(name)) && !taken }

    var body: some View {
        EditSheet(title: "Add Skill", confirm: "Add", canConfirm: valid, error: $error) {
            submit(store, .addSkill(path: path, name: trimmed(name)), error: $error, dismiss: dismiss)
        } content: {
            Section {
                LabeledContent {
                    Button("Choose…") {
                        guard let chosen = choosePath(title: "Choose a skill folder", folder: true) else { return }
                        path = chosen
                        if name.isEmpty { name = Self.suggestedName(URL(fileURLWithPath: chosen).lastPathComponent) }
                    }
                } label: {
                    Text("Folder")
                    Text(path.isEmpty ? "A folder containing SKILL.md" : path).truncationMode(.middle)
                }
                TextField("Name", text: $name, prompt: Text("code-review"))
            } header: {
                Text("New Skill")
            } footer: {
                Text(taken ? "A skill with that name already exists."
                     : "Larp copies the folder once into its skill store. New skills start on for every active profile.")
                    .foregroundStyle(.secondary)
            }
        }
    }

    /// Folder name lowercased into the name grammar, e.g. "Code Review" → "code-review".
    static func suggestedName(_ folder: String) -> String {
        let mapped = folder.lowercased().map { $0.isLetter || $0.isNumber || $0 == "_" ? $0 : "-" }
        let name = String(mapped).trimmingCharacters(in: CharacterSet(charactersIn: "-"))
        return LarpNames.isValid(String(name.prefix(32))) ? String(name.prefix(32)) : ""
    }
}

// MARK: - Confirmation

/// A destructive confirmation with explicit wording; the action runs once per confirmation.
struct Confirmation: Identifiable {
    let id = UUID()
    let title: String
    let message: String
    let button: String
    let action: LarpAction
}

extension View {
    func confirming(_ item: Binding<Confirmation?>, store: AppStore) -> some View {
        confirmationDialog(item.wrappedValue?.title ?? "", isPresented: Binding(
            get: { item.wrappedValue != nil }, set: { if !$0 { item.wrappedValue = nil } }
        ), titleVisibility: .visible, presenting: item.wrappedValue) { confirmation in
            Button(confirmation.button, role: .destructive) { store.perform(confirmation.action) }
            Button("Cancel", role: .cancel) {}
        } message: { confirmation in
            Text(confirmation.message)
        }
    }
}

/// Which binding sheet to show: a provider's desktop app or its command-line tool.
struct BindTarget: Identifiable {
    let provider: Provider
    let kind: BindSheet.Kind
    var id: String { "\(provider.rawValue)-\(kind)" }
}

extension View {
    func bindSheet(_ target: Binding<BindTarget?>, store: AppStore) -> some View {
        sheet(item: target) { BindSheet(store: store, provider: $0.provider, kind: $0.kind) }
    }
}
