#if DEBUG
import AppKit
import LarpModel

/// `--settings-snapshot=<abs path.png>`: waits for the real Settings window to open with sample
/// data, captures it with its window chrome and materials, and exits. Combine it with
/// `--page=`, `--theme=` and `--appearance=`. It first asks `screencapture -l` for the window
/// (real materials and glass), then falls back to drawing the window into a bitmap.
/// `--settings-snapshot-method=cache` skips screencapture. The method used goes to stderr.
/// `--sample-profile-ids` prints the sample profile IDs (one per line) and exits.
/// Debug builds only; sample data only. It never runs larp or reads the user's data.
enum SettingsSnapshot {
    private static let prefix = "--settings-snapshot="

    static func runIfRequested() {
        if CommandLine.arguments.contains("--sample-profile-ids") {
            Task {
                let ids = (try? await FixtureLarpClient().snapshot().profiles.map(\.id)) ?? []
                print(ids.joined(separator: "\n"))
                exit(ids.isEmpty ? 1 : 0)
            }
            return
        }
        guard let arg = CommandLine.arguments.first(where: { $0.hasPrefix(prefix) }) else { return }
        let path = String(arg.dropFirst(prefix.count))
        guard path.hasPrefix("/"), path.lowercased().hasSuffix(".png") else { exit(2) }
        Task { @MainActor in exit(await capture(to: path) ? 0 : 1) }
    }

    @MainActor private static func capture(to path: String) async -> Bool {
        var window: NSWindow?
        for _ in 0..<150 {
            window = NSApp.windows.first { $0.isVisible && $0.styleMask.contains(.titled) }
            if window != nil { break }
            try? await Task.sleep(nanoseconds: 100_000_000)
        }
        guard let window else { log("no Settings window"); return false }
        MenuBarDockPolicy.showDockIcon()
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
        window.center()
        // Sample data loads at once; this covers layout, sidebar materials and glass settling.
        try? await Task.sleep(nanoseconds: 2_500_000_000)
        window.displayIfNeeded()
        if !CommandLine.arguments.contains("--settings-snapshot-method=cache"),
           screencapture(window.windowNumber, to: path) {
            log("method=screencapture"); return true
        }
        guard cache(window, to: path) else { log("capture failed"); return false }
        log("method=cache"); return true
    }

    @MainActor private static func screencapture(_ windowNumber: Int, to path: String) -> Bool {
        try? FileManager.default.removeItem(atPath: path)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        process.arguments = ["-x", "-o", "-l", String(windowNumber), path]
        guard (try? process.run()) != nil else { return false }
        process.waitUntilExit()
        return process.terminationStatus == 0 && FileManager.default.fileExists(atPath: path)
    }

    /// Draws the whole window frame, title bar included. Behind-window materials may come out flat.
    @MainActor private static func cache(_ window: NSWindow, to path: String) -> Bool {
        guard let view = window.contentView?.superview ?? window.contentView,
              let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return false }
        view.cacheDisplay(in: view.bounds, to: rep)
        guard let png = rep.representation(using: .png, properties: [:]) else { return false }
        return (try? png.write(to: URL(fileURLWithPath: path))) != nil
    }

    private static func log(_ text: String) {
        FileHandle.standardError.write(Data("settings-snapshot: \(text)\n".utf8))
    }
}
#endif
