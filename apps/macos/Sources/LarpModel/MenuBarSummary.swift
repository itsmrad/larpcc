import Foundation

// Display logic for the menu bar and the profile usage section. Pure values only:
// nothing here reads files, runs processes or touches the network.

/// What the menu bar item shows.
public enum MenuBarMode: String, Sendable, CaseIterable {
    case tightest, icon
    public var title: String { self == .icon ? "Icon only" : "Session usage" }
}

/// The two primary rolling usage windows.
public enum UsageWindowKind: String, Sendable, CaseIterable {
    case fiveHour, weekly
    public var title: String { self == .fiveHour ? "5-hour" : "Weekly" }
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
    public static func text(credits: UsageResetCredits?) -> String {
        guard let credits else { return "Reset info not available" }
        switch credits.state {
        case .none: return "No resets available"
        case .unsupported, .notRecorded: return "Reset info not available"
        case .available:
            guard let count = credits.count else { return "Reset info not available" }
            let available = count == 1 ? "1 reset available" : "\(count) resets available"
            guard credits.expiresState == .recorded, let expiresAt = credits.expiresAt else { return available }
            return "\(available) · Expires \(expiresAt.formatted(date: .abbreviated, time: .shortened))"
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

/// Formats the pace supplied by the CLI at this window's capture time.
public enum UsagePace {
    public static func text(_ window: UsageWindow, spoken: Bool = false) -> String? {
        guard let pace = window.pace else { return nil }
        let delta = pace.reservePercent
        let status: String
        if abs(delta) <= 5 {
            status = "On pace"
        } else {
            let amount = Int(abs(delta).rounded())
            let unit = spoken ? " percent" : "%"
            status = "\(amount)\(unit) in \(delta > 0 ? "reserve" : "deficit")"
        }
        let projection: String
        if pace.lastsUntilReset == true {
            projection = "Lasts until reset"
        } else if let runsOut = pace.runsOutAt, let captured = window.capturedAt {
            let seconds = runsOut.timeIntervalSince(captured)
            let span = seconds <= 0 ? (spoken ? "0 minutes" : "0m")
                : (spoken ? UsageSpan.spoken(seconds) : UsageSpan.compact(seconds))
            projection = "Runs out in \(span)"
        } else {
            return status
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
            ?? candidates.first { usage[$0.id]?.fiveHour?.leftPercent != nil }
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

    /// VoiceOver text for a meter or its unavailable state.
    public static func spokenRow(title: String, window: UsageWindow, now: Date) -> String {
        var parts = [title]
        if let left = window.leftPercent {
            parts.append("\(Int(left.rounded())) percent left")
            if let resetsAt = window.resetsAt {
                parts.append(ResetCountdown.spoken(resetsAt: resetsAt, now: now))
            }
            if let word = UsageLevel(leftPercent: left).word { parts.append(word.lowercased()) }
        }
        if let reason = window.reason { parts.append(reason.text) }
        if let pace = UsagePace.text(window, spoken: true) { parts.append(pace) }
        return parts.joined(separator: ", ")
    }
}
