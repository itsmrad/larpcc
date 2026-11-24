import SwiftUI
import LarpModel

// MARK: - MCP servers and skills

struct CatalogPage: View {
    @ObservedObject var store: AppStore
    let snapshot: Snapshot
    let kind: CatalogKind
    @State private var confirmation: Confirmation?
    @State private var expandedServers: Set<String> = []

    private var items: [CatalogItem] { kind == .mcp ? snapshot.servers : snapshot.skills }
    private var noun: String { kind == .mcp ? "server" : "skill" }

    var body: some View {
        Form {
            Section {
                if kind == .mcp {
                    Toggle(isOn: Binding(
                        get: { snapshot.syncNewServers },
                        set: { on in
                            if on {
                                confirmation = Confirmation(title: "Turn on every server for every profile?",
                                    message: "Larp switches every current MCP server on for every active command-line profile. New servers and command-line profiles start with them on. Turning this off later keeps the switches as they are.",
                                    button: "Turn On", action: .setSyncNewServers(true))
                            } else {
                                store.perform(.setSyncNewServers(false))
                            }
                        }
                    )) {
                        Text("Turn on new MCP servers automatically")
                        Text("Turning this on also enables current servers for every active command-line profile.")
                            .foregroundStyle(SettingsTypography.supportingInk)
                    }
                }
                LabeledContent {
                    Button("Sync Now") { store.perform(kind == .mcp ? .syncServers : .syncSkills) }
                } label: {
                    Text(kind == .mcp ? "Server files" : "Skill links")
                    Text(kind == .mcp ? "Rewrites each profile's server files from these switches." : "Relinks each profile's skills from these switches.")
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
            }

            if items.isEmpty {
                Section {
                    LabeledContent {
                        Button(kind == .mcp ? "Add Server…" : "Add Skill…") { store.sheet = kind == .mcp ? .addServer : .addSkill }
                    } label: {
                        Text(kind == .mcp ? "No MCP servers yet" : "No skills yet")
                        Text(kind == .mcp ? "Add a server by its program or URL." : "Add a folder that contains SKILL.md.")
                            .foregroundStyle(SettingsTypography.supportingInk)
                    }
                }
            } else if kind == .mcp {
                Section {
                    ForEach(items) { item in
                        let expanded = expandedServers.contains(item.id)
                        HStack(spacing: SettingsSpacing.control) {
                            Button {
                                if expanded {
                                    expandedServers.remove(item.id)
                                } else {
                                    expandedServers.insert(item.id)
                                }
                            } label: {
                                HStack(spacing: SettingsSpacing.inline) {
                                    Image(systemName: expanded ? "chevron.down" : "chevron.right")
                                        .font(SettingsTypography.caption.weight(.semibold))
                                        .foregroundStyle(SettingsTypography.supportingInk)
                                        .frame(width: SettingsSpacing.control)
                                        .accessibilityHidden(true)
                                    itemLabel(item, symbol: "server.rack", color: .indigo)
                                    Spacer(minLength: SettingsSpacing.inline)
                                    enabledCount(item)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("\(item.id), \(enabledDescription(item))")
                            .accessibilityValue(expanded ? "Expanded" : "Collapsed")
                            .accessibilityHint(expanded ? "Hide profile switches" : "Show profile switches")
                            menu(for: item)
                        }
                        if expanded {
                            ForEach(snapshot.profiles) { profile in
                                profileSwitch(item, profile: profile)
                                    .padding(.leading, SettingsSpacing.page)
                            }
                            if snapshot.profiles.isEmpty {
                                Text("No profiles yet")
                                    .foregroundStyle(SettingsTypography.supportingInk)
                                    .padding(.leading, SettingsSpacing.page)
                            }
                        }
                    }
                } footer: {
                    Text("Switches choose which profiles receive each server definition. LarpCC never starts a server.")
                        .foregroundStyle(SettingsTypography.supportingInk)
                }
            } else {
                ForEach(items) { item in
                    Section {
                        HStack(spacing: SettingsSpacing.control) {
                            itemLabel(item, symbol: "sparkles", color: .pink)
                            Spacer(minLength: SettingsSpacing.inline)
                            enabledCount(item)
                            menu(for: item)
                        }
                        ForEach(snapshot.profiles) { profile in
                            profileSwitch(item, profile: profile)
                        }
                    } footer: {
                        if item.id == items.last?.id {
                            Text("Switches choose which profiles can use each skill.")
                                .foregroundStyle(SettingsTypography.supportingInk)
                        }
                    }
                }
            }
        }
        .themedFormStyle()
        .settingsAnimation(value: expandedServers)
        .confirming($confirmation, store: store)
    }

    private func itemLabel(_ item: CatalogItem, symbol: String, color: Color) -> some View {
        HStack(spacing: SettingsSpacing.inline) {
            IconTile(symbol: symbol, color: color)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.id)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(item.id)
                if !item.subtitle.isEmpty {
                    Text(item.subtitle)
                        .font(SettingsTypography.caption)
                        .foregroundStyle(SettingsTypography.supportingInk)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .help(item.subtitle)
                }
            }
        }
    }

    private func enabledDescription(_ item: CatalogItem) -> String {
        "\(item.profileIDs.count) of \(snapshot.profiles.count) on"
    }

    private func enabledCount(_ item: CatalogItem) -> some View {
        Text(enabledDescription(item))
            .monospacedDigit()
            .foregroundStyle(SettingsTypography.supportingInk)
            .fixedSize()
    }

    private func profileSwitch(_ item: CatalogItem, profile: Profile) -> some View {
        let enabled = item.profileIDs.contains(profile.id)
        // An inactive CLI surface cannot use a skill. A saved fixture switch can still be cleared.
        let reason = kind == .skill && !profile.cliEnabled && !enabled ? "Command line is off" : nil
        return LabeledContent {
            Toggle(isOn: Binding(
                get: { enabled },
                set: { store.perform(.setCatalog(kind: kind, itemID: item.id, profileID: profile.id, enabled: $0)) }
            )) {
                Text("\(profile.name), \(item.id)")
            }
            .labelsHidden()
            .toggleStyle(.switch)
            .controlSize(.small)
            .disabledReason(reason)
        } label: {
            Label {
                Text(profile.name)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(profile.name)
            } icon: {
                ProviderGlyph(provider: profile.provider, size: 14)
            }
            if let reason {
                Text(reason).foregroundStyle(SettingsTypography.supportingInk)
            }
        }
    }

    private func menu(for item: CatalogItem) -> some View {
        Menu {
            if kind == .mcp {
                Button("Turn On for All Profiles") { store.perform(.setServerForAll(name: item.id, enabled: true)) }
                Button("Turn Off for All Profiles") { store.perform(.setServerForAll(name: item.id, enabled: false)) }
                Divider()
            }
            Button("Remove…", role: .destructive) {
                confirmation = kind == .mcp
                    ? Confirmation(title: "Remove “\(item.id)”?",
                                   message: "Larp deletes this server definition and its switches, and removes it from every profile's server files.",
                                   button: "Remove Server", action: .removeServer(name: item.id))
                    : Confirmation(title: "Remove “\(item.id)”?",
                                   message: item.profileIDs.isEmpty
                                       ? "Larp deletes this skill from its store."
                                       : "Larp refuses while any profile still uses this skill. Turn it off for every profile first.",
                                   button: "Remove Skill", action: .removeSkill(name: item.id))
            }
        } label: {
            Image(systemName: "ellipsis.circle")
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("\(noun.capitalized) actions")
        .accessibilityLabel("\(item.id) actions")
    }
}
