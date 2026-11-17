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
                        Button("Recover") { store.perform(.recover(profileID: profile.id)) }
                    } label: {
                        Label {
                            Text("Delete was interrupted")
                            Text("Larp can finish recovering this profile's folders.")
                        } icon: {
                            Image(systemName: "exclamationmark.triangle.fill").symbolRenderingMode(.multicolor)
                        }
                    }
                } else if profile.needsAttention {
                    Label {
                        Text("Needs attention")
                        Text("A folder, alias or app link differs from what larp expects.")
                    } icon: {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .symbolRenderingMode(.multicolor)
                    }
                }
                Toggle("Command line", isOn: surfaceBinding(.cli))
                Toggle(isOn: surfaceBinding(.desktop)) {
                    Text("Desktop app")
                    Text("Keeps a separate \(profile.provider.desktopApp) data folder for this profile.")
                }
                LabeledContent {
                    Button("Bind…") { binding = BindTarget(provider: profile.provider, kind: .app) }
                } label: {
                    Text("\(profile.provider.desktopApp)")
                    Text(profile.desktopBinding?.state == .ok
                         ? [profile.desktopBinding?.displayPath, profile.desktopBinding.map(\.versionTitle)].compactMap { $0 }.joined(separator: " · ")
                         : "Not bound. Shared by every \(profile.provider.title) profile.")
                        .truncationMode(.middle)
                }
                ValueRow("Command aliases", value: profile.aliases.isEmpty ? "None" : profile.aliases.joined(separator: ", "))
                ValueRow("Folder", value: profile.folder, selectable: true)
            }

            Section {
                HStack(spacing: 8) {
                    Button { store.perform(.open(profileID: profile.id, surface: .cli)) } label: {
                        Label("Open in Terminal", systemImage: "terminal")
                    }
                    Button { store.perform(.open(profileID: profile.id, surface: .desktop)) } label: {
                        Label("Open \(profile.provider.desktopApp)", systemImage: "macwindow")
                    }
                    .disabled(!profile.desktopEnabled)
                }
                .labelStyle(.titleAndIcon)
            } footer: {
                Text(store.isFixture
                     ? "Sample actions only. Launching stays unavailable until qualification is approved."
                     : "Open requests go through larp and stay subject to launch qualification.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            catalogSection("MCP Servers", items: snapshot.servers, kind: .mcp,
                           empty: "No MCP servers in the catalog.")
            catalogSection("Skills", items: snapshot.skills, kind: .skill,
                           empty: "No skills in the catalog.")

            Section {
                LabeledContent {
                    Button("Forget…") {
                        confirmation = Confirmation(title: "Forget “\(profile.name)”?",
                            message: "Larp stops managing this profile but keeps its folder on disk. Aliases to it are removed. Keychain items and remote grants are unchanged.",
                            button: "Forget Profile", action: .forget(profileID: profile.id))
                    }
                } label: {
                    Text("Forget profile")
                    Text(store.isFixture ? "Removes this sample profile from memory."
                         : "Stops managing it and keeps the folder.")
                }
                LabeledContent {
                    Button("Delete…", role: .destructive) { confirmingDelete = true }
                        .foregroundStyle(.red)
                } label: {
                    Text("Delete profile")
                    Text(store.isFixture
                         ? "Removes this sample profile from memory."
                         : "Removes the managed profile folder and its aliases.")
                }
            }
        }
        .formStyle(.grouped)
        .sheet(isPresented: $renaming) { RenameProfileSheet(store: store, profile: profile) }
        .bindSheet($binding, store: store)
        .confirming($confirmation, store: store)
        .confirmationDialog("Delete “\(profile.name)”?", isPresented: $confirmingDelete, titleVisibility: .visible) {
            Button(store.isFixture ? "Delete Sample Profile" : "Delete Profile Folder", role: .destructive) {
                store.perform(.delete(profileID: profile.id))
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
            ProviderTile(provider: profile.provider, size: 36)
            VStack(alignment: .leading, spacing: 2) {
                Text(profile.name)
                    .font(.system(size: 15, weight: .semibold))
                Text("\(profile.provider.title) · \(profile.readiness.title.lowercased())")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 12)
            Button("Rename…") { renaming = true }
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
