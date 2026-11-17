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

enum TerminalChoice: String, CaseIterable, Identifiable {
    case terminal = "Terminal", iterm = "iTerm", ghostty = "Ghostty", warp = "Warp", wezterm = "WezTerm"
    var id: String { rawValue }
}

enum PreferenceKey {
    static let appearance = "appearance"
    static let terminal = "defaultTerminal"
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
            .frame(width: size, height: size)
            .background(
                RoundedRectangle(cornerRadius: size * 0.26, style: .continuous)
                    .fill(color.gradient)
            )
            .overlay(
                RoundedRectangle(cornerRadius: size * 0.26, style: .continuous)
                    .strokeBorder(.black.opacity(0.08), lineWidth: 0.5)
            )
            .accessibilityHidden(true)
    }
}

extension Provider {
    var desktopApp: String { self == .claude ? "Claude.app" : "ChatGPT.app" }
}

/// Monochrome provider glyph.
struct ProviderGlyph: View {
    let provider: Provider
    var size: CGFloat = 16

    var body: some View {
        Image(systemName: provider.symbol) // neutral SF Symbol, not a vendor mark
            .font(.system(size: size * 0.9, weight: .regular))
            .foregroundStyle(.primary)
            .frame(width: size + 4, height: size + 4)
            .accessibilityLabel(provider.title)
    }
}

extension Readiness {
    var color: Color {
        switch self {
        case .ready: return .green
        case .attention: return .yellow
        case .off: return .secondary
        }
    }
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
    var body: some View {
        Circle().fill(color).frame(width: 7, height: 7).accessibilityHidden(true)
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
        HStack(spacing: 10) {
            icon
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                if let subtitle, !subtitle.isEmpty {
                    Text(subtitle)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }
        }
    }
}

/// Unavailable state shown inside the detail pane.
struct UnavailablePane: View {
    let title: String
    let symbol: String
    let message: String
    var body: some View {
        ContentUnavailableView {
            Label(title, systemImage: symbol)
        } description: {
            Text(message)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
