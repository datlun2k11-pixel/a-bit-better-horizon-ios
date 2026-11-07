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

/// The logs in one `.zip`, so a bug report is one attachment instead of four. Store-only (no compression):
/// log text compresses poorly next to the code a compressor would need, and the files are small anyway.
enum LogsArchive {
    static func write(_ files: [URL]) -> URL? {
        let entries = files.compactMap { file -> (name: String, data: Data)? in
            guard let data = try? Data(contentsOf: file) else { return nil }
            return (file.lastPathComponent, data)
        }
        guard !entries.isEmpty else { return nil }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("Horizon-logs.zip")
        do {
            try zip(entries).write(to: url, options: .atomic)
            return url
        } catch {
            return nil
        }
    }

    private static func zip(_ entries: [(name: String, data: Data)]) -> Data {
        var archive = Data()
        var central = Data()
        let (time, date) = dosTime(Date())
        for entry in entries {
            let name = Data(entry.name.utf8)
            let size = UInt32(entry.data.count)
            let crc = crc32(entry.data)
            let offset = UInt32(archive.count)
            archive.append(le(UInt32(0x0403_4b50)))  // local file header
            archive.append(le(UInt16(20)))           // version needed
            archive.append(le(UInt16(0x0800)))       // names are UTF-8
            archive.append(le(UInt16(0)))            // stored
            archive.append(le(time))
            archive.append(le(date))
            archive.append(le(crc))
            archive.append(le(size))
            archive.append(le(size))
            archive.append(le(UInt16(name.count)))
            archive.append(le(UInt16(0)))            // extra field
            archive.append(name)
            archive.append(entry.data)

            central.append(le(UInt32(0x0201_4b50)))  // central directory record
            central.append(le(UInt16(20)))           // version made by
            central.append(le(UInt16(20)))           // version needed
            central.append(le(UInt16(0x0800)))
            central.append(le(UInt16(0)))
            central.append(le(time))
            central.append(le(date))
            central.append(le(crc))
            central.append(le(size))
            central.append(le(size))
            central.append(le(UInt16(name.count)))
            central.append(le(UInt16(0)))            // extra field
            central.append(le(UInt16(0)))            // comment
            central.append(le(UInt16(0)))            // disk
            central.append(le(UInt16(0)))            // internal attributes
            central.append(le(UInt32(0)))            // external attributes
            central.append(le(offset))
            central.append(name)
        }
        let start = UInt32(archive.count)
        archive.append(central)
        let count = UInt16(entries.count)
        archive.append(le(UInt32(0x0605_4b50)))      // end of central directory
        archive.append(le(UInt16(0)))
        archive.append(le(UInt16(0)))
        archive.append(le(count))
        archive.append(le(count))
        archive.append(le(UInt32(central.count)))
        archive.append(le(start))
        archive.append(le(UInt16(0)))
        return archive
    }

    private static func le<T: FixedWidthInteger>(_ value: T) -> Data {
        var value = value.littleEndian
        return withUnsafeBytes(of: &value) { Data($0) }
    }

    private static let crcTable: [UInt32] = (0..<256).map { index -> UInt32 in
        var value = UInt32(index)
        for _ in 0..<8 { value = value & 1 == 1 ? 0xEDB8_8320 ^ (value >> 1) : value >> 1 }
        return value
    }

    private static func crc32(_ data: Data) -> UInt32 {
        var crc: UInt32 = 0xFFFF_FFFF
        for byte in data { crc = crcTable[Int((crc ^ UInt32(byte)) & 0xFF)] ^ (crc >> 8) }
        return crc ^ 0xFFFF_FFFF
    }

    private static func dosTime(_ date: Date) -> (UInt16, UInt16) {
        let parts = Calendar(identifier: .gregorian).dateComponents(
            [.year, .month, .day, .hour, .minute, .second], from: date)
        let year = UInt16(clamping: (parts.year ?? 1980) - 1980)
        let time = UInt16((parts.hour ?? 0) << 11 | (parts.minute ?? 0) << 5 | (parts.second ?? 0) / 2)
        let day = year << 9 | UInt16(parts.month ?? 1) << 5 | UInt16(parts.day ?? 1)
        return (time, day)
    }
}
