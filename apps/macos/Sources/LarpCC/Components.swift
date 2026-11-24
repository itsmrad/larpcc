import SwiftUI
import AppKit
import LarpModel

// MARK: - Local view preferences (UserDefaults only; never sent to Larp)

enum AppearanceMode: String, CaseIterable, Identifiable {
    case system, light, dark
    var id: String { rawValue }
    var title: String { rawValue.capitalized }

    /// `--appearance=light|dark` (fixture screenshots) wins over the stored preference.
    static var launchOverride: AppearanceMode? {
        if CommandLine.arguments.contains("--appearance=dark") { return .dark }
        if CommandLine.arguments.contains("--appearance=light") { return .light }
        return nil
    }

    @MainActor func apply() {
        switch Self.launchOverride ?? self {
        case .system: NSApp.appearance = nil
        case .light: NSApp.appearance = NSAppearance(named: .aqua)
        case .dark: NSApp.appearance = NSAppearance(named: .darkAqua)
        }
    }
}

enum PreferenceKey {
    static let appearance = "appearance"
}

// MARK: - Glyphs

/// Small colored rounded-square tile used for settings pages.
struct IconTile: View {
    let symbol: String
    let color: Color
    var size: CGFloat = 20
    var body: some View {
        Image(systemName: symbol)
            .font(.system(size: size * 0.56, weight: .semibold))
            .symbolRenderingMode(.monochrome)
            .foregroundStyle(.white)
            .modifier(IconTileSurface(color: color, size: size))
            .accessibilityHidden(true)
    }
}

extension Provider {
    var desktopApp: String { self == .claude ? "Claude.app" : "ChatGPT.app" }
}

/// Monochrome provider glyph, sized like a sidebar symbol.
struct ProviderGlyph: View {
    let provider: Provider
    var size: CGFloat = 16
    @Environment(\.backgroundProminence) private var prominence

    var body: some View {
        // Explicit colors: the sidebar's vibrancy washes out thin shapes drawn in .primary.
        ProviderMark(provider: provider)
            .fill(prominence == .increased ? Color.white : Color(nsColor: .labelColor))
            .frame(width: size, height: size)
            .frame(width: size + 4, height: size + 4)
            .accessibilityLabel(provider.title)
    }
}

/// Provider glyph on a neutral rounded-square tile.
struct ProviderTile: View {
    let provider: Provider
    var size: CGFloat = 20

    var body: some View {
        ProviderMark(provider: provider)
            .fill(.white)
            .padding(size * 0.2)
            .modifier(IconTileSurface(color: provider.tileColor, size: size))
            .accessibilityHidden(true)
    }
}

private struct IconTileSurface: ViewModifier {
    let color: Color
    let size: CGFloat
    @Environment(\.theme) private var theme
    @Environment(\.colorSchemeContrast) private var contrast

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: size * theme.tokens.iconTileRadius)
        content
            .frame(width: size, height: size)
            .background(shape.fill(color.gradient))
            .overlay {
                shape.strokeBorder(.primary.opacity(contrast == .increased ? 0.45 : 0.12), lineWidth: 0.5)
            }
    }
}

/// The installed desktop app's own icon from /Applications, or the provider tile when it isn't installed.
/// Reads only the app bundle's icon through NSWorkspace; never its data.
struct DesktopAppIcon: View {
    let provider: Provider
    var size: CGFloat = 20

    var body: some View {
        let path = "/Applications/\(provider.desktopApp)"
        if FileManager.default.fileExists(atPath: path) {
            Image(nsImage: NSWorkspace.shared.icon(forFile: path))
                .resizable()
                .interpolation(.high)
                .scaledToFit()
                .frame(width: size + 4, height: size + 4)
                .padding(-2)
                .accessibilityHidden(true)
        } else {
            ProviderTile(provider: provider, size: size)
        }
    }
}

extension Readiness {
    var title: String {
        switch self {
        case .ready: return "Configured"
        case .attention: return "Needs attention"
        case .off: return "Off"
        }
    }
}

struct StatusDot: View {
    let color: Color
    @Environment(\.colorSchemeContrast) private var contrast

    var body: some View {
        Circle()
            .fill(color)
            .overlay {
                if contrast == .increased { Circle().strokeBorder(.primary, lineWidth: 0.75) }
            }
            .frame(width: 7, height: 7)
            .accessibilityHidden(true)
    }
}

extension DesktopBinding {
    var stateTitle: String {
        switch state {
        case .ok: return "Linked"
        case .unbound: return "Not linked"
        case .invalid: return "Needs attention"
        }
    }
    var stateColor: Color {
        switch state {
        case .ok: return .green
        case .unbound: return .secondary
        case .invalid: return .yellow
        }
    }
    var versionTitle: String {
        if let detectedVersion { return detectedVersion }
        if let recordedVersion { return "\(recordedVersion) (recorded)" }
        return "Not detected"
    }
}

// MARK: - Rows

/// Read-only key/value row. Values are display strings only.
struct ValueRow: View {
    let title: String
    let value: String
    var selectable = false
    init(_ title: String, value: String, selectable: Bool = false) {
        self.title = title; self.value = value; self.selectable = selectable
    }
    var body: some View {
        LabeledContent(title) {
            if selectable {
                Text(value).lineLimit(1).truncationMode(.middle).textSelection(.enabled).help(value)
            } else {
                Text(value).lineLimit(1).truncationMode(.middle)
            }
        }
    }
}

/// Row title with an optional secondary line and a leading icon.
struct RowLabel<Icon: View>: View {
    let title: String
    let subtitle: String?
    let icon: Icon
    init(_ title: String, subtitle: String? = nil, @ViewBuilder icon: () -> Icon) {
        self.title = title; self.subtitle = subtitle; self.icon = icon()
    }
    var body: some View {
        // Native title + description label so Form rows can wrap and compress.
        Label {
            Text(title)
            if let subtitle, !subtitle.isEmpty {
                Text(subtitle).foregroundStyle(SettingsTypography.supportingInk)
            }
        } icon: {
            icon
        }
    }
}

/// Fills the available detail area for empty, unavailable, and failed states.
/// Provide a short title, an SF Symbol name, and a message that explains the next step.
/// Supply `retry` when the operation can be retried in place.
struct UnavailablePane: View {
    let title: String
    let symbol: String
    let message: String
    var retry: (() -> Void)? = nil

    var body: some View {
        ContentUnavailableView {
            Label(title, systemImage: symbol)
        } description: {
            Text(message)
        } actions: {
            if let retry { Button("Retry", action: retry) }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// Fills the available detail area while its content loads. The title describes the work.
struct LoadingPane: View {
    let title: String

    var body: some View {
        ContentUnavailableView {
            VStack(spacing: SettingsSpacing.control) {
                ProgressView().controlSize(.small).accessibilityHidden(true)
                Text(title)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
