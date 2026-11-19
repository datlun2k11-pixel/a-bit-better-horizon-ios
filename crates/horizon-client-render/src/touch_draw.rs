//! Drawing the touch overlay's buttons: the game's controls over the HUD and the Touch Controls page's
//! preview, so the settings show exactly what a finger will press.
use crate::{
    hud::{HudAtlas, HudBatch, rgba},
    text::Text,
    touch::{Control, Rect},
};

/// A whole control: vanilla's button behind its mark.
pub fn draw_control(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    control: Control,
    r: Rect,
    held: bool,
    opacity: f32,
) {
    draw_button(batch, atlas, r, held, opacity);
    shadowed(r, |r, color| glyph(batch, atlas, control, r, color));
}

/// A player's button: its name ("NEW" until named) over vanilla's sprite, or over the solid background
/// and inside the stroke color the player picked.
pub fn draw_custom(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    label: &str,
    r: Rect,
    held: bool,
    opacity: f32,
    background: Option<u32>,
    stroke: Option<u32>,
) {
    let opacity = opacity.clamp(0.0, 1.0);
    match background {
        Some(color) => {
            let alpha = ((if held { 230.0 } else { 200.0 }) * opacity) as u8;
            batch.fill(atlas, r[0], r[1], r[2], r[3], with_alpha(color, alpha));
            if held {
                batch.fill(atlas, r[0], r[1], r[2], r[3], rgba(255, 255, 255, 40));
            }
        }
        None => draw_button(batch, atlas, r, held, opacity),
    }
    if let Some(color) = stroke {
        let t = (r[2].min(r[3]) / 16.0).round().clamp(1.0, 3.0);
        let color = with_alpha(color, (255.0 * opacity) as u8);
        batch.fill(atlas, r[0], r[1], r[2], t, color);
        batch.fill(atlas, r[0], r[1] + r[3] - t, r[2], t, color);
        batch.fill(atlas, r[0], r[1], t, r[3], color);
        batch.fill(atlas, r[0] + r[2] - t, r[1], t, r[3], color);
    }
    crate::ui::centered_text(
        batch,
        atlas,
        &Text::literal(label),
        r[0] + 2.0,
        r[1] + ((r[3] - 8.0) / 2.0).floor(),
        r[2] - 4.0,
        rgba(255, 255, 255, 255),
    );
}

/// A color from [`crate::touch::COLORS`] at a new alpha.
fn with_alpha(color: u32, alpha: u8) -> u32 {
    (color & 0x00FF_FFFF) | u32::from(alpha) << 24
}

/// A whole-pixel square of the stick's grid: its top-left corner and its side.
#[derive(Clone, Copy, Debug, PartialEq)]
struct StickCell {
    x: f32,
    y: f32,
    size: f32,
}

/// Which part of the stick a cell paints, in the order they are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StickLayer {
    Disc,
    Ring,
    Thumb,
}

/// The analog stick, drawn as pixel art: a dark disc, a light ring, and a thumb that follows the finger,
/// each made of hard-edged cells on one grid. `analog` is the stick's `[left, forward]`, each -1..=1; the
/// thumb sits towards the direction it moves the player and follows the finger without snapping.
pub fn draw_stick(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    r: Rect,
    analog: [f32; 2],
    held: bool,
    opacity: f32,
) {
    let alpha = (if held { 150.0 } else { 110.0 }) * opacity.clamp(0.0, 1.0);
    for (layer, cell) in stick_cells(r, analog) {
        let color = match layer {
            StickLayer::Disc => rgba(24, 24, 24, (alpha * 0.55) as u8),
            StickLayer::Ring => rgba(230, 230, 230, alpha as u8),
            StickLayer::Thumb => rgba(245, 245, 245, alpha as u8),
        };
        batch.fill(atlas, cell.x, cell.y, cell.size, cell.size, color);
    }
}

/// The cells the stick covers, disc first, ring over it, thumb last: the pure half of [`draw_stick`].
///
/// The grid is `cell = (radius / 9).round().max(2)` pixels across, centred on the stick's centre rounded to
/// whole pixels, so every cell has whole-pixel edges. A cell joins a layer by its centre's distance from
/// the stick's centre: the disc out to `0.94 * radius`, the ring between `0.86 * radius` and `radius`, and
/// the thumb the cells within `0.32 * radius` of the thumb's centre, which follows the finger smoothly.
fn stick_cells(r: Rect, analog: [f32; 2]) -> Vec<(StickLayer, StickCell)> {
    let radius = r[2].min(r[3]) / 2.0;
    if radius <= 0.0 {
        return Vec::new();
    }
    let cell = (radius / 9.0).round().max(2.0);
    let (cx, cy) = ((r[0] + r[2] / 2.0).round(), (r[1] + r[3] / 2.0).round());
    // Cells reach a whole cell past the radius on each side, enough that the circle never clips.
    let n = (radius / cell).ceil() as i32 + 1;
    let (ox, oy) = (cx - n as f32 * cell, cy - n as f32 * cell);

    // The thumb's centre follows the finger smoothly. Only the cells it paints sit on the grid: snapping
    // the centre to cell corners made a slow or jittery finger pop the thumb a whole cell off the touch.
    let thumb = radius * 0.32;
    let (ax, ay) = (-analog[0], -analog[1]);
    let len = ax.hypot(ay);
    let (dx, dy) = if len > 1.0 {
        (ax / len, ay / len)
    } else {
        (ax, ay)
    };
    let reach = radius - thumb;
    let thumb_centre = (cx + dx * reach, cy + dy * reach);

    let (mut disc, mut ring, mut thumb_cells) = (Vec::new(), Vec::new(), Vec::new());
    for j in 0..2 * n {
        for i in 0..2 * n {
            let x = ox + i as f32 * cell;
            let y = oy + j as f32 * cell;
            let (mx, my) = (x + cell / 2.0, y + cell / 2.0);
            let d = (mx - cx).hypot(my - cy);
            // Outside the stick's silhouette: nothing to paint, so it stays round and inside `radius`.
            if d > radius {
                continue;
            }
            let cell_rect = StickCell { x, y, size: cell };
            if d <= radius * 0.94 {
                disc.push(cell_rect);
            }
            if d >= radius * 0.86 {
                ring.push(cell_rect);
            }
            if (mx - thumb_centre.0).hypot(my - thumb_centre.1) <= thumb {
                thumb_cells.push(cell_rect);
            }
        }
    }
    let mut cells = Vec::with_capacity(disc.len() + ring.len() + thumb_cells.len());
    cells.extend(disc.into_iter().map(|c| (StickLayer::Disc, c)));
    cells.extend(ring.into_iter().map(|c| (StickLayer::Ring, c)));
    cells.extend(thumb_cells.into_iter().map(|c| (StickLayer::Thumb, c)));
    cells
}

/// Vanilla's button sprite, nine-sliced to the touch button's size: highlighted while held, and see-through
/// enough to keep the world behind it in sight.
pub fn draw_button(batch: &mut HudBatch, atlas: &HudAtlas, r: Rect, held: bool, opacity: f32) {
    let sprite = if held {
        "gui/sprites/widget/button_highlighted"
    } else {
        "gui/sprites/widget/button"
    };
    let alpha = (if held { 230.0 } else { 170.0 }) * opacity.clamp(0.0, 1.0);
    crate::ui::nine_slice(
        batch,
        atlas,
        sprite,
        r[0],
        r[1],
        r[2],
        r[3],
        rgba(255, 255, 255, alpha as u8),
    );
}

/// The size of a mark's cell on a button `r`: two GUI pixels on the old 26-pixel buttons, growing with them.
pub fn cell_size(r: Rect) -> f32 {
    (r[2] / 13.0).round().max(2.0)
}

/// Draw a mark twice, as Minecraft draws text: a dark copy a little down and right, then the white one.
pub fn shadowed(r: Rect, mut mark: impl FnMut(Rect, u32)) {
    let off = (cell_size(r) / 2.0).max(1.0);
    mark([r[0] + off, r[1] + off, r[2], r[3]], rgba(63, 63, 63, 230));
    mark(r, rgba(255, 255, 255, 240));
}

/// A button's mark, in cells of a few pixels centred on it.
fn glyph(batch: &mut HudBatch, atlas: &HudAtlas, control: Control, r: Rect, color: u32) {
    let (cx, cy) = ((r[0] + r[2] / 2.0).floor(), (r[1] + r[3] / 2.0).floor());
    let u = cell_size(r);
    let cell = |batch: &mut HudBatch, x: f32, y: f32, w: f32, h: f32| {
        batch.fill(atlas, cx + x * u, cy + y * u, w * u, h * u, color);
    };
    // A triangle pointing up, down, left or right: rows (or columns) narrowing to its tip.
    let arrow = |batch: &mut HudBatch, dir: (f32, f32)| {
        for i in 0..4 {
            let i = i as f32;
            let len = 7.0 - 2.0 * i;
            let at = 1.5 - i;
            match dir {
                (0.0, d) => cell(batch, -len / 2.0, at * -d - 0.5, len, 1.0),
                (d, _) => cell(batch, at * -d - 0.5, -len / 2.0, 1.0, len),
            }
        }
    };
    match control {
        Control::Forward => arrow(batch, (0.0, -1.0)),
        Control::Back => arrow(batch, (0.0, 1.0)),
        Control::Left => arrow(batch, (-1.0, 0.0)),
        Control::Right => arrow(batch, (1.0, 0.0)),
        // Sneak: a hollow diamond; jump: a filled one.
        Control::Sneak | Control::Jump => {
            for i in 0..4 {
                let i = i as f32;
                let len = 7.0 - 2.0 * i;
                for y in [-i - 0.5, i - 0.5] {
                    if control == Control::Jump {
                        cell(batch, -len / 2.0, y, len, 1.0);
                    } else {
                        cell(batch, -len / 2.0, y, 1.0, 1.0);
                        cell(batch, len / 2.0 - 1.0, y, 1.0, 1.0);
                    }
                }
            }
        }
        // Perspective: an eye.
        Control::Perspective => {
            cell(batch, -2.5, -1.5, 5.0, 1.0);
            cell(batch, -3.5, -0.5, 1.0, 1.0);
            cell(batch, -0.5, -0.5, 1.0, 1.0);
            cell(batch, 2.5, -0.5, 1.0, 1.0);
            cell(batch, -2.5, 0.5, 5.0, 1.0);
        }
        // Chat: a speech bubble with its tail.
        Control::Chat => {
            cell(batch, -3.0, -2.5, 6.0, 4.0);
            cell(batch, -2.0, 1.5, 1.0, 1.0);
        }
        // Pause: two bars.
        Control::Pause => {
            cell(batch, -2.0, -2.5, 1.5, 5.0);
            cell(batch, 0.5, -2.5, 1.5, 5.0);
        }
        // Hide HUD: a frame with a slash through it.
        Control::HideHud => {
            cell(batch, -3.5, -2.5, 7.0, 1.0);
            cell(batch, -3.5, 1.5, 7.0, 1.0);
            cell(batch, -3.5, -2.5, 1.0, 5.0);
            cell(batch, 2.5, -2.5, 1.0, 5.0);
            for i in 0..5 {
                let i = i as f32;
                cell(batch, -2.5 + i, 2.0 - i, 1.0, 1.0);
            }
        }
        // The inventory: three dots, as Pocket Edition's button beside the hotbar.
        Control::Inventory => {
            for x in [-3.0, -0.5, 2.0] {
                cell(batch, x, -0.5, 1.0, 1.0);
            }
        }
        Control::Hotbar(_) | Control::Custom(_) | Control::Stick => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 180-pixel stick on screen at x = 40, y = 30: centre (130, 120), cell 10, edges at whole 10s.
    const STICK: Rect = [40.0, 30.0, 180.0, 180.0];

    fn centre(c: &StickCell) -> (f32, f32) {
        (c.x + c.size / 2.0, c.y + c.size / 2.0)
    }

    fn cells(r: Rect, analog: [f32; 2], layer: StickLayer) -> Vec<StickCell> {
        stick_cells(r, analog)
            .into_iter()
            .filter(|(l, _)| *l == layer)
            .map(|(_, c)| c)
            .collect()
    }

    #[test]
    fn stick_is_symmetric_left_right_and_top_bottom() {
        let (cx, cy) = (130.0, 120.0);
        let grid = stick_cells(STICK, [0.0, 0.0]);
        let inside = |x: f32, y: f32, c: &[(StickLayer, StickCell)]| {
            c.iter().any(|(_, k)| k.x == x && k.y == y)
        };
        for (layer, c) in &grid {
            // The cell mirrored across the centre's vertical and horizontal axes, one cell wide.
            let mirrored_x = 2.0 * cx - c.x - c.size;
            let mirrored_y = 2.0 * cy - c.y - c.size;
            assert!(
                inside(mirrored_x, c.y, &grid),
                "{layer:?} cell at ({}, {}) has no left/right mirror",
                c.x,
                c.y
            );
            assert!(
                inside(c.x, mirrored_y, &grid),
                "{layer:?} cell at ({}, {}) has no top/bottom mirror",
                c.x,
                c.y
            );
        }
        assert!(!grid.is_empty(), "the stick drew nothing");
    }

    #[test]
    fn stick_cells_stay_inside_the_outer_circle() {
        let (cx, cy) = (130.0, 120.0);
        let radius = 90.0;
        for (layer, c) in stick_cells(STICK, [0.7, -0.4]) {
            let (mx, my) = centre(&c);
            let d = (mx - cx).hypot(my - cy);
            assert!(d <= radius, "{layer:?} cell at ({mx}, {my}) is {d} out, past {radius}");
        }
        // The ring is the silhouette: it must be there, in its band, or the stick reads as a blob.
        let ring = cells(STICK, [0.0, 0.0], StickLayer::Ring);
        assert!(!ring.is_empty(), "no ring cells");
        for c in &ring {
            let (mx, my) = centre(c);
            let d = (mx - cx).hypot(my - cy);
            assert!(d >= radius * 0.86, "ring cell at ({mx}, {my}) is only {d} out");
        }
    }

    #[test]
    fn stick_is_empty_when_radius_is_tiny() {
        for r in [[0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 1.0], [5.0, 5.0, 2.0, 2.0]] {
            let all = stick_cells(r, [0.0, 0.0]);
            assert!(all.is_empty(), "{r:?} drew {} cells", all.len());
            assert!(cells(r, [1.0, -1.0], StickLayer::Disc).is_empty());
        }
    }

    #[test]
    fn stick_cells_are_whole_pixels_and_the_thumb_follows_the_finger() {
        let all = stick_cells(STICK, [0.3, -0.8]);
        assert!(!all.is_empty());
        for (layer, c) in &all {
            assert_eq!(c.size, 10.0, "{layer:?} cell has the wrong size");
            assert_eq!(c.x.fract(), 0.0, "{layer:?} cell x {} is not a whole pixel", c.x);
            assert_eq!(c.y.fract(), 0.0, "{layer:?} cell y {} is not a whole pixel", c.y);
            // Every edge sits on the grid the centre defines, not half a cell off it.
            assert_eq!((c.x - 30.0) % c.size, 0.0);
            assert_eq!((c.y - 20.0) % c.size, 0.0);
        }
        // The bug this guards: snapping the thumb's centre to cell corners made it hop a whole cell off
        // the finger whenever the touch jittered across a boundary. A move just past half a cell must
        // shift the thumb by about that move, not by a cell.
        let centroid = |analog: [f32; 2]| {
            let thumb = cells(STICK, analog, StickLayer::Thumb);
            let n = thumb.len() as f32;
            let sum = thumb.iter().fold((0.0, 0.0), |(x, y), c| {
                (x + c.x + c.size / 2.0, y + c.y + c.size / 2.0)
            });
            (sum.0 / n, sum.1 / n)
        };
        let rest = centroid([0.0, 0.0]);
        let moved = centroid([0.0, 0.09]);
        let dy = (moved.1 - rest.1).abs();
        assert!(dy > 2.0, "a small move left the thumb behind ({dy} px)");
        assert!(dy < 9.0, "a small move popped the thumb {dy} px, a whole cell");
        let pushed = centroid([0.0, 1.0]);
        assert!(
            (pushed.1 - rest.1).abs() > 40.0,
            "a full push must move the thumb to the ring"
        );
    }
}
