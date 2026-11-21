//! The game as the platform runs it, for an executable to start (docs/layers.md, "How a Horizon
//! crate plugs in"). The official executable (`horizon-official`, Horizon layer) registers its
//! hooks (`horizon_platform::hooks`) and then calls [`main`]; it never names an engine crate.
//!
//! An executable built on it:
//!
//! ```ignore
//! static GAME_IDENTITY: [u8; horizon_game::GAME_IDENTITY.len()] = horizon_game::GAME_IDENTITY;
//!
//! fn main() {
//!     horizon_game::prepare_main_stack();
//!     std::hint::black_box(&GAME_IDENTITY);
//!     my_crate::register();
//!     horizon_game::main();
//! }
//! ```
//!
//! The platform's own parts register here, in [`main`], as they leave the engine (layer plan M6).
#![forbid(unsafe_code)]

/// Identifies an executable as the game for launcher imports, without an API version or build id.
pub const GAME_IDENTITY: [u8; horizon_mod_package::game_identity::MARK.len()] =
    horizon_mod_package::game_identity::MARK;

/// A launch stage in `Documents/horizon-boot.log` (`entry::boot_log`), so a build that dies
/// before logging starts still says how far it got.
pub use horizon_client::entry::boot_log;

#[cfg(target_os = "ios")]
fn log_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        boot_log(&format!("panic: {info}"));
        previous(info);
    }));
}

/// Runs the game with the hooks registered so far, and returns when it ends. Register hooks
/// (`horizon_platform::hooks::register`) before calling it.
pub fn main() {
    boot_log("main: enter");
    #[cfg(target_os = "ios")]
    log_panics();
    horizon_guest_stack::prepare_main();
    boot_log("main: stack ready");
    horizon_mod_host::exit_if_compile_child();
    boot_log("main: not a compile child");
    let startup = horizon_client::entry::prepare();
    boot_log("main: entry ready");
    // The mod holder keeps launcher imports alive; iOS has no launcher and no holder.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    std::hint::black_box(&horizon_mod_package::holder::CAPABILITY);
    horizon_mod_host::main_with(Vec::new(), &[], &[], startup);
    boot_log("main: returned");
}

/// Establish the executable main thread stack before initialization.
pub use horizon_guest_stack::prepare_main as prepare_main_stack;
