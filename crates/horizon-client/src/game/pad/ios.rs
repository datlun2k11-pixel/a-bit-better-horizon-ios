//! Controllers on iPhone and iPad, read through `horizon-gamecontroller` (Apple's GameController framework,
//! behind a safe API because the game forbids `unsafe`). Read only: no rumble, lights or adaptive triggers yet,
//! so the output half is the same no-op as a build without controllers.
use super::{super::Game, Pads};
use horizon_client::gamepad::{Button, GlyphStyle, PadState, Pulse, Rumble};
use serde_json::{Value, json};

/// How many controllers the last poll saw, to log connections the way gilrs' events do.
#[derive(Default)]
pub(super) struct Devices {
    connected: usize,
}

/// Splitscreen's reader of every player's controller. GameController gives one
/// active pad and no per-device movement, so the router sees no events; P1's pad
/// is still read by `Pads::read` as without splitscreen.
pub(in crate::game) type PadId = usize;
#[derive(Default)]
pub(in crate::game) struct PadRouter {}
impl PadRouter {
    pub(in crate::game) fn adopt(&mut self, _: &mut Pads) {}
    pub(in crate::game) fn hand_back(&mut self, _: &mut Pads) {}
    pub(in crate::game) fn events(&mut self) -> Vec<super::PadEvent> {
        Vec::new()
    }
    pub(in crate::game) fn device(
        &self,
        _: PadId,
    ) -> Option<horizon_client::controller_ui::Device> {
        None
    }
    pub(in crate::game) fn lend_output(&mut self, _: &mut Pads, _: PadId) {}
    pub(in crate::game) fn reclaim_output(&mut self, _: &mut Pads) {}
    pub(in crate::game) fn report_status(&self, _: &mut Pads, _: Option<PadId>) {}
    pub(in crate::game) fn connected(&self, _: PadId) -> bool {
        false
    }
    pub(in crate::game) fn uuid(&self, _: PadId) -> [u8; 16] {
        [0; 16]
    }
    pub(in crate::game) fn state(&self, _: PadId) -> PadState {
        PadState::default()
    }
    pub(in crate::game) fn name(&self, _: PadId) -> String {
        String::new()
    }
    pub(in crate::game) fn style(&self, _: PadId) -> GlyphStyle {
        GlyphStyle::Auto
    }
}

/// The glyphs to draw, from the product category iOS reports, through the vendor ids gilrs' pads are
/// matched by.
fn style(category: &str) -> GlyphStyle {
    let vendor = if category.contains("DualSense") || category.contains("DualShock") {
        Some(0x054c)
    } else if category.contains("Switch") || category.contains("Joy-Con") {
        Some(0x057e)
    } else {
        None
    };
    GlyphStyle::of_device(vendor, None)
}

impl Pads {
    /// No output half yet: nothing to release.
    pub(in crate::game) fn release_hardware(&mut self) {}

    /// Read the active pad.
    pub(super) fn read(&mut self) -> PadState {
        let controllers = horizon_gamecontroller::controllers();
        if controllers.len() != self.devices.connected {
            for controller in &controllers {
                let name = controller.name.as_deref().unwrap_or(&controller.category);
                log::info!("Controller connected: {name}");
            }
            self.devices.connected = controllers.len();
        }
        let Some(pad) = horizon_gamecontroller::active() else {
            return PadState::default();
        };
        self.style = style(&pad.category);
        let mut state = PadState::default();
        for (button, down) in [
            (Button::South, pad.a),
            (Button::East, pad.b),
            (Button::West, pad.x),
            (Button::North, pad.y),
            (Button::LeftBumper, pad.left_shoulder),
            (Button::RightBumper, pad.right_shoulder),
            (Button::Select, pad.options),
            (Button::Start, pad.menu),
            (Button::LeftStick, pad.left_stick_button),
            (Button::RightStick, pad.right_stick_button),
            (Button::DPadUp, pad.dpad_up),
            (Button::DPadDown, pad.dpad_down),
            (Button::DPadLeft, pad.dpad_left),
            (Button::DPadRight, pad.dpad_right),
        ] {
            state.set(button, down);
        }
        // GameController's sticks are -1..1 with up positive, as gilrs' are.
        state.left = pad.left_stick;
        state.right = pad.right_stick;
        state.triggers = pad.triggers;
        state
    }
}

impl Game<'_> {
    pub(super) fn pad_output_off(&mut self) {}
    pub(super) fn pad_output_frame(&mut self) {}
    pub(in crate::game) fn pad_close_output(&mut self) {}
    pub(super) fn pad_vibrate(&mut self, _: Rumble, _: Pulse) {}
    pub(super) fn pad_devices_status(&self) -> Value {
        let pads: Vec<Value> = horizon_gamecontroller::controllers()
            .into_iter()
            .map(|c| json!({ "name": c.name, "category": c.category, "gamepad": c.gamepad }))
            .collect();
        json!({ "backend": true, "controllers": pads })
    }
}
