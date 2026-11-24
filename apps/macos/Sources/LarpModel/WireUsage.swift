import Foundation

struct WireUsage: Decodable {
    enum Mode: String, Decodable { case settings, local, live }

    let requested_mode: Mode
    let generated_at: Stamp
    let profiles: [Row]

    func localReports() throws -> [String: UsageReport] {
        guard requested_mode == .local else { throw ClientError.invalidResponse }
        var reports: [String: UsageReport] = [:]
        for row in profiles {
            guard row.mode == .local, row.source != .liveEndpoint,
                  reports.updateValue(row.report, forKey: row.profile_id) == nil else {
                throw ClientError.invalidResponse
            }
        }
        return reports
    }

    struct Stamp: Decodable {
        let date: Date

        init(from decoder: Decoder) throws {
            let value = try decoder.singleValueContainer().decode(String.self)
            let formatter = ISO8601DateFormatter()
            guard let date = formatter.date(from: value), formatter.string(from: date) == value else {
                throw ClientError.invalidResponse
            }
            self.date = date
        }
    }

    struct Row: Decodable {
        let profile_id: String
        let mode: UsageSource
        let provider: String
        let source: UsageReport.CaptureSource?
        let captured_at: Stamp?
        let plan: Plan?
        let windows: Windows
        let reset_credits: Credits

        var report: UsageReport {
            let label = plan.flatMap { plan in
                plan.source == .user || (plan.source == .clientLocal && provider == "codex") ? plan.label : nil
            }
            return UsageReport(source: mode, capturedAt: captured_at?.date, fiveHour: windows.five_hour.window,
                               weekly: windows.weekly.window, resetCredits: reset_credits.value, captureSource: source,
                               planLabel: label, extra: (windows.extra ?? []).map {
                UsageReport.ExtraWindow(label: $0.label, window: $0.value.window)
            })
        }
    }

    struct Plan: Decodable {
        enum Source: String, Decodable { case user, clientLocal = "client_local", liveEndpoint = "live_endpoint" }
        let label: String
        let source: Source
    }

    struct Windows: Decodable {
        let five_hour: Window
        let weekly: Window
        let extra: [Extra]?
    }

    struct Extra: Decodable {
        let label: String
        let value: Window

        init(from decoder: Decoder) throws {
            label = try decoder.container(keyedBy: CodingKeys.self).decode(String.self, forKey: .label)
            value = try Window(from: decoder, allowsUnknownDuration: true)
        }

        private enum CodingKeys: String, CodingKey { case label }
    }

    struct Window: Decodable {
        let window: UsageWindow

        private enum CodingKeys: String, CodingKey {
            case used_percent, resets_at, captured_at, window_seconds, reason, pace
        }

        init(from decoder: Decoder) throws {
            try self.init(from: decoder, allowsUnknownDuration: false)
        }

        init(from decoder: Decoder, allowsUnknownDuration: Bool) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            let used = try c.decodeIfPresent(Double.self, forKey: .used_percent)
            let resets = try c.decodeIfPresent(Stamp.self, forKey: .resets_at)?.date
            let captured = try c.decodeIfPresent(Stamp.self, forKey: .captured_at)?.date
            let seconds = try c.decodeIfPresent(Int.self, forKey: .window_seconds)
            let reason = try c.decodeIfPresent(UsageReason.self, forKey: .reason)
            let pace = try c.decodeIfPresent(Pace.self, forKey: .pace)
            guard used.map({ (0...100).contains($0) && abs($0 * 10 - ($0 * 10).rounded()) < 1e-9 }) ?? true,
                  seconds.map({ (1...31_622_400).contains($0) }) ?? allowsUnknownDuration,
                  (used == nil) == (captured == nil),
                  (reason == nil) == (used != nil && resets != nil),
                  (reason == .resetTimeUnknown) == (used != nil && resets == nil),
                  (reason != .resetSinceCapture || used == nil && resets != nil),
                  pace == nil || (seconds != nil && used != nil && resets != nil) else {
                throw ClientError.invalidResponse
            }
            window = UsageWindow(usedPercent: used, resetsAt: resets, capturedAt: captured,
                                 reason: reason, pace: pace?.value)
        }
    }

    struct Pace: Decodable {
        let value: UsagePrediction

        private enum CodingKeys: String, CodingKey {
            case elapsed_percent, reserve_percent, runs_out_at, lasts_until_reset
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            let elapsed = try c.decode(Double.self, forKey: .elapsed_percent)
            let reserve = try c.decode(Double.self, forKey: .reserve_percent)
            guard (0...100).contains(elapsed), (-100...100).contains(reserve) else {
                throw ClientError.invalidResponse
            }
            value = UsagePrediction(elapsedPercent: elapsed, reservePercent: reserve,
                runsOutAt: try c.decodeIfPresent(Stamp.self, forKey: .runs_out_at)?.date,
                lastsUntilReset: try c.decodeIfPresent(Bool.self, forKey: .lasts_until_reset))
        }
    }

    struct Credits: Decodable {
        let value: UsageResetCredits

        private enum CodingKeys: String, CodingKey { case state, count, expires_state, expires_at }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            let state = try c.decode(UsageResetCredits.State.self, forKey: .state)
            let count = try c.decodeIfPresent(Int.self, forKey: .count)
            let expiresState = try c.decode(UsageResetCredits.Expiry.self, forKey: .expires_state)
            let expiresAt = try c.decodeIfPresent(Stamp.self, forKey: .expires_at)?.date
            let validCount: Bool
            switch state {
            case .available: validCount = count.map { (1...1000).contains($0) } ?? false
            case .none: validCount = count == 0
            case .notRecorded, .unsupported: validCount = count == nil
            }
            guard validCount, (expiresState == .recorded) == (expiresAt != nil) else {
                throw ClientError.invalidResponse
            }
            value = UsageResetCredits(state: state, count: count, expiresState: expiresState, expiresAt: expiresAt)
        }
    }
}
