import SwiftUI
import UIKit

/// The game's `Documents`: where it keeps saves, screenshots, options.txt and the imported game files.
enum GameFolder {
    static let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
        ?? URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Documents")
    static var saves: URL { documents.appendingPathComponent("saves") }
    static var screenshots: URL { documents.appendingPathComponent("screenshots") }
    static var resources: URL { documents.appendingPathComponent("resources") }
    /// Content `.hmod` mods the game loads, and the folder `horizon-mods.txt` names.
    static var mods: URL { documents.appendingPathComponent("horizon-mods") }
    /// Native `.hnmod` mods; on iOS they cannot be built into the game, but the folder is still the game's.
    static var nativeMods: URL { documents.appendingPathComponent("horizon-native-mods") }
    /// Where picked files are copied before the game validates and moves them in.
    static var imports: URL { documents.appendingPathComponent(".horizon-import") }

    /// Whether imported game files are in place (`import::resources_root`'s second stop).
    static var hasGameFiles: Bool {
        FileManager.default.fileExists(atPath: resources.appendingPathComponent("horizon-resources.json").path)
    }
}

/// A saved world, as the game's `worlds` lists it.
struct World: Identifiable, Equatable {
    let folder: String
    let name: String
    let mode: String
    let lastPlayed: Date
    let version: String
    var id: String { folder }

    init?(_ json: [String: Any]) {
        guard let folder = json["folder"] as? String else { return nil }
        self.folder = folder
        name = json["name"] as? String ?? folder
        mode = json["mode"] as? String ?? "survival"
        lastPlayed = Date(timeIntervalSince1970: (json["last_played"] as? Double ?? 0) / 1000)
        version = json["version"] as? String ?? ""
    }

    /// The picture the game took when the world was last left (`icon.png`), if any.
    var icon: UIImage? {
        UIImage(contentsOfFile: GameFolder.saves.appendingPathComponent(folder).appendingPathComponent("icon.png").path)
    }
}

/// A multiplayer server, as `servers` lists it: its place in servers.dat is its identity.
struct Server: Identifiable, Equatable {
    let index: Int
    let name: String
    let address: String
    let icon: String?
    var id: String { "\(index) \(address)" }
}

/// What a status ping said (`server-ping`).
struct ServerStatus: Equatable {
    var motd = ""
    var online = 0
    var max = 0
    var version = ""
    var ms = 0
    var icon: String?
    var failed: String?
}

/// An archive the game is unpacking (`import::status`): the zip a player dropped into the game's folder,
/// or one downloaded from a link. The launcher draws it over everything while it runs.
struct ImportProgress: Equatable {
    var label = ""
    var files = 0
    var totalFiles = 0
    var bytes: Int64 = 0
    /// Why the archive could not be installed; the game keeps it until the next import starts.
    var error: String?

    init?(_ json: [String: Any]?) {
        guard let json else { return nil }
        label = json["label"] as? String ?? ""
        files = (json["files"] as? NSNumber)?.intValue ?? 0
        totalFiles = (json["total_files"] as? NSNumber)?.intValue ?? 0
        bytes = (json["bytes"] as? NSNumber)?.int64Value ?? 0
        error = json["error"] as? String
    }

    var fraction: Double {
        totalFiles > 0 ? min(1, Double(files) / Double(totalFiles)) : 0
    }
}

/// The game under the launcher: whether it is starting, waiting for files or at its title, and what its
/// title screen would list. Polled while the launcher is up; everything it does goes through `Bridge`.
@MainActor
final class GameLink: ObservableObject {
    enum Phase: Equatable {
        /// No answer yet.
        case unknown
        /// Not started: nothing loads until the player starts the game (Play, a world, a server).
        case waiting
        /// Loading the game files, with how far along.
        case loading(Double)
        /// Nothing to load: the game waits for files (`import_screen.rs`), and says why.
        case needsFiles(String?)
        /// At the title screen: everything works.
        case title
        /// A world is open (the launcher is not normally up then).
        case world
    }

    @Published private(set) var phase: Phase = .unknown
    @Published private(set) var worlds: [World] = []
    @Published private(set) var servers: [Server] = []
    @Published private(set) var statuses: [String: ServerStatus] = [:]
    /// Counts server list reloads, so each one pings the servers again.
    @Published private(set) var serverRound = 0
    @Published private(set) var version = "26.3"
    /// What the Play button says while it waits for the game to be ready to take it somewhere.
    @Published var requestingGameFiles = false
    @Published private(set) var launching: String?
    /// The last thing that went wrong, for an alert.
    @Published var problem: String?
    /// An archive the game is unpacking right now, or the last one that failed.
    @Published private(set) var importing: ImportProgress?
    /// The failed import the player put away: the game keeps saying it until the next one starts.
    private var dismissedImport: String?
    /// The game is still installing what it found in `Documents` at launch.
    @Published private(set) var bootImporting = false
    /// The zip in `Documents` being handed to the game's importer now.
    private var droppedImport: Task<Void, Never>?
    /// The zips already handed over this run: a bad archive is not retried until the next launch.
    private var offeredDrops: Set<String> = []

    /// Uncover the game (Present.swift dismisses the launcher).
    var uncover: () -> Void = {}
    /// Installed by HomeView; every Play/Join/Create waits for an acknowledged session.
    var prepareSession: () async throws -> Void = {
        throw AccountError("Account setup is not ready. Try Play again.")
    }
    /// What to ask as soon as the title screen can take it: Play tapped while the game was loading.
    @MainActor
    private final class LaunchAttempt {
        let id = UUID()
        var submitted = false
        var accepted = false
        var cancelled = false
        var presented = false
        var failure: String?
        var cleanup: Task<Void, Never>?
    }
    private var currentLaunch: LaunchAttempt?
    private var phaseEpoch = UUID()
    private var queued: (id: UUID, action: () async -> [String: Any])?
    private let send: (String) async -> [String: Any]

    init(send: @escaping (String) async -> [String: Any] = { await Bridge.send($0) }) {
        self.send = send
    }

    var ready: Bool { phase == .title }
    /// Whether the worlds and servers can be read: they are files, so before the game starts too.
    var canList: Bool { phase == .title || phase == .waiting }

    /// Follow the game while the launcher is up: quickly while it loads, slowly once it is at its title.
    func watch() async {
        while !Task.isCancelled {
            await refresh()
            let pause: UInt64 = importing != nil
                ? 250_000_000
                : (ready || phase == .waiting ? 3_000_000_000 : 400_000_000)
            try? await Task.sleep(nanoseconds: pause)
        }
    }

    func refresh() async {
        let epoch = phaseEpoch
        let state = await self.send("launcher-state")
        guard epoch == phaseEpoch else { return }
        let was = phase
        switch state["phase"] as? String {
        case "waiting":
            if phase != .waiting { GameAudio.stop() }
            phase = .waiting
        case "loading": phase = .loading(state["progress"] as? Double ?? 0)
        case "needs_files": phase = .needsFiles(state["reason"] as? String)
        case "title": phase = .title
        case "world": phase = .world
        default: phase = .unknown
        }
        if let version = state["version"] as? String {
            self.version = version
        }
        applyImport(state["import"] as? [String: Any])
        bootImporting = state["importing"] as? Bool ?? false
        // A zip copied in while the launcher was up. Play stays disabled until the game files are
        // in, so the launcher is the one that has to hand it to the game's importer.
        if phase == .waiting, !bootImporting, !GameFolder.hasGameFiles, importing == nil {
            importDroppedArchives()
        }
        if phase == .title, was != .title {
            if let action = queued {
                queued = nil
                await run(action.action, attempt: action.id)
            }
        }
    }

    /// Take what the game says it is unpacking. A failure the player already put away stays away until
    /// the next import starts (or succeeds).
    private func applyImport(_ json: [String: Any]?) {
        guard let progress = ImportProgress(json) else {
            importing = nil
            dismissedImport = nil
            return
        }
        if progress.error == nil {
            dismissedImport = nil
        } else if progress.error == dismissedImport {
            importing = nil
            return
        }
        importing = progress
    }

    /// Put away the loading screen: the import is done, or its failure has been read.
    func dismissImport() {
        dismissedImport = importing?.error
        importing = nil
    }

    /// Every `.zip` the Files app put in the game's folder.
    private func zipsInDocuments() -> [URL] {
        let files = (try? FileManager.default.contentsOfDirectory(
            at: GameFolder.documents, includingPropertiesForKeys: nil, options: [.skipsHiddenFiles]
        )) ?? []
        return files.filter { $0.pathExtension.lowercased() == "zip" }
    }

    /// Hand the game's importer every zip in its folder that has not been offered yet. The game
    /// looks at that folder itself at launch, but only before the launcher takes Play away; this is
    /// the same door for a zip copied in while the launcher is up. The game checks what each archive
    /// holds, so its wait screen and this one install the same things. Says whether it found one.
    @discardableResult
    func importDroppedArchives() -> Bool {
        guard droppedImport == nil, !bootImporting, !GameFolder.hasGameFiles else { return false }
        let waiting = zipsInDocuments().filter {
            !$0.lastPathComponent.hasPrefix("horizon-download-")
                && offeredDrops.insert($0.path).inserted
        }
        guard !waiting.isEmpty else { return false }
        droppedImport = Task { @MainActor [weak self] in
            for zip in waiting {
                guard let self else { return }
                LauncherLog.write("offering \(zip.lastPathComponent) to the game's importer")
                let answer = await self.send("import \(zip.path)")
                if let error = answer.error {
                    LauncherLog.write("\(zip.lastPathComponent) was not installed: \(error)")
                    // A leftover archive the game would not take: say why, once.
                    self.problem = "\(zip.lastPathComponent): \(error)"
                } else {
                    LauncherLog.write("installed \(zip.lastPathComponent) as \(answer["kind"] as? String ?? "unknown")")
                }
                await self.refresh()
            }
            self?.droppedImport = nil
        }
        return true
    }

    /// Read the worlds and servers again.
    func reload() async {
        let listed = await self.send("worlds")
        if let list = listed["worlds"] as? [[String: Any]] {
            worlds = list.compactMap(World.init)
        }
        await reloadServers()
    }

    func reloadServers() async {
        let listed = await self.send("servers")
        guard let list = listed["servers"] as? [[String: Any]] else { return }
        servers = list.enumerated().compactMap { index, json in
            guard let address = json["address"] as? String else { return nil }
            return Server(index: index, name: json["name"] as? String ?? address, address: address,
                          icon: json["icon"] as? String)
        }
        serverRound += 1
    }

    // MARK: - Going somewhere

    /// Play: start the game and go to its title screen, or, with a world, into it.
    func play(_ world: World? = nil) async {
        Haptics.launch()
        guard let world else {
            guard let attempt = beginLaunch("Preparing account…") else { return }
            await protectingCancellation(of: attempt) {
                do {
                    try await self.start(attempt)
                    try await self.present(attempt)
                } catch { await self.failed(attempt, error: error) }
            }
            return
        }
        await go("Opening \(world.name)") { await Bridge.world("open", folder: world.folder) }
    }

    func create(_ settings: WorldCreationSettings) async {
        Haptics.launch()
        guard let data = try? JSONEncoder().encode(settings), let json = String(data: data, encoding: .utf8) else {
            problem = "Could not prepare world settings."; return
        }
        await go("Creating \(settings.name)") { await self.send("world-create \(json)") }
    }

    func join(_ address: String) async {
        Haptics.launch()
        let address = address.trimmingCharacters(in: .whitespaces)
        await go("Joining \(address)") { await self.send("connect \(address)") }
    }

    private func beginLaunch(_ label: String) -> LaunchAttempt? {
        guard launching == nil else { return nil }
        // Every entry point (Play, world creation/opening and joining a server)
        // offers setup before starting an engine with missing resources.
        if case .needsFiles = phase {
            requestingGameFiles = true
            return nil
        }
        if phase == .waiting && !GameFolder.hasGameFiles {
            requestingGameFiles = true
            return nil
        }
        let attempt = LaunchAttempt()
        currentLaunch = attempt
        phaseEpoch = UUID()
        launching = label
        return attempt
    }

    private func check(_ attempt: LaunchAttempt) throws {
        try Task.checkCancellation()
        guard currentLaunch === attempt, !attempt.cancelled else { throw CancellationError() }
    }

    /// Task cancellation can arrive while a bridge continuation is suspended.
    /// Cleanup is an independent task, not work inherited by the cancelled caller.
    private func protectingCancellation(of attempt: LaunchAttempt,
                                        operation: @MainActor () async -> Void) async {
        await withTaskCancellationHandler(operation: operation, onCancel: { [weak self] in
            Task { @MainActor in self?.requestCancellation(attempt) }
        })
    }

    private func start(_ attempt: LaunchAttempt) async throws {
        try await prepareSession()
        try check(attempt)
        // The cached phase can predate Quit/reset. Ask the backend on every
        // intent, and assign ownership before it is allowed to start loading.
        attempt.submitted = true
        let answer = await self.send("launcher-start \(attempt.id.uuidString)")
        guard answer["ok"] as? Bool == true, answer.error == nil else {
            try check(attempt)
            throw AccountError(answer.error ?? "The game could not start. Try again.")
        }
        attempt.accepted = true
        try check(attempt)
        phaseEpoch = UUID()
        switch answer["phase"] as? String {
        case "title": phase = .title
        case "world": phase = .world
        case "loading": phase = .loading(0)
        default:
            if phase == .waiting || phase == .unknown { phase = .loading(0) }
        }
    }

    /// Presentation has an explicit backend commit point. Once it commits,
    /// a late cancellation must show that engine instead of leaving it hidden.
    private func present(_ attempt: LaunchAttempt) async throws {
        try check(attempt)
        let answer = await self.send("launcher-commit-start \(attempt.id.uuidString)")
        if answer["ok"] as? Bool == true, answer["status"] as? String == "committed" {
            finishPresentation(attempt)
        } else {
            try check(attempt)
            throw AccountError(answer.error ?? "The game could not be shown. Try again.")
        }
    }

    private func finishPresentation(_ attempt: LaunchAttempt) {
        guard currentLaunch === attempt, !attempt.presented else { return }
        attempt.presented = true
        if queued?.id == attempt.id { queued = nil }
        currentLaunch = nil
        phaseEpoch = UUID()
        if phase == .waiting || phase == .unknown { phase = .loading(0) }
        launching = nil
        uncover()
    }

    private func failed(_ attempt: LaunchAttempt, error: Error) async {
        if error is CancellationError || Task.isCancelled || attempt.cancelled {
            requestCancellation(attempt)
        } else if currentLaunch === attempt {
            if attempt.accepted {
                attempt.failure = error.localizedDescription
                requestCancellation(attempt)
            } else {
                currentLaunch = nil
                launching = nil
                problem = error.localizedDescription
                Haptics.failed()
            }
        }
        if let cleanup = attempt.cleanup { await cleanup.value }
    }

    private func requestCancellation(_ attempt: LaunchAttempt) {
        guard currentLaunch === attempt, !attempt.presented else { return }
        attempt.cancelled = true
        if queued?.id == attempt.id { queued = nil }
        guard attempt.submitted else {
            currentLaunch = nil
            phaseEpoch = UUID()
            launching = nil
            return
        }
        launching = "Stopping game…"
        guard attempt.cleanup == nil else { return }
        attempt.cleanup = Task { @MainActor [weak self] in
            guard let self else { return }
            let answer = await self.send("launcher-cancel-start \(attempt.id.uuidString)")
            guard self.currentLaunch === attempt else { return }
            guard answer["ok"] as? Bool == true,
                  let status = answer["status"] as? String,
                  ["stopped", "stale", "committed"].contains(status) else {
                // Do not enable another Play while shutdown is unproven. A
                // second Cancel retries a failed bridge operation.
                attempt.cleanup = nil
                self.problem = answer.error ?? "The game could not stop. Tap Cancel to retry."
                Haptics.failed()
                return
            }
            if status == "committed" {
                self.finishPresentation(attempt)
                return
            }
            self.currentLaunch = nil
            self.phaseEpoch = UUID()
            self.launching = nil
            if status == "stopped" {
                if self.phase != .waiting { GameAudio.stop() }
                self.phase = .waiting
            }
            if let failure = attempt.failure {
                self.problem = failure
                Haptics.failed()
            }
        }
    }

    /// Ask now, or start and wait until the title can accept the action.
    private func go(_ label: String, _ action: @escaping () async -> [String: Any]) async {
        guard let attempt = beginLaunch(label) else { return }
        await protectingCancellation(of: attempt) {
            do {
                try await self.start(attempt)
                if self.ready {
                    await self.run(action, attempt: attempt.id)
                } else {
                    try self.check(attempt)
                    self.queued = (attempt.id, action)
                }
            } catch { await self.failed(attempt, error: error) }
        }
    }

    private func run(_ action: () async -> [String: Any], attempt id: UUID) async {
        guard let attempt = currentLaunch, attempt.id == id else { return }
        await protectingCancellation(of: attempt) {
            do {
                try self.check(attempt)
                let answer = await action()
                try self.check(attempt)
                if let error = answer.error { throw AccountError(error) }
                try await self.present(attempt)
            } catch { await self.failed(attempt, error: error) }
        }
    }

    /// Before start submission this is immediate. Once submitted, Play remains
    /// disabled until the matching backend cancellation has drained and replied.
    func cancelLaunch() {
        guard let attempt = currentLaunch else { return }
        requestCancellation(attempt)
    }

    // MARK: - Worlds

    func rename(_ world: World, to name: String) async {
        report(await Bridge.world("rename", folder: world.folder, name: name))
        await reload()
    }

    func delete(_ world: World) async {
        report(await Bridge.world("delete", folder: world.folder))
        await reload()
    }

    /// Back a world up into `backups/`, as Edit World's Make Backup does; the size it came to.
    func backup(_ world: World) async -> Int? {
        let answer = await Bridge.world("backup", folder: world.folder)
        report(answer)
        return answer["bytes"] as? Int
    }

    // MARK: - Servers

    func addServer(name: String, address: String) async {
        let address = address.trimmingCharacters(in: .whitespaces)
        guard !address.isEmpty, !address.contains(where: \.isWhitespace) else {
            problem = "A server address has no spaces: a host name or IP, with :port if it is not 25565."
            return
        }
        let name = name.trimmingCharacters(in: .whitespaces)
        report(await self.send("server-add \(address) \(name.isEmpty ? "Minecraft Server" : name)"))
        await reloadServers()
    }

    func removeServer(_ server: Server) async {
        report(await self.send("server-remove \(server.index)"))
        await reloadServers()
    }

    func ping(_ server: Server) async {
        let answer = await self.send("server-ping \(server.address)")
        var status = ServerStatus()
        if let error = answer.error {
            status.failed = error
        } else {
            status.motd = answer["motd"] as? String ?? ""
            status.online = answer["online"] as? Int ?? 0
            status.max = answer["max"] as? Int ?? 0
            status.version = answer["version"] as? String ?? ""
            status.ms = answer["ms"] as? Int ?? 0
            status.icon = answer["icon"] as? String
        }
        statuses[server.address] = status
    }

    private func report(_ answer: [String: Any]) {
        if let error = answer.error {
            problem = error
        }
    }
}

/// The taps a launch is felt by.
enum Haptics {
    static func launch() {
        UIImpactFeedbackGenerator(style: .medium).impactOccurred()
    }

    static func tap() {
        UISelectionFeedbackGenerator().selectionChanged()
    }

    static func failed() {
        UINotificationFeedbackGenerator().notificationOccurred(.error)
    }

    static func done() {
        UINotificationFeedbackGenerator().notificationOccurred(.success)
    }
}
