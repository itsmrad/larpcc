import SwiftUI
import LarpModel

struct SetupBinding: Identifiable {
    let provider: Provider
    let path: String
    let desktop: Bool
    let revision: String?
    var id: String { path }
}

struct SetupBindingSheet: View {
    @ObservedObject var store: AppStore
    let target: SetupBinding
    let onBound: () -> Void
    @State private var replace = false
    @State private var error: String?
    @Environment(\.dismiss) private var dismiss

    private var label: String { target.desktop ? target.provider.desktopApp : target.provider.title }
    private var canBind: Bool {
        store.snapshot != nil && store.snapshot?.revision == target.revision
            && !store.busy && MCPInput.isAbsolutePath(target.path)
            && (!target.desktop || (target.provider == .codex && target.path.hasSuffix(".app")))
    }

    var body: some View {
        EditSheet(title: "Bind \(label)", confirm: "Bind", canConfirm: canBind, error: $error) {
            guard canBind else { return }
            let action: LarpAction = target.desktop
                ? .bindApp(provider: target.provider, path: target.path, version: nil, replace: replace)
                : .bindProvider(provider: target.provider, path: target.path, replace: replace)
            error = nil
            store.perform(action) { failure in
                if let failure {
                    error = failure
                } else {
                    onBound()
                    dismiss()
                }
            }
        } content: {
            Section {
                Text(target.path).lineLimit(2).truncationMode(.middle).help(target.path)
                Toggle("Replace existing binding", isOn: $replace)
            } footer: {
                Text(store.snapshot?.revision == target.revision
                     ? "This changes the shared binding for \(target.provider.title) profiles."
                     : "Local metadata changed. Go back and scan again.")
                    .foregroundStyle(SettingsTypography.supportingInk)
            }
        }
        .interactiveDismissDisabled(store.busy)
    }
}
