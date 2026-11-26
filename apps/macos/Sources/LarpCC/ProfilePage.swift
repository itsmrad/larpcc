import SwiftUI
import LarpModel

struct ProfilePage: View {
    @ObservedObject var store: AppStore
    let snapshot: Snapshot
    let profile: Profile
    @State private var confirmingDelete = false
    @State private var renaming = false
    @State private var binding: BindTarget?
    @State private var confirmation: Confirmation?

    var body: some View {
        Form {
            Section {
                header
                if profile.recoverable {
                    LabeledContent {
                        Button("Finish Deleting…", role: .destructive) {
                            confirmation = Confirmation(title: "Finish deleting “\(profile.name)”?",
                                message: "Quit this profile’s command-line and desktop sessions first. Larp will resume the interrupted delete and remove the managed folder.",
                                button: "Finish Deleting", action: .recover(profileID: profile.id))
                        }
                        .foregroundStyle(SettingsTypography.criticalInk)
                    } label: {
                        RowLabel("Delete interrupted", subtitle: "Finish deleting to remove the profile’s managed folder.") {
                            Image(systemName: "exclamationmark.triangle.fill").symbolRenderingMode(.multicolor)
                        }
                    }
                } else if profile.needsAttention {
                    Label {
                        Text(attentionDetail)
                            .font(SettingsTypography.supporting)
                            .foregroundStyle(SettingsTypography.supportingInk)
                            .fixedSize(horizontal: false, vertical: true)
                    } icon: {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .symbolRenderingMode(.multicolor)
                    }
                }
                launchActions
            }

            UsageSection(store: store, profile: profile)

            Section("Configuration") {
                Toggle("Command line", isOn: surfaceBinding(.cli))
                Toggle(isOn: surfaceBinding(.desktop)) {
                    Text("Desktop app")
                    Text("Keeps a separate \(profile.provider.desktopApp) data folder for this profile.")
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
                LabeledContent {
                    Button("Bind…") { binding = BindTarget(provider: profile.provider, kind: .app) }
                        .disabledReason(bindingUnavailableReason)
                } label: {
                    Text("\(profile.provider.desktopApp)")
                    Text(bindingDetail)
                        .truncationMode(.middle)
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
                ValueRow("Folder", value: profile.folder, selectable: true)
            }

            catalogSection("MCP Servers", items: snapshot.servers, kind: .mcp,
                           empty: "Add a server in MCP Servers to enable it for this profile.")
            catalogSection("Skills", items: snapshot.skills, kind: .skill,
                           empty: "Add a skill in Skills to enable it for this profile.")

            Section("Remove Profile") {
                LabeledContent {
                    Button("Forget…") {
                        confirmation = Confirmation(title: "Forget “\(profile.name)”?",
                            message: store.isFixture
                                ? "This removes the sample profile and its assignments from memory."
                                : "Larp keeps the folder on disk and retires the profile ID. The folder can’t be reattached. Remove any command aliases and shared items before forgetting the profile.",
                            button: "Forget Profile", action: .forget(profileID: profile.id))
                    }
                } label: {
                    Text("Forget profile")
                    Text(store.isFixture ? "Removes this sample profile from memory."
                         : "Keeps the profile folder on disk.")
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
                LabeledContent {
                    Button("Delete…", role: .destructive) { confirmingDelete = true }
                        .foregroundStyle(SettingsTypography.criticalInk)
                } label: {
                    Text("Delete profile")
                    Text(store.isFixture
                         ? "Removes this sample profile from memory."
                         : "Removes the managed profile folder and its aliases.")
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
            }
        }
        .themedFormStyle()
        .sheet(isPresented: $renaming) { RenameProfileSheet(store: store, profile: profile) }
        .bindSheet($binding, store: store)
        .confirming($confirmation, store: store)
        .confirmationDialog("Delete “\(profile.name)”?", isPresented: $confirmingDelete, titleVisibility: .visible) {
            Button(store.isFixture ? "Delete Sample Profile" : "Delete Profile Folder", role: .destructive) {
                store.perform(.delete(profileID: profile.id))
            }
            .disabled(store.busy)
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(store.isFixture
                 ? "This removes the sample profile and its sample assignments from memory. No folder or alias on disk is removed."
                 : "Quit this profile’s command-line and desktop sessions first. Larp permanently deletes the managed folder and its aliases. You may remain signed in, and Keychain items and remote grants may remain.")
        }
    }

    private var header: some View {
        let status = profile.recoverable ? "Delete interrupted" : profile.readiness.title
        let aliases = profile.aliases.joined(separator: ", ")
        return HStack(spacing: SettingsSpacing.control) {
            ProviderTile(provider: profile.provider, size: 44)
            VStack(alignment: .leading, spacing: SettingsSpacing.tight) {
                Text(profile.name)
                    .font(SettingsTypography.title)
                    .accessibilityAddTraits(.isHeader)
                Text("\(profile.provider.title) · \(status)")
                    .font(SettingsTypography.supporting)
                    .foregroundStyle(SettingsTypography.supportingInk)
                if let label = store.usage[profile.id]?.planLabel {
                    Text(label)
                        .font(SettingsTypography.supporting)
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
                Text(aliases.isEmpty ? "No command aliases" : aliases)
                    .font(aliases.isEmpty ? SettingsTypography.supporting : SettingsTypography.supporting.monospaced())
                    .foregroundStyle(SettingsTypography.supportingInk)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(aliases.isEmpty ? "No command aliases" : aliases)
                    .accessibilityLabel(aliases.isEmpty ? "No command aliases" : "Command aliases: \(aliases)")
            }
            Spacer(minLength: SettingsSpacing.control)
            Button("Rename…") { renaming = true }
        }
        .padding(.vertical, SettingsSpacing.tight)
    }

    private var launchActions: some View {
        let terminalReason = store.isFixture ? "Launching is unavailable in sample mode."
            : "Terminal launch is unavailable from Settings."
        let desktopReason = desktopLaunchReason
        return VStack(alignment: .leading, spacing: SettingsSpacing.inline) {
            HStack(spacing: SettingsSpacing.inline) {
                Button { store.perform(.open(profileID: profile.id, surface: .cli)) } label: {
                    Label("Open in Terminal", systemImage: "terminal")
                }
                .disabledReason(terminalReason)
                Button { store.perform(.open(profileID: profile.id, surface: .desktop)) } label: {
                    Label("Open \(profile.provider.desktopApp)", systemImage: "macwindow")
                }
                .disabledReason(desktopReason)
            }
            .labelStyle(.titleAndIcon)
            Text(store.isFixture ? terminalReason
                 : "\(terminalReason) \(desktopReason ?? "Desktop launch requires larp qualification.")")
                .font(SettingsTypography.supporting)
                .foregroundStyle(SettingsTypography.supportingInk)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var desktopLaunchReason: String? {
        if store.isFixture { return "Launching is unavailable in sample mode." }
        if profile.recoverable { return "Finish the interrupted delete first." }
        if bindingUnavailableReason != nil { return "Opening Claude.app requires desktop qualification." }
        if !profile.desktopEnabled { return "Turn on Desktop app to open it." }
        if profile.desktopBinding?.state != .ok { return "Bind \(profile.provider.desktopApp) in Configuration first." }
        return nil
    }

    private var bindingUnavailableReason: String? {
        // The CLI rejects Claude desktop bindings while qualification is pending.
        !store.isFixture && profile.provider == .claude ? "Binding Claude.app requires desktop qualification." : nil
    }

    private var bindingDetail: String {
        if let bindingUnavailableReason { return bindingUnavailableReason }
        switch profile.desktopBinding?.state {
        case .ok:
            return [profile.desktopBinding?.displayPath, profile.desktopBinding.map(\.versionTitle)]
                .compactMap { $0 }.joined(separator: " · ")
        case .invalid:
            return "Binding needs attention. Choose Bind and replace it."
        default:
            return "No app bound. The binding is shared by all \(profile.provider.title) profiles."
        }
    }

    private var attentionDetail: String {
        if profile.desktopBinding?.state != .ok, let bindingUnavailableReason {
            return bindingUnavailableReason
        }
        if profile.desktopBinding?.state == .invalid {
            return "Choose Bind in Configuration to replace the \(profile.provider.desktopApp) binding."
        }
        if profile.desktopBinding?.state == .unbound {
            return "Choose Bind in Configuration to link \(profile.provider.desktopApp)."
        }
        if store.isFixture { return "This sample profile has a configuration warning." }
        return "In Terminal, run larp cc profile show \(profile.name) --json to check the folder, aliases and app binding."
    }

    private func catalogSection(_ title: String, items: [CatalogItem], kind: CatalogKind, empty: String) -> some View {
        Section(title) {
            if items.isEmpty {
                Text(empty).foregroundStyle(SettingsTypography.supportingInk)
            }
            ForEach(items) { item in
                Toggle(isOn: catalogBinding(item, kind: kind)) {
                    Text(item.id)
                    Text(item.subtitle)
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
            }
        }
    }

    private func surfaceBinding(_ surface: Surface) -> Binding<Bool> {
        Binding(get: { surface == .cli ? profile.cliEnabled : profile.desktopEnabled },
                set: { store.perform(.setSurface(profileID: profile.id, surface: surface, enabled: $0)) })
    }

    private func catalogBinding(_ item: CatalogItem, kind: CatalogKind) -> Binding<Bool> {
        Binding(get: { item.profileIDs.contains(profile.id) },
                set: { store.perform(.setCatalog(kind: kind, itemID: item.id, profileID: profile.id, enabled: $0)) })
    }
}
