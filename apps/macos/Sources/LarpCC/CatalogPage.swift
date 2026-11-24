import SwiftUI
import LarpModel

// MARK: - MCP servers and skills

struct CatalogPage: View {
    @ObservedObject var store: AppStore
    let snapshot: Snapshot
    let kind: CatalogKind
    @State private var confirmation: Confirmation?

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
                                    message: "Larp switches every current MCP server on for every active profile, and new servers and profiles start with them on. Turning this off later keeps the switches as they are.",
                                    button: "Turn On", action: .setSyncNewServers(true))
                            } else {
                                store.perform(.setSyncNewServers(false))
                            }
                        }
                    )) {
                        Text("New MCP servers start on for every profile")
                        Text("Turning this on also switches every current server on for every active profile.")
                    }
                }
                LabeledContent {
                    Button("Sync Now") { store.perform(kind == .mcp ? .syncServers : .syncSkills) }
                } label: {
                    Text(kind == .mcp ? "Server files" : "Skill links")
                    Text(kind == .mcp ? "Rewrites each profile's server files from these switches." : "Relinks each profile's skills from these switches.")
                }
            }

            if items.isEmpty {
                Section {
                    LabeledContent {
                        Button(kind == .mcp ? "Add Server…" : "Add Skill…") { store.sheet = kind == .mcp ? .addServer : .addSkill }
                    } label: {
                        Text(kind == .mcp ? "No MCP servers yet" : "No skills yet")
                        Text(kind == .mcp ? "Add a server by its program or URL." : "Add a folder that contains SKILL.md.")
                    }
                }
            }
            // One section per item: a header row, then a switch per profile.
            ForEach(items) { item in
                Section {
                    LabeledContent {
                        HStack(spacing: 10) {
                            Text("\(item.profileIDs.count) of \(snapshot.profiles.count) on")
                                .monospacedDigit()
                            menu(for: item)
                        }
                    } label: {
                        RowLabel(item.id, subtitle: item.subtitle) {
                            IconTile(symbol: kind == .mcp ? "server.rack" : "sparkles",
                                     color: kind == .mcp ? .indigo : .pink)
                        }
                    }
                    ForEach(snapshot.profiles) { profile in
                        Toggle(isOn: Binding(
                            get: { item.profileIDs.contains(profile.id) },
                            set: { store.perform(.setCatalog(kind: kind, itemID: item.id, profileID: profile.id, enabled: $0)) }
                        )) {
                            Label {
                                Text(profile.name).accessibilityLabel("\(profile.name), \(item.id)")
                            } icon: {
                                ProviderGlyph(provider: profile.provider, size: 14)
                            }
                        }
                        .foregroundStyle(profile.readiness == .off ? .secondary : .primary)
                    }
                } footer: {
                    if item.id == items.last?.id {
                        Text(kind == .mcp
                             ? "Switches choose which profiles receive each server definition. LarpCC never starts a server."
                             : "Switches choose which profiles can use each skill.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .themedFormStyle()
        .confirming($confirmation, store: store)
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
