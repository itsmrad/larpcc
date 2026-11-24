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

func trimmed(_ value: String) -> String { value.trimmingCharacters(in: .whitespacesAndNewlines) }

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
                .disabled(store.busy)
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
