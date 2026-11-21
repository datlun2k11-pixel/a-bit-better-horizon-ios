//! The game on an AirPlay display (iPhone and iPad; Horizon's own): while one is connected, the renderer draws
//! on it at its own size, the touch controls are put away, and the phone says "Playing on AirPlay" over
//! everything (crate `horizon-airplay`). It is played with a controller meanwhile.
//!
//! The merged renderer presents through its own frame/output API; the fork's display target
//! (`Renderer::set_target`/`targeted`) has not been ported onto it yet, so drawing stays on the
//! phone's window until it is. `horizon-airplay` is still registered and the phone-side pieces
//! (`phone_shows_game`, `phone_controls`) still work; only the move of the renderer is pending.
use super::Game;
use horizon_airplay::Screen;
use horizon_client::gfx::Renderer;
use std::sync::Arc;
use winit::window::Window;

/// The view the renderer draws on: its id and size, or `None` for the phone's own window.
pub(super) type Shown = Option<(u64, (u32, u32))>;

/// Move the renderer to `screen`, a view of the AirPlay display, or back to `window` without one, when that
/// changed since `shown`. Pending the merged renderer's display-target API: it keeps drawing on the window.
pub(super) fn sync(
    _renderer: Option<&mut Renderer>,
    _window: Option<&Arc<Window>>,
    shown: &mut Shown,
    screen: Option<Screen>,
) {
    *shown = screen.as_ref().map(|s| (s.id, s.size));
}

impl Game<'_> {
    /// The game draws on an AirPlay display, so the phone's window is not where it is played.
    pub(super) fn on_airplay(&self) -> bool {
        false
    }

    pub(super) fn sync_airplay(&mut self) {}
}
