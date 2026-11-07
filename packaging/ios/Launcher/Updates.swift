import Network
import SwiftUI
import UIKit

/// One published build: a release on the repository the launcher checks, parsed from its tag.
struct Release: Identifiable, Equatable {
    let tag: String
    let version: [Int]
    let url: URL

    var id: String { tag }
    var versionText: String { version.map { String($0) }.joined(separator: ".") }
}

/// Checks the repository's releases for one newer than the bundle, at launch and again whenever the network
/// comes back, unless the player turned it off in Settings. The switch, and the last version already
/// announced, live in UserDefaults so each release is offered once.
@MainActor
final class UpdateChecker: ObservableObject {
    /// The repository whose releases are the iOS builds. Change it here if the app moves.
    static let repository = "datlun2k11-pixel/a-bit-better-horizon-ios"
    private static let api = URL(string: "https://api.github.com/repos/\(repository)/releases?per_page=20")!
    private static let enabledKey = "horizon.checkUpdates"
    private static let announcedKey = "horizon.announcedUpdate"

    enum State: Equatable {
        case idle, checking, upToDate
        case available(Release)
        case offline
        case failed(String)
    }

    @Published private(set) var state: State = .idle
    /// The release to announce; cleared when the alert is dismissed or opened.
    @Published var offered: Release?
    @Published private(set) var online = true
    @Published var enabled: Bool {
        didSet {
            UserDefaults.standard.set(enabled, forKey: Self.enabledKey)
            LauncherLog.write("update checks \(enabled ? "on" : "off")")
            if enabled {
                Task { await check() }
            } else {
                state = .idle
                offered = nil
            }
        }
    }

    private let monitor = NWPathMonitor()

    init() {
        enabled = UserDefaults.standard.object(forKey: Self.enabledKey) as? Bool ?? true
        monitor.pathUpdateHandler = { [weak self] path in
            let online = path.status == .satisfied
            // A constant, not the weak var: a concurrent closure may not capture a var.
            let checker = self
            DispatchQueue.main.async {
                guard let checker, checker.online != online else { return }
                checker.online = online
                LauncherLog.write("network \(online ? "back" : "gone"); update check \(online ? "resumed" : "paused")")
                if online {
                    Task { await checker.check() }
                } else if checker.enabled {
                    checker.state = .offline
                }
            }
        }
        monitor.start(queue: DispatchQueue(label: "dev.horizon.launcher.updates"))
    }

    deinit {
        monitor.cancel()
    }

    /// The bundle's own version: the game app around this framework, or the framework when previewed alone.
    static var currentVersion: String {
        let bundle = Bundle.main
        return (bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String)
            ?? (Bundle(for: UpdateChecker.self).object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String)
            ?? "0"
    }

    func check() async {
        guard enabled else {
            state = .idle
            return
        }
        guard online else {
            state = .offline
            LauncherLog.write("update check skipped: offline")
            return
        }
        state = .checking
        LauncherLog.write("update check: bundle \(Self.currentVersion), asking \(Self.repository)")
        var request = URLRequest(url: Self.api)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        request.cachePolicy = .reloadIgnoringLocalCacheData
        do {
            let (data, response) = try await URLSession.shared.data(for: request)
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            switch status {
            case 200:
                break
            case 403:
                // GitHub's hourly limit for requests without an account.
                state = .failed("GitHub asked to wait a while. Try again later.")
                LauncherLog.write("update check failed: GitHub said 403 (rate limit)")
                return
            case 404:
                // The repository has no releases yet.
                state = .upToDate
                LauncherLog.write("update check: repository has no releases")
                return
            default:
                state = .failed("GitHub answered \(status).")
                LauncherLog.write("update check failed: GitHub answered \(status)")
                return
            }
            let mine = Self.parts(Self.currentVersion) ?? []
            guard let newest = Self.parse(data).filter({ Self.isNewer($0.version, than: mine) }).max(by: { Self.isNewer($1.version, than: $0.version) }) else {
                state = .upToDate
                LauncherLog.write("update check: up to date")
                return
            }
            state = .available(newest)
            LauncherLog.write("update check: \(newest.tag) is newer than \(Self.currentVersion)")
            if UserDefaults.standard.string(forKey: Self.announcedKey) != newest.tag {
                UserDefaults.standard.set(newest.tag, forKey: Self.announcedKey)
                offered = newest
            }
        } catch {
            state = online ? .failed(error.localizedDescription) : .offline
            LauncherLog.write("update check failed: \(error.localizedDescription)")
        }
    }

    /// Releases the launcher can offer: published (no drafts), not prereleases (the nightly channel), and with a
    /// tag that starts with numbers, so `v0.2.8` and `0.2.8-ios` both count.
    private static func parse(_ data: Data) -> [Release] {
        guard let list = (try? JSONSerialization.jsonObject(with: data)) as? [[String: Any]] else { return [] }
        return list.compactMap { item in
            guard item["draft"] as? Bool != true,
                  item["prerelease"] as? Bool != true,
                  let tag = item["tag_name"] as? String,
                  let version = parts(tag),
                  let url = (item["html_url"] as? String).flatMap(URL.init(string:))
            else { return nil }
            return Release(tag: tag, version: version, url: url)
        }
    }

    private static func parts(_ text: String) -> [Int]? {
        let numbers = text.drop(while: { !$0.isNumber })
            .prefix(while: { $0.isNumber || $0 == "." })
        let parts = numbers.split(separator: ".").compactMap { Int($0) }
        return parts.isEmpty ? nil : parts
    }

    /// Version arrays compare part by part (`0.2.10` is newer than `0.2.9`); Swift's arrays have no order of their own.
    private static func isNewer(_ a: [Int], than b: [Int]) -> Bool {
        for (lhs, rhs) in zip(a, b) where lhs != rhs { return lhs > rhs }
        return a.count > b.count
    }
}

/// The Settings card: the switch, and what the last check found.
struct UpdateSettings: View {
    @ObservedObject var updates: UpdateChecker

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Toggle(isOn: $updates.enabled) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Check for updates").font(Theme.sans(14, .semiBold)).foregroundStyle(Theme.text)
                    Text("Offers the release page when a newer version is out.")
                        .font(Theme.sans(12)).foregroundStyle(Theme.textMuted)
                }
            }
            .tint(Theme.accent)
            .accessibilityIdentifier("launcher-update-toggle")
            if updates.enabled {
                Divider().overlay(Theme.border)
                status
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .card()
    }

    @ViewBuilder private var status: some View {
        switch updates.state {
        case .idle:
            row(icon: "arrow.clockwise", tone: Theme.textMuted, text: "Not checked yet.") {
                Button("Check now") { Haptics.tap(); Task { await updates.check() } }
                    .font(Theme.sans(12, .semiBold))
            }
        case .checking:
            row(icon: "arrow.triangle.2.circlepath", tone: Theme.textMuted, text: "Checking for updates…") {
                ProgressView().controlSize(.small)
            }
        case .upToDate:
            row(icon: "checkmark.circle", tone: Theme.ready, text: "You're on the latest version.") {
                Button("Check now") { Haptics.tap(); Task { await updates.check() } }
                    .font(Theme.sans(12, .semiBold))
            }
        case let .available(release):
            row(icon: "sparkles", tone: Theme.accentText, text: "Horizon \(release.versionText) is available.") {
                Button("View release page") { open(release) }
                    .font(Theme.sans(12, .semiBold))
            }
        case .offline:
            row(icon: "wifi.slash", tone: Theme.warn,
                text: "You're offline, so update checks are paused until you're back online.") { EmptyView() }
        case let .failed(reason):
            row(icon: "exclamationmark.triangle", tone: Theme.warn, text: reason) {
                Button("Try again") { Haptics.tap(); Task { await updates.check() } }
                    .font(Theme.sans(12, .semiBold))
            }
        }
    }

    private func open(_ release: Release) {
        Haptics.tap()
        UIApplication.shared.open(release.url)
    }

    private func row<Trailing: View>(icon: String, tone: Color, text: String,
                                    @ViewBuilder trailing: () -> Trailing) -> some View {
        HStack(spacing: 10) {
            Image(systemName: icon).font(.system(size: 12, weight: .semibold)).foregroundStyle(tone)
            Text(text).font(Theme.sans(12)).foregroundStyle(Theme.textMuted)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
            trailing()
        }
    }
}
