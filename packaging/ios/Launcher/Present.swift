import SwiftUI
import UIKit

// Presentation requests can overlap while the Rust bridge acknowledges them.
private var presentationPending = false

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
    DispatchQueue.main.async { present(attempts: 20) }
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
    if #available(iOS 16.0, *) {
        for window in scene.windows {
            window.rootViewController?.setNeedsUpdateOfSupportedInterfaceOrientations()
            var top = window.rootViewController
            while let next = top?.presentedViewController {
                top = next
            }
            top?.setNeedsUpdateOfSupportedInterfaceOrientations()
        }
        scene.requestGeometryUpdate(.iOS(interfaceOrientations: orientations)) { error in
            NSLog("Horizon orientation request failed: %@", error.localizedDescription)
            LauncherLog.write("orientation request failed: \(error.localizedDescription)")
        }
    } else {
        // iOS 15 has no scene geometry request: UIKit turns when a view controller that supports the
        // orientations is presented, and asks again after this nudge.
        UIViewController.attemptRotationToDeviceOrientation()
    }
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
