//! Touch screens (phones and tablets; Horizon's own): Pocket Edition's controls while playing, and a finger
//! as the mouse over every screen. The layout and the fingers' state are the lib's `touch`; this file acts on
//! what they say the way the keyboard and mouse would: the D-pad and buttons are their default keys, a tap is
//! a click, holding still is a held left button, and a drag turns the view as the mouse does.
use super::{Game, Screen, event_loop::EventLoopCtl};
use horizon_client::{
    Result,
    held::Source,
    hud::HudBatch,
    keymapping::SNEAK,
    touch::{self, Control, Out, Touches},
    touch_draw,
};
use std::{collections::BTreeSet, time::Instant};
use winit::{
    dpi::PhysicalPosition,
    event::{
        DeviceId, ElementState, KeyEvent, MouseButton, MouseScrollDelta, Touch, TouchPhase,
        WindowEvent,
    },
    keyboard::KeyCode,
};

/// GUI pixels a finger drags over a menu for one line of scrolling.
const SCROLL_LINE: f32 = 12.0;

#[derive(Default)]
pub(super) struct TouchInput {
    fingers: Touches,
    /// A touch has been seen: the controls are drawn (from the start on iPhone and iPad).
    seen: bool,
    /// The last input device controls only the visibility of touch overlays.
    hardware: super::input_mode::InputMode,
    /// The finger that is the pointer over a menu.
    menu: Option<MenuFinger>,
    /// Clicks to release once a tick has passed: vanilla samples buttons once a tick, so a tap released in
    /// the same frame would be lost (the pad holds its short presses back the same way).
    late: Vec<(MouseButton, u64)>,
    /// The button a finger held still on the world is holding down: attack to mine, or use to eat, drink or
    /// draw a bow.
    holding: Option<MouseButton>,
    /// The keys and mouse buttons each custom button sent while it is held, in the order they were
    /// pressed, so its release sends the same ones in reverse even if the mapping changed meanwhile.
    custom_keys: std::collections::BTreeMap<u16, Vec<touch::CustomKey>>,
    /// The toggle buttons that are holding their keys down.
    toggled: BTreeSet<u16>,
    /// Controls let go after the tick that last saw them: the game samples keys once a tick, so a
    /// tap pressed and released in the same frame would otherwise be invisible to it (a double
    /// tap of forward to sprint).
    late_keys: Vec<(Control, u64)>,
    /// When each held control went down, in ticks.
    pressed_at: std::collections::BTreeMap<Control, u64>,
    /// The hide-HUD button was tapped: the HUD and this overlay are not drawn, but the buttons stay
    /// where they were, so a finger that remembers them can bring the HUD back.
    pub(super) hud_hidden: bool,
    /// The analog stick's `[left, forward]` while a finger holds it.
    analog: Option<[f32; 2]>,
    clock: Option<Instant>,
}

impl TouchInput {
    /// Whether a finger-held button (or one toggled on) is holding `key` down.
    pub(super) fn holds_key(&self, key: KeyCode) -> bool {
        self.custom_keys
            .values()
            .flatten()
            .any(|k| matches!(k, touch::CustomKey::Key(k) if *k == key))
    }
}

/// A finger over a menu. Its press waits for a frame drawn with the pointer under it, and its release for a
/// frame after the press: screens find what is under the pointer while drawing, so a pointer that moved and
/// pressed in one go would press whatever was under the old position (or nothing).
struct MenuFinger {
    id: u64,
    /// Where the scrolling last caught up with the finger (GUI pixels).
    scrolled: (f32, f32),
    /// Loop iterations since it went down.
    frames: u8,
    pressed: bool,
    lifted: bool,
    /// It went down on the close button, which already acted: the rest of it is ignored.
    closed: bool,
}

/// The key a button stands for: the vanilla default of its mapping.
fn key_of(control: Control) -> Option<KeyCode> {
    Some(match control {
        Control::Forward => KeyCode::KeyW,
        Control::Back => KeyCode::KeyS,
        Control::Left => KeyCode::KeyA,
        Control::Right => KeyCode::KeyD,
        Control::Jump => KeyCode::Space,
        Control::Perspective => KeyCode::F5,
        Control::Chat => KeyCode::KeyT,
        Control::Pause => KeyCode::Escape,
        Control::Inventory => KeyCode::KeyE,
        Control::HideHud
        | Control::Sneak
        | Control::Hotbar(_)
        | Control::Custom(_)
        | Control::Stick => return None,
    })
}

impl Game<'_> {
    /// Where touches land, in GUI pixels: the screen the game draws on, or the phone's window when the
    /// phone is splitscreen's controller ([`Self::touch_on_phone`]).
    fn touch_batch(&self) -> Option<HudBatch> {
        if self.touch_on_phone() {
            return self.window.as_ref().map(|w| {
                let size = w.inner_size();
                HudBatch::new([size.width, size.height])
            });
        }
        self.screen_batch()
    }

    /// The screen the game draws on, in GUI pixels.
    fn screen_batch(&self) -> Option<HudBatch> {
        self.renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]))
    }

    /// Splitscreen's second player on the AirPlay display (game/splitscreen.rs): the phone is their
    /// controller, with the buttons drawn on it and not on the display, laid out in its own pixels.
    pub(super) fn touch_on_phone(&self) -> bool {
        self.splitscreen.guest && self.on_airplay()
    }

    /// A point of the touch screen on the screen the game draws on: the same point, or, when the phone is the
    /// controller, the point as far across and down the display's half as it was across and down the phone.
    fn screen_point(&self, at: (f32, f32)) -> (f32, f32) {
        if !self.touch_on_phone() {
            return at;
        }
        match (self.touch_batch(), self.screen_batch()) {
            (Some(phone), Some(screen)) => (
                at.0 / phone.width.max(1.0) * screen.width,
                at.1 / phone.height.max(1.0) * screen.height,
            ),
            _ => at,
        }
    }

    fn touch_now(&mut self) -> f64 {
        self.touch
            .clock
            .get_or_insert_with(Instant::now)
            .elapsed()
            .as_secs_f64()
    }

    /// The world is under the fingers, with no screen over it.
    pub(super) fn touch_playing(&self) -> bool {
        self.title.is_none()
            && self.screen == Screen::Playing
            && self.sign_edit.is_none()
            && self.book.is_none()
            && self.server_packs.prompt.is_none()
            && !self.dialog_open()
            && self.player.is_some()
    }

    /// The controls are on screen: after a touch, from the start on iPhone and iPad, or with `HORIZON_TOUCH=1`
    /// (laying them out on a computer).
    /// Hide only touch overlays, and only while the last-used hardware is still connected.
    fn hardware_hides_touch(&self) -> bool {
        self.touch.hardware.hides_touch(
            horizon_gamecontroller::keyboard_connected(),
            horizon_gamecontroller::mouse_connected(),
        )
    }

    pub(super) fn touch_keyboard_used(&mut self, event_loop: &dyn EventLoopCtl) {
        self.touch_release_menu(event_loop);
        if horizon_gamecontroller::keyboard_connected() {
            self.touch.hardware.keyboard();
        }
    }

    pub(super) fn touch_mouse_used(&mut self, event_loop: &dyn EventLoopCtl) {
        self.touch_release_menu(event_loop);
        self.touch.hardware.mouse();
    }

    fn touch_shown(&self) -> bool {
        // On AirPlay the phone is covered and the game is played with a controller (game/airplay.rs), but for
        // splitscreen's second player, whose controller the phone is.
        (!self.on_airplay() || self.splitscreen.guest)
            && (self.touch.seen
                || cfg!(any(target_os = "ios", target_os = "android"))
                || std::env::var_os("HORIZON_TOUCH").is_some_and(|v| v == "1"))
    }

    /// winit's `WindowEvent::Touch`.
    pub(super) fn touch_event(&mut self, event_loop: &dyn EventLoopCtl, touch: Touch) -> Result<()> {
        self.touch.seen = true;
        self.touch.hardware.touch();
        self.last_input = Instant::now();
        let Some(batch) = self.touch_batch() else {
            return Ok(());
        };
        let at = (
            touch.location.x as f32 / batch.scale,
            touch.location.y as f32 / batch.scale,
        );
        let playing = self.touch_playing();
        if playing {
            // Loading or keyboard navigation can consume a menu finger's release.
            // That old finger must not route a new world touch back into the menu.
            self.touch_release_menu(event_loop);
        } else {
            if touch.phase == TouchPhase::Cancelled {
                if self.touch.menu.as_ref().is_some_and(|f| f.id == touch.id) {
                    self.touch_release_menu(event_loop);
                }
            } else {
                self.touch_menu(event_loop, touch.id, touch.phase, at);
            }
            return Ok(());
        }
        let layout = self.settings.touch.layout(batch.width, batch.height);
        let out = match touch.phase {
            TouchPhase::Started => {
                let now = self.touch_now();
                self.touch
                    .fingers
                    .start(touch.id, at, now, &layout, &self.settings.touch.swipes())
            }
            TouchPhase::Moved => self.touch.fingers.moved(
                touch.id,
                at,
                &layout,
                &self.settings.touch.swipes(),
            ),
            TouchPhase::Ended => self.touch.fingers.end(touch.id),
            TouchPhase::Cancelled => self.touch.fingers.cancel(touch.id),
        };
        self.touch_act(event_loop, out, batch.scale)
    }

    /// Once a frame: a held finger starts mining, held-back clicks release, and a screen that opened over the
    /// game lets go of every finger on it.
    pub(super) fn touch_frame(&mut self, event_loop: &dyn EventLoopCtl) {
        self.touch.hardware.refresh(
            horizon_gamecontroller::keyboard_connected(),
            horizon_gamecontroller::mouse_connected(),
        );
        // A button the player deleted (or Reset Layout removed) lets go of what it held.
        if let Err(e) = self.touch_release_removed(event_loop) {
            log::warn!("Horizon: touch: {e}");
        }
        if !self.touch_shown() {
            return;
        }
        let playing = self.touch_playing();
        if playing {
            self.touch_release_menu(event_loop);
        } else {
            self.touch_menu_frame(event_loop);
            // A toggle left on must not keep its key held over a menu.
            if let Err(e) = self.touch_release_toggles(event_loop) {
                log::warn!("Horizon: touch: {e}");
            }
        }
        let out = if playing {
            let now = self.touch_now();
            self.touch.fingers.frame(now)
        } else {
            self.touch.fingers.release_all()
        };
        let ticks = self.ticks;
        let (due, keep) = std::mem::take(&mut self.touch.late)
            .into_iter()
            .partition(|(_, at)| ticks > *at || !playing);
        self.touch.late = keep;
        let (keys_due, keys_keep) = std::mem::take(&mut self.touch.late_keys)
            .into_iter()
            .partition(|(_, at)| ticks > *at || !playing);
        self.touch.late_keys = keys_keep;
        let scale = self.touch_batch().map_or(1.0, |b| b.scale);
        let result = due
            .into_iter()
            .try_for_each(|(button, _)| self.play_mouse(button, false))
            .and_then(|()| {
                keys_due
                    .into_iter()
                    .try_for_each(|(control, at)| self.touch_release_late(event_loop, control, at))
            })
            .and_then(|()| self.touch_act(event_loop, out, scale));
        if let Err(e) = result {
            log::warn!("Horizon: touch: {e}");
        }
    }

    /// Mouse look and clicks only count with the pointer grabbed, which a touch screen has no pointer for:
    /// grab it as a click into the window would, or act as an agent's pointer where it cannot be grabbed.
    fn touch_grab(&mut self) {
        // Only when neither holds: a grab that fails (iOS has no pointer to grab) would otherwise be retried
        // on every drag, and each try makes the mouse drop its next move (`grabMouse`'s ignoreFirstMove).
        if !self.captured && !self.agent_pointer {
            self.capture(true);
        }
        // The finger is the pointer whatever the platform says: a grab that reports itself as held while the
        // window is (or is reported as) unfocused would otherwise swallow every drag. An agent's pointer
        // counts in an inactive window, which is exactly what a touch screen needs.
        self.agent_pointer = true;
    }

    /// The held item is used for as long as the button stays down: something eaten or drunk (its
    /// `consumable` component), or a bow, crossbow, trident, shield, spyglass, goat horn or brush.
    fn touch_holds_use(&self) -> bool {
        let Some(stack) = self.held() else {
            return false;
        };
        matches!(
            &*stack.item,
            "minecraft:bow"
                | "minecraft:crossbow"
                | "minecraft:trident"
                | "minecraft:shield"
                | "minecraft:spyglass"
                | "minecraft:goat_horn"
                | "minecraft:brush"
        ) || self
            .res
            .items()
            .get(&stack.item)
            .is_some_and(|info| info.consumable.is_some())
    }

    fn touch_act(&mut self, event_loop: &dyn EventLoopCtl, out: Vec<Out>, scale: f32) -> Result<()> {
        for o in out {
            match o {
                Out::Press(Control::Hotbar(slot)) => self.selected = usize::from(slot),
                Out::Press(Control::Sneak) => {
                    // A tap, not a held Shift: flip sneak the way the pad's toggle-sneak does.
                    // A latch that no keyboard or pad holds is healed away each frame
                    // (`heal_hold_latches`), so the held key the keyboard path leaves behind
                    // would not survive.
                    let screen_open = !super::gameplay_has_keyboard(
                        self.screen,
                        self.title.is_some(),
                        self.dialog_open(),
                    );
                    self.toggles.pad_toggle(SNEAK, screen_open);
                    self.sync_toggle(SNEAK);
                    self.key_pressed(KeyCode::ShiftLeft, KeyCode::ShiftLeft)?;
                }
                Out::Press(Control::HideHud) => {
                    self.touch.hud_hidden = !self.touch.hud_hidden;
                }
                Out::Press(control) | Out::Release(control) => {
                    let pressed = matches!(o, Out::Press(_));
                    if pressed {
                        self.touch.pressed_at.insert(control, self.ticks);
                    } else if self
                        .touch
                        .pressed_at
                        .get(&control)
                        .is_some_and(|&at| at == self.ticks)
                    {
                        // Down and up before a tick could sample it: let go after the next one.
                        self.touch.late_keys.push((control, self.ticks));
                        continue;
                    } else {
                        self.touch.pressed_at.remove(&control);
                    }
                    match control {
                        Control::Custom(id) => {
                            self.touch_custom_key(event_loop, id, pressed)?;
                        }
                        _ => {
                            if let Some(key) = key_of(control) {
                                self.mapped_key(key, pressed, false, key, Source::Keyboard)?;
                            }
                        }
                    }
                }
                Out::Move(left, forward) => {
                    self.touch.analog = (left.hypot(forward) > 1e-4).then_some([left, forward]);
                }
                Out::Look(dx, dy) => {
                    self.touch_grab();
                    let (dx, dy) = (f64::from(dx * scale), f64::from(dy * scale));
                    // Relative motion, as an agent's `mouse-move` adds it: no pointer position to compare with.
                    self.mouse.add_motion(dx, dy);
                }
                // Pocket Edition: a tap hits the entity it is on, and uses the item or block otherwise.
                Out::Tap => {
                    self.touch_grab();
                    let button = if self.entity_targeted() {
                        MouseButton::Left
                    } else {
                        MouseButton::Right
                    };
                    self.play_mouse(button, true)?;
                    self.touch.late.push((button, self.ticks));
                }
                // Holding still: what the hand holds decides, as in Pocket Edition. Food, potions and the items
                // used for as long as they are held (a bow, a shield...) are used until the finger lifts;
                // anything else mines.
                Out::MineStart => {
                    self.touch_grab();
                    let button = if self.touch_holds_use() {
                        MouseButton::Right
                    } else {
                        MouseButton::Left
                    };
                    self.touch.holding = Some(button);
                    self.play_mouse(button, true)?;
                }
                Out::MineEnd => {
                    let button = self.touch.holding.take().unwrap_or(MouseButton::Left);
                    self.play_mouse(button, false)?;
                }
            }
        }
        Ok(())
    }

    /// A tap let go after the tick that first saw it (`late_keys`): a press and release in one
    /// frame would otherwise leave the game's once-a-tick key sampling with nothing to see.
    fn touch_release_late(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        control: Control,
        at: u64,
    ) -> Result<()> {
        if self.touch.pressed_at.get(&control) != Some(&at) {
            // Pressed again since: that press's own release lets go.
            return Ok(());
        }
        self.touch.pressed_at.remove(&control);
        match control {
            // The same path as a live release: a toggle button ignores it.
            Control::Custom(id) => self.touch_custom_key(event_loop, id, false),
            _ => match key_of(control) {
                Some(key) => self.mapped_key(key, false, false, key, Source::Keyboard),
                None => Ok(()),
            },
        }
    }

    /// The analog stick's `[left, forward]`, `None` while no finger holds it.
    pub(super) fn touch_move(&self) -> Option<[f32; 2]> {
        self.touch.analog
    }

    /// A player's button: its mapped keys and mouse buttons, pressed in order and released in reverse.
    /// A toggle button holds them down until its next tap.
    fn touch_custom_key(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: u16,
        pressed: bool,
    ) -> Result<()> {
        let toggle = self.settings.touch.button(id).is_some_and(|b| b.toggle);
        if !pressed {
            if toggle {
                return Ok(());
            }
            return self.touch_release_custom(event_loop, id);
        }
        if toggle && self.touch.toggled.remove(&id) {
            return self.touch_release_custom(event_loop, id);
        }
        // A second finger on the same button does not press its keys twice.
        if self.touch.custom_keys.contains_key(&id) {
            return Ok(());
        }
        if toggle {
            self.touch.toggled.insert(id);
        }
        let keys = self.settings.touch.button_mappings(id);
        self.touch.custom_keys.insert(id, keys.clone());
        for key in keys {
            self.touch_custom_event(event_loop, key, ElementState::Pressed)?;
        }
        Ok(())
    }

    /// Lets go of everything `id` pressed, in the reverse of the order it went down.
    fn touch_release_custom(&mut self, event_loop: &dyn EventLoopCtl, id: u16) -> Result<()> {
        let Some(keys) = self.touch.custom_keys.remove(&id) else {
            return Ok(());
        };
        for key in keys.into_iter().rev() {
            self.touch_custom_event(event_loop, key, ElementState::Released)?;
        }
        Ok(())
    }

    /// Every toggle button lets go: a screen opened over the game, or the player left it.
    fn touch_release_toggles(&mut self, event_loop: &dyn EventLoopCtl) -> Result<()> {
        let ids: Vec<u16> = std::mem::take(&mut self.touch.toggled)
            .into_iter()
            .collect();
        for id in ids {
            self.touch_release_custom(event_loop, id)?;
        }
        Ok(())
    }

    /// Buttons that no longer exist (deleted, or cleared by Reset Layout) let go of their keys, so a
    /// deletion never leaves a key stuck down.
    fn touch_release_removed(&mut self, event_loop: &dyn EventLoopCtl) -> Result<()> {
        let missing = |id: &u16| self.settings.touch.button(*id).is_none();
        if !self.touch.custom_keys.keys().any(missing) && !self.touch.toggled.iter().any(missing) {
            return Ok(());
        }
        let ids: Vec<u16> = self
            .touch
            .custom_keys
            .keys()
            .copied()
            .chain(self.touch.toggled.iter().copied())
            .filter(|id| self.settings.touch.button(*id).is_none())
            .collect();
        for id in ids {
            self.touch.toggled.remove(&id);
            self.touch_release_custom(event_loop, id)?;
        }
        Ok(())
    }

    fn touch_custom_event(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        key: touch::CustomKey,
        state: ElementState,
    ) -> Result<()> {
        match key {
            touch::CustomKey::Key(key) => self.synthetic_key(event_loop, key, state),
            // Directly, as a tap does: a touch screen never captures the pointer, so a `MouseInput`
            // event would be taken for a first click that only grabs and would act on nothing.
            touch::CustomKey::Mouse(button) => {
                if state == ElementState::Pressed {
                    self.touch_grab();
                }
                self.play_mouse(button, state == ElementState::Pressed)
            }
        }
    }

    /// A keyboard event the player's button stands for, through the same path as a real key.
    fn synthetic_key(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        key: KeyCode,
        state: ElementState,
    ) -> Result<()> {
        let Some(id) = self.window.as_ref().map(|w| w.id()) else {
            return Ok(());
        };
        self.handle_window_event(
            event_loop,
            id,
            WindowEvent::KeyboardInput {
                device_id: DeviceId::dummy(),
                event: KeyEvent::synthetic(key, state),
                is_synthetic: true,
            },
        super::input_trace::InputOrigin::User,
        );
        Ok(())
    }

    // ---- menus: a finger is the mouse ----

    /// The ✕ over a screen with no Done or Back of its own: an inventory, a container, creative, chat, the
    /// statistics and advancements, a sign or a book. The title screens and the options have theirs.
    fn touch_close_shown(&self) -> bool {
        self.touch_shown()
            && self.title.is_none()
            && (self.sign_edit.is_some()
                || self.book.is_some()
                || !matches!(
                    self.screen,
                    Screen::Playing | Screen::Paused | Screen::Dead | Screen::Win | Screen::Options
                ))
    }

    fn touch_menu(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: u64,
        phase: TouchPhase,
        at: (f32, f32),
    ) {
        match phase {
            TouchPhase::Started => {
                if self.touch.menu.is_some() {
                    return;
                }
                let on_close = self.touch_close_shown()
                    && self
                        .touch_batch()
                        .is_some_and(|b| touch::hit_rect(touch::close_rect(b.width, b.height), at));
                let on_keyboard = !on_close
                    && self.touch_shown()
                    && self.field_focused()
                    && self.touch_batch().is_some_and(|b| {
                        touch::hit_rect(touch::keyboard_rect(b.width, b.height), at)
                    });
                if on_close {
                    // What Escape does where the finger is (the pad's B).
                    if let Err(e) = self.pad_back() {
                        log::warn!("Horizon: touch close: {e}");
                    }
                } else if on_keyboard {
                    self.toggle_system_keyboard();
                } else {
                    // A finger outside the field the keyboard belongs to puts the system's keyboard away, as
                    // it would on any phone (and one on the field brings it back after the button hid it);
                    // the tap still does what it was on.
                    match self.focused_field_rect() {
                        Some(rect) if touch::hit_rect(rect, self.screen_point(at)) => {
                            if self.system_keyboard_hidden() {
                                self.show_system_keyboard();
                            }
                        }
                        Some(_) if self.system_keyboard_up() => self.hide_system_keyboard(),
                        _ => {}
                    }
                    self.touch_pointer(event_loop, at);
                }
                self.touch.menu = Some(MenuFinger {
                    id,
                    scrolled: at,
                    frames: 0,
                    pressed: false,
                    lifted: false,
                    closed: on_close || on_keyboard,
                });
            }
            TouchPhase::Moved => {
                let Some(finger) = self.touch.menu.as_ref().filter(|f| f.id == id && !f.closed)
                else {
                    return;
                };
                let last = finger.scrolled;
                self.touch_pointer(event_loop, at);
                // A drag scrolls lists as a wheel would, the content following the finger. Not in an
                // inventory, where a drag carries the item it pressed on.
                let lines = ((at.1 - last.1) / SCROLL_LINE).trunc();
                let scrolls = self.screen != Screen::Inventory;
                if let Some(f) = self.touch.menu.as_mut() {
                    f.scrolled = (at.0, last.1 + lines * SCROLL_LINE);
                }
                if scrolls && lines != 0.0 {
                    self.touch_wheel(event_loop, lines);
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some(finger) = self.touch.menu.as_mut().filter(|f| f.id == id) else {
                    return;
                };
                finger.lifted = true;
                if !finger.closed {
                    self.touch_pointer(event_loop, at);
                }
            }
        }
    }

    /// Abandon a menu gesture when another input device or the world takes over.
    /// A missing UIKit end event must neither latch all later fingers nor create a click.
    fn touch_release_menu(&mut self, event_loop: &dyn EventLoopCtl) {
        if let Some(finger) = self.touch.menu.take()
            && finger.pressed
            && !finger.closed
        {
            self.touch_button(event_loop, false);
        }
    }

    /// Advance the delayed menu press/release after drawing the pointer.
    fn touch_menu_frame(&mut self, event_loop: &dyn EventLoopCtl) {
        let Some(finger) = self.touch.menu.as_mut() else {
            return;
        };
        finger.frames = finger.frames.saturating_add(1);
        if finger.closed {
            if finger.lifted {
                self.touch.menu = None;
            }
        } else if !finger.pressed {
            if finger.frames >= 2 {
                finger.pressed = true;
                self.touch_button(event_loop, true);
            }
        } else if finger.lifted {
            self.touch.menu = None;
            self.touch_button(event_loop, false);
        }
    }

    fn touch_pointer(&mut self, event_loop: &dyn EventLoopCtl, at: (f32, f32)) {
        let (Some(id), Some(batch)) = (self.window.as_ref().map(|w| w.id()), self.screen_batch())
        else {
            return;
        };
        let at = self.screen_point(at);
        let position =
            PhysicalPosition::new(f64::from(at.0 * batch.scale), f64::from(at.1 * batch.scale));
        self.handle_window_event(
            event_loop,
            id,
            WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position,
            },
        super::input_trace::InputOrigin::User,
        );
    }

    fn touch_button(&mut self, event_loop: &dyn EventLoopCtl, pressed: bool) {
        let Some(id) = self.window.as_ref().map(|w| w.id()) else {
            return;
        };
        let state = if pressed {
            ElementState::Pressed
        } else {
            ElementState::Released
        };
        self.handle_window_event(
            event_loop,
            id,
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state,
                button: MouseButton::Left,
            },
        super::input_trace::InputOrigin::User,
        );
    }

    fn touch_wheel(&mut self, event_loop: &dyn EventLoopCtl, lines: f32) {
        let Some(id) = self.window.as_ref().map(|w| w.id()) else {
            return;
        };
        self.handle_window_event(
            event_loop,
            id,
            WindowEvent::MouseWheel {
                device_id: DeviceId::dummy(),
                delta: MouseScrollDelta::LineDelta(0.0, lines),
                phase: TouchPhase::Moved,
            },
        super::input_trace::InputOrigin::User,
        );
    }

    // ---- drawing ----

    /// The buttons over the HUD while playing: vanilla's button, a little see-through as Pocket Edition's
    /// were, lit while held, with white marks shadowed like Minecraft's text.
    pub(super) fn draw_touch_controls(&self, batch: &mut HudBatch) {
        if !self.touch_shown()
            || self.hardware_hides_touch()
            || !self.touch_playing()
            || self.touch_on_phone()
        {
            return;
        }
        let atlas = &self.gui.atlas;
        for (control, r) in self.settings.touch.layout(batch.width, batch.height) {
            if matches!(control, Control::Hotbar(_)) {
                continue;
            }
            if let Control::Custom(id) = control {
                let Some(button) = self.settings.touch.button(id) else {
                    continue;
                };
                touch_draw::draw_custom(
                    batch,
                    atlas,
                    &self.settings.touch.button_label(id),
                    r,
                    self.touch.fingers.held(control) || self.touch.toggled.contains(&id),
                    button.opacity,
                    button.background,
                    button.stroke,
                );
                continue;
            }
            if control == Control::Stick {
                touch_draw::draw_stick(
                    batch,
                    atlas,
                    r,
                    self.touch.analog.unwrap_or([0.0, 0.0]),
                    self.touch.fingers.held(control),
                    self.settings.touch.stick_opacity,
                );
                continue;
            }
            touch_draw::draw_control(
                batch,
                atlas,
                control,
                r,
                self.touch.fingers.held(control),
                self.settings.touch.style(control).opacity,
            );
        }
    }
}

impl Game<'_> {
    /// The buttons over a screen: ✕ where the screen has no way out of its own on a touch screen
    /// (`touch_close_shown`), and ⌨ beside it while a text field has focus.
    pub(super) fn draw_touch_close(&mut self, batch: &mut HudBatch) {
        if self.hardware_hides_touch() || self.touch_on_phone() {
            return;
        }
        let held = self.touch.menu.as_ref().is_some_and(|f| f.closed);
        let keyboard = self.touch_shown() && self.field_focused();
        let atlas = &self.gui.atlas;
        let opacity = self.settings.touch.opacity;
        if self.touch_close_shown() {
            let r = touch::close_rect(batch.width, batch.height);
            touch_draw::draw_button(batch, atlas, r, held, opacity);
            touch_draw::shadowed(r, |r, color| {
                // Two diagonals of cells.
                let u = touch_draw::cell_size(r);
                let (cx, cy) = ((r[0] + r[2] / 2.0).floor(), (r[1] + r[3] / 2.0).floor());
                for i in -3..=3 {
                    let d = i as f32 * u;
                    batch.fill(atlas, cx + d - u / 2.0, cy + d - u / 2.0, u, u, color);
                    batch.fill(atlas, cx - d - u / 2.0, cy + d - u / 2.0, u, u, color);
                }
            });
        }
        if keyboard {
            let r = touch::keyboard_rect(batch.width, batch.height);
            touch_draw::draw_button(batch, atlas, r, false, opacity);
            touch_draw::shadowed(r, |r, color| {
                // A keyboard: two rows of keys over a space bar.
                let u = touch_draw::cell_size(r) * 0.75;
                let (cx, cy) = ((r[0] + r[2] / 2.0).floor(), (r[1] + r[3] / 2.0).floor());
                for row in 0..2 {
                    for key in 0..5 {
                        let x = cx + (key as f32 - 2.5) * u * 1.6;
                        let y = cy + (row as f32 - 1.5) * u * 1.6;
                        batch.fill(atlas, x, y, u, u, color);
                    }
                }
                batch.fill(atlas, cx - 2.5 * u, cy + 1.7 * u, 5.0 * u, u, color);
            });
        }
    }
}

impl Game<'_> {
    /// What the phone shows when it is splitscreen's controller (`touch_on_phone`): the buttons that are
    /// otherwise drawn over the game, in the phone window's pixels.
    pub(super) fn phone_buttons(&mut self) -> Vec<horizon_airplay::Button> {
        let Some(batch) = self.touch_batch().filter(|_| self.touch_shown()) else {
            return Vec::new();
        };
        let focused = self.field_focused();
        let px = |r: touch::Rect| r.map(|v| v * batch.scale);
        let mut buttons = Vec::new();
        if self.touch_playing() {
            for (control, r) in self.settings.touch.layout(batch.width, batch.height) {
                let label = match control {
                    Control::Forward => "▲".to_owned(),
                    Control::Back => "▼".to_owned(),
                    Control::Left => "◀".to_owned(),
                    Control::Right => "▶".to_owned(),
                    Control::Sneak => "Sneak".to_owned(),
                    Control::Jump => "Jump".to_owned(),
                    Control::Perspective => "View".to_owned(),
                    Control::Chat => "Chat".to_owned(),
                    Control::Pause => "II".to_owned(),
                    Control::HideHud => "HUD".to_owned(),
                    Control::Inventory => "•••".to_owned(),
                    Control::Hotbar(slot) => (slot + 1).to_string(),
                    Control::Custom(id) => self.settings.touch.button_label(id),
                    Control::Stick => "Stick".to_owned(),
                };
                buttons.push(horizon_airplay::Button {
                    label,
                    rect: px(r),
                    held: self.touch.fingers.held(control)
                        || matches!(control, Control::Custom(id) if self.touch.toggled.contains(&id)),
                });
            }
        } else {
            if self.touch_close_shown() {
                buttons.push(horizon_airplay::Button {
                    label: "✕".to_owned(),
                    rect: px(touch::close_rect(batch.width, batch.height)),
                    held: self.touch.menu.as_ref().is_some_and(|f| f.closed),
                });
            }
            if focused {
                buttons.push(horizon_airplay::Button {
                    label: "⌨".to_owned(),
                    rect: px(touch::keyboard_rect(batch.width, batch.height)),
                    held: false,
                });
            }
        }
        buttons
    }
}
