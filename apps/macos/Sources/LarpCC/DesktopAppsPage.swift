import SwiftUI
import LarpModel

// MARK: - Desktop apps

struct DesktopAppsPage: View {
    @ObservedObject var store: AppStore
    let snapshot: Snapshot
    @State private var binding: BindTarget?

    var body: some View {
        Form {
            Section {
                ForEach(Provider.allCases, id: \.self) { provider in
                    appRow(provider)
                }
            } header: {
                Text("Applications")
            } footer: {
                Text("App not installed? Install it, then choose Bind. Bindings are shared by provider; recorded versions are notes, not qualification.")
                    .font(.footnote)
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
            Section {
                if snapshot.profiles.isEmpty {
                    UnavailablePane(title: "No profiles yet", symbol: "person.crop.circle.badge.plus",
                                    message: "Create a profile to give its desktop app a separate data folder.")
                        .foregroundStyle(SettingsTypography.supportingInk, SettingsTypography.supportingInk)
                    Button("New Profile…") { store.sheet = .addProfile }
                }
                ForEach(snapshot.profiles) { profile in
                    profileRow(profile)
                }
            } header: {
                Text("Profiles")
            } footer: {
                Text(store.isFixture
                     ? "Sample data only. Switches change this preview; no apps will open."
                     : "Open requests go through larp, which checks the app before dispatch. Opening an app does not confirm sign-in or isolation.")
                    .font(.footnote)
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
        }
        .themedFormStyle()
        .bindSheet($binding, store: store)
        .settingsAnimation(value: snapshot.profiles)
    }

    private func appRow(_ provider: Provider) -> some View {
        let bindings = snapshot.profiles.filter { $0.provider == provider }.compactMap(\.desktopBinding)
        let binding = bindings.first { $0.state == .ok } ?? bindings.first
        return LabeledContent {
            Button("Bind…") { self.binding = BindTarget(provider: provider, kind: .app) }
                .accessibilityLabel("Bind \(provider.desktopApp)")
                .disabledReason(store.busy ? "Wait for the current operation to finish." :
                    (!store.isFixture && provider == .claude ? "Claude.app binding is closed while qualification is pending." : nil))
        } label: {
            HStack(spacing: SettingsSpacing.control) {
                DesktopAppIcon(provider: provider)
                VStack(alignment: .leading, spacing: SettingsSpacing.tight) {
                    Text(provider.desktopApp)
                    HStack(spacing: SettingsSpacing.tight) {
                        StatusDot(color: binding?.stateColor ?? .secondary)
                        Text("\(binding?.stateTitle ?? "Binding unavailable") · Version: \(binding?.versionTitle ?? "Not detected")")
                            .foregroundStyle(binding?.state == .invalid ? SettingsTypography.warningInk : SettingsTypography.supportingInk)
                    }
                    .font(SettingsTypography.supporting)
                    if let path = binding?.displayPath {
                        Text(path)
                            .font(SettingsTypography.supporting)
                            .foregroundStyle(SettingsTypography.supportingInk)
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .help(path)
                    } else {
                        Text(binding?.state == .invalid ? "Choose the installed app again with Bind." : "Choose an installed app with Bind.")
                            .font(SettingsTypography.supporting)
                            .foregroundStyle(SettingsTypography.supportingInk)
                    }
                    if provider == .claude {
                        Text(store.isFixture ? "Launch closed · Qualification pending" : "Binding and launch closed · Qualification pending")
                            .font(SettingsTypography.supporting)
                            .foregroundStyle(SettingsTypography.warningInk)
                    }
                }
            }
        }
    }

    private func profileRow(_ profile: Profile) -> some View {
        let reason = openDisabledReason(profile)
        return LabeledContent {
            HStack(spacing: SettingsSpacing.control) {
                if profile.desktopEnabled {
                    Button("Open") { store.perform(.open(profileID: profile.id, surface: .desktop)) }
                        .controlSize(.small)
                        .accessibilityLabel("Open \(profile.provider.desktopApp) for \(profile.name)")
                        .disabledReason(reason)
                }
                Toggle("Desktop app for \(profile.name)", isOn: Binding(
                    get: { profile.desktopEnabled },
                    set: { store.perform(.setSurface(profileID: profile.id, surface: .desktop, enabled: $0)) }
                ))
                .labelsHidden()
                .toggleStyle(.switch)
                .disabledReason(store.busy ? "Wait for the current operation to finish." :
                    (profile.recoverable ? "Finish deleting this profile from its profile page." : nil))
            }
        } label: {
            HStack(spacing: SettingsSpacing.control) {
                ProviderGlyph(provider: profile.provider).accessibilityHidden(true)
                VStack(alignment: .leading, spacing: SettingsSpacing.tight) {
                    Text(profile.name)
                    Text("\(profile.provider.desktopApp) · \(profile.desktopEnabled ? "Separate data folder" : "Desktop app disabled")")
                        .font(SettingsTypography.supporting)
                        .foregroundStyle(SettingsTypography.supportingInk)
                    if profile.desktopEnabled, let reason {
                        Text(reason)
                            .font(SettingsTypography.supporting)
                            .foregroundStyle(SettingsTypography.supportingInk)
                    }
                }
            }
        }
    }

    private func openDisabledReason(_ profile: Profile) -> String? {
        if profile.recoverable { return "Finish deleting this profile from its profile page." }
        if !profile.desktopEnabled { return "Enable the desktop app for this profile first." }
        // The CLI keeps Claude desktop dispatch closed; binding metadata cannot unlock it.
        if profile.provider == .claude { return "Qualification pending — launch closed." }
        if store.isFixture { return "Sample data — no apps will open." }
        guard let binding = profile.desktopBinding else { return "Refresh to load app binding metadata." }
        if binding.state == .unbound { return "Bind an installed app before opening." }
        if binding.state == .invalid { return "App binding needs attention. Choose Bind again." }
        if store.busy { return "Wait for the current operation to finish." }
        return nil
    }
}
