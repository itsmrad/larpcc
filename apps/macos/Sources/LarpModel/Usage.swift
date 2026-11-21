import Foundation

// Usage limits as reported by `larp cc usage [PROFILE] [--live] --json` (contract pending).
// The app never reads tokens: Local reads data the official apps saved, and Live is
// the CLI's own opt-in mode. These are display values only.

public enum UsageSource: String, Sendable, CaseIterable {
    case local, live
    public var title: String { self == .local ? "Local" : "Live" }
}

public struct UsageWindow: Sendable, Equatable {
    public let usedPercent: Double
    public let resetsAt: Date?
    public init(usedPercent: Double, resetsAt: Date?) { self.usedPercent = usedPercent; self.resetsAt = resetsAt }
    /// Percent left, clamped to 0...100.
    public var leftPercent: Double { min(100, max(0, 100 - usedPercent)) }
}

public struct UsageReport: Sendable, Equatable {
    public let source: UsageSource
    public let capturedAt: Date?
    public let fiveHour: UsageWindow?
    public let weekly: UsageWindow?
    /// Limit-reset credits, when the provider reports them.
    public let resetCredits: Int?
    public init(source: UsageSource, capturedAt: Date?, fiveHour: UsageWindow?, weekly: UsageWindow?, resetCredits: Int?) {
        self.source = source; self.capturedAt = capturedAt; self.fiveHour = fiveHour; self.weekly = weekly; self.resetCredits = resetCredits
    }
    /// The tighter of the two windows, for the sidebar hint.
    public var lowestLeftPercent: Double? { [fiveHour, weekly].compactMap { $0?.leftPercent }.min() }
}

extension FixtureLarpClient {
    /// Sample usage keyed by profile ID. Profiles without saved data are absent.
    public func usage(source: UsageSource) async throws -> [String: UsageReport] {
        let now = Date()
        let captured = source == .live ? now : now.addingTimeInterval(-4 * 60)
        func report(_ five: Double, _ fiveResetMinutes: Double, _ week: Double, _ weekResetHours: Double, credits: Int?) -> UsageReport {
            UsageReport(source: source, capturedAt: captured,
                        fiveHour: UsageWindow(usedPercent: five, resetsAt: now.addingTimeInterval(fiveResetMinutes * 60)),
                        weekly: UsageWindow(usedPercent: week, resetsAt: now.addingTimeInterval(weekResetHours * 3600)),
                        resetCredits: credits)
        }
        let profiles = try await snapshot().profiles.map(\.id)
        let sample: [String: UsageReport] = [
            "1": report(38, 134, 62, 74, credits: 2),
            "2": report(91, 41, 70, 30, credits: nil),
            "3": report(12, 260, 40, 140, credits: 0)
        ]
        return sample.filter { profiles.contains($0.key) }
    }
}
