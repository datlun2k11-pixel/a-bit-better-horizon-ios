import SwiftUI
import UIKit

// Presentation requests can overlap while the Rust bridge acknowledges them.
private var presentationPending = false

/// The orientation the last `turn` asked for, kept for the repairs below (see `verify`). The count
/// lets a check queued by an older turn stand down when a newer one took over.
private var rotationTarget: UIInterfaceOrientationMask = .landscape
private var rotationCount = 0
private var rotationWatchInstalled = false

/// The C entry point `horizon-ios-launcher` looks up: show the launcher while the engine is stopped or starting. Play dismisses it; the game's Quit Game asks for it again. `submit` is how the
/// launcher asks the game things (Bridge.swift). Called on the main thread; the presenting waits a turn of the
/// run loop, because at startup the game's view controller may not be on screen yet while its first frame is
/// being set up.
@_cdecl("horizon_launcher_present")
public func horizonLauncherPresent(_ submit: HorizonSubmit?) -> Bool {
    Bridge.submit = submit
    Theme.registerFonts()
    let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "?"
    LauncherLog.write("launcher presented; Horizon \(version) on iOS \(UIDevice.current.systemVersion)")
    DispatchQueue.main.async {
        watchRotations()
        present(attempts: 20)
    }
    return true
}

private func present(attempts: Int) {
    guard let root = gameViewController(), root.view.window != nil else {
        // Not on screen yet: try again shortly, for a couple of seconds at most.
        if attempts > 0 {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { present(attempts: attempts - 1) }
        }
        // Do not leave the resource worker waiting forever if UIKit could not present.
        else { Task { _ = await Bridge.send("launcher-start") } }
        return
    }
    // Already up: a second request (Quit Game pressed twice) changes nothing.
    if root is LauncherHostingController || presentationPending {
        return
    }
    presentationPending = true
    let host = LauncherHostingController(rootView: AnyView(EmptyView()))
    host.rootView = AnyView(
        HomeView(onPlay: { [weak host] in
            guard let host else { return }
            GameAudio.start()
            let scene = host.view.window?.windowScene
            host.dismiss(animated: true) {
                Task { @MainActor in
                    // The window allows only landscape again before the phone turns back.
                    _ = await Bridge.send("launcher-visible false")
                    turn(scene, to: .landscape)
                }
            }
        })
    )
    host.modalPresentationStyle = .fullScreen
    host.modalTransitionStyle = .crossDissolve
    Task { @MainActor in
        // The launcher turns with the device, so the window must allow portrait before it is presented.
        _ = await Bridge.send("launcher-visible true")
        guard root.view.window != nil else {
            presentationPending = false
            _ = await Bridge.send("launcher-visible false")
            return
        }
        root.present(host, animated: false) {
            presentationPending = false
        }
    }
}

/// Ask the scene to turn to `orientations` now, rather than when the phone is next moved.
func turn(_ scene: UIWindowScene?, to orientations: UIInterfaceOrientationMask) {
    guard let scene else { return }
    watchRotations()
    rotationCount += 1
    let count = rotationCount
    rotationTarget = orientations
    if #available(iOS 16.0, *) {
        let current = mask(for: scene.effectiveGeometry.interfaceOrientation)
        LauncherLog.write("orientation turn to \(label(orientations)) from \(current.map(label) ?? "unknown"), device \(UIDevice.current.orientation.rawValue), window \(sizeText(scene.windows.first?.bounds.size ?? .zero)), screen \(sizeText(scene.screen.bounds.size))")
        // `launcher-visible` changes the game's mask, and UIKit may already be turning the scene
        // because of it. A second geometry request in the middle of that turn leaves the window at
        // the intersection of the portrait and landscape sizes (a square game view on iOS 16), so
        // only ask when the scene is still somewhere the target does not allow.
        if let current, orientations.contains(current) {
            verify(scene, count: count, attempt: 1, after: 0.8)
            return
        }
        for window in scene.windows {
            window.rootViewController?.setNeedsUpdateOfSupportedInterfaceOrientations()
            var top = window.rootViewController
            while let next = top?.presentedViewController {
                top = next
            }
            top?.setNeedsUpdateOfSupportedInterfaceOrientations()
        }
        // Let those updates reach UIKit before the request, so the two do not race.
        DispatchQueue.main.async {
            guard count == rotationCount else { return }
            scene.requestGeometryUpdate(.iOS(interfaceOrientations: orientations)) { error in
                NSLog("Horizon orientation request failed: %@", error.localizedDescription)
                LauncherLog.write("orientation request failed: \(error.localizedDescription)")
            }
        }
    } else {
        // iOS 15 has no scene geometry request: UIKit turns when a view controller that supports the
        // orientations is presented, and asks again after this nudge.
        UIViewController.attemptRotationToDeviceOrientation()
    }
    verify(scene, count: count, attempt: 1, after: 0.8)
}

/// Every physical turn is a chance for UIKit to stop a turn half-way, and the same can be left
/// behind by the launch itself: check the window after one (see `verify`).
private func watchRotations() {
    guard !rotationWatchInstalled else { return }
    rotationWatchInstalled = true
    UIDevice.current.beginGeneratingDeviceOrientationNotifications()
    let check: (Notification) -> Void = { _ in
        guard let scene = windowScene() else { return }
        verify(scene, count: rotationCount, attempt: 1, after: 0.6)
    }
    NotificationCenter.default.addObserver(
        forName: UIDevice.orientationDidChangeNotification, object: nil, queue: .main, using: check)
    NotificationCenter.default.addObserver(
        forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main, using: check)
    // The launch itself may have ended in a wrong window before anything asked for an orientation.
    if let scene = windowScene() {
        verify(scene, count: rotationCount, attempt: 1, after: 1.2)
    }
}

/// A turn UIKit could not finish leaves the window at the intersection of the portrait and
/// landscape sizes — the game draws into a square beside black — and nothing lays it out again, so
/// it stays that way until the app is restarted. Look once the turn had time to finish, ask the
/// scene one more time, and as a last resort take the screen's size back for the requested
/// orientation and lay the views out so winit sends a new `Resized`.
private func verify(_ scene: UIWindowScene, count: Int, attempt: Int, after delay: TimeInterval) {
    DispatchQueue.main.asyncAfter(deadline: .now() + delay) {
        guard count == rotationCount, !presentationPending else { return }
        guard let window = scene.windows.first(where: \.isKeyWindow) ?? scene.windows.first else { return }
        let screen = scene.screen.bounds.size
        let bounds = window.bounds.size
        let covers = (abs(bounds.width - screen.width) < 1 && abs(bounds.height - screen.height) < 1)
            || (abs(bounds.width - screen.height) < 1 && abs(bounds.height - screen.width) < 1)
        let root = window.rootViewController
        let gameCovers = root.map { fills($0.view.bounds.size, bounds) } ?? true
        // The launcher is a full-screen modal over the game: it must cover the window too.
        let top = topmost(root)
        let presentedCovers = top === root || top.map { fills($0.view.bounds.size, bounds) } ?? true
        if covers && gameCovers && presentedCovers { return }
        LauncherLog.write("rotation left window \(sizeText(bounds)) on a \(sizeText(screen)) screen (attempt \(attempt), device \(UIDevice.current.orientation.rawValue))")
        // The window is right and only a view lagged a layout: one pass is enough.
        if covers {
            layOut(window)
            return
        }
        if attempt == 1 {
            if #available(iOS 16.0, *) {
                scene.requestGeometryUpdate(.iOS(interfaceOrientations: rotationTarget)) { error in
                    LauncherLog.write("rotation retry failed: \(error.localizedDescription)")
                }
            } else {
                UIViewController.attemptRotationToDeviceOrientation()
            }
            verify(scene, count: count, attempt: 2, after: 1.2)
            return
        }
        let short = min(screen.width, screen.height)
        let long = max(screen.width, screen.height)
        let portrait = rotationTarget.contains(.portrait) && !rotationTarget.contains(.landscape)
        window.frame = CGRect(origin: .zero, size: portrait
            ? CGSize(width: short, height: long) : CGSize(width: long, height: short))
        layOut(window)
    }
}

/// Lay the window's views out from its bounds, so winit reports the size it now has.
private func layOut(_ window: UIWindow) {
    let root = window.rootViewController
    let top = topmost(root)
    for controller in top === root ? [root] : [root, top] {
        controller?.view.frame = window.bounds
        controller?.view.setNeedsLayout()
        controller?.view.layoutIfNeeded()
    }
    LauncherLog.write("laid out window \(sizeText(window.bounds.size)); game view \(sizeText(root?.view.bounds.size ?? .zero)); launcher view \(sizeText(top === root ? .zero : top?.view.bounds.size ?? .zero))")
}

private func fills(_ a: CGSize, _ b: CGSize) -> Bool {
    abs(a.width - b.width) < 1 && abs(a.height - b.height) < 1
}

private func topmost(_ controller: UIViewController?) -> UIViewController? {
    var top = controller
    while let next = top?.presentedViewController {
        top = next
    }
    return top
}

@available(iOS 16.0, *)
private func mask(for orientation: UIInterfaceOrientation) -> UIInterfaceOrientationMask? {
    switch orientation {
    case .portrait, .portraitUpsideDown: return .portrait
    case .landscapeLeft, .landscapeRight: return .landscape
    default: return nil
    }
}

private func label(_ mask: UIInterfaceOrientationMask) -> String {
    if mask == .portrait { return "portrait" }
    if mask == .landscape { return "landscape" }
    return "mask \(mask.rawValue)"
}

private func sizeText(_ size: CGSize) -> String {
    "\(Int(size.width.rounded()))x\(Int(size.height.rounded()))"
}

private func windowScene() -> UIWindowScene? {
    let scenes = UIApplication.shared.connectedScenes
        .filter { $0.session.role == .windowApplication }
        .compactMap { $0 as? UIWindowScene }
    return scenes.first { $0.windows.contains(where: \.isKeyWindow) } ?? scenes.first
}

/// The view controller of winit's window: the one the launcher is presented from.
func gameViewController() -> UIViewController? {
    let windows = UIApplication.shared.connectedScenes
        .filter { $0.session.role == .windowApplication }
        .compactMap { $0 as? UIWindowScene }
        .flatMap(\.windows)
    let window = windows.first(where: \.isKeyWindow) ?? windows.first
    var top = window?.rootViewController
    while let presented = top?.presentedViewController {
        top = presented
    }
    return top
}

/// The launcher turns with the device like any screen; the game under it is held sideways by its own view
/// controller, which the phone turns back to when Play dismisses this one. GameInfo.plist allows both.
private final class LauncherHostingController: UIHostingController<AnyView> {
    override var preferredStatusBarStyle: UIStatusBarStyle { .default }
}
