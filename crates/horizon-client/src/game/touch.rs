//! Touch controls (iOS): touches become the keys, mouse buttons and view
//! turns of a keyboard and mouse. In game, a stick in the lower left walks
//! (the controller's analog move vector), a drag elsewhere turns the view,
//! a tap uses and a long press mines, and buttons stand for the rest
//! (horizon_client::touch lays them out). In menus one finger is the mouse,
//! two scroll, and the on-screen keyboard (osk.rs) types.
use super::event_loop::EventLoopCtl;
use super::{Game, Screen};
use horizon_client::{
    hud::HudBatch,
    osk,
    touch::{self, Button, Overlay},
};
use std::time::{Duration, Instant};
use winit::{
    dpi::PhysicalPosition,
    event::{
        DeviceId, ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent,
    },
    keyboard::KeyCode,
    window::WindowId,
};

/// Touch controls are on where a touch screen is the input.
pub(super) const ENABLED: bool = cfg!(target_os = "ios");

/// A tap is shorter than this and moves less than `TAP_SLOP`.
const TAP_TIME: Duration = Duration::from_millis(250);
/// A press held this long without moving mines (holds the attack button).
const HOLD_TIME: Duration = Duration::from_millis(400);
/// dp a finger may move and still tap or hold.
const TAP_SLOP: f32 = 12.0;
/// The view turn per dp of drag, in mouse counts.
const LOOK_SPEED: f64 = 1.6;

/// What a finger is doing.
#[derive(Clone, Copy, Debug)]
enum Finger {
    /// Holds the move stick, centred where it went down.
    Stick { centre: (f32, f32), at: (f32, f32) },
    /// Turns the view; may become a tap (use) or a hold (mine).
    Look {
        down: (f32, f32),
        at: (f32, f32),
        since: Instant,
        moved: bool,
        mining: bool,
    },
    /// Holds a button.
    Button(Button),
    /// The pointer in a menu.
    Pointer,
    /// A second finger in a menu: scrolls.
    Scroll { at: (f32, f32) },
    /// Ignored until it lifts (a hotbar or keyboard tap).
    Done,
}

#[derive(Default)]
pub(super) struct TouchState {
    fingers: Vec<(u64, Finger)>,
    /// The sneak toggle.
    sneaking: bool,
    /// A touch has come: the overlay shows from then on.
    used: bool,
    /// The keyboard was opened for this text field already (it opens once per field).
    typed_screen: Option<Screen>,
}

impl Game<'_> {
    /// The window's size in physical pixels and the dp scale.
    fn touch_metrics(&self) -> Option<([u32; 2], f32)> {
        let r = self.renderer.as_ref()?;
        let size = [r.config.width, r.config.height];
        Some((size, touch::density(size[0] as f32, size[1] as f32)))
    }

    fn touch_playing(&self) -> bool {
        self.title.is_none() && self.player.is_some() && !self.pad_in_menu() && !self.dialog_open()
    }

    /// The analog move vector from the stick (`[left, forward]`), as the
    /// controller's left stick gives it.
    pub(super) fn touch_move(&self) -> Option<[f32; 2]> {
        if !ENABLED || !self.touch_playing() {
            return None;
        }
        self.touch.fingers.iter().find_map(|(_, f)| match *f {
            Finger::Stick { centre, at } => horizon_client::gamepad::move_vector(
                [
                    (at.0 - centre.0) / touch::STICK_RADIUS,
                    (centre.1 - at.1) / touch::STICK_RADIUS,
                ],
                0.15,
            ),
            _ => None,
        })
    }

    /// A touch on the window.
    pub(super) fn touch_event(&mut self, event_loop: &dyn EventLoopCtl, id: WindowId, t: Touch) {
        let Some((size, density)) = self.touch_metrics() else {
            return;
        };
        self.touch.used = true;
        self.last_input = Instant::now();
        let (w, h) = (size[0] as f32 / density, size[1] as f32 / density);
        let p = (t.location.x as f32 / density, t.location.y as f32 / density);
        let playing = self.touch_playing();
        match t.phase {
            TouchPhase::Started => {
                let finger = self.touch_start(event_loop, id, (w, h), p, playing, density);
                self.touch.fingers.push((t.id, finger));
            }
            TouchPhase::Moved => {
                let Some(i) = self.touch.fingers.iter().position(|(f, _)| *f == t.id) else {
                    return;
                };
                let finger = self.touch.fingers[i].1;
                let finger = self.touch_move_finger(event_loop, id, finger, p, density);
                self.touch.fingers[i].1 = finger;
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some(i) = self.touch.fingers.iter().position(|(f, _)| *f == t.id) else {
                    return;
                };
                let (_, finger) = self.touch.fingers.remove(i);
                self.touch_end(
                    event_loop,
                    id,
                    finger,
                    p,
                    density,
                    t.phase == TouchPhase::Ended,
                );
            }
        }
    }

    fn touch_start(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: WindowId,
        (w, h): (f32, f32),
        p: (f32, f32),
        playing: bool,
        density: f32,
    ) -> Finger {
        // The on-screen keyboard takes the touches on it.
        if self.pad.menu.osk.is_some() {
            let at_top = self.osk_at_top(self.gui_size().1);
            if osk::Keyboard::covers(w, h, at_top, p) {
                let out = self
                    .pad
                    .menu
                    .osk
                    .as_mut()
                    .and_then(|k| k.tap(w, h, at_top, p));
                if let Some(out) = out {
                    self.pad_osk_output(out);
                }
                return Finger::Done;
            }
        }
        if let Some(button) = touch::button_at(w, h, playing, p) {
            self.touch_button(button, true);
            return Finger::Button(button);
        }
        if !playing {
            if self
                .touch
                .fingers
                .iter()
                .any(|(_, f)| matches!(f, Finger::Pointer))
            {
                return Finger::Scroll { at: p };
            }
            self.touch_pointer(event_loop, id, p, density, Some(ElementState::Pressed));
            return Finger::Pointer;
        }
        if let Some(slot) = self.touch_hotbar_slot(p, density) {
            self.selected = slot;
            return Finger::Done;
        }
        if touch::in_stick_zone(w, h, p) {
            return Finger::Stick { centre: p, at: p };
        }
        Finger::Look {
            down: p,
            at: p,
            since: Instant::now(),
            moved: false,
            mining: false,
        }
    }

    fn touch_move_finger(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: WindowId,
        finger: Finger,
        p: (f32, f32),
        density: f32,
    ) -> Finger {
        match finger {
            Finger::Stick { centre, .. } => {
                // The knob stays within the ring; the ring follows a finger that goes past it.
                let (dx, dy) = (p.0 - centre.0, p.1 - centre.1);
                let len = (dx * dx + dy * dy).sqrt();
                let centre = if len > touch::STICK_RADIUS {
                    let k = (len - touch::STICK_RADIUS) / len;
                    (centre.0 + dx * k, centre.1 + dy * k)
                } else {
                    centre
                };
                // Pushed to the edge: sprint, as a double tap forward would.
                let full = len >= touch::STICK_RADIUS * 0.95 && dy < 0.0;
                if full {
                    self.keys.insert(KeyCode::ControlLeft);
                } else {
                    self.keys.remove(&KeyCode::ControlLeft);
                }
                Finger::Stick { centre, at: p }
            }
            Finger::Look {
                down,
                at,
                since,
                moved,
                mining,
            } => {
                let moved = moved || dist(down, p) > TAP_SLOP;
                if self.screen == Screen::Playing && moved {
                    let (dx, dy) = (
                        f64::from(p.0 - at.0) * LOOK_SPEED,
                        f64::from(p.1 - at.1) * LOOK_SPEED,
                    );
                    // MouseHandler.onMove with the pointer grabbed; applied once per frame.
                    self.mouse.on_move(0.0, 0.0, dx, dy, true, true);
                }
                Finger::Look {
                    down,
                    at: p,
                    since,
                    moved,
                    mining,
                }
            }
            Finger::Pointer => {
                self.touch_pointer(event_loop, id, p, density, None);
                Finger::Pointer
            }
            Finger::Scroll { at } => {
                let steps = (p.1 - at.1) / 24.0;
                if steps.abs() < 1.0 {
                    return finger;
                }
                self.handle_window_event(
                    event_loop,
                    id,
                    WindowEvent::MouseWheel {
                        device_id: DeviceId::dummy(),
                        delta: MouseScrollDelta::LineDelta(0.0, steps.trunc()),
                        phase: TouchPhase::Moved,
                    },
                );
                Finger::Scroll { at: p }
            }
            other => other,
        }
    }

    fn touch_end(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: WindowId,
        finger: Finger,
        p: (f32, f32),
        density: f32,
        ended: bool,
    ) {
        match finger {
            Finger::Stick { .. } => {
                self.keys.remove(&KeyCode::ControlLeft);
            }
            Finger::Look {
                since,
                moved,
                mining,
                ..
            } => {
                if mining {
                    let _ = self.play_mouse(MouseButton::Left, false);
                } else if ended
                    && !moved
                    && since.elapsed() < TAP_TIME
                    && self.screen == Screen::Playing
                {
                    // A tap: use, as a right click (place, open, eat a bite).
                    let _ = self.play_mouse(MouseButton::Right, true);
                    let _ = self.play_mouse(MouseButton::Right, false);
                }
            }
            Finger::Button(button) => self.touch_button(button, false),
            Finger::Pointer => {
                self.touch_pointer(event_loop, id, p, density, Some(ElementState::Released));
            }
            Finger::Scroll { .. } | Finger::Done => {}
        }
    }

    /// Once a frame: a look finger held still long enough starts mining; the
    /// keyboard opens over a text field that took the focus.
    pub(super) fn touch_frame(&mut self) {
        if !ENABLED {
            return;
        }
        let playing = self.touch_playing();
        let mut start_mining = false;
        for (_, f) in &mut self.touch.fingers {
            if let Finger::Look {
                since,
                moved: false,
                mining,
                ..
            } = f
                && !*mining
                && playing
                && since.elapsed() >= HOLD_TIME
            {
                *mining = true;
                start_mining = true;
            }
        }
        if start_mining {
            let _ = self.play_mouse(MouseButton::Left, true);
        }
        if !playing {
            // Leaving the game lets go of everything it held.
            self.touch_release_game();
        }
        if self.touch.used && self.pad_typing() {
            if self.touch.typed_screen != Some(self.screen) && self.pad.menu.osk.is_none() {
                self.pad.menu.osk = Some(osk::Keyboard::default());
            }
            self.touch.typed_screen = Some(self.screen);
        } else {
            self.touch.typed_screen = None;
        }
    }

    fn touch_release_game(&mut self) {
        let (mut left, mut right) = (false, false);
        for (_, f) in &mut self.touch.fingers {
            match *f {
                Finger::Stick { .. } | Finger::Look { mining: false, .. } => *f = Finger::Done,
                Finger::Look { mining: true, .. } | Finger::Button(Button::Attack) => {
                    left = true;
                    *f = Finger::Done;
                }
                Finger::Button(Button::Use) => {
                    right = true;
                    *f = Finger::Done;
                }
                Finger::Button(Button::Jump) => {
                    self.keys.remove(&KeyCode::Space);
                    *f = Finger::Done;
                }
                _ => {}
            }
        }
        self.keys.remove(&KeyCode::ControlLeft);
        if left {
            let _ = self.play_mouse(MouseButton::Left, false);
        }
        if right {
            let _ = self.play_mouse(MouseButton::Right, false);
        }
    }

    fn touch_button(&mut self, button: Button, down: bool) {
        let result = match (button, down) {
            (Button::Jump, true) => {
                self.keys.insert(KeyCode::Space);
                Ok(())
            }
            (Button::Jump, false) => {
                self.keys.remove(&KeyCode::Space);
                Ok(())
            }
            (Button::Sneak, true) => {
                self.touch.sneaking = !self.touch.sneaking;
                if self.touch.sneaking {
                    self.keys.insert(KeyCode::ShiftLeft);
                } else {
                    self.keys.remove(&KeyCode::ShiftLeft);
                }
                Ok(())
            }
            (Button::Attack, _) => self.play_mouse(MouseButton::Left, down),
            (Button::Use, _) => self.play_mouse(MouseButton::Right, down),
            (_, false) => Ok(()),
            (Button::Inventory, true) => self.key_pressed(KeyCode::KeyE),
            (Button::Drop, true) => self.key_pressed(KeyCode::KeyQ),
            (Button::Chat, true) => self.key_pressed(KeyCode::KeyT),
            (Button::Perspective, true) => self.key_pressed(KeyCode::F5),
            (Button::Back, true) => {
                if self.pad.menu.osk.take().is_some() {
                    Ok(())
                } else {
                    self.pad_back()
                }
            }
            (Button::Keyboard, true) => {
                self.pad.menu.osk = match self.pad.menu.osk {
                    Some(_) => None,
                    None => Some(osk::Keyboard::default()),
                };
                Ok(())
            }
        };
        if let Err(e) = result {
            log::warn!("Horizon: touch {button:?}: {e}");
        }
    }

    /// The pointer moves to `p` (dp) and, with `press`, the left button goes down or up there.
    fn touch_pointer(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        id: WindowId,
        p: (f32, f32),
        density: f32,
        press: Option<ElementState>,
    ) {
        let device_id = DeviceId::dummy();
        let position = PhysicalPosition::new(f64::from(p.0 * density), f64::from(p.1 * density));
        self.handle_window_event(
            event_loop,
            id,
            WindowEvent::CursorMoved {
                device_id,
                position,
            },
        );
        if let Some(state) = press {
            self.handle_window_event(
                event_loop,
                id,
                WindowEvent::MouseInput {
                    device_id,
                    state,
                    button: MouseButton::Left,
                },
            );
        }
    }

    /// The hotbar slot under `p` (dp), as `Gui.renderItemHotbar` places it.
    fn touch_hotbar_slot(&self, p: (f32, f32), density: f32) -> Option<usize> {
        let (size, _) = self.touch_metrics()?;
        let b = HudBatch::new(size);
        let (gx, gy) = (p.0 * density / b.scale, p.1 * density / b.scale);
        let left = b.width / 2.0 - 91.0;
        if gy < b.height - 22.0 || gx < left || gx >= left + 182.0 {
            return None;
        }
        Some((((gx - left) / 20.0) as usize).min(8))
    }

    /// The buttons and the stick, over everything but the on-screen keyboard.
    pub(super) fn draw_touch(&self, batch: &mut HudBatch) {
        if !ENABLED || !self.touch.used {
            return;
        }
        let mut overlay = Overlay {
            playing: self.touch_playing(),
            sneak: self.touch.sneaking,
            jump: self.keys.contains(&KeyCode::Space),
            ..Overlay::default()
        };
        for (_, f) in &self.touch.fingers {
            match *f {
                Finger::Stick { centre, at } => {
                    let (dx, dy) = (at.0 - centre.0, at.1 - centre.1);
                    let len = (dx * dx + dy * dy).sqrt().max(touch::STICK_RADIUS);
                    let k = touch::STICK_RADIUS / len;
                    overlay.stick = Some((centre, (centre.0 + dx * k, centre.1 + dy * k)));
                }
                Finger::Look { mining: true, .. } | Finger::Button(Button::Attack) => {
                    overlay.attack = true;
                }
                Finger::Button(Button::Use) => overlay.using = true,
                _ => {}
            }
        }
        let mut ui = horizon_client::ui_theme::Ui::new(&self.gui.atlas, batch, self.cursor);
        touch::draw(&mut ui, &overlay);
        ui.finish();
    }
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}
