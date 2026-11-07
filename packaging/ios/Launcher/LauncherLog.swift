import Foundation

/// The launcher's own log, beside the game's in `Documents/logs/launcher.log`: what the launcher was asked,
/// which update check ran and what it found, so a bug report carries both halves of the session. The game's
/// `logs/latest.log` is written by the engine itself (`horizon-client-render`'s logging).
enum LauncherLog {
    static var folder: URL? {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first?
            .appendingPathComponent("logs", isDirectory: true)
    }

    private static let queue = DispatchQueue(label: "dev.horizon.launcher.log")
    private static let formatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm:ss"
        return formatter
    }()
    /// Past this, the file becomes `launcher-1.log` and a fresh one starts: a log that never stops growing
    /// would fill the phone's storage on a long-lived install.
    private static let limit = 512 * 1024

    static func write(_ message: String) {
        queue.async {
            guard let folder = folder else { return }
            let file = folder.appendingPathComponent("launcher.log")
            let line = "[\(formatter.string(from: Date()))] \(message)\n"
            try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let size = (try? file.resourceValues(forKeys: [.fileSizeKey]))?.fileSize ?? 0
            if size > limit {
                let old = folder.appendingPathComponent("launcher-1.log")
                try? FileManager.default.removeItem(at: old)
                try? FileManager.default.moveItem(at: file, to: old)
            }
            if let handle = try? FileHandle(forWritingTo: file) {
                defer { try? handle.close() }
                _ = try? handle.seekToEnd()
                try? handle.write(contentsOf: Data(line.utf8))
            } else {
                try? Data(line.utf8).write(to: file)
            }
        }
    }
}
