//! Touch controls (iOS), laid out as Bedrock Edition's: a D-pad in the lower
//! left with sneak in its middle, jump in the lower right, the inventory's
//! button beside the hotbar and pause, chat and the camera at the top. The
//! game (game/touch.rs) turns touches into the keys, mouse buttons and view
//! turns a keyboard and mouse would give; this module only lays the buttons
//! out and draws them, in the design system's dp.
use crate::ui_theme::{self, Face, Ui};
use std::path::Path;

/// An on-screen button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    /// Held: `key.jump` (Space).
    Jump,
    /// Toggles `key.sneak` (Shift): the D-pad's middle.
    Sneak,
    /// Toggles `key.sprint` (Control).
    Sprint,
    /// Held: `key.attack` (left button): mine, hit.
    Attack,
    /// Held: `key.use` (right button): place, eat, open.
    Use,
    /// `key.inventory` (E): beside the hotbar.
    Inventory,
    /// `key.chat` (T).
    Chat,
    /// `key.togglePerspective` (F5).
    Perspective,
    /// Escape: the pause menu.
    Pause,
    /// Shows or hides the frame counter.
    Fps,
    /// Escape in menus: back.
    Back,
    /// Opens or closes the on-screen keyboard in menus.
    Keyboard,
}

/// A direction of the D-pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    UpLeft,
    UpRight,
    Left,
    Right,
    Down,
}

impl Dir {
    /// The move vector, `[left, forward]` as a controller's left stick gives it.
    pub fn vector(self) -> [f32; 2] {
        const D: f32 = std::f32::consts::FRAC_1_SQRT_2;
        match self {
            Dir::Up => [0.0, 1.0],
            Dir::UpLeft => [D, D],
            Dir::UpRight => [-D, D],
            Dir::Left => [1.0, 0.0],
            Dir::Right => [-1.0, 0.0],
            Dir::Down => [0.0, -1.0],
        }
    }
    pub fn forward(self) -> bool {
        matches!(self, Dir::Up | Dir::UpLeft | Dir::UpRight)
    }
}

/// A rectangle in dp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub fn contains(&self, (px, py): (f32, f32)) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    fn centre(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// Physical pixels per dp for a screen of `pw` x `ph` pixels: `Ui::new`'s.
pub fn density(pw: f32, ph: f32) -> f32 {
    (pw / 1040.0).min(ph / 700.0).clamp(1.0, 4.0)
}

const MARGIN: f32 = 24.0;
/// A D-pad cell.
const CELL: f32 = 62.0;
const BIG: f32 = 72.0;
const SMALL: f32 = 46.0;
const GAP: f32 = 8.0;

/// The screen's buttons for a `w` x `h` dp screen. `hotbar` is where the
/// hotbar is drawn, for the inventory button beside it.
pub struct Layout {
    pub w: f32,
    pub h: f32,
    pub hotbar: Rect,
}

impl Layout {
    /// The D-pad's cell at column `col` and row `row` of its 3 x 3 grid.
    fn cell(&self, col: f32, row: f32) -> Rect {
        let y0 = self.h - MARGIN - 3.0 * CELL - GAP;
        Rect {
            x: MARGIN + col * (CELL + GAP / 2.0),
            y: y0 + row * (CELL + GAP / 2.0),
            w: CELL,
            h: CELL,
        }
    }
    /// The D-pad's whole grid.
    pub fn pad(&self) -> Rect {
        let a = self.cell(0.0, 0.0);
        let b = self.cell(2.0, 2.0);
        Rect {
            x: a.x,
            y: a.y,
            w: b.x + b.w - a.x,
            h: b.y + b.h - a.y,
        }
    }
    /// The D-pad's arrows; the diagonals show while walking forward, as in Bedrock.
    pub fn arrows(&self) -> [(Dir, Rect); 6] {
        [
            (Dir::Up, self.cell(1.0, 0.0)),
            (Dir::UpLeft, self.cell(0.0, 0.0)),
            (Dir::UpRight, self.cell(2.0, 0.0)),
            (Dir::Left, self.cell(0.0, 1.0)),
            (Dir::Right, self.cell(2.0, 1.0)),
            (Dir::Down, self.cell(1.0, 2.0)),
        ]
    }
    /// The direction a finger at `p` on the D-pad walks: by its angle from the
    /// middle, so a finger that slides off the pad keeps walking. None in the middle.
    pub fn dir_at(&self, p: (f32, f32)) -> Option<Dir> {
        let (cx, cy) = self.pad().centre();
        let (dx, dy) = (p.0 - cx, p.1 - cy);
        if dx.abs() < CELL / 2.0 && dy.abs() < CELL / 2.0 {
            return None;
        }
        // Degrees clockwise from straight up.
        let a = dx.atan2(-dy).to_degrees();
        let left = a < 0.0;
        Some(match a.abs() {
            x if x <= 22.5 => Dir::Up,
            x if x <= 67.5 && left => Dir::UpLeft,
            x if x <= 67.5 => Dir::UpRight,
            x if x <= 135.0 && left => Dir::Left,
            x if x <= 135.0 => Dir::Right,
            _ => Dir::Down,
        })
    }
    /// Whether a touch at `p` takes the D-pad: on it, or just off its edge.
    pub fn on_pad(&self, p: (f32, f32)) -> bool {
        let r = self.pad();
        let slop = 16.0;
        Rect {
            x: r.x - slop,
            y: r.y - slop,
            w: r.w + 2.0 * slop,
            h: r.h + 2.0 * slop,
        }
        .contains(p)
    }

    /// The buttons, in game (`playing`) or in a menu.
    pub fn buttons(&self, playing: bool) -> Vec<(Button, Rect)> {
        let (w, h) = (self.w, self.h);
        let top = |i: f32| Rect {
            x: w - MARGIN - SMALL - i * (SMALL + GAP),
            y: MARGIN,
            w: SMALL,
            h: SMALL,
        };
        if !playing {
            return vec![(Button::Back, top(0.0)), (Button::Keyboard, top(1.0))];
        }
        // The lower right, from the corner: jump; attack over it, sprint over that;
        // use left of attack. The row beside the hotbar stays free for the inventory.
        let big = |col: f32, row: f32| Rect {
            x: w - MARGIN - BIG - col * (BIG + GAP),
            y: h - MARGIN - BIG - row * (BIG + GAP),
            w: BIG,
            h: BIG,
        };
        let bar = self.hotbar;
        vec![
            (Button::Pause, top(0.0)),
            (Button::Chat, top(1.0)),
            (Button::Perspective, top(2.0)),
            (Button::Fps, top(3.0)),
            (Button::Sneak, self.cell(1.0, 1.0)),
            (Button::Jump, big(0.0, 0.0)),
            (Button::Attack, big(0.0, 1.0)),
            (Button::Use, big(1.0, 1.0)),
            (Button::Sprint, big(0.0, 2.0)),
            (
                Button::Inventory,
                Rect {
                    x: bar.x + bar.w + GAP,
                    y: bar.y,
                    w: bar.h,
                    h: bar.h,
                },
            ),
        ]
    }

    /// The button under `point` (dp).
    pub fn button_at(&self, playing: bool, point: (f32, f32)) -> Option<Button> {
        self.buttons(playing)
            .into_iter()
            .find(|(_, r)| r.contains(point))
            .map(|(b, _)| b)
    }
}

/// Whether the frame counter shows: `touchFps` in horizon.txt in the game folder. Off by default.
pub fn fps_shown(dir: &Path) -> bool {
    ui_theme::preference(dir, "touchFps").as_deref() == Some("true")
}
pub fn set_fps_shown(dir: &Path, shown: bool) {
    ui_theme::set_preference(dir, "touchFps", shown.then_some("true"));
}

/// What the overlay shows this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Overlay {
    pub playing: bool,
    /// Where the D-pad walks.
    pub walking: Option<Dir>,
    /// Lit buttons: held, or a toggle that is on.
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
    pub attack: bool,
    pub using: bool,
    /// The frame counter, when it shows.
    pub fps: Option<u32>,
}

const PLATE: u32 = 0x59_00_00_00;
const PLATE_LIT: u32 = 0x73_ff_ff_ff;
const EDGE: u32 = 0x40_ff_ff_ff;
const INK: u32 = 0xeb_ff_ff_ff;

/// Draw the D-pad, the buttons and the frame counter, translucent so the world shows through.
pub fn draw(ui: &mut Ui, layout: &Layout, overlay: &Overlay) {
    if overlay.playing {
        for (dir, r) in layout.arrows() {
            let diagonal = matches!(dir, Dir::UpLeft | Dir::UpRight);
            if diagonal && !overlay.walking.is_some_and(Dir::forward) {
                continue;
            }
            plate(ui, r, overlay.walking == Some(dir));
            let turn = match dir {
                Dir::Up => 0.0,
                Dir::UpRight => 45.0,
                Dir::Right => 90.0,
                Dir::Down => 180.0,
                Dir::Left => 270.0,
                Dir::UpLeft => 315.0,
            };
            arrow(ui, r.centre(), r.w * 0.22, turn);
        }
    }
    for (button, r) in layout.buttons(overlay.playing) {
        let lit = match button {
            Button::Jump => overlay.jump,
            Button::Sneak => overlay.sneak,
            Button::Sprint => overlay.sprint,
            Button::Attack => overlay.attack,
            Button::Use => overlay.using,
            Button::Fps => overlay.fps.is_some(),
            _ => false,
        };
        plate(ui, r, lit);
        icon(ui, button, r);
    }
    if let Some(fps) = overlay.fps.filter(|_| overlay.playing) {
        let text = format!("{fps} FPS");
        let size = ui_theme::SMALL;
        let w = ui.measure(Face::Medium, size, &text) + 16.0;
        let (x, y) = (MARGIN, MARGIN);
        ui.rounded(x, y, w, size * 1.45 + 8.0, 6.0, PLATE);
        ui.text(Face::Medium, size, x + 8.0, y + 4.0, INK, &text);
    }
}

fn plate(ui: &mut Ui, r: Rect, lit: bool) {
    ui.rounded(r.x, r.y, r.w, r.h, 8.0, if lit { PLATE_LIT } else { PLATE });
    ui.border(r.x, r.y, r.w, r.h, 8.0, EDGE);
}

fn icon(ui: &mut Ui, button: Button, r: Rect) {
    let (cx, cy) = r.centre();
    let s = r.w * 0.22;
    match button {
        Button::Jump => {
            arrow(ui, (cx, cy - s * 0.25), s * 1.1, 0.0);
            bar(
                ui,
                (cx - s, cy + s * 0.75),
                (cx + s, cy + s * 0.75),
                s * 0.22,
            );
        }
        Button::Sneak => {
            arrow(ui, (cx, cy - s * 0.15), s * 0.9, 180.0);
            bar(
                ui,
                (cx - s, cy + s * 0.85),
                (cx + s, cy + s * 0.85),
                s * 0.22,
            );
        }
        Button::Sprint => {
            arrow(ui, (cx - s * 0.45, cy), s * 0.8, 90.0);
            arrow(ui, (cx + s * 0.55, cy), s * 0.8, 90.0);
        }
        Button::Attack => {
            // A sword: blade up and to the right, guard across it, grip below.
            let k = std::f32::consts::FRAC_1_SQRT_2;
            let along = |t: f32| (cx + t * s * k, cy - t * s * k);
            bar(ui, along(-0.3), along(1.3), s * 0.32);
            let g = along(-0.35);
            bar(
                ui,
                (g.0 - s * 0.55 * k, g.1 - s * 0.55 * k),
                (g.0 + s * 0.55 * k, g.1 + s * 0.55 * k),
                s * 0.24,
            );
            bar(ui, along(-0.4), along(-1.2), s * 0.24);
        }
        Button::Use => {
            // A block: an outlined square with its top face.
            let (x0, y0, x1, y1) = (cx - s, cy - s * 0.8, cx + s, cy + s * 1.0);
            let t = s * 0.2;
            bar(ui, (x0, y0), (x1, y0), t);
            bar(ui, (x0, y1), (x1, y1), t);
            bar(ui, (x0, y0), (x0, y1), t);
            bar(ui, (x1, y0), (x1, y1), t);
            bar(ui, (x0, y0 + s * 0.6), (x1, y0 + s * 0.6), t * 0.8);
        }
        Button::Inventory => {
            for i in [-1.0, 0.0, 1.0] {
                let (x, y) = (cx + i * s * 0.8, cy);
                fill(
                    ui,
                    [
                        (x - s * 0.18, y - s * 0.18),
                        (x + s * 0.18, y - s * 0.18),
                        (x + s * 0.18, y + s * 0.18),
                        (x - s * 0.18, y + s * 0.18),
                    ],
                );
            }
        }
        Button::Pause => {
            bar(ui, (cx - s * 0.4, cy - s), (cx - s * 0.4, cy + s), s * 0.38);
            bar(ui, (cx + s * 0.4, cy - s), (cx + s * 0.4, cy + s), s * 0.38);
        }
        Button::Chat => {
            // A speech bubble.
            let (x0, y0, x1, y1) = (cx - s * 1.1, cy - s * 0.8, cx + s * 1.1, cy + s * 0.5);
            fill(ui, [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]);
            fill(
                ui,
                [
                    (cx - s * 0.6, y1 - 1.0),
                    (cx - s * 0.05, y1 - 1.0),
                    (cx - s * 0.7, y1 + s * 0.6),
                    (cx - s * 0.7, y1 + s * 0.6),
                ],
            );
        }
        Button::Perspective => {
            // An eye.
            let t = s * 0.18;
            let (l, rr, u, d) = (
                (cx - s * 1.2, cy),
                (cx + s * 1.2, cy),
                (cx, cy - s * 0.7),
                (cx, cy + s * 0.7),
            );
            bar(ui, l, u, t);
            bar(ui, u, rr, t);
            bar(ui, rr, d, t);
            bar(ui, d, l, t);
            fill(
                ui,
                [
                    (cx - s * 0.3, cy - s * 0.3),
                    (cx + s * 0.3, cy - s * 0.3),
                    (cx + s * 0.3, cy + s * 0.3),
                    (cx - s * 0.3, cy + s * 0.3),
                ],
            );
        }
        Button::Back => {
            arrow(ui, (cx - s * 0.3, cy), s * 0.9, 270.0);
            bar(ui, (cx - s * 0.2, cy), (cx + s, cy), s * 0.3);
        }
        Button::Fps | Button::Keyboard => {
            let text = if button == Button::Fps { "FPS" } else { "abc" };
            let size = ui_theme::SMALL;
            let line = (size * 1.45).round();
            ui.text_centered(Face::Medium, size, cx, (cy - line / 2.0).round(), INK, text);
        }
    }
}

/// A filled four-cornered shape in dp.
fn fill(ui: &mut Ui, corners: [(f32, f32); 4]) {
    let k = ui.density / ui.batch.scale;
    let c = corners.map(|(x, y)| [x * k, y * k]);
    ui.batch.fill_quad(ui.atlas, c, INK);
}

/// A thick line from `a` to `b`, `width` dp across.
fn bar(ui: &mut Ui, a: (f32, f32), b: (f32, f32), width: f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let (nx, ny) = (-dy / len * width / 2.0, dx / len * width / 2.0);
    fill(
        ui,
        [
            (a.0 + nx, a.1 + ny),
            (b.0 + nx, b.1 + ny),
            (b.0 - nx, b.1 - ny),
            (a.0 - nx, a.1 - ny),
        ],
    );
}

/// A triangle pointing `turn` degrees clockwise from up, `size` dp from its middle to its tip.
fn arrow(ui: &mut Ui, (cx, cy): (f32, f32), size: f32, turn: f32) {
    let (sin, cos) = turn.to_radians().sin_cos();
    let at = |x: f32, y: f32| (cx + x * cos - y * sin, cy + x * sin + y * cos);
    let tip = at(0.0, -size);
    fill(
        ui,
        [
            tip,
            at(size, size * 0.6),
            at(-size, size * 0.6),
            at(-size, size * 0.6),
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An iPhone in landscape (its safe area) and an iPad, with their hotbars.
    fn layouts() -> Vec<Layout> {
        [(2250.0, 1143.0, 4.0), (2732.0, 2048.0, 8.0)]
            .into_iter()
            .map(|(pw, ph, gui): (f32, f32, f32)| {
                let d = density(pw, ph);
                let (w, h) = (pw / d, ph / d);
                let (bw, bh) = (182.0 * gui / d, 22.0 * gui / d);
                Layout {
                    w,
                    h,
                    hotbar: Rect {
                        x: (w - bw) / 2.0,
                        y: h - bh,
                        w: bw,
                        h: bh,
                    },
                }
            })
            .collect()
    }

    fn apart(a: &Rect, b: &Rect) -> bool {
        a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y
    }

    #[test]
    fn controls_do_not_overlap_and_stay_on_screen() {
        for l in layouts() {
            for playing in [true, false] {
                let mut all: Vec<Rect> = l.buttons(playing).into_iter().map(|(_, r)| r).collect();
                if playing {
                    all.extend(l.arrows().map(|(_, r)| r));
                    // The sneak button is the D-pad's middle, which no arrow covers.
                    for (_, r) in l.buttons(true) {
                        assert!(apart(&r, &l.hotbar), "{r:?} on the hotbar");
                    }
                }
                for (i, a) in all.iter().enumerate() {
                    assert!(a.x >= 0.0 && a.y >= 0.0 && a.x + a.w <= l.w && a.y + a.h <= l.h);
                    for b in &all[i + 1..] {
                        assert!(apart(a, b), "{a:?} {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_pad_walks_by_angle() {
        let l = &layouts()[0];
        for (dir, r) in l.arrows() {
            assert_eq!(l.dir_at(r.centre()), Some(dir));
        }
        let (cx, cy) = l.pad().centre();
        assert_eq!(l.dir_at((cx, cy)), None);
        // Off the pad, straight up still walks forward.
        assert_eq!(l.dir_at((cx, cy - 400.0)), Some(Dir::Up));
        assert_eq!(l.button_at(true, (cx, cy)), Some(Button::Sneak));
    }
}
