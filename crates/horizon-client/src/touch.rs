//! Touch controls (iOS): where the on-screen buttons and the move stick are
//! and how they look. The game (game/touch.rs) turns touches into the keys,
//! mouse buttons and view turns a keyboard and mouse would give; this module
//! only lays them out and draws them, in the design system's dp.
use crate::ui_theme::{self, Face, Ui, color};

/// An on-screen button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    /// Held: `key.jump` (Space).
    Jump,
    /// Toggles `key.sneak` (Shift).
    Sneak,
    /// Held: `key.attack` (left button): mine, hit.
    Attack,
    /// Held: `key.use` (right button): place, eat, open.
    Use,
    /// `key.inventory` (E).
    Inventory,
    /// `key.drop` (Q).
    Drop,
    /// `key.chat` (T).
    Chat,
    /// `key.togglePerspective` (F5).
    Perspective,
    /// Escape: the pause menu while playing, back in menus.
    Back,
    /// Opens or closes the on-screen keyboard in menus.
    Keyboard,
}

impl Button {
    pub fn label(self) -> &'static str {
        match self {
            Button::Jump => "Jump",
            Button::Sneak => "Sneak",
            Button::Attack => "Hit",
            Button::Use => "Use",
            Button::Inventory => "Inv",
            Button::Drop => "Drop",
            Button::Chat => "Chat",
            Button::Perspective => "View",
            Button::Back => "Back",
            Button::Keyboard => "Keys",
        }
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
}

/// Physical pixels per dp for a screen of `pw` x `ph` pixels: `Ui::new`'s.
pub fn density(pw: f32, ph: f32) -> f32 {
    (pw / 1040.0).min(ph / 700.0).clamp(1.0, 4.0)
}

const MARGIN: f32 = 20.0;
const BIG: f32 = 76.0;
const SMALL: f32 = 52.0;
const GAP: f32 = 12.0;
/// The move stick's travel in dp.
pub const STICK_RADIUS: f32 = 60.0;

/// The buttons over a `w` x `h` dp screen: in game (`playing`) or in a menu.
pub fn buttons(w: f32, h: f32, playing: bool) -> Vec<(Button, Rect)> {
    let top = |i: f32| Rect {
        x: w - MARGIN - SMALL - i * (SMALL + GAP),
        y: MARGIN,
        w: SMALL,
        h: SMALL,
    };
    if !playing {
        return vec![(Button::Back, top(0.0)), (Button::Keyboard, top(1.0))];
    }
    let big = |col: f32, row: f32| Rect {
        x: w - MARGIN - BIG - col * (BIG + GAP),
        y: h - MARGIN - BIG - row * (BIG + GAP),
        w: BIG,
        h: BIG,
    };
    vec![
        (Button::Back, top(0.0)),
        (Button::Chat, top(1.0)),
        (Button::Perspective, top(2.0)),
        (Button::Inventory, top(3.0)),
        (Button::Drop, top(4.0)),
        (Button::Jump, big(0.0, 0.0)),
        (Button::Sneak, big(1.0, 0.0)),
        (Button::Attack, big(0.0, 1.0)),
        (Button::Use, big(1.0, 1.0)),
    ]
}

/// The button under `point` (dp).
pub fn button_at(w: f32, h: f32, playing: bool, point: (f32, f32)) -> Option<Button> {
    buttons(w, h, playing)
        .into_iter()
        .find(|(_, r)| r.contains(point))
        .map(|(b, _)| b)
}

/// Where a touch starts the move stick: the lower left, away from the top
/// row and the hotbar's middle.
pub fn in_stick_zone(w: f32, h: f32, (x, y): (f32, f32)) -> bool {
    x < w * 0.4 && y > h * 0.3
}

/// What the overlay shows this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Overlay {
    pub playing: bool,
    /// The stick's centre and its knob, in dp, while a finger holds it.
    pub stick: Option<((f32, f32), (f32, f32))>,
    /// Lit buttons: held, or a toggle that is on.
    pub jump: bool,
    pub sneak: bool,
    pub attack: bool,
    pub using: bool,
}

/// Draw the buttons and the stick, translucent so the world shows through.
pub fn draw(ui: &mut Ui, overlay: &Overlay) {
    let lit = |b: Button| match b {
        Button::Jump => overlay.jump,
        Button::Sneak => overlay.sneak,
        Button::Attack => overlay.attack,
        Button::Use => overlay.using,
        _ => false,
    };
    for (button, r) in buttons(ui.w, ui.h, overlay.playing) {
        let (plate, ink) = if lit(button) {
            (color::with_alpha(color::ACCENT, 0.8), color::ON_ACCENT)
        } else {
            (color::with_alpha(color::BG, 0.45), color::TEXT)
        };
        ui.rounded(r.x, r.y, r.w, r.h, r.w / 2.0, plate);
        ui.border(
            r.x,
            r.y,
            r.w,
            r.h,
            r.w / 2.0,
            color::with_alpha(color::BORDER, 0.8),
        );
        let size = if r.w >= BIG {
            ui_theme::BODY
        } else {
            ui_theme::SMALL
        };
        let line = (size * 1.45).round();
        ui.text_centered(
            Face::Medium,
            size,
            r.x + r.w / 2.0,
            r.y + ((r.h - line) / 2.0).round(),
            ink,
            button.label(),
        );
    }
    if let Some(((cx, cy), (kx, ky))) = overlay.stick {
        let d = STICK_RADIUS * 2.0;
        ui.rounded(
            cx - STICK_RADIUS,
            cy - STICK_RADIUS,
            d,
            d,
            STICK_RADIUS,
            color::with_alpha(color::BG, 0.35),
        );
        ui.border(
            cx - STICK_RADIUS,
            cy - STICK_RADIUS,
            d,
            d,
            STICK_RADIUS,
            color::with_alpha(color::BORDER, 0.8),
        );
        let k = 26.0;
        ui.rounded(
            kx - k,
            ky - k,
            k * 2.0,
            k * 2.0,
            k,
            color::with_alpha(color::TEXT, 0.6),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_do_not_overlap_and_stay_on_screen() {
        // An iPhone in landscape, and an iPad.
        for (pw, ph) in [(2532.0, 1170.0), (2732.0, 2048.0)] {
            let d = density(pw, ph);
            let (w, h) = (pw / d, ph / d);
            for playing in [true, false] {
                let all = buttons(w, h, playing);
                for (i, (_, a)) in all.iter().enumerate() {
                    assert!(a.x >= 0.0 && a.y >= 0.0 && a.x + a.w <= w && a.y + a.h <= h);
                    for (_, b) in &all[i + 1..] {
                        let apart = a.x + a.w <= b.x
                            || b.x + b.w <= a.x
                            || a.y + a.h <= b.y
                            || b.y + b.h <= a.y;
                        assert!(apart, "{a:?} {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_stick_zone_misses_the_buttons() {
        let (w, h) = (1516.0, 700.0);
        for (_, r) in buttons(w, h, true) {
            assert!(!in_stick_zone(w, h, (r.x, r.y + r.h - 1.0)), "{r:?}");
        }
        assert!(in_stick_zone(w, h, (100.0, 600.0)));
    }
}
