import SwiftUI
import LarpModel

// MARK: - Desktop apps

struct DesktopAppsPage: View {
    @ObservedObject var store: AppStore
    let snapshot: Snapshot
    @State private var binding: BindTarget?

    var body: some View {
        Form {
            Section("Applications") {
                ForEach(Provider.allCases, id: \.self) { provider in
                    appRow(provider)
                }
            }
            Section {
                if snapshot.profiles.isEmpty {
                    Text("No profiles.").foregroundStyle(.secondary)
                }
                ForEach(snapshot.profiles) { profile in
                    LabeledContent {
                        HStack(spacing: 10) {
                            if profile.desktopEnabled {
                                Button("Open") { store.perform(.open(profileID: profile.id, surface: .desktop)) }
                                    .controlSize(.small)
                            }
                            Toggle("Desktop app for \(profile.name)", isOn: Binding(
                                get: { profile.desktopEnabled },
                                set: { store.perform(.setSurface(profileID: profile.id, surface: .desktop, enabled: $0)) }
                            ))
                            .labelsHidden()
                            .toggleStyle(.switch)
                        }
                    } label: {
                        RowLabel(profile.name, subtitle: profile.desktopEnabled ? "Own \(profile.provider.desktopApp) data folder" : "Off") {
                            ProviderGlyph(provider: profile.provider)
                        }
                    }
                }
            } header: {
                Text("Profiles")
            } footer: {
                Text("Open requests go through larp. Launching stays unavailable until qualification is approved.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .themedFormStyle()
        .bindSheet($binding, store: store)
    }

    private func appRow(_ provider: Provider) -> some View {
        let bindings = snapshot.profiles.filter { $0.provider == provider }.compactMap(\.desktopBinding)
        let binding = bindings.first { $0.state == .ok } ?? bindings.first
        let state = [binding?.stateTitle ?? "No profiles", binding?.detectedVersion].compactMap { $0 }.joined(separator: " · ")
        return LabeledContent {
            HStack(spacing: 6) {
                StatusDot(color: binding?.stateColor ?? .secondary)
                Text(state)
                Button("Bind…") { self.binding = BindTarget(provider: provider, kind: .app) }
                    .padding(.leading, 6)
            }
        } label: {
            RowLabel(provider.desktopApp, subtitle: binding?.displayPath ?? "Not linked") {
                DesktopAppIcon(provider: provider)
            }
        }
    }
}
