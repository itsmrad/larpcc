import SwiftUI
import LarpModel

struct ProfilePage: View {
    @ObservedObject var store: AppStore
    let snapshot: Snapshot
    let profile: Profile
    let onDeleted: () -> Void
    @State private var confirmingDelete = false

    var body: some View {
        Form {
            Section {
                header
                ValueRow("Provider", value: profile.provider.title)
                ValueRow("Command aliases", value: profile.aliases.isEmpty ? "None" : profile.aliases.joined(separator: ", "))
                ValueRow("Folder", value: profile.folder, selectable: true)
                Toggle(isOn: surfaceBinding(.desktop)) {
                    Text("Desktop app")
                    Text("Keeps a separate \(profile.provider.desktopApp) data folder for this profile.")
                }
                ValueRow("App version", value: profile.desktopBinding?.versionTitle ?? "Not detected")
                ValueRow("Account", value: "Not observed")
                if profile.needsAttention {
                    LabeledContent {
                        Label("Needs attention", systemImage: "exclamationmark.triangle.fill")
                            .foregroundStyle(.yellow)
                            .labelStyle(.titleAndIcon)
                    } label: {
                        Text("Status")
                        Text("A folder, alias or app link differs from what larp expects. Run larp cc doctor in Terminal.")
                    }
                }
            }

            catalogSection("MCP Servers", items: snapshot.servers, kind: .mcp,
                           empty: "No MCP servers in the catalog.")
            catalogSection("Skills", items: snapshot.skills, kind: .skill,
                           empty: "No skills in the catalog.")

            Section {
                HStack(spacing: 8) {
                    Button { store.perform(.open(profileID: profile.id, surface: .cli)) } label: {
                        Label("Open in Terminal", systemImage: "terminal")
                    }
                    Button { store.perform(.open(profileID: profile.id, surface: .desktop)) } label: {
                        Label("Open \(profile.provider.desktopApp)", systemImage: "macwindow")
                    }
                    .disabled(!profile.desktopEnabled)
                    Spacer()
                    Button("Delete…", role: .destructive) { confirmingDelete = true }
                        .foregroundStyle(.red)
                }
                .labelStyle(.titleAndIcon)
            } footer: {
                Text(store.isFixture
                     ? "Sample actions only. Launching stays unavailable until qualification is approved."
                     : "Open requests go through larp and stay subject to launch qualification.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .confirmationDialog("Delete “\(profile.name)”?", isPresented: $confirmingDelete, titleVisibility: .visible) {
            Button(store.isFixture ? "Delete Sample Profile" : "Delete Profile Folder", role: .destructive) {
                store.perform(.delete(profileID: profile.id))
                onDeleted()
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(store.isFixture
                 ? "This removes the sample profile and its sample assignments from memory. No folder or alias on disk is removed."
                 : "Larp removes the managed profile folder and its aliases. This deletes files; it is not a metadata-only forget. Keychain items and remote grants may remain, and no logout is performed.")
        }
    }

    private var header: some View {
        HStack(spacing: 12) {
            Image(systemName: profile.provider.symbol)
                .font(.system(size: 24, weight: .regular))
                .foregroundStyle(.primary)
                .frame(width: 36, height: 36)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(profile.name)
                    .font(.system(size: 15, weight: .semibold))
                Text("\(profile.provider.title) · \(profile.readiness.title.lowercased()) · sign-in not observed")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 12)
            Button { store.startRefresh() } label: {
                Image(systemName: "arrow.clockwise")
            }
            .buttonStyle(.borderless)
            .help("Reload local metadata")
            .accessibilityLabel("Reload local metadata")
            Toggle("Command line", isOn: surfaceBinding(.cli))
                .labelsHidden()
                .toggleStyle(.switch)
                .help(profile.cliEnabled ? "Command line on" : "Command line off")
        }
        .padding(.vertical, 4)
    }

    private func catalogSection(_ title: String, items: [CatalogItem], kind: CatalogKind, empty: String) -> some View {
        Section(title) {
            if items.isEmpty {
                Text(empty).foregroundStyle(.secondary)
            }
            ForEach(items) { item in
                Toggle(isOn: catalogBinding(item, kind: kind)) {
                    Text(item.id)
                    Text(item.subtitle)
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
