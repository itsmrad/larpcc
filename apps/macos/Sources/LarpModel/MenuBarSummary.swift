import Foundation

// Display logic for the menu bar and the profile usage section. Pure values only:
// nothing here reads files, runs processes or touches the network.

/// What the menu bar item shows next to its glyph.
public enum MenuBarMode: String, Sendable, CaseIterable {
    case tightest, profile, icon
    public var title: String {
        switch self {
        case .tightest: return "Tightest limit"
        case .profile: return "A chosen profile"
        case .icon: return "Icon only"
        }
    }
}

/// The two rolling usage windows and their lengths.
public enum UsageWindowKind: String, Sendable, CaseIterable {
    case fiveHour, weekly
    public var title: String { self == .fiveHour ? "5-hour" : "Weekly" }
    public var length: TimeInterval { self == .fiveHour ? 5 * 3600 : 7 * 86_400 }
    public func window(in report: UsageReport?) -> UsageWindow? {
        self == .fiveHour ? report?.fiveHour : report?.weekly
    }
}

/// Warning level by percent left: low below 25%, critical below 10%.
public enum UsageLevel: Sendable, Equatable {
    case normal, low, critical
    public init(leftPercent: Double) {
        self = leftPercent < 10 ? .critical : leftPercent < 25 ? .low : .normal
    }
    /// Text that goes with the warning tint, so color is never the only signal.
    public var word: String? {
        switch self {
        case .normal: return nil
        case .low: return "Low"
        case .critical: return "Almost out"
        }
    }
}

/// Limit-reset credits wording. Never blank.
public enum ResetAvailability {
    public static func text(credits: Int?) -> String {
        guard let credits, credits >= 0 else { return "Reset info not available" }
        switch credits {
        case 0: return "No resets available"
        case 1: return "1 reset available"
        default: return "\(credits) resets available"
        }
    }
}

/// Compact time spans with minute precision and at most two units.
public enum UsageSpan {
    /// "2h 13m", "3d 1h", "45m". Rounds up by default so a countdown never reads 0m early.
    public static func compact(_ seconds: TimeInterval, roundingUp: Bool = true) -> String {
        let (d, h, m) = parts(seconds, roundingUp: roundingUp)
        if d > 0 { return h > 0 ? "\(d)d \(h)h" : "\(d)d" }
        if h > 0 { return m > 0 ? "\(h)h \(m)m" : "\(h)h" }
        return "\(m)m"
    }

    /// "2 hours 13 minutes", for VoiceOver.
    public static func spoken(_ seconds: TimeInterval) -> String {
        let (d, h, m) = parts(seconds, roundingUp: true)
        func unit(_ n: Int, _ name: String) -> String { "\(n) \(name)\(n == 1 ? "" : "s")" }
        if d > 0 { return h > 0 ? "\(unit(d, "day")) \(unit(h, "hour"))" : unit(d, "day") }
        if h > 0 { return m > 0 ? "\(unit(h, "hour")) \(unit(m, "minute"))" : unit(h, "hour") }
        return unit(m, "minute")
    }

    private static func parts(_ seconds: TimeInterval, roundingUp: Bool) -> (Int, Int, Int) {
        let minutes = max(0, seconds) / 60
        let total = max(1, Int(roundingUp ? minutes.rounded(.up) : minutes.rounded(.down)))
        return (total / 1440, total % 1440 / 60, total % 60)
    }
}

/// "Resets in 2h 13m" / "Resetting now" / "Reset time unknown".
public enum ResetCountdown {
    public static func text(resetsAt: Date?, now: Date) -> String {
        guard let resetsAt else { return "Reset time unknown" }
        let left = resetsAt.timeIntervalSince(now)
        return left > 0 ? "Resets in \(UsageSpan.compact(left))" : "Resetting now"
    }
    public static func spoken(resetsAt: Date?, now: Date) -> String {
        guard let resetsAt else { return "reset time unknown" }
        let left = resetsAt.timeIntervalSince(now)
        return left > 0 ? "resets in \(UsageSpan.spoken(left))" : "resetting now"
    }
}

/// Elapsed-time pace: how much would be left if usage were spread evenly over the window.
public enum UsagePace {
    /// Percentage points of slack before a window counts as ahead of pace.
    public static let margin = 5.0

    /// Percent left on an even pace, or nil when the reset time is unknown.
    public static func expectedLeftPercent(resetsAt: Date?, length: TimeInterval, now: Date) -> Double? {
        guard let resetsAt, length > 0 else { return nil }
        return min(100, max(0, resetsAt.timeIntervalSince(now) / length * 100))
    }

    /// True when the window has less left than an even pace would leave, beyond the margin.
    public static func isAhead(_ window: UsageWindow, kind: UsageWindowKind, now: Date) -> Bool {
        guard let expected = expectedLeftPercent(resetsAt: window.resetsAt, length: kind.length, now: now) else { return false }
        return window.leftPercent < expected - margin
    }
}

public enum MenuBarSummary {
    public static func isEnabled(_ profile: Profile) -> Bool { profile.cliEnabled || profile.desktopEnabled }

    /// Lowest percent left across enabled profiles and both windows.
    public static func tightestLeftPercent(profiles: [Profile], usage: [String: UsageReport]) -> Double? {
        profiles.filter(isEnabled).compactMap { usage[$0.id]?.lowestLeftPercent }.min()
    }

    /// The percent the menu bar shows for a mode, or nil for the glyph alone.
    public static func leftPercent(mode: MenuBarMode, chosenProfileID: String?, profiles: [Profile],
                                   usage: [String: UsageReport]) -> Double? {
        switch mode {
        case .icon: return nil
        case .tightest: return tightestLeftPercent(profiles: profiles, usage: usage)
        case .profile:
            guard let id = chosenProfileID, profiles.contains(where: { $0.id == id }) else { return nil }
            return usage[id]?.lowestLeftPercent
        }
    }

    /// Menu bar text such as "62%", or nil for the glyph alone.
    public static func labelText(mode: MenuBarMode, chosenProfileID: String?, profiles: [Profile],
                                 usage: [String: UsageReport]) -> String? {
        leftPercent(mode: mode, chosenProfileID: chosenProfileID, profiles: profiles, usage: usage)
            .map { "\(Int($0.rounded()))%" }
    }

    /// Template SF Symbol for the menu bar; the needle drops as limits get tight.
    public static func symbolName(leftPercent: Double?) -> String {
        guard let leftPercent else { return "gauge.with.dots.needle.67percent" }
        switch UsageLevel(leftPercent: leftPercent) {
        case .normal: return "gauge.with.dots.needle.67percent"
        case .low: return "gauge.with.dots.needle.33percent"
        case .critical: return "gauge.with.dots.needle.0percent"
        }
    }

    /// Enabled profiles, tightest limit first; profiles without usage last, then by name.
    public static func orderedProfiles(_ profiles: [Profile], usage: [String: UsageReport]) -> [Profile] {
        profiles.filter(isEnabled).sorted { a, b in
            switch (usage[a.id]?.lowestLeftPercent, usage[b.id]?.lowestLeftPercent) {
            case let (x?, y?) where x != y: return x < y
            case (.some, nil): return true
            case (nil, .some): return false
            default: return a.name < b.name
            }
        }
    }

    /// VoiceOver text for one window row, e.g.
    /// "5-hour window, 62 percent left, resets in 2 hours 13 minutes, low".
    public static func spokenRow(kind: UsageWindowKind, window: UsageWindow, now: Date) -> String {
        var parts = ["\(kind.title) window", "\(Int(window.leftPercent.rounded())) percent left",
                     ResetCountdown.spoken(resetsAt: window.resetsAt, now: now)]
        if let word = UsageLevel(leftPercent: window.leftPercent).word { parts.append(word.lowercased()) }
        if UsagePace.isAhead(window, kind: kind, now: now) { parts.append("ahead of pace") }
        return parts.joined(separator: ", ")
    }
}
