//! Active input chooses only touch overlays; normal menus and the game HUD are unaffected.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
enum Device {
    #[default]
    Touch,
    Keyboard,
    Mouse,
}

#[derive(Default)]
pub(crate) struct InputMode {
    device: Device,
    /// The keyboard and mouse attachments last seen, to log the moment either changes.
    hardware: Option<(bool, bool)>,
}

impl InputMode {
    pub(crate) fn touch(&mut self) {
        self.switch(Device::Touch);
    }
    pub(crate) fn keyboard(&mut self) {
        self.switch(Device::Keyboard);
    }
    pub(crate) fn mouse(&mut self) {
        self.switch(Device::Mouse);
    }

    /// The overlay's side of a bug report: which device last moved, and whether the buttons are therefore
    /// hidden, is what a "the controls vanished" or "a mouse appeared and the pad stayed" report turns on.
    fn switch(&mut self, device: Device) {
        if self.device != device {
            log::info!("touch overlays: input {:?} -> {device:?}", self.device);
            self.device = device;
        }
    }

    pub(crate) fn used_mouse(&self) -> bool {
        self.device == Device::Mouse
    }

    pub(crate) fn hides_touch(&self, keyboard: bool, mouse: bool) -> bool {
        match self.device {
            Device::Touch => false,
            Device::Keyboard => keyboard,
            Device::Mouse => mouse,
        }
    }

    pub(crate) fn refresh(&mut self, keyboard: bool, mouse: bool) {
        // Attaching or removing a keyboard or mouse flips the overlay's visibility: a report about controls
        // appearing or vanishing needs the moment it changed, and nothing else logs it.
        if self.hardware != Some((keyboard, mouse)) {
            log::info!(
                "touch overlays: keyboard {}, mouse {}; touch controls {}",
                if keyboard { "attached" } else { "none" },
                if mouse { "attached" } else { "none" },
                if self.hides_touch(keyboard, mouse) {
                    "hidden"
                } else {
                    "shown"
                }
            );
            self.hardware = Some((keyboard, mouse));
        }
        if !self.hides_touch(keyboard, mouse) {
            self.touch();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connected_hardware_does_not_hide_touch_until_used_and_touch_restores_it() {
        let mut mode = InputMode::default();
        assert!(!mode.hides_touch(true, true));
        mode.keyboard();
        assert!(mode.hides_touch(true, true));
        mode.mouse();
        assert!(mode.hides_touch(true, true));
        mode.touch();
        assert!(!mode.hides_touch(true, true));
    }
    #[test]
    fn disconnect_and_reconnect_require_new_hardware_activity() {
        let mut mode = InputMode::default();
        mode.mouse();
        mode.refresh(true, false);
        assert!(
            !mode.hides_touch(true, true),
            "keyboard still attached; mouse reconnect alone is not use"
        );
        mode.keyboard();
        mode.refresh(false, true);
        assert!(!mode.hides_touch(true, true));
        mode.keyboard();
        assert!(mode.hides_touch(true, false));
    }
}
