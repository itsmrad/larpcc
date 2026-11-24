import SwiftUI
import LarpModel

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
        if trimmed(name).isEmpty { return "Enter a server name." }
        if !LarpNames.isValid(trimmed(name)) { return catalogNameHint }
        if store.snapshot?.servers.contains(where: { $0.id == trimmed(name) }) == true { return "A server with that name already exists." }
        return MCPInput.problem(name: trimmed(name), transport: transport)
    }

    var body: some View {
        EditSheet(title: "Add MCP Server", confirm: "Add", canConfirm: problem == nil && !store.busy, error: $error) {
            submit(store, .addServer(name: trimmed(name), transport: transport), error: $error, dismiss: dismiss)
        } content: {
            Section {
                TextField("Name", text: $name, prompt: Text("files"))
                    .textFieldStyle(.roundedBorder)
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
        }
    }

    private var transportFooter: some View {
        VStack(alignment: .leading, spacing: SettingsSpacing.tight) {
            if let problem {
                Text(problem)
            }
            Text("Use environment variable names for references. LarpCC never starts a server.")
        }
        .foregroundStyle(SettingsTypography.supportingInk)
    }

    private var commandFields: some View {
        Section {
            LabeledContent {
                Button("Choose…") { if let chosen = choosePath(title: "Choose the server program") { path = chosen } }
            } label: {
                Text("Program")
                Text(path.isEmpty ? "None chosen" : path)
                    .foregroundStyle(SettingsTypography.supportingInk)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(path)
            }
            Picker("Argument", selection: $stdioArg) {
                Text("None").tag("")
                ForEach(MCPInput.allowedArgs, id: \.self) { Text($0).tag($0) }
            }
            ForEach($envRefs) { $field in
                HStack(spacing: SettingsSpacing.inline) {
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
                .disabledReason(envRefs.count >= MCPInput.maxRefs ? "Maximum of \(MCPInput.maxRefs) references" : nil)
        } header: {
            Text("Command")
        } footer: {
            transportFooter
        }
    }

    private var urlFields: some View {
        Section {
            TextField("Address", text: $url, prompt: Text("https://example.com/mcp"))
                .textFieldStyle(.roundedBorder)
            ForEach($headers) { $field in
                HStack(spacing: SettingsSpacing.inline) {
                    TextField("Header", text: $field.header, prompt: Text("Authorization"))
                        .labelsHidden()
                        .textFieldStyle(.roundedBorder)
                    Text("from").foregroundStyle(SettingsTypography.supportingInk)
                    TextField("Environment variable", text: $field.envVar, prompt: Text("API_TOKEN"))
                        .labelsHidden()
                        .textFieldStyle(.roundedBorder)
                    removeButton { headers.removeAll { $0.id == field.id } }
                }
            }
            Button("Add Header Reference") { headers.append(HeaderField()) }
                .disabledReason(headers.count >= MCPInput.maxRefs ? "Maximum of \(MCPInput.maxRefs) references" : nil)
        } header: {
            Text("URL")
        } footer: {
            transportFooter
        }
    }

    private func removeButton(_ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: "minus.circle.fill").foregroundStyle(SettingsTypography.supportingInk)
        }
        .buttonStyle(.borderless)
        .help("Remove reference")
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

    private var problem: String? {
        if path.isEmpty { return "Choose a folder containing SKILL.md." }
        if !MCPInput.isAbsolutePath(path) { return "Choose a folder with a valid absolute path." }
        if trimmed(name).isEmpty { return "Enter a skill name." }
        if !LarpNames.isValid(trimmed(name)) {
            return catalogNameHint
        }
        if store.snapshot?.skills.contains(where: { $0.id == trimmed(name) }) == true {
            return "A skill with that name already exists."
        }
        return nil
    }

    var body: some View {
        EditSheet(title: "Add Skill", confirm: "Add", canConfirm: problem == nil && !store.busy, error: $error) {
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
                    Text(path.isEmpty ? "None chosen" : path)
                        .foregroundStyle(SettingsTypography.supportingInk)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .help(path)
                }
                TextField("Name", text: $name, prompt: Text("code-review"))
                    .textFieldStyle(.roundedBorder)
            } header: {
                Text("New Skill")
            } footer: {
                Text(problem ?? "Larp copies the folder once into its skill store. New skills start on for every active command-line profile.")
                    .foregroundStyle(SettingsTypography.supportingInk)
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

private let catalogNameHint = "Start with a lowercase letter. Use up to 32 lowercase letters, digits, hyphens, or underscores."
