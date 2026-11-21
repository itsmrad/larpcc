import Foundation
import CryptoKit
#if canImport(Darwin)
import Darwin
#else
import Glibc
#endif

struct ProcessOutput: Sendable { let stdout: Data; let status: Int32 }
protocol CLITransport: Sendable {
    /// `mutation` runs without the read timeout: killing a CLI mid-transaction leaves an unknown outcome.
    func run(_ arguments: [String], mutation: Bool) async throws -> ProcessOutput
}

// Identity is pinned to the explicitly chosen regular file, never resolved via PATH.
public struct SelectedLarp: Sendable {
    let url: URL
    private let identity: FileIdentity
    public init(url: URL) throws {
        guard url.isFileURL, url.path.hasPrefix("/"), url.lastPathComponent == "larp",
              url.standardizedFileURL == url, url.resolvingSymlinksInPath() == url else { throw ClientError.invalidExecutable }
        self.url = url
        identity = try FileIdentity(url)
    }
    func recheck() throws { guard url.resolvingSymlinksInPath() == url, try FileIdentity(url) == identity else { throw ClientError.invalidExecutable } }
}
private struct FileIdentity: Equatable, Sendable {
    let digest: Data
    let device: UInt64; let inode: UInt64; let size: UInt64; let modified: Date; let changed: Date
    init(_ url: URL) throws {
        let fm = FileManager.default
        guard let a = try? fm.attributesOfItem(atPath: url.path), a[.type] as? FileAttributeType == .typeRegular,
              fm.isExecutableFile(atPath: url.path), let dev = a[.systemNumber] as? NSNumber,
              let ino = a[.systemFileNumber] as? NSNumber, let size = a[.size] as? NSNumber,
              let date = a[.modificationDate] as? Date, let created = a[.creationDate] as? Date else { throw ClientError.invalidExecutable }
        // Reject scripts: production selection is a native Mach-O executable only.
        guard let file = try? FileHandle(forReadingFrom: url) else { throw ClientError.invalidExecutable }
        defer { try? file.close() }
        guard size.uint64Value <= 134_217_728 else { throw ClientError.invalidExecutable }
        let magic = try file.read(upToCount: 4) ?? Data()
        let allowed: [[UInt8]] = [[0xcf,0xfa,0xed,0xfe], [0xce,0xfa,0xed,0xfe], [0xca,0xfe,0xba,0xbe], [0xca,0xfe,0xba,0xbf]]
        guard allowed.contains(Array(magic)) else { throw ClientError.invalidExecutable }
        try file.seek(toOffset: 0)
        var hash = SHA256()
        while let bytes = try file.read(upToCount: 65_536), !bytes.isEmpty { hash.update(data: bytes) }
        digest = Data(hash.finalize())
        device = dev.uint64Value; inode = ino.uint64Value; self.size = size.uint64Value; modified = date; changed = created
    }
}

struct ProcessTransport: CLITransport {
    let selected: SelectedLarp
    func run(_ arguments: [String], mutation: Bool) async throws -> ProcessOutput {
        try await ProcessRunner.run(url: selected.url, arguments: arguments, timeout: mutation ? nil : 15,
                                    recheck: { try selected.recheck() })
    }
}

// Internal runner permits synthetic executables in tests; production only constructs ProcessTransport.
enum ProcessRunner {
    static func run(url: URL, arguments: [String], timeout: TimeInterval? = 15,
                    recheck: @escaping @Sendable () throws -> Void = {}) async throws -> ProcessOutput {
        let cancellation = CancellationFlag()
        return try await withTaskCancellationHandler(operation: {
            try await withCheckedThrowingContinuation { continuation in
                DispatchQueue.global(qos: .userInitiated).async {
                    do { continuation.resume(returning: try execute(url, arguments, timeout, cancellation, recheck)) }
                    catch { continuation.resume(throwing: error) }
                }
            }
        }, onCancel: { cancellation.cancel() })
    }
    private static func execute(_ url: URL, _ arguments: [String], _ timeout: TimeInterval?,
                                _ cancellation: CancellationFlag, _ recheck: () throws -> Void) throws -> ProcessOutput {
        let child = Process(), out = Pipe(), err = Pipe()
        child.executableURL = url; child.arguments = arguments
        child.standardOutput = out; child.standardError = err; child.standardInput = FileHandle.nullDevice
        // No inherited provider/account environment or user-controlled dynamic loader variables.
        child.environment = ["PATH": "/usr/bin:/bin", "HOME": NSHomeDirectory(), "LANG": "en_US.UTF-8"]
        try recheck()
        if cancellation.cancelled { throw CancellationError() }
        do { try child.run() } catch { throw ClientError.commandUnavailable }
        out.fileHandleForWriting.closeFile(); err.fileHandleForWriting.closeFile()
        let handles = [out.fileHandleForReading, err.fileHandleForReading]
        defer { handles.forEach { try? $0.close() } }
        for h in handles { _ = fcntl(h.fileDescriptor, F_SETFL, O_NONBLOCK) }
        let start = ProcessInfo.processInfo.systemUptime
        var output = Data(), counts = [0, 0], ended = [false, false]
        var failure: Error?
        var stoppedAt: TimeInterval?
        while child.isRunning || !ended.allSatisfy({ $0 }) {
            let now = ProcessInfo.processInfo.systemUptime
            if failure == nil {
                if cancellation.cancelled { failure = CancellationError() }
                else if let timeout, now - start >= timeout { failure = ClientError.timedOut }
            }
            if failure != nil, stoppedAt == nil {
                stoppedAt = now
                if child.isRunning { child.terminate() }
            }
            if let stop = stoppedAt, now - stop > 0.25 {
                if child.isRunning { _ = kill(child.processIdentifier, SIGKILL) }
                // Descendants may inherit pipes; never wait on or kill them.
                ended = [true, true]
            }
            for index in handles.indices where !ended[index] {
                var bytes = [UInt8](repeating: 0, count: 8192)
                let n = read(handles[index].fileDescriptor, &bytes, bytes.count)
                if n > 0 {
                    counts[index] += n
                    if counts[index] > CLIContract.limit { failure = ClientError.outputLimit }
                    else if index == 0 { output.append(contentsOf: bytes.prefix(n)) }
                } else if n == 0 { ended[index] = true }
                else if errno != EAGAIN && errno != EINTR { failure = ClientError.invalidResponse; ended[index] = true }
            }
            if child.isRunning || !ended.allSatisfy({ $0 }) { Thread.sleep(forTimeInterval: 0.002) }
        }
        child.waitUntilExit() // Only on this background queue, after draining both pipes.
        if let failure { throw failure }
        guard counts[1] == 0 else { throw ClientError.invalidResponse }
        return ProcessOutput(stdout: output, status: child.terminationStatus)
    }
}
private final class CancellationFlag: @unchecked Sendable {
    private let lock = NSLock()
    private var value = false
    var cancelled: Bool { lock.lock(); defer { lock.unlock() }; return value }
    func cancel() { lock.lock(); value = true; lock.unlock() }
}
