import Foundation

// The look of LarpCC's own content: the Settings sidebar, forms and the menu bar panel.
// Window frames, title bars and the menu bar itself are drawn by macOS and follow the
// running system whatever the theme.

/// A macOS-era look.
public enum Theme: String, Sendable, CaseIterable {
    case tahoe, sequoia, sonoma

    public var title: String { rawValue.capitalized }

    /// The theme of a macOS release: 26 and later Tahoe, 15 Sequoia, earlier Sonoma.
    public static func native(osMajor: Int) -> Theme {
        osMajor >= 26 ? .tahoe : osMajor == 15 ? .sequoia : .sonoma
    }
}

/// The stored Theme setting. Automatic follows the running macOS.
public enum ThemeChoice: String, Sendable, CaseIterable {
    case automatic, tahoe, sequoia, sonoma

    public static let key = "theme"

    /// Unknown or missing stored values read as Automatic.
    public init(stored: String?) {
        self = stored.flatMap(Self.init(rawValue:)) ?? .automatic
    }

    public var title: String { self == .automatic ? "Automatic" : rawValue.capitalized }

    /// Tahoe needs Liquid Glass, which only macOS 26 has.
    public func isAvailable(osMajor: Int) -> Bool { self != .tahoe || osMajor >= 26 }

    /// The theme drawn on this macOS. An unavailable choice falls back to Automatic.
    public func resolved(osMajor: Int) -> Theme {
        switch self {
        case .automatic: return Theme.native(osMajor: osMajor)
        case .tahoe: return isAvailable(osMajor: osMajor) ? .tahoe : Theme.native(osMajor: osMajor)
        case .sequoia: return .sequoia
        case .sonoma: return .sonoma
        }
    }
}
