import Foundation

public enum UsageSource: String, Sendable, CaseIterable, Decodable {
    case local, live
    public var title: String { self == .local ? "Local" : "Live" }
}

public enum UsageReason: String, Sendable, Decodable {
    case notCaptured = "not_captured"
    case notReported = "not_reported"
    case unsupported
    case unverifiedVersion = "unverified_version"
    case resetSinceCapture = "reset_since_capture"
    case resetTimeUnknown = "reset_time_unknown"
    case captureNotInstalled = "capture_not_installed"
    case invalidRecord = "invalid_record"
    case liveDisabled = "live_disabled"
    case signInAgain = "sign_in_again"
    case liveUnavailable = "live_unavailable"

    public var text: String {
        switch self {
        case .notCaptured: return "Not available yet"
        case .notReported: return "Not reported"
        case .unsupported: return "Not supported"
        case .unverifiedVersion: return "Client version unverified"
        case .resetSinceCapture: return "Reset due"
        case .resetTimeUnknown: return "Reset time unknown"
        case .captureNotInstalled: return "Not available"
        case .invalidRecord: return "Invalid usage record"
        case .liveDisabled: return "Live usage disabled"
        case .signInAgain: return "Sign in again in the official client"
        case .liveUnavailable: return "Live usage unavailable"
        }
    }
}

public struct UsagePrediction: Sendable, Equatable {
    public let elapsedPercent: Double
    public let reservePercent: Double
    public let runsOutAt: Date?
    public let lastsUntilReset: Bool?
}

public struct UsageWindow: Sendable, Equatable {
    public let usedPercent: Double?
    public let resetsAt: Date?
    public let capturedAt: Date?
    public let reason: UsageReason?
    public let pace: UsagePrediction?

    public init(usedPercent: Double?, resetsAt: Date?, capturedAt: Date? = nil,
                reason: UsageReason? = nil, pace: UsagePrediction? = nil) {
        self.usedPercent = usedPercent
        self.resetsAt = resetsAt
        self.capturedAt = capturedAt
        self.reason = reason
        self.pace = pace
    }

    public var leftPercent: Double? { usedPercent.map { min(100, max(0, 100 - $0)) } }
}

public struct UsageResetCredits: Sendable, Equatable {
    public enum State: String, Sendable, Decodable {
        case available, none, unsupported
        case notRecorded = "not_recorded"
    }

    public enum Expiry: String, Sendable, Decodable {
        case recorded, none, unsupported
        case notRecorded = "not_recorded"
    }

    public let state: State
    public let count: Int?
    public let expiresState: Expiry
    public let expiresAt: Date?
}

public struct UsageReport: Sendable, Equatable {
    public enum CaptureSource: String, Sendable, Decodable {
        case codexSessionLog = "codex_session_log"
        case claudeStatusline = "claude_statusline"
        case liveEndpoint = "live_endpoint"
    }

    public struct ExtraWindow: Sendable, Equatable {
        public let label: String
        public let window: UsageWindow

        public func title(source: UsageSource?) -> String {
            let suffix = " weekly"
            guard source == .live, label.hasSuffix(suffix) else { return label }
            let name = label.dropLast(suffix.count)
            let fallbackPrefix = "Other limit "
            if name.hasPrefix(fallbackPrefix), Int(name.dropFirst(fallbackPrefix.count)) != nil {
                return label
            }
            return "Weekly · \(name)"
        }
    }

    public let source: UsageSource
    public let captureSource: CaptureSource?
    public let capturedAt: Date?
    public let fiveHour: UsageWindow?
    public let weekly: UsageWindow?
    public let extra: [ExtraWindow]
    public let planLabel: String?
    public let resetCredits: UsageResetCredits?

    public init(source: UsageSource, capturedAt: Date?, fiveHour: UsageWindow?, weekly: UsageWindow?,
                resetCredits: UsageResetCredits?, captureSource: CaptureSource? = nil,
                planLabel: String? = nil, extra: [ExtraWindow] = []) {
        self.source = source
        self.capturedAt = capturedAt
        self.fiveHour = fiveHour
        self.weekly = weekly
        self.resetCredits = resetCredits
        self.captureSource = captureSource
        self.planLabel = planLabel
        self.extra = extra
    }

    public var lowestLeftPercent: Double? {
        ([fiveHour, weekly].compactMap { $0 } + extra.map(\.window)).compactMap(\.leftPercent).min()
    }
}

extension FixtureLarpClient {
    /// Sample usage keyed by profile ID. Profiles without saved data are absent.
    public func usage(source: UsageSource) async throws -> [String: UsageReport] {
        let now = Date()
        let captured = now.addingTimeInterval(-4 * 60)
        func window(_ used: Double, resetMinutes: Double, elapsed: Double, reserve: Double,
                    runsOutSeconds: Double? = nil) -> UsageWindow {
            UsageWindow(usedPercent: used, resetsAt: now.addingTimeInterval(resetMinutes * 60),
                        capturedAt: captured, pace: UsagePrediction(elapsedPercent: elapsed, reservePercent: reserve,
                            runsOutAt: runsOutSeconds.map { captured.addingTimeInterval($0) } ?? now.addingTimeInterval(resetMinutes * 60),
                            lastsUntilReset: runsOutSeconds == nil))
        }
        let sample: [String: UsageReport] = [
            "1": UsageReport(source: source, capturedAt: captured,
                fiveHour: window(38, resetMinutes: 134, elapsed: 54, reserve: 16),
                weekly: window(62, resetMinutes: 5400, elapsed: 46.4, reserve: -15.6, runsOutSeconds: 171956),
                resetCredits: UsageResetCredits(state: .available, count: 2, expiresState: .notRecorded, expiresAt: nil),
                planLabel: "Max 5x", extra: [
                    .init(label: "Opus weekly", window: window(48, resetMinutes: 5400, elapsed: 46.4, reserve: -1.6, runsOutSeconds: 303940)),
                    .init(label: "Sonnet weekly", window: window(32, resetMinutes: 5400, elapsed: 46.4, reserve: 14.4)),
                    .init(label: "Other limit 1", window: UsageWindow(usedPercent: 24, resetsAt: nil,
                        capturedAt: captured, reason: .resetTimeUnknown))
                ]),
            "2": UsageReport(source: source, capturedAt: captured,
                fiveHour: window(91, resetMinutes: 41, elapsed: 85, reserve: -6, runsOutSeconds: 1513),
                weekly: window(70, resetMinutes: 3000, elapsed: 70.2, reserve: 0.2),
                resetCredits: UsageResetCredits(state: .notRecorded, count: nil, expiresState: .notRecorded, expiresAt: nil)),
            "3": UsageReport(source: source, capturedAt: captured,
                fiveHour: window(12, resetMinutes: 260, elapsed: 12, reserve: 0),
                weekly: window(40, resetMinutes: 6060, elapsed: 39.8, reserve: -0.2, runsOutSeconds: 361440),
                resetCredits: UsageResetCredits(state: .none, count: 0, expiresState: .none, expiresAt: nil),
                extra: [
                    .init(label: "Codex", window: UsageWindow(usedPercent: nil, resetsAt: now, reason: .resetSinceCapture)),
                    .init(label: "Other limit 1", window: UsageWindow(usedPercent: nil, resetsAt: nil, reason: .notCaptured))
                ])
        ]
        let ids = Set(try await snapshot().profiles.map(\.id))
        return sample.filter { ids.contains($0.key) }
    }
}
