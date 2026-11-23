import SwiftUI
import AppKit
import LarpModel

// MARK: - Tokens

/// How profile cards are drawn in the menu bar panel.
enum PanelCardStyle {
    /// Liquid Glass cards floating on the panel (Tahoe).
    case glass
    /// No box: rows on the menu material, split by hairlines (Sequoia).
    case flat
    /// Filled rounded tiles with a soft shadow (Sonoma).
    case widget
}

/// The few values that differ between themes. Views read these instead of branching on the theme.
struct ThemeTokens {
    // Menu bar panel
    /// Opaque panel with glass cards (Tahoe), or the classic translucent menu material.
    let panelMaterial: Bool
    let card: PanelCardStyle
    /// Cards' inset from the panel edge and their corner radius (concentric with the
    /// 16 pt panel window on macOS 26: radius + inset = 16 for the card styles that box).
    let cardInset: CGFloat
    let cardRadius: CGFloat
    let cardPadding: CGFloat
    let cardSpacing: CGFloat
    /// Footer rows in their own card (Tahoe), or as plain menu items after a separator.
    let footerInCard: Bool
    /// Menu-item highlight: the accent colour (classic menus) or a neutral fill.
    let accentMenuHighlight: Bool
    let menuRowRadius: CGFloat
    /// Status item percentage weight.
    let statusWeight: Font.Weight

    // Settings window
    /// The floating, inset glass sidebar of macOS 26, or an edge-to-edge material sidebar.
    let floatingSidebar: Bool
    let sidebarRowSize: SidebarRowSize
    /// Page icon tile corner radius as a fraction of its size.
    let iconTileRadius: CGFloat
    /// Grouped form sections drawn by LarpCC (radius), or nil for the system's own.
    let formSectionRadius: CGFloat?
}

extension Theme {
    var tokens: ThemeTokens {
        switch self {
        case .tahoe:
            return ThemeTokens(panelMaterial: false, card: .glass, cardInset: 8, cardRadius: 8, cardPadding: 12,
                               cardSpacing: 8, footerInCard: true, accentMenuHighlight: false, menuRowRadius: 4,
                               statusWeight: .medium, floatingSidebar: true, sidebarRowSize: .medium,
                               iconTileRadius: 0.26, formSectionRadius: nil)
        case .sequoia:
            return ThemeTokens(panelMaterial: true, card: .flat, cardInset: 5, cardRadius: 6, cardPadding: 10,
                               cardSpacing: 0, footerInCard: false, accentMenuHighlight: true, menuRowRadius: 5,
                               statusWeight: .regular, floatingSidebar: false, sidebarRowSize: .medium,
                               iconTileRadius: 0.24, formSectionRadius: 10)
        case .sonoma:
            return ThemeTokens(panelMaterial: true, card: .widget, cardInset: 6, cardRadius: 10, cardPadding: 10,
                               cardSpacing: 6, footerInCard: false, accentMenuHighlight: true, menuRowRadius: 4,
                               statusWeight: .regular, floatingSidebar: false, sidebarRowSize: .small,
                               iconTileRadius: 0.22, formSectionRadius: 8)
        }
    }
}

// MARK: - Environment

private struct ThemeKey: EnvironmentKey {
    static let defaultValue = ThemePreference.current
}

extension EnvironmentValues {
    var theme: Theme {
        get { self[ThemeKey.self] }
        set { self[ThemeKey.self] = newValue }
    }
}

enum ThemePreference {
    static var osMajor: Int { ProcessInfo.processInfo.operatingSystemVersion.majorVersion }

    /// `--theme=tahoe|sequoia|sonoma` (screenshots) wins over the stored setting.
    static var launchOverride: Theme? {
        CommandLine.arguments.first { $0.hasPrefix("--theme=") }
            .flatMap { Theme(rawValue: String($0.dropFirst("--theme=".count))) }
            .flatMap { ThemeChoice(rawValue: $0.rawValue)?.isAvailable(osMajor: osMajor) == true ? $0 : nil }
    }

    static func resolve(_ stored: String?) -> Theme {
        launchOverride ?? ThemeChoice(stored: stored).resolved(osMajor: osMajor)
    }

    static var current: Theme { resolve(UserDefaults.standard.string(forKey: ThemeChoice.key)) }
}

/// Puts the live Theme setting into a scene's environment, so a change restyles it at once.
private struct Themed: ViewModifier {
    @AppStorage(ThemeChoice.key) private var stored = ThemeChoice.automatic.rawValue

    func body(content: Content) -> some View {
        content.environment(\.theme, ThemePreference.resolve(stored))
    }
}

extension View {
    func themed() -> some View { modifier(Themed()) }
}

// MARK: - Forms

extension View {
    /// Grouped forms in the theme's style. macOS draws grouped forms in the look of the running
    /// release, so LarpCC only draws its own sections when an earlier look runs on macOS 26.
    func themedFormStyle() -> some View { modifier(ThemedFormStyle()) }
}

private struct ThemedFormStyle: ViewModifier {
    @Environment(\.theme) private var theme

    func body(content: Content) -> some View {
        if #available(macOS 26, *), let radius = theme.tokens.formSectionRadius {
            content.formStyle(ClassicFormStyle(radius: radius))
        } else {
            content.formStyle(.grouped)
        }
    }
}

/// Pre-glass grouped sections: a bold header, rows on the control background inside a
/// hairline-bordered rounded box, and inset separators between rows.
@available(macOS 15, *)
private struct ClassicFormStyle: FormStyle {
    let radius: CGFloat

    func makeBody(configuration: Configuration) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                ForEach(sections: configuration.content) { section in
                    VStack(alignment: .leading, spacing: 8) {
                        if !section.header.isEmpty {
                            VStack(alignment: .leading, spacing: 0) { section.header }
                                .font(.headline)
                                .padding(.horizontal, 10)
                        }
                        VStack(alignment: .leading, spacing: 0) {
                            ForEach(Array(section.content.enumerated()), id: \.element.id) { index, row in
                                if index > 0 { Divider().padding(.horizontal, 10) }
                                row
                                    .padding(.horizontal, 10)
                                    .padding(.vertical, 8)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                        .background(shape.fill(Color.primary.opacity(0.035)))
                        .overlay(shape.strokeBorder(Color(nsColor: .separatorColor), lineWidth: 0.5))
                        if !section.footer.isEmpty {
                            VStack(alignment: .leading, spacing: 4) { section.footer }
                                .font(.callout)
                                .foregroundStyle(.secondary)
                                .padding(.horizontal, 10)
                        }
                    }
                }
            }
            .padding(20)
        }
        .labeledContentStyle(ClassicRowStyle())
        .toggleStyle(ClassicToggleStyle())
    }
}

/// Splits a two-line label into a title and a secondary description, as grouped forms do.
@available(macOS 15, *)
private struct FormRowLabel<Label: View>: View {
    let label: Label

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Group(subviews: label) { parts in
                ForEach(Array(parts.enumerated()), id: \.element.id) { index, part in
                    if index == 0 {
                        part
                    } else {
                        part.font(.callout).foregroundStyle(.secondary)
                    }
                }
            }
        }
    }
}

@available(macOS 15, *)
private struct ClassicRowStyle: LabeledContentStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(alignment: .center, spacing: 12) {
            FormRowLabel(label: configuration.label)
            Spacer(minLength: 12)
            configuration.content
        }
    }
}

@available(macOS 15, *)
private struct ClassicToggleStyle: ToggleStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(alignment: .center, spacing: 12) {
            FormRowLabel(label: configuration.label)
            Spacer(minLength: 12)
            Toggle("", isOn: configuration.$isOn).labelsHidden().toggleStyle(.switch)
        }
    }
}
