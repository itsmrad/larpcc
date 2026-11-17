import AppKit

/// `--snapshot-to=<new file>.png`: after launch in sample mode, render the settings
/// window (including its toolbar) into a new PNG and quit. Used for fixture
/// screenshots on machines where screen capture is unavailable. It never
/// overwrites an existing file and writes nothing else.
@MainActor
enum FixtureSnapshot {
    static var requestedURL: URL? {
        guard let arg = CommandLine.arguments.first(where: { $0.hasPrefix("--snapshot-to=") }) else { return nil }
        let path = String(arg.dropFirst("--snapshot-to=".count))
        guard path.hasSuffix(".png"), path.hasPrefix("/") else { return nil }
        return URL(fileURLWithPath: path)
    }

    static func runIfRequested(isSample: Bool) {
        guard let url = requestedURL else { return }
        guard isSample else { exit(1) }
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 2_000_000_000) // one-shot settle delay, not polling
            exit(write(to: url) ? 0 : 1)
        }
    }

    private static func write(to url: URL) -> Bool {
        guard let window = NSApp.windows.first(where: { $0.isVisible && $0.contentView != nil }),
              let view = window.contentView?.superview ?? window.contentView,
              let capture = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return false }
        view.cacheDisplay(in: view.bounds, to: capture)
        guard let output = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: capture.pixelsWide, pixelsHigh: capture.pixelsHigh,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
              let context = NSGraphicsContext(bitmapImageRep: output) else { return false }
        output.size = capture.size
        let rect = NSRect(origin: .zero, size: capture.size)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        // Behind-window materials do not cache; back them with the window color.
        window.effectiveAppearance.performAsCurrentDrawingAppearance {
            NSColor.windowBackgroundColor.setFill()
            rect.fill()
        }
        capture.draw(in: rect)
        NSGraphicsContext.restoreGraphicsState()
        guard let data = output.representation(using: .png, properties: [:]) else { return false }
        return (try? data.write(to: url, options: .withoutOverwriting)) != nil
    }
}
