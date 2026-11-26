//! Gameplay and startup, above the reusable client library.
// Android's exported native entry needs unsafe; other targets forbid it.
#![cfg_attr(not(target_os = "android"), forbid(unsafe_code))]
#![cfg_attr(target_os = "android", deny(unsafe_code))]
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
pub mod android;
extern crate self as horizon_client;
pub use horizon_client_lib::carrier_store;
pub use horizon_client_lib::*;
#[doc(hidden)]
pub mod entry;
pub mod boot_logging;

// The game's allocator lives in the library, not the executable: the prebuilt game (arismoko/horizon#589) is this crate as a
// dylib, and the allocator must be the one its code and the mods' executable share. The Windows and macOS archives
// build with `system-allocator`: their loaders bind each library to std's own allocator, which everything then
// shares (arismoko/horizon#2394, arismoko/horizon#3877).
#[cfg(not(feature = "system-allocator"))]
#[global_allocator]
static ALLOCATOR: horizon_threads::GameAllocator = horizon_threads::GAME_ALLOCATOR;
pub mod game;
pub mod orb_limits;
