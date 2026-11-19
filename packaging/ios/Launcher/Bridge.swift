import Foundation

/// How the game answers: the context the launcher passed, and the answer as NUL-terminated UTF-8 JSON that
/// lives only for the call.
public typealias HorizonDone = @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?) -> Void
/// How the launcher asks: one line of the game's control language (docs/horizon-ctl-commands.md), a context,
/// and where the answer goes. `horizon-ios-launcher` hands this over with `horizon_launcher_present`.
public typealias HorizonSubmit = @convention(c) (UnsafePointer<CChar>?, UnsafeMutableRawPointer?, HorizonDone) -> Void

/// The launcher's line to the game under it (crates/horizon-ios-launcher, crates/horizon-client/src/game/launcher.rs).
/// The game reads the line on its next frame, on this same main thread, so nothing here blocks: the answer
/// arrives through the callback, which resumes the waiting task.
enum Bridge {
    /// Set by `horizon_launcher_present`; nil in a preview, where nothing is under the launcher.
    static var submit: HorizonSubmit?

    /// How long a line waits for the game's answer before the launcher gives up on it. The game reads a
    /// line on its next frame, so this is only reached by a game that stopped answering at all.
    private static let timeout: UInt64 = 15_000_000_000

    /// Send `line` to the game and wait for its JSON answer. A game that cannot answer gives `["error": ...]`.
    @MainActor
    static func send(_ line: String) async -> [String: Any] {
        guard let submit else {
            return ["error": "no game is running under the launcher"]
        }
        // The answer comes back through the C callback, which can arrive on the game's thread after any
        // delay; the timeout is the other way out. Both go through `finish`, and only the first one to
        // reach it resumes the continuation, so a late answer after the timeout is simply dropped.
        let reply = Reply()
        let context = Unmanaged.passRetained(reply).toOpaque()
        return await withCheckedContinuation { continuation in
            reply.arm(continuation)
            line.withCString { submit($0, context, replied) }
            Task {
                try? await Task.sleep(nanoseconds: timeout)
                reply.finish(["error": "no answer"])
            }
        }
    }

    /// `launcher-world` with its arguments as JSON, so a folder with spaces in its name arrives whole.
    @MainActor
    static func world(_ action: String, folder: String, name: String? = nil) async -> [String: Any] {
        var args: [String: String] = ["action": action, "folder": folder]
        args["name"] = name
        guard let data = try? JSONSerialization.data(withJSONObject: args),
              let json = String(data: data, encoding: .utf8)
        else {
            return ["error": "unreadable world name"]
        }
        return await send("launcher-world \(json)")
    }
}

/// One line's answer, handed over once. The game's callback and the timeout both race to `finish`, and the
/// first one through resumes the waiting task; a late answer then finds nothing left to resume. The lock is
/// what makes it safe to hold across the game's thread and the timeout's task.
private final class Reply: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<[String: Any], Never>?

    /// The task that waits for the answer; assigned before the line goes out.
    func arm(_ continuation: CheckedContinuation<[String: Any], Never>) {
        lock.lock()
        self.continuation = continuation
        lock.unlock()
    }

    func finish(_ answer: [String: Any]) {
        lock.lock()
        let continuation = self.continuation
        self.continuation = nil
        lock.unlock()
        continuation?.resume(returning: answer)
    }
}

/// The game calls this once per line it was sent; the context is the retained `Reply`, released here.
private let replied: HorizonDone = { context, text in
    guard let context else { return }
    // Released by whichever call comes last: the answer itself, or the timeout's own `finish` when the
    // game answers after it. A game that never answers keeps the one small object alive, which is the
    // price of never touching it after it is gone.
    let reply = Unmanaged<Reply>.fromOpaque(context).takeRetainedValue()
    let answer = text
        .flatMap { String(cString: $0).data(using: .utf8) }
        .flatMap { (try? JSONSerialization.jsonObject(with: $0)) as? [String: Any] }
    reply.finish(answer ?? ["error": "the game's answer was not JSON"])
}

extension Dictionary where Key == String, Value == Any {
    /// The answer's error, when it has one.
    var error: String? { self["error"] as? String }
}
