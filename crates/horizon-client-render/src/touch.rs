//! Touch controls for phones and tablets (Horizon's own), after Minecraft: Pocket Edition's: a D-pad (or an
//! analog stick) on the left, jump on the right, perspective, chat and pause at the top, and the hotbar and
//! an inventory button along the bottom. A finger anywhere else turns the view; a short tap uses (or attacks
//! the entity under the crosshair), and holding still mines.
//!
//! This module is the layout and the fingers' state: positions in GUI pixels in, [`Out`]s for the game to act
//! on out. game/touch.rs feeds it winit's touches and acts on what comes back.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use winit::{event::MouseButton, keyboard::KeyCode};

use crate::keybinds;

/// A button of the layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Control {
    Forward,
    Back,
    Left,
    Right,
    /// The D-pad's centre: sneaking, toggled.
    Sneak,
    Jump,
    Perspective,
    Chat,
    Pause,
    /// HUD on or off. Hidden with the HUD it hides, but still where it was: a remembered tap brings the
    /// HUD back.
    HideHud,
    Inventory,
    Hotbar(u8),
    /// A button the player added, by its id in [`TouchSettings::buttons`].
    Custom(u16),
    /// The analog stick that replaces the D-pad: a finger on it moves at its own angle and speed.
    Stick,
}

impl Control {
    /// The D-pad's directions, which a finger can slide between.
    fn moves(self) -> bool {
        matches!(
            self,
            Control::Forward | Control::Back | Control::Left | Control::Right
        )
    }
}

/// How the left thumb moves the player.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Movement {
    /// Four directions slid between: the D-pad.
    #[default]
    DPad,
    /// An analog stick: any direction, any speed up to full.
    Stick,
}

/// `[x, y, width, height]` in GUI pixels.
pub type Rect = [f32; 4];

/// The D-pad's and jump's button size on a `h` GUI-pixel-high screen: about a seventh of the height, so a
/// phone (GUI scale 4, 292 high) gets 40 GUI pixels, some 53 points, over Apple's 44-point minimum.
pub fn button_size(h: f32) -> f32 {
    (h * 0.14).clamp(26.0, 48.0)
}
/// The gap kept from the screen's edges.
const MARGIN: f32 = 14.0;
/// How far above the hotbar its slots still take a tap: a thumb lands high.
const HOTBAR_REACH: f32 = 8.0;

/// Where every button is on a `w` x `h` GUI-pixel screen.
pub fn layout(w: f32, h: f32) -> Vec<(Control, Rect)> {
    let s = button_size(h);
    let small = (s * 0.65).round();
    // The D-pad's centre cell, high enough that its bottom row clears the screen's edge.
    let (cx, cy) = (MARGIN + s, h - MARGIN - 2.0 * s - 8.0);
    let mut v = vec![
        (Control::Forward, [cx, cy - s, s, s]),
        (Control::Left, [cx - s, cy, s, s]),
        (Control::Sneak, [cx, cy, s, s]),
        (Control::Right, [cx + s, cy, s, s]),
        (Control::Back, [cx, cy + s, s, s]),
        (Control::Jump, [w - MARGIN - s, cy, s, s]),
    ];
    let top = [
        Control::Perspective,
        Control::Chat,
        Control::Pause,
        Control::HideHud,
    ];
    let x0 = (w - 4.0 * small - 12.0) / 2.0;
    for (i, control) in top.into_iter().enumerate() {
        v.push((control, [x0 + i as f32 * (small + 4.0), 2.0, small, small]));
    }
    // Gui.extractItemHotbar's slots: the bar is 182 wide, centred, 22 high at the bottom.
    let (hx, hy) = ((w / 2.0 - 91.0).floor(), h - 22.0);
    for slot in 0..9u8 {
        v.push((
            Control::Hotbar(slot),
            [
                hx + 1.0 + 20.0 * f32::from(slot),
                hy - HOTBAR_REACH,
                20.0,
                22.0 + HOTBAR_REACH,
            ],
        ));
    }
    v.push((
        Control::Inventory,
        [hx + 184.0, hy - HOTBAR_REACH, 30.0, 22.0 + HOTBAR_REACH],
    ));
    v
}

/// The close button over a screen that has none of its own (an inventory, chat, a sign): the top right.
pub fn close_rect(w: f32, h: f32) -> Rect {
    let s = (button_size(h) * 0.75).round();
    [w - s - 4.0, 4.0, s, s]
}

/// The keyboard button, left of the close button: over a focused text field, it asks for the system's
/// keyboard again.
pub fn keyboard_rect(w: f32, h: f32) -> Rect {
    let c = close_rect(w, h);
    [c[0] - c[2] - 4.0, c[1], c[2], c[3]]
}

/// The point is inside the rectangle.
pub fn hit_rect(r: Rect, at: (f32, f32)) -> bool {
    contains(r, at)
}

fn contains(r: Rect, (x, y): (f32, f32)) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

/// The button under a point.
pub fn hit(layout: &[(Control, Rect)], at: (f32, f32)) -> Option<Control> {
    layout
        .iter()
        .find(|(_, r)| contains(*r, at))
        .map(|(c, _)| *c)
}

/// Horizon's touch overlay settings (`horizon-touch.txt`).
pub const FILE: &str = "horizon-touch.txt";
/// The layouts other than the active one: one file each, by name.
pub const LAYOUTS_DIR: &str = "touch-layouts";
/// The layout every player starts with.
pub const DEFAULT_PROFILE: &str = "Default";
pub const MAX_PROFILE_NAME: usize = 16;

/// A layout profile's name, which is also its file's stem (so it shows in the Files app): letters,
/// digits, spaces, `_` and `-`, 1 to [`MAX_PROFILE_NAME`] of them.
pub fn valid_profile_name(name: &str) -> bool {
    let name = name.trim();
    (1..=MAX_PROFILE_NAME).contains(&name.chars().count())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-'))
}

/// A button moved by the player: its top-left corner as fractions of the space it can move in (the
/// screen minus the button), so a layout survives a rotation, a resize or a change of the button's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
}

/// A button the player added: what it sends and how it looks.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomButton {
    /// What the button shows; empty until the player names it ("NEW").
    pub name: String,
    /// The keys it sends, in order; empty slots are skipped. Pressed in order, released in reverse.
    pub keys: [Option<String>; MAX_KEYS],
    /// Its top-left corner as fractions of the screen, as [`Placement`].
    pub placement: Placement,
    /// Its width and height, `MIN_SIZE`..=`MAX_SIZE`, multiplied by the overlay's scale.
    pub width: f32,
    pub height: f32,
    /// Its own background opacity, `MIN_OPACITY`..=1.0, multiplied by the overlay's.
    pub opacity: f32,
    /// One tap holds the keys down, the next lets go.
    pub toggle: bool,
    /// The keys are held only while the finger stays on the button: sliding off lets go, sliding back
    /// presses again. Off, a press keeps its keys until the finger lifts.
    pub swipe: bool,
    /// A solid background color in place of vanilla's button sprite, and the stroke around it.
    pub background: Option<u32>,
    pub stroke: Option<u32>,
}

/// How many keys one button sends.
pub const MAX_KEYS: usize = 4;
/// The most characters a button's name takes.
pub const MAX_NAME: usize = 32;

impl Default for CustomButton {
    fn default() -> Self {
        Self {
            name: String::new(),
            keys: [None, None, None, None],
            placement: Placement { x: 0.5, y: 0.5 },
            width: 1.0,
            height: 1.0,
            opacity: 1.0,
            toggle: false,
            swipe: false,
            background: None,
            stroke: None,
        }
    }
}

/// A key or mouse button a player's button sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomKey {
    Key(KeyCode),
    Mouse(MouseButton),
}

impl CustomKey {
    /// The `key.keyboard.*` or `key.mouse.*` name as a key, if it is one.
    pub fn of(name: &str) -> Option<Self> {
        if let Some(button) = keybinds::mouse_button(name) {
            return Some(Self::Mouse(button));
        }
        keybinds::key_code(name).map(Self::Key)
    }
}

/// A mapping's name as the button and the options page show it: "LMB"/"RMB" for a mouse button,
/// else the key's display name.
pub fn key_label(name: &str) -> String {
    match keybinds::mouse_button(name) {
        Some(MouseButton::Left) => "LMB".to_owned(),
        Some(MouseButton::Right) => "RMB".to_owned(),
        Some(MouseButton::Middle) => "MMB".to_owned(),
        _ => keybinds::display_name(name).plain(),
    }
}

/// How see-through the button backgrounds may become.
pub const MIN_OPACITY: f32 = 0.2;
/// How small and how large the buttons may grow.
pub const MIN_SCALE: f32 = 0.75;
pub const MAX_SCALE: f32 = 1.5;
/// How small and how large a button may be drawn, on either side.
pub const MIN_SIZE: f32 = 0.5;
pub const MAX_SIZE: f32 = 3.0;

/// A built-in button's own width, height and opacity. Each button keeps its own, so resizing one never
/// moves or fades another.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonStyle {
    /// Multipliers of the button's own size, `MIN_SIZE`..=`MAX_SIZE`.
    pub width: f32,
    pub height: f32,
    /// `MIN_OPACITY`..=1.0, multiplied by the overlay's.
    pub opacity: f32,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        Self {
            width: 1.0,
            height: 1.0,
            opacity: 1.0,
        }
    }
}

/// Whether a control takes a style of its own: the built-in buttons, but not the stick (the Touch
/// Controls page sizes it), the hotbar (the HUD places it) or a player's buttons (they carry their own).
pub fn styled(control: Control) -> bool {
    matches!(
        control,
        Control::Forward
            | Control::Back
            | Control::Left
            | Control::Right
            | Control::Sneak
            | Control::Jump
            | Control::Perspective
            | Control::Chat
            | Control::Pause
            | Control::HideHud
            | Control::Inventory
    )
}

/// Whether the player may turn a button off: the pause button is a phone's way back, and the hide-HUD
/// button is how the HUD comes back.
pub fn can_hide(control: Control) -> bool {
    matches!(
        control,
        Control::Forward
            | Control::Back
            | Control::Left
            | Control::Right
            | Control::Sneak
            | Control::Jump
            | Control::Perspective
            | Control::Chat
            | Control::Inventory
    )
}

/// Whether the editor's long press opens the button's own popup: everything but the hotbar (the HUD
/// places it) and the stick (sized on the Touch Controls page).
pub fn editable(control: Control) -> bool {
    !matches!(control, Control::Hotbar(_) | Control::Stick)
}

/// The colors a button's background and stroke can take, with the names the options page shows. Packed as
/// `hud::rgba` packs them: alpha in the top byte, then blue, green, red.
pub const COLORS: [(&str, u32); 9] = [
    ("White", 0xFFF0F0F0),
    ("Gray", 0xFF6E6E6E),
    ("Black", 0xFF1E1E1E),
    ("Red", 0xFF2C34D8),
    ("Orange", 0xFF1C84E8),
    ("Yellow", 0xFF28C8E6),
    ("Green", 0xFF46AA3C),
    ("Blue", 0xFFDC7832),
    ("Purple", 0xFFD25096),
];

/// The next color after `current`: the palette follows "Default".
pub fn cycle_color(current: Option<u32>) -> Option<u32> {
    let index = current.and_then(|c| COLORS.iter().position(|(_, v)| *v == c));
    match index {
        None => Some(COLORS[0].1),
        Some(i) => COLORS.get(i + 1).map(|(_, v)| *v),
    }
}

/// The color's name, "Default" for none.
pub fn color_name(color: Option<u32>) -> &'static str {
    match color {
        None => "Default",
        Some(c) => COLORS
            .iter()
            .find(|(_, v)| *v == c)
            .map_or("Custom", |(name, _)| *name),
    }
}

/// The mouse buttons a custom button can send, by the names [`keybinds::mouse_button`] reads. The
/// middle one picks the targeted block, as it does on a mouse.
pub const MOUSE_KEYS: [&str; 3] = ["key.mouse.left", "key.mouse.right", "key.mouse.middle"];

/// The control's name in `horizon-touch.txt`; the hotbar stays where the HUD puts it.
pub fn control_name(control: Control) -> Option<&'static str> {
    Some(match control {
        Control::Forward => "forward",
        Control::Back => "back",
        Control::Left => "left",
        Control::Right => "right",
        Control::Sneak => "sneak",
        Control::Jump => "jump",
        Control::Perspective => "perspective",
        Control::Chat => "chat",
        Control::Pause => "pause",
        Control::HideHud => "hide_hud",
        Control::Inventory => "inventory",
        Control::Stick => "stick",
        Control::Hotbar(_) | Control::Custom(_) => return None,
    })
}

pub fn control_from_name(name: &str) -> Option<Control> {
    Some(match name {
        "forward" => Control::Forward,
        "back" => Control::Back,
        "left" => Control::Left,
        "right" => Control::Right,
        "sneak" => Control::Sneak,
        "jump" => Control::Jump,
        "perspective" => Control::Perspective,
        "chat" => Control::Chat,
        "pause" => Control::Pause,
        "hide_hud" => Control::HideHud,
        "inventory" => Control::Inventory,
        "stick" => Control::Stick,
        _ => return None,
    })
}

/// The lang key naming a button on the settings page.
pub fn control_caption(control: Control) -> &'static str {
    match control {
        Control::Forward => "horizon.options.touch.forward",
        Control::Back => "horizon.options.touch.back",
        Control::Left => "horizon.options.touch.left",
        Control::Right => "horizon.options.touch.right",
        Control::Sneak => "horizon.options.touch.sneak",
        Control::Jump => "horizon.options.touch.jump",
        Control::Perspective => "horizon.options.touch.perspective",
        Control::Chat => "horizon.options.touch.chat",
        Control::Pause => "horizon.options.touch.pause",
        Control::HideHud => "horizon.options.touch.hide_hud",
        Control::Inventory => "horizon.options.touch.inventory",
        Control::Hotbar(_) => "horizon.options.touch.hotbar",
        Control::Custom(_) => "horizon.options.touch.custom",
        Control::Stick => "horizon.options.touch.stick",
    }
}

/// The touch overlay as the player arranged it (`horizon-touch.txt`).
#[derive(Clone, Debug, PartialEq)]
pub struct TouchSettings {
    /// The name of the layout profile this is; the active one lives in `horizon-touch.txt`.
    pub profile: String,
    /// The overlay-wide opacity older builds had. Each button keeps its own now: a file's value is
    /// handed to every button on load (`migrate_global_style`) and this returns to 1.
    pub opacity: f32,
    /// The overlay-wide size older builds had, carried into every button the same way.
    pub scale: f32,
    /// Buttons the player moved, by name: where their top-left corner sits.
    pub placements: BTreeMap<Control, Placement>,
    /// Buttons the player turned off. The pause button is never hidden: a phone needs a way back.
    pub hidden: BTreeSet<Control>,
    /// Each built-in button's own width, height and opacity.
    pub styles: BTreeMap<Control, ButtonStyle>,
    /// The player's own buttons, by id.
    pub buttons: BTreeMap<u16, CustomButton>,
    /// The D-pad, or the analog stick in its place.
    pub movement: Movement,
    /// The stick's own size, `MIN_SCALE`..=`MAX_SCALE`, multiplied by the overlay's scale.
    pub stick_scale: f32,
    /// The stick's own opacity, `MIN_OPACITY`..=1.0, multiplied by the overlay's.
    pub stick_opacity: f32,
}

impl Default for TouchSettings {
    fn default() -> Self {
        Self {
            profile: DEFAULT_PROFILE.to_owned(),
            opacity: 1.0,
            scale: 1.0,
            placements: BTreeMap::new(),
            hidden: BTreeSet::new(),
            styles: BTreeMap::new(),
            buttons: BTreeMap::new(),
            movement: Movement::DPad,
            stick_scale: 1.0,
            stick_opacity: 1.0,
        }
    }
}

fn clamp_unit(value: &str, default: f32, lo: f32, hi: f32) -> f32 {
    value
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .map_or(default, |v| v.clamp(lo, hi))
}

/// A mapping as a button keeps it: a key or mouse button name, or `None` for an empty slot.
fn parse_key(value: &str) -> Option<String> {
    let value = value.trim();
    CustomKey::of(value).map(|_| value.to_owned())
}

/// `-` for no color, else eight hex digits.
fn parse_color(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.is_empty() || value == "-" {
        return None;
    }
    u32::from_str_radix(value, 16).ok()
}

fn write_color(color: Option<u32>) -> String {
    color.map_or_else(|| "-".to_owned(), |c| format!("{c:08X}"))
}

/// A name keeps its characters but not the file's own separators.
fn escape_name(name: &str) -> String {
    name.replace('%', "%25").replace(':', "%3A")
}
fn unescape_name(name: &str) -> String {
    name.replace("%3A", ":").replace("%25", "%")
}

/// `button:`'s value: the id and the button, in either the old five-field format or the current one.
fn parse_button(value: &str) -> Option<(u16, CustomButton)> {
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() != 5 && parts.len() != 14 {
        return None;
    }
    let id = parts[0].trim().parse::<u16>().ok()?;
    let (x, y) = parts[1].split_once(',')?;
    let default = Placement { x: 0.5, y: 0.5 };
    let placement = Placement {
        x: clamp_unit(x.trim(), default.x, 0.0, 1.0),
        y: clamp_unit(y.trim(), default.y, 0.0, 1.0),
    };
    if parts.len() == 5 {
        // `<scale>:<opacity>:<key>`, a square button. The button showed its key's label, so that
        // becomes its name rather than every older button turning into "NEW".
        let scale = clamp_unit(parts[2], 1.0, MIN_SCALE, MAX_SCALE);
        let key = parse_key(parts[4]);
        return Some((
            id,
            CustomButton {
                name: key
                    .as_deref()
                    .map(key_label)
                    .unwrap_or_default()
                    .chars()
                    .take(MAX_NAME)
                    .collect(),
                placement,
                width: scale,
                height: scale,
                opacity: clamp_unit(parts[3], 1.0, MIN_OPACITY, 1.0),
                keys: [key, None, None, None],
                ..CustomButton::default()
            },
        ));
    }
    Some((
        id,
        CustomButton {
            name: unescape_name(parts[5].trim())
                .chars()
                .take(MAX_NAME)
                .collect(),
            keys: std::array::from_fn(|i| parse_key(parts[10 + i])),
            placement,
            width: clamp_unit(parts[2], 1.0, MIN_SIZE, MAX_SIZE),
            height: clamp_unit(parts[3], 1.0, MIN_SIZE, MAX_SIZE),
            opacity: clamp_unit(parts[4], 1.0, MIN_OPACITY, 1.0),
            toggle: parts[6].trim() == "1",
            swipe: parts[7].trim() == "1",
            background: parse_color(parts[8]),
            stroke: parse_color(parts[9]),
        },
    ))
}

impl TouchSettings {
    pub fn load(dir: &Path) -> Self {
        match fs::read_to_string(dir.join(FILE)) {
            Ok(text) => {
                let mut s = Self::parse(&text);
                s.spread_stacked_buttons();
                s
            }
            Err(_) => Self::default(),
        }
    }
    /// Written once something differs from the defaults, so players who never touch the overlay get no new
    /// file.
    pub fn save(&self, dir: &Path) {
        let path = dir.join(FILE);
        if *self != Self::default() || path.exists() {
            let _ = fs::write(path, self.serialize());
        }
    }
    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "profile" => {
                    let name = unescape_name(value).trim().to_owned();
                    if valid_profile_name(&name) {
                        s.profile = name;
                    }
                }
                "opacity" => s.opacity = clamp_unit(value, s.opacity, MIN_OPACITY, 1.0),
                "scale" => s.scale = clamp_unit(value, s.scale, MIN_SCALE, MAX_SCALE),
                "movement" => {
                    if value == "stick" {
                        s.movement = Movement::Stick;
                    }
                }
                // `stick:<scale>:<opacity>`, as `serialize` writes it.
                "stick" => {
                    if let Some((scale, opacity)) = value.split_once(':') {
                        s.stick_scale =
                            clamp_unit(scale.trim(), s.stick_scale, MIN_SCALE, MAX_SCALE);
                        s.stick_opacity =
                            clamp_unit(opacity.trim(), s.stick_opacity, MIN_OPACITY, 1.0);
                    }
                }
                "hidden" => {
                    if let Some(control) = control_from_name(value) {
                        s.set_hidden(control, true);
                    }
                }
                // `place:<name>:x,y`, as `serialize` writes it.
                "place" => {
                    if let Some((name, xy)) = value.split_once(':')
                        && let Some(control) = control_from_name(name.trim())
                        && let Some((x, y)) = xy.split_once(',')
                    {
                        let default = Placement { x: 0.0, y: 0.0 };
                        let placement = Placement {
                            x: clamp_unit(x.trim(), default.x, 0.0, 1.0),
                            y: clamp_unit(y.trim(), default.y, 0.0, 1.0),
                        };
                        s.placements.insert(control, placement);
                    }
                }
                // `size:<name>:<width>:<height>:<opacity>`, as `serialize` writes it.
                "size" => {
                    if let Some((name, sides)) = value.split_once(':')
                        && let Some(control) = control_from_name(name.trim())
                    {
                        let mut sides = sides.split(':');
                        let mut next = |default: f32, lo: f32, hi: f32| {
                            clamp_unit(sides.next().unwrap_or(""), default, lo, hi)
                        };
                        let style = ButtonStyle {
                            width: next(1.0, MIN_SIZE, MAX_SIZE),
                            height: next(1.0, MIN_SIZE, MAX_SIZE),
                            opacity: next(1.0, MIN_OPACITY, 1.0),
                        };
                        s.set_style(control, style);
                    }
                }
                // `button:<id>:x,y:width:height:opacity:name:toggle:swipe:bg:stroke:k0:k1:k2:k3`, as
                // `serialize` writes it; an older five-field line
                // (`button:<id>:x,y:scale:opacity:key`) still reads.
                "button" => {
                    if let Some((id, button)) = parse_button(value) {
                        s.buttons.insert(id, button);
                    }
                }
                _ => {}
            }
        }
        s.migrate_global_style();
        s
    }
    /// Older builds had one size and opacity for every button; each button carries its own now. A file
    /// from then hands those values to every built-in (and to the stick, which keeps its own sliders), so
    /// the overlay looks the same after the update, then stops linking the buttons together.
    fn migrate_global_style(&mut self) {
        let (opacity, scale) = (self.opacity, self.scale);
        if opacity < 1.0 || scale != 1.0 {
            for control in [
                Control::Forward,
                Control::Back,
                Control::Left,
                Control::Right,
                Control::Sneak,
                Control::Jump,
                Control::Perspective,
                Control::Chat,
                Control::Pause,
                Control::HideHud,
                Control::Inventory,
            ] {
                let mut style = self.style(control);
                style.width *= scale;
                style.height *= scale;
                style.opacity *= opacity;
                self.set_style(control, style);
            }
            self.stick_scale = (self.stick_scale * scale).clamp(MIN_SCALE, MAX_SCALE);
            self.stick_opacity = (self.stick_opacity * opacity).clamp(MIN_OPACITY, 1.0);
        }
        self.opacity = 1.0;
        self.scale = 1.0;
    }
    pub fn serialize(&self) -> String {
        let mut text = String::new();
        // Older builds skip the line: the file stays a plain layout to them.
        if self.profile != DEFAULT_PROFILE {
            text.push_str(&format!("profile:{}\n", escape_name(&self.profile)));
        }
        text.push_str(&format!("opacity:{}\nscale:{}\n", self.opacity, self.scale));
        if self.movement == Movement::Stick {
            text.push_str("movement:stick\n");
        }
        if self.stick_scale != 1.0 || self.stick_opacity != 1.0 {
            text.push_str(&format!(
                "stick:{}:{}\n",
                self.stick_scale, self.stick_opacity
            ));
        }
        for (control, placement) in &self.placements {
            if let Some(name) = control_name(*control) {
                text.push_str(&format!("place:{name}:{},{}\n", placement.x, placement.y));
            }
        }
        for control in &self.hidden {
            if let Some(name) = control_name(*control) {
                text.push_str(&format!("hidden:{name}\n"));
            }
        }
        for (control, style) in &self.styles {
            if let Some(name) = control_name(*control)
                && *style != ButtonStyle::default()
            {
                text.push_str(&format!(
                    "size:{name}:{}:{}:{}\n",
                    style.width, style.height, style.opacity
                ));
            }
        }
        for (id, button) in &self.buttons {
            let key = |slot: usize| button.keys[slot].as_deref().unwrap_or("");
            text.push_str(&format!(
                "button:{id}:{},{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}\n",
                button.placement.x,
                button.placement.y,
                button.width,
                button.height,
                button.opacity,
                escape_name(&button.name),
                u8::from(button.toggle),
                u8::from(button.swipe),
                write_color(button.background),
                write_color(button.stroke),
                key(0),
                key(1),
                key(2),
                key(3),
            ));
        }
        text
    }
    /// Default positions, resized and moved; the hotbar follows the HUD and is never hidden.
    pub fn layout(&self, w: f32, h: f32) -> Vec<(Control, Rect)> {
        self.layout_at(w, h, false)
    }
    /// The same, with the buttons the player turned off still in place: the editor shows them faintly so
    /// a long press can turn them back on.
    pub fn editor_layout(&self, w: f32, h: f32) -> Vec<(Control, Rect)> {
        self.layout_at(w, h, true)
    }
    fn layout_at(&self, w: f32, h: f32, hidden_too: bool) -> Vec<(Control, Rect)> {
        let mut v = layout(w, h);
        if self.movement == Movement::Stick {
            // The stick takes the D-pad's 3×3 box, and sneak moves to the left of jump.
            v.retain(|(control, _)| {
                !matches!(
                    control,
                    Control::Forward | Control::Back | Control::Left | Control::Right
                )
            });
            let s = button_size(h);
            let (cx, cy) = (MARGIN + s, h - MARGIN - 2.0 * s - 8.0);
            v.push((Control::Stick, [cx - s, cy - s, 3.0 * s, 3.0 * s]));
            if let Some((_, sneak)) = v.iter_mut().find(|(c, _)| *c == Control::Sneak) {
                *sneak = [w - MARGIN - 2.0 * s - 8.0, cy, s, s];
            }
        }
        if !hidden_too {
            v.retain(|(control, _)| self.visible(*control));
        }
        for (control, r) in &mut v {
            if matches!(control, Control::Hotbar(_)) {
                continue;
            }
            let own = if *control == Control::Stick {
                self.stick_scale.clamp(MIN_SCALE, MAX_SCALE)
            } else {
                1.0
            };
            let style = self.style(*control);
            let (bw, bh) = (
                r[2] * own * style.width,
                r[3] * own * style.height,
            );
            let (mut x, mut y) = (r[0] + (r[2] - bw) / 2.0, r[1] + (r[3] - bh) / 2.0);
            if let Some(placement) = self.placements.get(control) {
                x = placement.x * (w - bw).max(0.0);
                y = placement.y * (h - bh).max(0.0);
            }
            *r = [
                x.clamp(0.0, (w - bw).max(0.0)),
                y.clamp(0.0, (h - bh).max(0.0)),
                bw,
                bh,
            ];
        }
        // The player's buttons, after the built-in ones: each its own size.
        let base = button_size(h);
        for (id, button) in &self.buttons {
            let side = |v: f32| {
                (base * v.clamp(MIN_SIZE, MAX_SIZE))
                    .round()
                    .max(1.0)
            };
            let (bw, bh) = (side(button.width), side(button.height));
            let x = button.placement.x * (w - bw).max(0.0);
            let y = button.placement.y * (h - bh).max(0.0);
            v.push((
                Control::Custom(*id),
                [
                    x.clamp(0.0, (w - bw).max(0.0)),
                    y.clamp(0.0, (h - bh).max(0.0)),
                    bw,
                    bh,
                ],
            ));
        }
        v
    }
    pub fn visible(&self, control: Control) -> bool {
        matches!(
            control,
            Control::Hotbar(_)
                | Control::Pause
                | Control::HideHud
                | Control::Custom(_)
                | Control::Stick
        ) || !self.hidden.contains(&control)
    }
    pub fn set_hidden(&mut self, control: Control, hidden: bool) {
        if matches!(
            control,
            Control::Hotbar(_)
                | Control::Pause
                | Control::HideHud
                | Control::Custom(_)
                | Control::Stick
        ) {
            return;
        }
        if hidden {
            self.hidden.insert(control);
        } else {
            self.hidden.remove(&control);
        }
    }
    /// A built-in button's own size and opacity, or the defaults.
    pub fn style(&self, control: Control) -> ButtonStyle {
        self.styles.get(&control).copied().unwrap_or_default()
    }
    /// Stores a built-in button's style; a default one is forgotten, so the file stays small.
    pub fn set_style(&mut self, control: Control, style: ButtonStyle) {
        if !styled(control) {
            return;
        }
        let style = ButtonStyle {
            width: style.width.clamp(MIN_SIZE, MAX_SIZE),
            height: style.height.clamp(MIN_SIZE, MAX_SIZE),
            opacity: style.opacity.clamp(MIN_OPACITY, 1.0),
        };
        if style == ButtonStyle::default() {
            self.styles.remove(&control);
        } else {
            self.styles.insert(control, style);
        }
    }
    pub fn set_style_size(&mut self, control: Control, width: f32, height: f32) {
        let mut style = self.style(control);
        style.width = width;
        style.height = height;
        self.set_style(control, style);
    }
    pub fn set_style_opacity(&mut self, control: Control, opacity: f32) {
        let mut style = self.style(control);
        style.opacity = opacity;
        self.set_style(control, style);
    }
    /// Where a button's top-left corner goes, as fractions of the space it can move in.
    pub fn set_placement(&mut self, control: Control, x: f32, y: f32) {
        let placement = Placement {
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
        };
        match control {
            Control::Hotbar(_) => {}
            Control::Custom(id) => {
                if let Some(button) = self.buttons.get_mut(&id) {
                    button.placement = placement;
                }
            }
            _ => {
                self.placements.insert(control, placement);
            }
        }
    }
    /// A new button at the middle of the screen, showing "NEW" until it is named. Each further one steps
    /// away from the last instead of piling up on it.
    pub fn add_button(&mut self, w: f32, h: f32) -> u16 {
        let id = self
            .buttons
            .keys()
            .next_back()
            .copied()
            .map_or(0, |id| id.saturating_add(1));
        let size = button_size(h).round();
        let free = ((w - size).max(1.0), (h - size).max(1.0));
        let step = (size * 0.45).max(8.0) * self.buttons.len() as f32;
        let x = ((w - size) / 2.0 + step).rem_euclid(free.0);
        let y = ((h - size) / 2.0 + step).rem_euclid(free.1);
        self.buttons.insert(
            id,
            CustomButton {
                placement: Placement {
                    x: x / free.0,
                    y: y / free.1,
                },
                ..CustomButton::default()
            },
        );
        id
    }
    pub fn remove_button(&mut self, id: u16) {
        self.buttons.remove(&id);
    }
    pub fn button(&self, id: u16) -> Option<&CustomButton> {
        self.buttons.get(&id)
    }
    /// The name the player gave it; the button shows "NEW" while it is empty.
    pub fn set_button_name(&mut self, id: u16, name: String) {
        if let Some(button) = self.buttons.get_mut(&id) {
            button.name = name.chars().filter(|c| *c != '\n').take(MAX_NAME).collect();
        }
    }
    pub fn set_button_key(&mut self, id: u16, slot: usize, key: Option<String>) {
        if let Some(button) = self.buttons.get_mut(&id)
            && slot < MAX_KEYS
        {
            button.keys[slot] = key;
        }
    }
    pub fn clear_button_keys(&mut self, id: u16) {
        if let Some(button) = self.buttons.get_mut(&id) {
            button.keys = std::array::from_fn(|_| None);
        }
    }
    pub fn set_button_size(&mut self, id: u16, width: f32, height: f32) {
        if let Some(button) = self.buttons.get_mut(&id) {
            button.width = width.clamp(MIN_SIZE, MAX_SIZE);
            button.height = height.clamp(MIN_SIZE, MAX_SIZE);
        }
    }
    pub fn set_button_opacity(&mut self, id: u16, opacity: f32) {
        if let Some(button) = self.buttons.get_mut(&id) {
            button.opacity = opacity.clamp(MIN_OPACITY, 1.0);
        }
    }
    pub fn set_button_toggle(&mut self, id: u16, toggle: bool) {
        if let Some(button) = self.buttons.get_mut(&id) {
            button.toggle = toggle;
        }
    }
    pub fn set_button_swipe(&mut self, id: u16, swipe: bool) {
        if let Some(button) = self.buttons.get_mut(&id) {
            button.swipe = swipe;
        }
    }
    /// The background (`stroke` false) or stroke color, `None` for vanilla's look.
    pub fn set_button_color(&mut self, id: u16, stroke: bool, color: Option<u32>) {
        if let Some(button) = self.buttons.get_mut(&id) {
            if stroke {
                button.stroke = color;
            } else {
                button.background = color;
            }
        }
    }
    /// The mapping in `slot`, if one is set.
    pub fn button_key_name(&self, id: u16, slot: usize) -> Option<&str> {
        self.button(id)?.keys.get(slot)?.as_deref()
    }
    /// Every mapping the button sends, in order.
    pub fn button_mappings(&self, id: u16) -> Vec<CustomKey> {
        let Some(button) = self.button(id) else {
            return Vec::new();
        };
        button
            .keys
            .iter()
            .flatten()
            .filter_map(|name| CustomKey::of(name))
            .collect()
    }
    /// The buttons that let go as soon as the finger leaves them.
    pub fn swipes(&self) -> BTreeSet<Control> {
        self.buttons
            .iter()
            .filter(|(_, button)| button.swipe)
            .map(|(id, _)| Control::Custom(*id))
            .collect()
    }
    /// What the button shows: the name the player gave it, or "NEW".
    pub fn button_label(&self, id: u16) -> String {
        match self.button(id) {
            Some(button) if !button.name.is_empty() => button.name.clone(),
            _ => "NEW".to_owned(),
        }
    }
    /// The default layout: positions, sizes, opacities, the buttons turned off and the player's own
    /// buttons all start over. The movement mode and the stick's own size and opacity stay.
    pub fn reset_layout(&mut self) {
        self.opacity = 1.0;
        self.scale = 1.0;
        self.placements.clear();
        self.hidden.clear();
        self.styles.clear();
        self.buttons.clear();
    }
    /// Older builds put every new button on the same spot; a file from then steps the duplicates apart
    /// instead of showing them piled on one another.
    fn spread_stacked_buttons(&mut self) {
        let mut taken: Vec<Placement> = Vec::new();
        for button in self.buttons.values_mut() {
            let mut placement = button.placement;
            while taken.iter().any(|p| {
                (p.x - placement.x).abs() < 1e-4 && (p.y - placement.y).abs() < 1e-4
            }) {
                if placement.x >= 1.0 && placement.y >= 1.0 {
                    break;
                }
                placement.x = (placement.x + 0.05).min(1.0);
                placement.y = (placement.y + 0.05).min(1.0);
            }
            button.placement = placement;
            taken.push(placement);
        }
    }
}

/// The touch layouts besides the active one: one `touch-layouts/<name>.txt` each. The active layout
/// is [`TouchSettings`] itself (as `horizon-touch.txt` keeps it), so an older build still starts
/// with the layout the player last used.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TouchProfiles {
    /// The inactive layouts, by name.
    pub layouts: BTreeMap<String, TouchSettings>,
}

impl TouchProfiles {
    pub fn load(dir: &Path, active: &str) -> Self {
        let mut layouts = BTreeMap::new();
        let Ok(entries) = fs::read_dir(dir.join(LAYOUTS_DIR)) else {
            return Self::default();
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("txt") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            // The active one is `horizon-touch.txt`, whatever a file here is called.
            if !valid_profile_name(name) || name.eq_ignore_ascii_case(active) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let mut layout = TouchSettings::parse(&text);
            layout.spread_stacked_buttons();
            layout.profile = name.to_owned();
            layouts.insert(name.to_owned(), layout);
        }
        Self { layouts }
    }
    /// Writes every layout and drops the files of layouts no longer listed (or of the active one).
    /// The folder exists only while another layout does.
    pub fn save(&self, dir: &Path, active: &TouchSettings) {
        let folder = dir.join(LAYOUTS_DIR);
        if let Ok(entries) = fs::read_dir(&folder) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("txt") {
                    continue;
                }
                let stale = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_none_or(|name| {
                        !self.layouts.contains_key(name)
                            || name.eq_ignore_ascii_case(&active.profile)
                    });
                if stale {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        if self.layouts.is_empty() {
            let _ = fs::remove_dir(&folder);
            return;
        }
        if fs::create_dir_all(&folder).is_err() {
            return;
        }
        for (name, layout) in &self.layouts {
            let _ = fs::write(folder.join(format!("{name}.txt")), layout.serialize());
        }
    }
}

/// What the game does with the fingers' changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    Press(Control),
    Release(Control),
    /// Turn the view by this much, in GUI pixels dragged.
    Look(f32, f32),
    /// A short tap away from the buttons: use, or attack what the crosshair is on.
    Tap,
    /// A finger held still away from the buttons: mining starts, and stops when it lifts.
    MineStart,
    MineEnd,
    /// The analog stick's position: `[left, forward]`, each -1..=1.
    Move(f32, f32),
}

/// How far a finger moves (GUI pixels) before a touch is a drag and not a tap.
const SLOP: f32 = 8.0;
/// How long a finger stays still (seconds) before it mines.
const HOLD: f64 = 0.35;
/// How far from the stick's centre (as a share of its radius) a finger moves nothing.
const STICK_DEADZONE: f32 = 0.15;

/// The stick's `[left, forward]` for a finger at `at`: zero in the middle, full at the ring and beyond.
fn stick_vector(r: Rect, at: (f32, f32)) -> (f32, f32) {
    let radius = (r[2].min(r[3]) / 2.0).max(1.0);
    let dx = (at.0 - (r[0] + r[2] / 2.0)) / radius;
    let dy = (at.1 - (r[1] + r[3] / 2.0)) / radius;
    let len = dx.hypot(dy);
    if len <= STICK_DEADZONE {
        return (0.0, 0.0);
    }
    let mag = ((len - STICK_DEADZONE) / (1.0 - STICK_DEADZONE)).min(1.0);
    // Screen right is strafing right, screen down is walking back.
    (-dx / len * mag, -dy / len * mag)
}

fn stick_rect(layout: &[(Control, Rect)]) -> Option<Rect> {
    layout
        .iter()
        .find(|(c, _)| *c == Control::Stick)
        .map(|(_, r)| *r)
}

#[derive(Clone, Debug)]
enum Kind {
    Button(Control),
    World {
        dragged: bool,
        mining: bool,
        /// A swipeable button the finger is gliding over, held until it leaves.
        swiping: Option<Control>,
    },
    /// A player's swipeable button: its keys follow the finger in and out of the button.
    Swipe {
        control: Control,
        pressed: bool,
    },
    /// The analog stick, wherever its rect is now (the layout may change under the finger).
    Stick,
}

#[derive(Clone, Debug)]
struct Finger {
    id: u64,
    kind: Kind,
    start: (f32, f32),
    last: (f32, f32),
    since: f64,
}

/// The fingers on the screen while playing.
#[derive(Clone, Debug, Default)]
pub struct Touches {
    fingers: Vec<Finger>,
}

impl Touches {
    /// A finger went down at `at`, `now` seconds into some clock. `swipes` are the buttons whose keys let
    /// go when the finger slides off them.
    pub fn start(
        &mut self,
        id: u64,
        at: (f32, f32),
        now: f64,
        layout: &[(Control, Rect)],
        swipes: &BTreeSet<Control>,
    ) -> Vec<Out> {
        let mut out = self.end(id);
        let kind = match hit(layout, at) {
            Some(Control::Stick) => {
                if let Some(r) = stick_rect(layout) {
                    let (left, forward) = stick_vector(r, at);
                    out.push(Out::Move(left, forward));
                }
                Kind::Stick
            }
            Some(control) if swipes.contains(&control) => {
                out.push(Out::Press(control));
                Kind::Swipe {
                    control,
                    pressed: true,
                }
            }
            Some(control) => {
                out.push(Out::Press(control));
                Kind::Button(control)
            }
            None => Kind::World {
                dragged: false,
                mining: false,
                swiping: None,
            },
        };
        self.fingers.push(Finger {
            id,
            kind,
            start: at,
            last: at,
            since: now,
        });
        out
    }

    /// A finger moved: the D-pad follows it from direction to direction, a finger on the world turns the
    /// view and presses any swipeable button it glides over, and a swipeable button's keys follow the
    /// finger in and out.
    pub fn moved(
        &mut self,
        id: u64,
        at: (f32, f32),
        layout: &[(Control, Rect)],
        swipes: &BTreeSet<Control>,
    ) -> Vec<Out> {
        let Some(f) = self.fingers.iter_mut().find(|f| f.id == id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match &mut f.kind {
            Kind::Stick => {
                if let Some(r) = stick_rect(layout) {
                    let (left, forward) = stick_vector(r, at);
                    out.push(Out::Move(left, forward));
                }
            }
            Kind::Button(control) if control.moves() => {
                if let Some(next) = hit(layout, at).filter(|c| c.moves() && c != control) {
                    out.push(Out::Release(*control));
                    out.push(Out::Press(next));
                    *control = next;
                }
            }
            Kind::Button(_) => {}
            Kind::Swipe { control, pressed } => {
                let inside = layout
                    .iter()
                    .find(|(c, _)| c == control)
                    .is_some_and(|(_, r)| contains(*r, at));
                if inside != *pressed {
                    *pressed = inside;
                    out.push(if inside {
                        Out::Press(*control)
                    } else {
                        Out::Release(*control)
                    });
                }
            }
            Kind::World {
                dragged,
                swiping,
                ..
            } => {
                let (dx, dy) = (at.0 - f.start.0, at.1 - f.start.1);
                if !*dragged && dx.hypot(dy) > SLOP {
                    *dragged = true;
                }
                if *dragged {
                    out.push(Out::Look(at.0 - f.last.0, at.1 - f.last.1));
                }
                // Gliding over a swipeable button presses it without stopping the view.
                let over = hit(layout, at).filter(|c| swipes.contains(c));
                if over != *swiping {
                    if let Some(old) = swiping.take() {
                        out.push(Out::Release(old));
                    }
                    if let Some(next) = over {
                        out.push(Out::Press(next));
                    }
                    *swiping = over;
                }
            }
        }
        f.last = at;
        out
    }

    /// A finger lifted.
    pub fn end(&mut self, id: u64) -> Vec<Out> {
        let Some(i) = self.fingers.iter().position(|f| f.id == id) else {
            return Vec::new();
        };
        match self.fingers.remove(i).kind {
            Kind::Stick => vec![Out::Move(0.0, 0.0)],
            Kind::Button(control) => vec![Out::Release(control)],
            Kind::Swipe {
                control,
                pressed: true,
            } => vec![Out::Release(control)],
            Kind::Swipe { .. } => Vec::new(),
            Kind::World {
                mining: true,
                swiping,
                ..
            } => {
                let mut out: Vec<Out> = swiping.into_iter().map(Out::Release).collect();
                out.push(Out::MineEnd);
                out
            }
            Kind::World {
                swiping: Some(control),
                ..
            } => vec![Out::Release(control)],
            Kind::World { dragged: false, .. } => vec![Out::Tap],
            Kind::World { .. } => Vec::new(),
        }
    }

    /// UIKit cancelled a finger: release its holds without interpreting it as a tap.
    pub fn cancel(&mut self, id: u64) -> Vec<Out> {
        let Some(i) = self.fingers.iter().position(|f| f.id == id) else {
            return Vec::new();
        };
        match self.fingers.remove(i).kind {
            Kind::Stick => vec![Out::Move(0.0, 0.0)],
            Kind::Button(control) => vec![Out::Release(control)],
            Kind::Swipe {
                control,
                pressed: true,
            } => vec![Out::Release(control)],
            Kind::World {
                mining: true,
                swiping,
                ..
            } => {
                let mut out: Vec<Out> = swiping.into_iter().map(Out::Release).collect();
                out.push(Out::MineEnd);
                out
            }
            Kind::World {
                swiping: Some(control),
                ..
            } => vec![Out::Release(control)],
            Kind::World { .. } | Kind::Swipe { .. } => Vec::new(),
        }
    }

    /// Once a frame: a finger on the world that has stayed still long enough starts mining.
    pub fn frame(&mut self, now: f64) -> Vec<Out> {
        let mut out = Vec::new();
        for f in &mut self.fingers {
            if let Kind::World {
                dragged: false,
                mining,
                swiping: None,
            } = &mut f.kind
                && !*mining
                && now - f.since >= HOLD
            {
                *mining = true;
                out.push(Out::MineStart);
            }
        }
        out
    }

    /// Every finger lifted at once (a screen opened over the game).
    pub fn release_all(&mut self) -> Vec<Out> {
        let ids: Vec<u64> = self.fingers.iter().map(|f| f.id).collect();
        ids.into_iter().flat_map(|id| self.cancel(id)).collect()
    }

    /// A finger holds this button down (drawn pressed).
    pub fn held(&self, control: Control) -> bool {
        self.fingers.iter().any(|f| match f.kind {
            Kind::Button(c) => c == control,
            Kind::Swipe { control: c, pressed } => pressed && c == control,
            Kind::Stick => control == Control::Stick,
            Kind::World {
                swiping: Some(c), ..
            } => c == control,
            Kind::World { .. } => false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(layout: &[(Control, Rect)], control: Control) -> (f32, f32) {
        let r = layout.iter().find(|(c, _)| *c == control).unwrap().1;
        (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
    }

    const NO_SWIPES: &BTreeSet<Control> = &BTreeSet::new();

    #[test]
    fn buttons_do_not_overlap_and_fit_the_screen() {
        for (w, h) in [(633.0, 292.0), (427.0, 240.0), (480.0, 270.0)] {
            let l = layout(w, h);
            for (i, (a, ra)) in l.iter().enumerate() {
                assert!(
                    ra[0] >= 0.0 && ra[1] >= 0.0 && ra[0] + ra[2] <= w && ra[1] + ra[3] <= h,
                    "{a:?} off screen"
                );
                for (b, rb) in &l[i + 1..] {
                    let apart = ra[0] + ra[2] <= rb[0]
                        || rb[0] + rb[2] <= ra[0]
                        || ra[1] + ra[3] <= rb[1]
                        || rb[1] + rb[3] <= ra[1];
                    assert!(apart, "{a:?} overlaps {b:?} at {w}x{h}");
                }
            }
        }
    }

    #[test]
    fn the_dpad_follows_a_sliding_finger() {
        let l = layout(633.0, 292.0);
        let mut t = Touches::default();
        assert_eq!(
            t.start(1, at(&l, Control::Forward), 0.0, &l, NO_SWIPES),
            [Out::Press(Control::Forward)]
        );
        assert!(t.held(Control::Forward));
        assert_eq!(
            t.moved(1, at(&l, Control::Right), &l, NO_SWIPES),
            [Out::Release(Control::Forward), Out::Press(Control::Right)]
        );
        // Off the pad, the last direction stays held, as in Pocket Edition.
        assert_eq!(t.moved(1, (300.0, 100.0), &l, NO_SWIPES), []);
        assert_eq!(t.end(1), [Out::Release(Control::Right)]);
        assert!(!t.held(Control::Right));
    }

    #[test]
    fn a_quick_still_touch_taps_and_a_held_one_mines() {
        let l = layout(633.0, 292.0);
        let mut t = Touches::default();
        assert_eq!(t.start(1, (300.0, 100.0), 0.0, &l, NO_SWIPES), []);
        assert_eq!(t.frame(0.1), []);
        assert_eq!(t.end(1), [Out::Tap]);

        t.start(2, (300.0, 100.0), 1.0, &l, NO_SWIPES);
        assert_eq!(t.frame(1.3), []);
        assert_eq!(t.frame(1.4), [Out::MineStart]);
        assert_eq!(t.frame(1.5), []);
        assert_eq!(t.end(2), [Out::MineEnd]);
    }

    #[test]
    fn a_dragging_finger_turns_the_view_and_does_not_tap() {
        let l = layout(633.0, 292.0);
        let mut t = Touches::default();
        t.start(1, (300.0, 100.0), 0.0, &l, NO_SWIPES);
        // Within the slop (a thumb's wobble): no look yet, and still a tap candidate.
        assert_eq!(t.moved(1, (305.0, 100.0), &l, NO_SWIPES), []);
        assert_eq!(
            t.moved(1, (313.0, 104.0), &l, NO_SWIPES),
            [Out::Look(8.0, 4.0)]
        );
        assert_eq!(t.frame(5.0), []);
        assert_eq!(t.end(1), []);
    }

    #[test]
    fn two_fingers_move_and_look_at_once() {
        let l = layout(633.0, 292.0);
        let mut t = Touches::default();
        t.start(1, at(&l, Control::Forward), 0.0, &l, NO_SWIPES);
        t.start(2, (400.0, 100.0), 0.0, &l, NO_SWIPES);
        assert_eq!(
            t.moved(2, (420.0, 100.0), &l, NO_SWIPES),
            [Out::Look(20.0, 0.0)]
        );
        assert!(t.held(Control::Forward));
        let mut all = t.release_all();
        all.sort_by_key(|o| format!("{o:?}"));
        assert_eq!(all, [Out::Release(Control::Forward)]);
    }

    fn placed(l: &[(Control, Rect)], control: Control) -> Rect {
        l.iter().find(|(c, _)| *c == control).unwrap().1
    }

    #[test]
    fn touch_settings_survive_a_round_trip() {
        let mut s = TouchSettings::default();
        s.set_placement(Control::Jump, 0.25, 0.6);
        s.set_hidden(Control::Chat, true);
        s.set_style_size(Control::Jump, 1.2, 0.9);
        s.set_style_opacity(Control::Jump, 0.4);
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
        let line = s.serialize();
        let line = line.lines().find(|l| l.starts_with("size:jump:")).unwrap();
        assert_eq!(line.split(':').count(), 5);
        // Defaults write no size lines at all.
        assert!(!TouchSettings::default().serialize().contains("size:"));
    }

    #[test]
    fn a_layout_keeps_its_profile_name() {
        let mut s = TouchSettings::default();
        assert_eq!(s.profile, DEFAULT_PROFILE);
        assert!(!s.serialize().contains("profile:"));
        s.profile = "pvp".to_owned();
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
        assert!(s.serialize().starts_with("profile:pvp\n"));
        // A file from before profiles, or an unusable name, starts as Default.
        assert_eq!(TouchSettings::parse("opacity:1\n").profile, DEFAULT_PROFILE);
        assert_eq!(
            TouchSettings::parse("profile:bad/name\n").profile,
            DEFAULT_PROFILE
        );
        // Resetting the arrangement keeps the profile.
        s.set_placement(Control::Jump, 0.5, 0.5);
        s.reset_layout();
        assert_eq!(s.profile, "pvp");
    }

    #[test]
    fn profile_names_are_file_stems() {
        assert!(valid_profile_name("pvp"));
        assert!(valid_profile_name("PvP 1v1"));
        assert!(valid_profile_name("a"));
        assert!(!valid_profile_name(""));
        assert!(!valid_profile_name("   "));
        assert!(!valid_profile_name("bad/name"));
        assert!(!valid_profile_name("dots.dots"));
        assert!(!valid_profile_name(&"x".repeat(MAX_PROFILE_NAME + 1)));
    }

    #[test]
    fn layouts_round_trip_through_their_folder() {
        let dir = std::env::temp_dir().join(format!("horizon-touch-layouts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut active = TouchSettings::default();
        active.profile = "pvp".to_owned();
        let mut profiles = TouchProfiles::default();
        // A layout equal to the default is still a layout: its file is what remembers it.
        profiles
            .layouts
            .insert("Default".to_owned(), TouchSettings::default());
        let mut building = TouchSettings::default();
        building.profile = "building".to_owned();
        building.set_placement(Control::Jump, 0.25, 0.6);
        profiles.layouts.insert("building".to_owned(), building);
        profiles.save(&dir, &active);
        assert_eq!(TouchProfiles::load(&dir, &active.profile), profiles);
        // The active layout is `horizon-touch.txt`, not one of the folder's files.
        assert!(!dir.join(LAYOUTS_DIR).join("pvp.txt").exists());
        // Deleting one drops its file; deleting every one drops the folder.
        profiles.layouts.remove("building");
        profiles.save(&dir, &active);
        assert!(!dir.join(LAYOUTS_DIR).join("building.txt").exists());
        profiles.layouts.clear();
        profiles.save(&dir, &active);
        assert!(!dir.join(LAYOUTS_DIR).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_hotbar_pause_and_hide_hud_stay_on_and_in_place() {
        let mut s = TouchSettings::default();
        s.set_hidden(Control::Pause, true);
        s.set_hidden(Control::HideHud, true);
        s.set_hidden(Control::Hotbar(3), true);
        assert!(s.visible(Control::Pause));
        assert!(s.visible(Control::HideHud));
        assert!(s.visible(Control::Hotbar(3)));
        let l = s.layout(633.0, 292.0);
        assert!(l.iter().any(|(c, _)| *c == Control::Pause));
        assert!(l.iter().any(|(c, _)| *c == Control::HideHud));
        assert!(l.iter().any(|(c, _)| *c == Control::Hotbar(3)));
        // A file naming it is read but still leaves it on.
        let old = TouchSettings::parse("hidden:hide_hud\n");
        assert!(old.visible(Control::HideHud));
        assert_eq!(control_from_name("hide_hud"), Some(Control::HideHud));
    }

    #[test]
    fn the_hide_hud_button_sits_in_the_top_row_after_pause() {
        let l = layout(633.0, 292.0);
        let (pause, hud) = (placed(&l, Control::Pause), placed(&l, Control::HideHud));
        assert!(hud[0] > pause[0]);
        assert!((hud[1] - pause[1]).abs() < 0.01);
        assert!((hud[2] - pause[2]).abs() < 0.01);
    }

    #[test]
    fn a_swipe_button_holds_its_keys_only_while_the_finger_is_on_it() {
        let mut s = TouchSettings::default();
        let id = s.add_button(633.0, 292.0);
        s.set_button_swipe(id, true);
        assert_eq!(s.swipes(), BTreeSet::from([Control::Custom(id)]));
        let l = s.layout(633.0, 292.0);
        let r = placed(&l, Control::Custom(id));
        let centre = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        let outside = (r[0] + r[2] + 40.0, centre.1);
        let swipes = s.swipes();
        let mut t = Touches::default();
        // A finger on it presses its keys...
        assert_eq!(
            t.start(1, centre, 0.0, &l, &swipes),
            [Out::Press(Control::Custom(id))]
        );
        assert!(t.held(Control::Custom(id)));
        // ...lets go as soon as it slides off...
        assert_eq!(
            t.moved(1, outside, &l, &swipes),
            [Out::Release(Control::Custom(id))]
        );
        assert!(!t.held(Control::Custom(id)));
        // ...and presses again on coming back.
        assert_eq!(
            t.moved(1, centre, &l, &swipes),
            [Out::Press(Control::Custom(id))]
        );
        // Lifting inside releases; a finger that already slid off releases nothing.
        assert_eq!(t.end(1), [Out::Release(Control::Custom(id))]);
        t.start(2, centre, 1.0, &l, &swipes);
        t.moved(2, outside, &l, &swipes);
        assert_eq!(t.end(2), []);
        // A cancelled finger lets go of what it held.
        t.start(3, centre, 2.0, &l, &swipes);
        assert_eq!(t.cancel(3), [Out::Release(Control::Custom(id))]);
        // A button that is not swipeable keeps its keys held even as the finger slides off.
        t.start(4, centre, 3.0, &l, NO_SWIPES);
        assert_eq!(t.moved(4, outside, &l, NO_SWIPES), []);
        assert!(t.held(Control::Custom(id)));
        assert_eq!(t.end(4), [Out::Release(Control::Custom(id))]);
    }

    #[test]
    fn a_finger_gliding_over_a_swipe_button_presses_it_and_still_turns_the_view() {
        let mut s = TouchSettings::default();
        let id = s.add_button(633.0, 292.0);
        s.set_button_swipe(id, true);
        let l = s.layout(633.0, 292.0);
        let r = placed(&l, Control::Custom(id));
        let y = r[1] + r[3] / 2.0;
        let start = (r[0] - 60.0, y);
        let inside = (r[0] + 10.0, y);
        let past = (r[0] + r[2] + 10.0, y);
        let swipes = s.swipes();
        let mut t = Touches::default();
        // A finger that starts away from the buttons turns the view...
        assert_eq!(t.start(1, start, 0.0, &l, &swipes), []);
        // ...and presses a swipeable button as it glides onto it, still turning.
        assert_eq!(
            t.moved(1, inside, &l, &swipes),
            [
                Out::Look(inside.0 - start.0, 0.0),
                Out::Press(Control::Custom(id))
            ]
        );
        assert!(t.held(Control::Custom(id)));
        // Gliding off lets go and keeps turning; lifting afterwards is not a tap.
        assert_eq!(
            t.moved(1, past, &l, &swipes),
            [
                Out::Look(past.0 - inside.0, 0.0),
                Out::Release(Control::Custom(id))
            ]
        );
        assert!(!t.held(Control::Custom(id)));
        assert_eq!(t.end(1), []);
    }

    #[test]
    fn colors_cycle_through_the_palette_and_back_to_default() {
        let mut color = None;
        for (name, value) in COLORS {
            color = cycle_color(color);
            assert_eq!(color, Some(value));
            assert_eq!(color_name(color), name);
        }
        assert_eq!(cycle_color(color), None);
        assert_eq!(color_name(None), "Default");
    }

    #[test]
    fn a_hidden_button_leaves_the_layout() {
        let mut s = TouchSettings::default();
        s.set_hidden(Control::Chat, true);
        let l = s.layout(633.0, 292.0);
        assert!(!l.iter().any(|(c, _)| *c == Control::Chat));
        assert_eq!(l.len(), layout(633.0, 292.0).len() - 1);
    }

    #[test]
    fn a_placement_moves_a_button_and_keeps_it_on_screen() {
        let mut s = TouchSettings::default();
        s.set_placement(Control::Jump, 0.1, 0.2);
        let r = placed(&s.layout(600.0, 300.0), Control::Jump);
        // Fractions of the space the button can move in, so a resize keeps the layout's shape.
        assert!((r[0] - 0.1 * (600.0 - r[2])).abs() < 0.01);
        assert!((r[1] - 0.2 * (300.0 - r[3])).abs() < 0.01);
        // Far over the edge: the button is pulled back onto the screen.
        s.set_placement(Control::Jump, 5.0, 5.0);
        let r = placed(&s.layout(600.0, 300.0), Control::Jump);
        assert!(r[0] >= 0.0 && r[1] >= 0.0);
        assert!(r[0] + r[2] <= 600.0 && r[1] + r[3] <= 300.0);
    }

    #[test]
    fn an_old_global_size_and_opacity_become_each_buttons_own() {
        let s = TouchSettings::parse("opacity:0.5\nscale:1.25\n");
        // The globals are gone; every built-in carries them, and the stick keeps its own sliders.
        assert_eq!((s.opacity, s.scale), (1.0, 1.0));
        let style = s.style(Control::Jump);
        assert!((style.width - 1.25).abs() < 0.001);
        assert!((style.height - 1.25).abs() < 0.001);
        assert!((style.opacity - 0.5).abs() < 0.001);
        assert!((s.stick_scale - 1.25).abs() < 0.001);
        assert!((s.stick_opacity - 0.5).abs() < 0.001);
        let base = layout(600.0, 300.0);
        let l = s.layout(600.0, 300.0);
        let (b, j) = (placed(&base, Control::Jump), placed(&l, Control::Jump));
        assert!((j[2] - b[2] * 1.25).abs() < 0.01);
        // The hotbar follows the HUD and never takes a style.
        assert_eq!(
            placed(&l, Control::Hotbar(0)),
            placed(&base, Control::Hotbar(0))
        );
        // Saved back, the file carries per-button sizes, not the old globals.
        let text = s.serialize();
        assert!(text.contains("size:jump:"));
        assert!(text.contains("opacity:1\nscale:1\n"));
        assert_eq!(TouchSettings::parse(&text), s);
    }

    #[test]
    fn custom_buttons_survive_a_round_trip() {
        let mut s = TouchSettings::default();
        let id = s.add_button(633.0, 292.0);
        s.set_button_name(id, "Fly".into());
        s.set_button_key(id, 0, Some("key.keyboard.f3".into()));
        s.set_button_key(id, 2, Some("key.mouse.left".into()));
        s.set_button_size(id, 1.25, 0.75);
        s.set_button_opacity(id, 0.5);
        s.set_button_toggle(id, true);
        s.set_button_swipe(id, true);
        s.set_button_color(id, false, Some(COLORS[3].1));
        s.set_button_color(id, true, Some(COLORS[6].1));
        s.set_placement(Control::Custom(id), 0.1, 0.7);
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
        // The keys press in order; empty slots are skipped.
        assert_eq!(
            s.button_mappings(id),
            [
                CustomKey::Key(winit::keyboard::KeyCode::F3),
                CustomKey::Mouse(winit::event::MouseButton::Left)
            ]
        );
        // A name keeps even the file's own separators.
        s.set_button_name(id, "a:b%c".into());
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
        assert_eq!(s.button_label(id), "a:b%c");
    }

    #[test]
    fn old_button_lines_still_load_and_new_ones_are_written() {
        let s = TouchSettings::parse("button:7:0.25,0.5:1.5:0.5:key.keyboard.g\n");
        let b = s.button(7).unwrap();
        assert_eq!((b.width, b.height), (1.5, 1.5));
        assert_eq!(b.opacity, 0.5);
        assert_eq!(b.keys[0].as_deref(), Some("key.keyboard.g"));
        // The old button showed its key's label, so that label becomes its name: updating the app must
        // not turn every button into "NEW".
        assert_eq!(b.name, key_label("key.keyboard.g"));
        assert!(!b.toggle && !b.swipe);
        assert_eq!(b.background, None);
        // Saved back, the button keeps its shape in the current format.
        let again = TouchSettings::parse(&s.serialize());
        assert_eq!(again, s);
        assert!(s.serialize().contains("button:7:"));
        let line = s.serialize();
        let line = line.lines().find(|l| l.starts_with("button:")).unwrap();
        assert_eq!(line.split(':').count(), 15);
    }

    #[test]
    fn a_custom_button_can_send_a_mouse_click() {
        let mut s = TouchSettings::default();
        let id = s.add_button(600.0, 300.0);
        s.set_button_key(id, 0, Some("key.mouse.left".into()));
        assert_eq!(key_label("key.mouse.left"), "LMB");
        assert_eq!(
            s.button_mappings(id),
            [CustomKey::Mouse(winit::event::MouseButton::Left)]
        );
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
        s.set_button_key(id, 0, Some("key.mouse.right".into()));
        assert_eq!(key_label("key.mouse.right"), "RMB");
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
        s.set_button_key(id, 0, Some("key.mouse.middle".into()));
        assert_eq!(key_label("key.mouse.middle"), "MMB");
        assert_eq!(
            s.button_mappings(id),
            [CustomKey::Mouse(winit::event::MouseButton::Middle)]
        );
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
        // A name that is neither a key nor a mouse button is still refused.
        let bad = TouchSettings::parse("button:1:0.5,0.5:1:1:1::0:0:-:-:key.mouse.nope:::\n");
        assert!(bad.button_mappings(1).is_empty());
    }

    #[test]
    fn a_new_button_is_centred_and_labelled_new() {
        let mut s = TouchSettings::default();
        let id = s.add_button(600.0, 300.0);
        assert_eq!(s.button_label(id), "NEW");
        assert!(s.button_mappings(id).is_empty());
        let r = placed(&s.layout(600.0, 300.0), Control::Custom(id));
        let size = r[2];
        assert!((r[0] - (600.0 - size) / 2.0).abs() < 0.01);
        assert!((r[1] - (300.0 - size) / 2.0).abs() < 0.01);
        // Naming it replaces "NEW"; clearing the name brings it back.
        s.set_button_name(id, "Sprint".into());
        assert_eq!(s.button_label(id), "Sprint");
        s.set_button_name(id, String::new());
        assert_eq!(s.button_label(id), "NEW");
    }

    #[test]
    fn custom_buttons_get_unique_ids_and_can_be_removed() {
        let mut s = TouchSettings::default();
        let a = s.add_button(640.0, 360.0);
        let b = s.add_button(640.0, 360.0);
        assert_ne!(a, b);
        s.remove_button(a);
        assert!(s.button(a).is_none());
        assert!(
            s.layout(640.0, 360.0)
                .iter()
                .any(|(c, _)| *c == Control::Custom(b))
        );
    }

    #[test]
    fn resetting_the_layout_clears_positions_styles_and_custom_buttons() {
        let mut s = TouchSettings::default();
        let id = s.add_button(600.0, 300.0);
        s.set_button_key(id, 0, Some("key.keyboard.f3".into()));
        s.set_placement(Control::Jump, 0.25, 0.6);
        s.set_hidden(Control::Chat, true);
        s.set_style_size(Control::Jump, 1.5, 0.5);
        s.set_style_opacity(Control::Jump, 0.3);
        s.opacity = 0.5;
        s.scale = 1.25;
        s.reset_layout();
        assert!(s.buttons.is_empty());
        assert!(s.placements.is_empty());
        assert!(s.hidden.is_empty());
        assert!(s.styles.is_empty());
        assert_eq!((s.opacity, s.scale), (1.0, 1.0));
        assert_eq!(s, TouchSettings::default());
    }

    #[test]
    fn each_built_in_keeps_its_own_size_and_opacity() {
        let mut s = TouchSettings::default();
        let base = s.layout(600.0, 300.0);
        s.set_style_size(Control::Jump, 1.5, 0.5);
        s.set_style_opacity(Control::Jump, 0.4);
        let l = s.layout(600.0, 300.0);
        let (jump, before) = (placed(&l, Control::Jump), placed(&base, Control::Jump));
        assert!((jump[2] - before[2] * 1.5).abs() < 0.01);
        assert!((jump[3] - before[3] * 0.5).abs() < 0.01);
        assert_eq!(s.style(Control::Jump).opacity, 0.4);
        // Every other button stays exactly where and how it was.
        for control in [Control::Forward, Control::Chat, Control::Perspective] {
            assert_eq!(placed(&l, control), placed(&base, control));
            assert_eq!(s.style(control), ButtonStyle::default());
        }
        // Only styled controls take a style; the hotbar and the stick keep their own pages.
        s.set_style_size(Control::Hotbar(0), 2.0, 2.0);
        s.set_style_size(Control::Stick, 2.0, 2.0);
        assert_eq!(s.styles, BTreeMap::from([(Control::Jump, s.style(Control::Jump))]));
        // A style back at the default is forgotten.
        s.set_style_size(Control::Jump, 1.0, 1.0);
        s.set_style_opacity(Control::Jump, 1.0);
        assert!(s.styles.is_empty());
    }

    #[test]
    fn the_editor_shows_buttons_the_player_turned_off() {
        let mut s = TouchSettings::default();
        s.set_hidden(Control::Chat, true);
        assert!(!s.layout(600.0, 300.0).iter().any(|(c, _)| *c == Control::Chat));
        let editor = s.editor_layout(600.0, 300.0);
        assert!(editor.iter().any(|(c, _)| *c == Control::Chat));
        // The hidden button keeps its default place, so it can be turned back on where it was.
        assert_eq!(
            placed(&editor, Control::Chat),
            placed(&layout(600.0, 300.0), Control::Chat)
        );
    }

    #[test]
    fn new_buttons_step_away_from_the_last() {
        let mut s = TouchSettings::default();
        let ids: Vec<u16> = (0..3).map(|_| s.add_button(600.0, 300.0)).collect();
        let spots: Vec<(f32, f32)> = ids
            .iter()
            .map(|id| {
                let p = s.button(*id).unwrap().placement;
                (p.x, p.y)
            })
            .collect();
        for (i, a) in spots.iter().enumerate() {
            for b in &spots[i + 1..] {
                assert!((a.0 - b.0).abs() > 1e-3 || (a.1 - b.1).abs() > 1e-3);
            }
        }
    }

    #[test]
    fn stacked_buttons_from_an_older_file_are_spread_on_load() {
        let mut s = TouchSettings::parse(
            "button:1:0.5,0.5:1:1:1\n\
             button:2:0.5,0.5:1:1:1\n\
             button:3:0.5,0.5:1:1:1\n",
        );
        let stacked = |s: &TouchSettings| {
            let p: Vec<Placement> = s.buttons.values().map(|b| b.placement).collect();
            (p[0] == p[1]) || (p[1] == p[2]) || (p[0] == p[2])
        };
        assert!(stacked(&s));
        s.spread_stacked_buttons();
        assert!(!stacked(&s));
    }

    #[test]
    fn only_some_buttons_may_be_hidden_or_styled() {
        for control in [Control::Forward, Control::Jump, Control::Chat, Control::Inventory] {
            assert!(can_hide(control) && styled(control));
        }
        // Pause is a phone's way back and the hide-HUD button brings the HUD back: neither can be
        // turned off, though both can be resized and faded.
        for control in [Control::Pause, Control::HideHud] {
            assert!(!can_hide(control) && styled(control));
        }
        // The hotbar follows the HUD and the stick has the Touch Controls page for its own size.
        for control in [Control::Hotbar(0), Control::Stick] {
            assert!(!can_hide(control) && !styled(control) && !editable(control));
        }
        assert!(editable(Control::Forward) && editable(Control::Custom(0)));
    }

    #[test]
    fn a_custom_button_uses_its_own_width_height_and_opacity() {
        let mut s = TouchSettings::default();
        let id = s.add_button(600.0, 300.0);
        let base = placed(&s.layout(600.0, 300.0), Control::Custom(id));
        s.set_button_size(id, 1.5, 0.5);
        s.set_button_opacity(id, 0.1);
        let bigger = placed(&s.layout(600.0, 300.0), Control::Custom(id));
        assert!((bigger[2] - base[2] * 1.5).abs() < 0.01);
        assert!((bigger[3] - base[3] * 0.5).abs() < 0.01);
        let button = s.button(id).unwrap();
        assert_eq!(button.opacity, MIN_OPACITY);
        // Far past either end, the sides stay in bounds.
        s.set_button_size(id, 9.0, 0.0);
        let button = s.button(id).unwrap();
        assert_eq!((button.width, button.height), (MAX_SIZE, MIN_SIZE));
    }

    #[test]
    fn bad_button_lines_are_skipped_or_clamped() {
        let s = TouchSettings::parse(
            "button:x:0,0:1:1:key.keyboard.f3\n\
             button:2:2,-1:9:nope:\n\
             button:3:0.5,0.5:1.25:0.5:key.keyboard.a\n\
             button:4:0,0:1:1:key.keyboard.nope\n\
             button:5:0.5,0.5:1:1:1::0:0:-:-::::\n",
        );
        assert!(s.button(1).is_none());
        let two = s.button(2).unwrap();
        assert_eq!(two.placement, Placement { x: 1.0, y: 0.0 });
        assert_eq!((two.width, two.height), (MAX_SCALE, MAX_SCALE));
        assert_eq!(two.opacity, 1.0);
        assert!(two.keys.iter().all(Option::is_none));
        let three = s.button(3).unwrap();
        assert_eq!(three.keys[0].as_deref(), Some("key.keyboard.a"));
        // An unknown key name leaves the slot empty.
        assert!(s.button(4).unwrap().keys.iter().all(Option::is_none));
        // A new-format line with every slot empty still parses.
        assert_eq!(s.button(5).unwrap().keys, [None, None, None, None]);
        // Too few or too many parts are not a button.
        assert!(TouchSettings::parse("button:6:0,0:1\n").button(6).is_none());
    }

    #[test]
    fn bad_values_fall_back_to_the_defaults() {
        let s = TouchSettings::parse("opacity:9\nscale:nope\nhidden:pause\nplace:jump:2,-1\n");
        assert_eq!(s.opacity, 1.0);
        assert_eq!(s.scale, 1.0);
        assert!(s.visible(Control::Pause));
        assert_eq!(
            s.placements.get(&Control::Jump),
            Some(&Placement { x: 1.0, y: 0.0 })
        );
    }

    fn stick_settings() -> TouchSettings {
        TouchSettings {
            movement: Movement::Stick,
            ..TouchSettings::default()
        }
    }

    #[test]
    fn stick_mode_replaces_the_dpad_and_moves_sneak_left_of_jump() {
        let s = stick_settings();
        let l = s.layout(633.0, 292.0);
        assert!(l.iter().all(|(c, _)| !c.moves()));
        let stick = placed(&l, Control::Stick);
        let s_size = button_size(292.0);
        assert!((stick[2] - 3.0 * s_size).abs() < 0.01);
        assert!((stick[3] - 3.0 * s_size).abs() < 0.01);
        let (sneak, jump) = (placed(&l, Control::Sneak), placed(&l, Control::Jump));
        assert!((sneak[0] + sneak[2] + 8.0 - jump[0]).abs() < 0.01);
        assert!((sneak[1] - jump[1]).abs() < 0.01);
        // The D-pad mode still has its four buttons and no stick.
        let d = TouchSettings::default().layout(633.0, 292.0);
        assert!(d.iter().any(|(c, _)| *c == Control::Forward));
        assert!(d.iter().all(|(c, _)| *c != Control::Stick));
    }

    #[test]
    fn stick_mode_buttons_do_not_overlap_and_fit_the_screen() {
        for (w, h) in [(633.0, 292.0), (427.0, 240.0), (480.0, 270.0)] {
            let l = stick_settings().layout(w, h);
            for (i, (a, ra)) in l.iter().enumerate() {
                assert!(
                    ra[0] >= 0.0 && ra[1] >= 0.0 && ra[0] + ra[2] <= w && ra[1] + ra[3] <= h,
                    "{a:?} off screen"
                );
                for (b, rb) in &l[i + 1..] {
                    let apart = ra[0] + ra[2] <= rb[0]
                        || rb[0] + rb[2] <= ra[0]
                        || ra[1] + ra[3] <= rb[1]
                        || rb[1] + rb[3] <= ra[1];
                    assert!(apart, "{a:?} overlaps {b:?} at {w}x{h}");
                }
            }
        }
    }

    #[test]
    fn the_stick_is_dead_in_the_middle_and_full_past_the_ring() {
        let s = stick_settings();
        let r = placed(&s.layout(633.0, 292.0), Control::Stick);
        let centre = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        let radius = r[2] / 2.0;
        assert_eq!(stick_vector(r, centre), (0.0, 0.0));
        assert_eq!(
            stick_vector(r, (centre.0 + 0.1 * radius, centre.1)),
            (0.0, 0.0)
        );
        // Right strafes right, up walks forward.
        let (left, forward) = stick_vector(r, (centre.0 + radius, centre.1));
        assert!((left + 1.0).abs() < 0.001 && forward.abs() < 0.001);
        let (left, forward) = stick_vector(r, (centre.0, centre.1 - radius));
        assert!(left.abs() < 0.001 && (forward - 1.0).abs() < 0.001);
        // Half way out: half speed once past the deadzone.
        let (_, forward) = stick_vector(r, (centre.0, centre.1 + radius / 2.0));
        assert!((forward + (0.5 - STICK_DEADZONE) / (1.0 - STICK_DEADZONE)).abs() < 0.001);
        // Off the ring, the stick stays at full, and never past it.
        let (left, forward) = stick_vector(r, (centre.0 - 2.0 * radius, centre.1 + 2.0 * radius));
        assert!((left.hypot(forward) - 1.0).abs() < 0.001);
        assert!(left > 0.0 && forward < 0.0);
    }

    #[test]
    fn a_finger_on_the_stick_moves_and_a_lift_stops_it() {
        let s = stick_settings();
        let l = s.layout(633.0, 292.0);
        let r = placed(&l, Control::Stick);
        let centre = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        let radius = r[2] / 2.0;
        let mut t = Touches::default();
        assert_eq!(
            t.start(1, centre, 0.0, &l, NO_SWIPES),
            [Out::Move(0.0, 0.0)]
        );
        assert!(t.held(Control::Stick));
        assert_eq!(
            t.moved(1, (centre.0 - radius, centre.1), &l, NO_SWIPES),
            [Out::Move(1.0, 0.0)]
        );
        assert_eq!(t.end(1), [Out::Move(0.0, 0.0)]);
        assert!(!t.held(Control::Stick));
    }

    #[test]
    fn the_stick_moves_while_another_finger_looks() {
        let s = stick_settings();
        let l = s.layout(633.0, 292.0);
        let r = placed(&l, Control::Stick);
        let centre = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        let mut t = Touches::default();
        t.start(1, centre, 0.0, &l, NO_SWIPES);
        t.start(2, (400.0, 100.0), 0.0, &l, NO_SWIPES);
        assert_eq!(
            t.moved(2, (420.0, 100.0), &l, NO_SWIPES),
            [Out::Look(20.0, 0.0)]
        );
        assert!(t.held(Control::Stick));
        assert_eq!(t.release_all(), [Out::Move(0.0, 0.0)]);
    }

    #[test]
    fn stick_settings_survive_a_round_trip() {
        let mut s = stick_settings();
        s.stick_scale = 1.25;
        s.stick_opacity = 0.5;
        s.set_placement(Control::Stick, 0.1, 0.6);
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
        // A file written before the stick existed has no such lines, and the default file gains none.
        let old = TouchSettings::parse("opacity:1\nscale:1\n");
        assert_eq!(old.movement, Movement::DPad);
        let default = TouchSettings::default().serialize();
        assert!(!default.contains("movement") && !default.contains("stick:"));
        // Resetting the layout keeps the mode and the stick's own size and opacity.
        s.reset_layout();
        assert_eq!(s.movement, Movement::Stick);
        assert_eq!((s.stick_scale, s.stick_opacity), (1.25, 0.5));
        assert!(!s.placements.contains_key(&Control::Stick));
    }

    #[test]
    fn bad_stick_values_fall_back_to_the_defaults() {
        let s = TouchSettings::parse("movement:nope\nstick:9:nope\n");
        assert_eq!(s.movement, Movement::DPad);
        assert_eq!(s.stick_scale, MAX_SCALE);
        assert_eq!(s.stick_opacity, 1.0);
    }
}
