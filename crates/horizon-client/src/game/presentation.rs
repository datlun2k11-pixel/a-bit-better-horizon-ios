//! Input belongs to geometry the renderer has actually presented.
use super::*;

#[cfg(feature = "test-support")]
pub(super) mod probe;

impl Game<'_> {
    /// No renderer means a headless control session, not an unpresented window.
    ///
    /// The fork tracked the renderer's presented geometry (`Renderer::presentation_current`,
    /// `presentation_generation`), which the merged frame/output API doesn't carry; until that
    /// is ported input is never retired for an unpresented frame.
    pub(super) fn presentation_stale(&self) -> bool {
        false
    }

    pub(super) fn menu_input_blocked(&self) -> bool {
        self.reload_blocks_input() || self.presentation_stale()
    }

    /// Retire geometry-dependent gestures once per unpresented interval. Do not
    /// release keyboard movement: rotating a world is not opening a new screen.
    pub(super) fn sync_presentation_input(&mut self, event_loop: &dyn EventLoopCtl) {
        let stale = self.presentation_stale();
        if !stale {
            self.presentation_input_blocked = false;
            return;
        }
        if std::mem::replace(&mut self.presentation_input_blocked, true) {
            return;
        }
        self.drag_button = None;
        self.dragging = false;
        self.inv_drag = None;
        self.ui.options.release();
        if let Some(title) = &mut self.title {
            title.release();
            title.release_field();
        }
        self.hud_editor_release();
        let touch_buttons = [MouseButton::Left, MouseButton::Right]
            .map(|button| (button, self.touch_holds_mouse_mapping(button)));
        self.cancel_touch_for_reload();
        for (button, held) in touch_buttons {
            if held && let Err(error) = self.mouse_mapping(button, false, Source::Touch) {
                log::warn!("Horizon: retiring touch after geometry change: {error}");
            }
        }
        self.cancel_emote_controller();
        self.pad_cancel_inputs(event_loop);
        self.suspend_system_keyboard();
        self.capture(false);
    }
}
