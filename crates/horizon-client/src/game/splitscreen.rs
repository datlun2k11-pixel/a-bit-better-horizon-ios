//! Splitscreen on an AirPlay display (iPhone and iPad; Horizon's own).
//!
//! The merged renderer and the new `runtime/split` own splitscreen now; the fork's AirPlay
//! second player needs the renderer display target the merge dropped (`Renderer::set_target`,
//! `presentation_current`). Until that is ported this stays a no-op that keeps the state other
//! modules read (`Splitscreen::guest`), so the phone draws its single player as usual.
use super::Game;
use winit::event::WindowEvent;

#[derive(Default)]
pub(super) struct Splitscreen {
    /// This game is the second player.
    pub guest: bool,
    /// The second player plays alongside this one.
    pub active: bool,
    /// The pause menu asked to start (true) or end (false) it.
    pub request: Option<bool>,
}

impl<'a> Game<'a> {
    /// Splitscreen can start: pending the renderer's display target, it cannot.
    pub(super) fn splitscreen_available(&self) -> bool {
        false
    }

    /// Release the memory connection before dropping its renderer and client state.
    pub(super) fn stop_splitscreen(&mut self) {}

    pub(super) fn poll_splitscreen(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {}
}

/// Starts and ends the second player, once a frame after both games' own.
pub(super) fn update<'a>(_host: &mut Game<'a>, _guest: &mut Option<Box<Game<'a>>>) {}

/// Input from the phone's window: the second player's, while there is one. The rest (redraws, resizes,
/// focus) goes to both games: the first draws on the AirPlay display whenever the window redraws.
pub(super) fn guest_input(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::Touch(_)
            | WindowEvent::KeyboardInput { .. }
            | WindowEvent::Ime(_)
            | WindowEvent::ModifiersChanged(_)
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. }
            | WindowEvent::CursorMoved { .. }
            | WindowEvent::CursorEntered { .. }
            | WindowEvent::CursorLeft { .. }
    )
}

/// Native lifecycle and redraw events apply to each renderer on the shared phone window.
pub(super) fn shared_event(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. }
            | WindowEvent::Focused(_)
            | WindowEvent::Occluded(_)
            | WindowEvent::RedrawRequested
    )
}

/// A pause menu cannot stop the world while another local player is connected.
pub(super) fn server_paused(paused: bool, active: bool) -> bool {
    paused && !active
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_player_keeps_the_integrated_world_running() {
        assert!(server_paused(true, false));
        assert!(!server_paused(true, true));
        assert!(!server_paused(false, true));
        assert!(!server_paused(false, false));
    }

    #[test]
    fn focus_resize_and_redraw_are_shared_while_cursor_input_is_not() {
        for event in [
            WindowEvent::Focused(false),
            WindowEvent::RedrawRequested,
            WindowEvent::Resized(winit::dpi::PhysicalSize::new(1280, 720)),
        ] {
            assert!(shared_event(&event));
            assert!(!guest_input(&event));
        }
        let event = WindowEvent::CursorLeft {
            device_id: winit::event::DeviceId::dummy(),
        };
        assert!(guest_input(&event));
        assert!(!shared_event(&event));
    }
}
