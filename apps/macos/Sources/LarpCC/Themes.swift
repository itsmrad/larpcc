import SwiftUI
import AppKit
import LarpModel

// MARK: - Tokens

enum SettingsSpacing {
    static let tight: CGFloat = 4
    static let inline: CGFloat = 8
    static let control: CGFloat = 12
    static let inset: CGFloat = 16
    static let page: CGFloat = 20
}

enum SettingsTypography {
    static let title: Font = .title2.weight(.semibold)
    static let heading: Font = .headline
    static let body: Font = .body
    static let supporting: Font = .callout
    static let caption: Font = .caption

    /// Secondary text on a Settings surface in either appearance.
    static let supportingInk = adaptiveInk(light: (0.35, 0.35, 0.35), dark: (0.76, 0.76, 0.76))
    static let criticalInk = adaptiveInk(light: (0.65, 0.10, 0.12), dark: (1, 0.62, 0.61))
    static let warningInk = adaptiveInk(light: (0.54, 0.30, 0.06), dark: (1, 0.73, 0.45))
    static let successInk = adaptiveInk(light: (0.13, 0.40, 0.23), dark: (0.47, 0.83, 0.56))

    private static func adaptiveInk(light: (CGFloat, CGFloat, CGFloat), dark: (CGFloat, CGFloat, CGFloat)) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            let (red, green, blue) = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? dark : light
            return NSColor(srgbRed: red, green: green, blue: blue, alpha: 1)
        })
    }
}

/// The few values that differ between themes. Views read these instead of branching on the theme.
/// The menu bar panel uses the same values as that theme's Settings window, so the two match.
struct ThemeTokens {
    // Shared by the Settings forms and the panel cards
    /// Grouped section corner radius, fill (primary at this opacity) and hairline border.
    let sectionRadius: CGFloat
    let sectionFill: Double
    let sectionBorder: Bool
    /// Row description size (secondary text under a row title).
    let descriptionSize: CGFloat

    // Menu bar panel
    /// The translucent material behind the panel: the Settings page's (Tahoe) or the
    /// sidebar's (Sequoia, Sonoma), matching what shows the wallpaper in that theme's Settings.
    let panelMaterial: NSVisualEffectView.Material
    let menuRowRadius: CGFloat
    let menuRowHeight: CGFloat
    /// Status item percentage weight.
    let statusWeight: Font.Weight

    // Settings window
    /// The floating, inset glass sidebar of macOS 26, or an edge-to-edge material sidebar.
    let floatingSidebar: Bool
    /// Sidebar density for the classic sidebar; nil leaves the user's system setting alone.
    let sidebarRowSize: SidebarRowSize?
    /// Page icon tile corner radius as a fraction of its size.
    let iconTileRadius: CGFloat
    /// The icon column aligns colored tiles and template provider marks.
    let sidebarIconSize: CGFloat
    /// Insets inside classic form groups on macOS 26.
    let formRowInset: CGFloat
    let formRowVerticalInset: CGFloat
    let formSectionSpacing: CGFloat
    /// LarpCC draws the grouped form sections itself (earlier looks on macOS 26), or leaves
    /// them to the system.
    let drawsFormSections: Bool
}

extension Theme {
    var tokens: ThemeTokens {
        switch self {
        case .tahoe:
            return ThemeTokens(sectionRadius: 12, sectionFill: 0.03, sectionBorder: false, descriptionSize: 11,
                               panelMaterial: .windowBackground, menuRowRadius: 6, menuRowHeight: 24,
                               statusWeight: .medium, floatingSidebar: true, sidebarRowSize: nil,
                               iconTileRadius: 0.26, sidebarIconSize: 20, formRowInset: 10,
                               formRowVerticalInset: 8, formSectionSpacing: 24, drawsFormSections: false)
        case .sequoia:
            return ThemeTokens(sectionRadius: 10, sectionFill: 0.035, sectionBorder: true, descriptionSize: 12,
                               panelMaterial: .sidebar, menuRowRadius: 5, menuRowHeight: 22,
                               statusWeight: .regular, floatingSidebar: false, sidebarRowSize: .medium,
                               iconTileRadius: 0.24, sidebarIconSize: 20, formRowInset: 10,
                               formRowVerticalInset: 8, formSectionSpacing: 22, drawsFormSections: true)
        case .sonoma:
            return ThemeTokens(sectionRadius: 8, sectionFill: 0.035, sectionBorder: true, descriptionSize: 12,
                               panelMaterial: .sidebar, menuRowRadius: 4, menuRowHeight: 22,
                               statusWeight: .regular, floatingSidebar: false, sidebarRowSize: .small,
                               iconTileRadius: 0.22, sidebarIconSize: 18, formRowInset: 10,
                               formRowVerticalInset: 7, formSectionSpacing: 20, drawsFormSections: true)
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

    /// Debug screenshots can override the stored theme with `--theme=tahoe|sequoia|sonoma`.
    static var launchOverride: Theme? {
        #if DEBUG
        return CommandLine.arguments.first { $0.hasPrefix("--theme=") }
            .flatMap { Theme(rawValue: String($0.dropFirst("--theme=".count))) }
            .flatMap { ThemeChoice(rawValue: $0.rawValue)?.isAvailable(osMajor: osMajor) == true ? $0 : nil }
        #else
        return nil
        #endif
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

    /// Animates a local state change with the Settings timing and honors Reduce Motion.
    func settingsAnimation<Value: Equatable>(value: Value) -> some View {
        modifier(SettingsAnimation(value: value))
    }

    /// Disables a control when a reason is present. Keep important reasons visible nearby too.
    /// The surrounding view carries the tooltip so the disabled control can still be explained.
    func disabledReason(_ reason: String?) -> some View {
        ZStack {
            self.disabled(reason != nil)
                .accessibilityHint(reason ?? "")
        }
        .help(reason ?? "")
    }
}

private struct SettingsAnimation<Value: Equatable>: ViewModifier {
    let value: Value
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content.animation(reduceMotion ? nil : .easeInOut(duration: 0.15), value: value)
    }
}

// MARK: - Forms

extension View {
    /// A grouped section box in the theme's style: shared by the Settings forms and the menu
    /// bar panel's cards, so a profile card looks like a Settings group.
    func sectionBox(_ tokens: ThemeTokens, fill: Color? = nil) -> some View {
        let shape = RoundedRectangle(cornerRadius: tokens.sectionRadius, style: .continuous)
        return background(shape.fill(fill ?? Color.primary.opacity(tokens.sectionFill)))
            .overlay {
                if tokens.sectionBorder {
                    shape.strokeBorder(Color(nsColor: .separatorColor), lineWidth: 0.5)
                        .allowsHitTesting(false)
                }
            }
    }
}

extension View {
    /// Grouped forms in the theme's style. macOS draws grouped forms in the look of the running
    /// release, so LarpCC only draws its own sections when an earlier look runs on macOS 26.
    func themedFormStyle() -> some View { modifier(ThemedFormStyle()) }
}

private struct ThemedFormStyle: ViewModifier {
    @Environment(\.theme) private var theme

    func body(content: Content) -> some View {
        if #available(macOS 26, *), theme.tokens.drawsFormSections {
            content.formStyle(ClassicFormStyle(tokens: theme.tokens))
        } else {
            content.formStyle(.grouped)
        }
    }
}

/// Pre-glass grouped sections: a bold header, rows on the control background inside a
/// hairline-bordered rounded box, and inset separators between rows.
@available(macOS 15, *)
private struct ClassicFormStyle: FormStyle {
    let tokens: ThemeTokens
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.colorSchemeContrast) private var contrast

    func makeBody(configuration: Configuration) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: tokens.formSectionSpacing) {
                ForEach(sections: configuration.content) { section in
                    VStack(alignment: .leading, spacing: SettingsSpacing.inline) {
                        if !section.header.isEmpty {
                            VStack(alignment: .leading, spacing: 0) { section.header }
                                .font(SettingsTypography.heading)
                                .padding(.horizontal, tokens.formRowInset)
                        }
                        VStack(alignment: .leading, spacing: 0) {
                            ForEach(Array(section.content.enumerated()), id: \.element.id) { index, row in
                                if index > 0 { Divider().padding(.horizontal, tokens.formRowInset) }
                                row
                                    .padding(.horizontal, tokens.formRowInset)
                                    .padding(.vertical, tokens.formRowVerticalInset)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                        .sectionBox(tokens, fill: colorScheme == .light ? .white.opacity(0.55) : nil)
                        .overlay {
                            if contrast == .increased {
                                RoundedRectangle(cornerRadius: tokens.sectionRadius)
                                    .strokeBorder(.primary.opacity(0.5), lineWidth: 1)
                                    .allowsHitTesting(false)
                            }
                        }
                        if !section.footer.isEmpty {
                            VStack(alignment: .leading, spacing: SettingsSpacing.tight) { section.footer }
                                .font(SettingsTypography.supporting)
                                .foregroundStyle(.secondary)
                                .padding(.horizontal, tokens.formRowInset)
                        }
                    }
                }
            }
            .padding(SettingsSpacing.page)
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
                        part.font(SettingsTypography.supporting).foregroundStyle(SettingsTypography.supportingInk)
                    }
                }
            }
        }
    }
}

@available(macOS 15, *)
private struct ClassicRowStyle: LabeledContentStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(alignment: .center, spacing: SettingsSpacing.control) {
            FormRowLabel(label: configuration.label)
            Spacer(minLength: SettingsSpacing.control)
            configuration.content
        }
    }
}

@available(macOS 15, *)
private struct ClassicToggleStyle: ToggleStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(alignment: .center, spacing: SettingsSpacing.control) {
            FormRowLabel(label: configuration.label).accessibilityHidden(true)
            Spacer(minLength: SettingsSpacing.control)
            // The real switch keeps the label for VoiceOver; the drawn label is for the eye only.
            Toggle(isOn: configuration.$isOn) { configuration.label }
                .labelsHidden()
                .toggleStyle(.switch)
                .controlSize(.small)
        }
    }
}
