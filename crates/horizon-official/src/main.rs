//! The official game executable (docs/layers.md, "How a Horizon crate plugs in").
#![forbid(unsafe_code)]

/// The game identity marker for launcher imports, retained by `main`.
static GAME_IDENTITY: [u8; horizon_game::GAME_IDENTITY.len()] = horizon_game::GAME_IDENTITY;

fn main() {
    #[cfg(target_os = "ios")]
    {
        let envs: Vec<String> = std::env::vars_os()
            .filter(|(key, _)| {
                let key = key.to_string_lossy();
                key == "HOME"
                    || key.starts_with("LC_")
                    || key.starts_with("HORIZON")
                    || key.starts_with("DYLD")
            })
            .map(|(key, value)| format!("{}={}", key.to_string_lossy(), value.to_string_lossy()))
            .collect();
        horizon_game::boot_log(&format!(
            "launch: args={:?} env={envs:?}",
            std::env::args().collect::<Vec<_>>()
        ));
    }
    horizon_game::prepare_main_stack();
    std::hint::black_box(&GAME_IDENTITY);
    horizon_official::register();
    horizon_game::boot_log("launch: hooks registered");
    horizon_game::main();
}
