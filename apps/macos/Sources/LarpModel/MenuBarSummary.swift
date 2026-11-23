import Foundation

// Display logic for the menu bar and the profile usage section. Pure values only:
// nothing here reads files, runs processes or touches the network.

/// What the menu bar item shows.
public enum MenuBarMode: String, Sendable, CaseIterable {
    case tightest, icon
    public var title: String { self == .icon ? "Icon only" : "Session usage" }
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

/// "resets in 2 hours 13 minutes" / "resetting now" / "reset time unknown", for VoiceOver.
public enum ResetCountdown {
    public static func spoken(resetsAt: Date?, now: Date) -> String {
        guard let resetsAt else { return "reset time unknown" }
        let left = resetsAt.timeIntervalSince(now)
        return left > 0 ? "resets in \(UsageSpan.spoken(left))" : "resetting now"
    }
}

/// Elapsed-time pace: how much would be left if usage were spread evenly over the window.
public enum UsagePace {
    /// Percentage points of slack on either side of even pace.
    public static let margin = 5.0
    public static let outOfDate = "Out of date — refresh"

    /// True once the window's reset time has passed: the saved numbers describe a window that has ended.
    public static func isOutOfDate(_ window: UsageWindow, now: Date) -> Bool {
        window.resetsAt.map { $0 <= now } ?? false
    }

    /// Compares quota left with time left and projects exhaustion at the average rate, as of the
    /// moment the sample was taken (`capturedAt`), so an older sample never drifts. Without a
    /// capture time there is no pace. After the reset time has passed the estimate is out of date.
    public static func text(_ window: UsageWindow, kind: UsageWindowKind, capturedAt: Date?, now: Date,
                            spoken: Bool = false) -> String? {
        if isOutOfDate(window, now: now) { return spoken ? "out of date, refresh" : outOfDate }
        guard let capturedAt, let remaining = window.resetsAt?.timeIntervalSince(capturedAt),
              remaining > 0, remaining < kind.length else { return nil }
        let elapsed = kind.length - remaining
        let left = window.leftPercent
        let delta = left - remaining / kind.length * 100
        let used = 100 - left

        let status: String
        if abs(delta) <= margin {
            status = "On pace"
        } else {
            let amount = Int(abs(delta).rounded())
            let unit = spoken ? " percent" : "%"
            status = "\(amount)\(unit) in \(delta > 0 ? "reserve" : "deficit")"
        }
        let timeToEmpty = used > 0 ? left / (used / elapsed) : .infinity
        let projection: String
        if timeToEmpty < remaining {
            let span = timeToEmpty == 0 ? (spoken ? "0 minutes" : "0m")
                : (spoken ? UsageSpan.spoken(timeToEmpty) : UsageSpan.compact(timeToEmpty))
            projection = "Runs out in \(span)"
        } else {
            projection = "Lasts until reset"
        }
        return status + (spoken ? ", " : " · ") + projection
    }
}

public enum MenuBarSummary {
    public static func isEnabled(_ profile: Profile) -> Bool { profile.cliEnabled || profile.desktopEnabled }

    /// Session percent left from the chosen enabled profile, or the most constrained one with a session window.
    public static func sessionLeftPercent(provider: Provider, chosenProfileID: String, profiles: [Profile],
                                          usage: [String: UsageReport]) -> Double? {
        let candidates = orderedProfiles(profiles, usage: usage, provider: provider)
        let profile = candidates.first { $0.id == chosenProfileID }
            ?? candidates.first { usage[$0.id]?.fiveHour != nil }
        return profile.flatMap { usage[$0.id]?.fiveHour?.leftPercent }
    }

    /// Enabled profiles grouped by provider. Groups and profiles run tightest first.
    public static func groups(_ profiles: [Profile], usage: [String: UsageReport]) -> [(provider: Provider, profiles: [Profile])] {
        let ordered = orderedProfiles(profiles, usage: usage)
        var providers: [Provider] = []
        for profile in ordered where !providers.contains(profile.provider) { providers.append(profile.provider) }
        return providers.map { provider in (provider, ordered.filter { $0.provider == provider }) }
    }

    public static func tightestLeftPercent(profiles: [Profile], usage: [String: UsageReport]) -> Double? {
        profiles.filter(isEnabled).compactMap { usage[$0.id]?.lowestLeftPercent }.min()
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
    public static func orderedProfiles(_ profiles: [Profile], usage: [String: UsageReport],
                                       provider: Provider? = nil) -> [Profile] {
        profiles.filter { isEnabled($0) && (provider == nil || $0.provider == provider) }.sorted { a, b in
            switch (usage[a.id]?.lowestLeftPercent, usage[b.id]?.lowestLeftPercent) {
            case let (x?, y?) where x != y: return x < y
            case (.some, nil): return true
            case (nil, .some): return false
            default: return a.name == b.name ? a.id < b.id : a.name < b.name
            }
        }
    }

    /// VoiceOver text for one window row, e.g.
    /// "5-hour window, 62 percent left, resets in 2 hours 13 minutes, low".
    public static func spokenRow(kind: UsageWindowKind, window: UsageWindow, capturedAt: Date?, now: Date) -> String {
        var parts = ["\(kind.title) window", "\(Int(window.leftPercent.rounded())) percent left"]
        if !UsagePace.isOutOfDate(window, now: now) {
            parts.append(ResetCountdown.spoken(resetsAt: window.resetsAt, now: now))
        }
        if let word = UsageLevel(leftPercent: window.leftPercent).word { parts.append(word.lowercased()) }
        if let pace = UsagePace.text(window, kind: kind, capturedAt: capturedAt, now: now, spoken: true) {
            parts.append(pace)
        }
        return parts.joined(separator: ", ")
    }
}
