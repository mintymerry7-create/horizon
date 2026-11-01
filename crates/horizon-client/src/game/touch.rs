//! Touch controls (iOS), as Bedrock Edition's: touches become the keys, mouse
//! buttons and view turns of a keyboard and mouse. In game, the D-pad in the
//! lower left walks (a double tap forward sprints), a drag elsewhere turns the
//! view, a tap uses and a long press mines, a long press on a hotbar slot
//! drops its item, and buttons stand for the rest (horizon_client::touch lays
//! them out). In menus one finger is the mouse, two scroll, and the on-screen
//! keyboard (osk.rs) types.
use super::event_loop::EventLoopCtl;
use super::{Game, Screen};
use horizon_client::{
    hud::HudBatch,
    osk,
    touch::{self, Button, Dir, Layout, Overlay, Rect},
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
/// A second press forward this soon after the last one sprints.
const DOUBLE_TAP: Duration = Duration::from_millis(300);
/// A hotbar slot held this long drops its item.
const DROP_TIME: Duration = Duration::from_millis(500);

/// What a finger is doing.
#[derive(Clone, Copy, Debug)]
enum Finger {
    /// Holds the D-pad, walking towards `dir`.
    Pad { dir: Option<Dir> },
    /// Holds a hotbar slot; drops its item once held long enough.
    Hotbar { since: Instant, dropped: bool },
    /// A finger in a menu: a tap clicks, a drag up or down scrolls, a drag
    /// sideways drags the pointer (sliders).
    Menu {
        down: (f32, f32),
        at: (f32, f32),
        drag: MenuDrag,
    },
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
    /// Ignored until it lifts (a hotbar or keyboard tap).
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuDrag {
    /// Not moved past `TAP_SLOP` yet.
    Undecided,
    Scroll,
    Pointer,
}

/// dp of drag per scroll step in menus.
const SCROLL_STEP: f32 = 18.0;

#[derive(Default)]
pub(super) struct TouchState {
    fingers: Vec<(u64, Finger)>,
    /// The sneak and sprint toggles.
    sneaking: bool,
    sprinting: bool,
    /// Sprinting from a double tap forward, until the D-pad is let go.
    dashing: bool,
    /// When a press forward on the D-pad ended, for the double tap.
    forward_ended: Option<Instant>,
    /// A touch has come: the overlay shows from then on.
    used: bool,
    /// The keyboard was opened for this text field already (it opens once per field).
    typed_screen: Option<Screen>,
}

impl Game<'_> {
    /// The window's size in physical pixels and the dp scale: the surface's,
    /// which on iOS fills the screen as the touches do.
    fn touch_metrics(&self) -> Option<([u32; 2], f32)> {
        let r = self.renderer.as_ref()?;
        let size = [r.config.width, r.config.height];
        Some((size, touch::density(size[0] as f32, size[1] as f32)))
    }

    fn touch_playing(&self) -> bool {
        self.title.is_none() && self.player.is_some() && !self.pad_in_menu() && !self.dialog_open()
    }

    /// The buttons' layout on this window, with the hotbar where `Gui.renderItemHotbar` puts it.
    fn touch_layout(&self) -> Option<Layout> {
        let (size, density) = self.touch_metrics()?;
        let b = HudBatch::new(size);
        let k = b.scale / density;
        let (w, h) = (size[0] as f32 / density, size[1] as f32 / density);
        // The safe area, which winit gives as the inner size and position on iOS.
        let safe = self
            .window
            .as_ref()
            .and_then(|win| {
                let (inner, outer) = (win.inner_position().ok()?, win.outer_position().ok()?);
                let s = win.inner_size();
                Some(Rect {
                    x: (inner.x - outer.x) as f32 / density,
                    y: (inner.y - outer.y) as f32 / density,
                    w: s.width as f32 / density,
                    h: s.height as f32 / density,
                })
            })
            .filter(|r| r.w > 0.0 && r.h > 0.0 && r.x + r.w <= w + 1.0 && r.y + r.h <= h + 1.0)
            .unwrap_or(Rect {
                x: 0.0,
                y: 0.0,
                w,
                h,
            });
        Some(Layout {
            w,
            h,
            safe,
            hotbar: Rect {
                x: (b.width / 2.0 - 91.0) * k,
                y: (b.height - 22.0) * k,
                w: 182.0 * k,
                h: 22.0 * k,
            },
        })
    }

    /// The move vector from the D-pad (`[left, forward]`), as the controller's left stick gives it.
    pub(super) fn touch_move(&self) -> Option<[f32; 2]> {
        if !ENABLED || !self.touch_playing() {
            return None;
        }
        self.touch.fingers.iter().find_map(|(_, f)| match *f {
            Finger::Pad { dir } => dir.map(Dir::vector),
            _ => None,
        })
    }

    /// Hold the sprint key while either way of sprinting is on.
    fn touch_sprint_key(&mut self) {
        if self.touch.sprinting || self.touch.dashing {
            self.keys.insert(KeyCode::ControlLeft);
        } else {
            self.keys.remove(&KeyCode::ControlLeft);
        }
    }

    /// A touch on the window.
    pub(super) fn touch_event(&mut self, event_loop: &dyn EventLoopCtl, id: WindowId, t: Touch) {
        let (Some((_, density)), Some(layout)) = (self.touch_metrics(), self.touch_layout()) else {
            return;
        };
        self.touch.used = true;
        self.last_input = Instant::now();
        let p = (t.location.x as f32 / density, t.location.y as f32 / density);
        let playing = self.touch_playing();
        match t.phase {
            TouchPhase::Started => {
                let finger = self.touch_start(event_loop, id, &layout, p, playing, density);
                self.touch.fingers.push((t.id, finger));
            }
            TouchPhase::Moved => {
                let Some(i) = self.touch.fingers.iter().position(|(f, _)| *f == t.id) else {
                    return;
                };
                let finger = self.touch.fingers[i].1;
                let finger = self.touch_move_finger(event_loop, id, &layout, finger, p, density);
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
        layout: &Layout,
        p: (f32, f32),
        playing: bool,
        density: f32,
    ) -> Finger {
        let (w, h) = (layout.w, layout.h);
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
        if let Some(button) = layout.button_at(playing, p) {
            self.touch_button(button, true);
            return Finger::Button(button);
        }
        if !playing {
            // The pointer goes there, for what a scroll scrolls; the click waits for the lift.
            self.touch_pointer(event_loop, id, p, density, None);
            return Finger::Menu {
                down: p,
                at: p,
                drag: MenuDrag::Undecided,
            };
        }
        if let Some(slot) = self.touch_hotbar_slot(p, density) {
            self.selected = slot;
            return Finger::Hotbar {
                since: Instant::now(),
                dropped: false,
            };
        }
        if layout.on_pad(p) {
            let dir = layout.dir_at(p);
            // A second press forward soon after the last sprints, as in Bedrock.
            if dir.is_some_and(Dir::forward)
                && self
                    .touch
                    .forward_ended
                    .is_some_and(|t| t.elapsed() < DOUBLE_TAP)
            {
                self.touch.dashing = true;
                self.touch_sprint_key();
            }
            return Finger::Pad { dir };
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
        layout: &Layout,
        finger: Finger,
        p: (f32, f32),
        density: f32,
    ) -> Finger {
        match finger {
            // The finger slides between the arrows, and off the pad keeps its direction.
            Finger::Pad { .. } => Finger::Pad {
                dir: layout.dir_at(p),
            },
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
            Finger::Menu { down, at, drag } => {
                let drag = match drag {
                    MenuDrag::Undecided if dist(down, p) > TAP_SLOP => {
                        if (p.1 - down.1).abs() >= (p.0 - down.0).abs() {
                            MenuDrag::Scroll
                        } else {
                            // A sideways drag holds the button down where the finger went down.
                            self.touch_pointer(
                                event_loop,
                                id,
                                down,
                                density,
                                Some(ElementState::Pressed),
                            );
                            MenuDrag::Pointer
                        }
                    }
                    d => d,
                };
                match drag {
                    MenuDrag::Undecided => Finger::Menu { down, at, drag },
                    MenuDrag::Pointer => {
                        self.touch_pointer(event_loop, id, p, density, None);
                        Finger::Menu { down, at: p, drag }
                    }
                    MenuDrag::Scroll => {
                        // The list follows the finger: dragging up scrolls down.
                        let steps = ((p.1 - at.1) / SCROLL_STEP).trunc();
                        if steps == 0.0 {
                            return Finger::Menu { down, at, drag };
                        }
                        self.handle_window_event(
                            event_loop,
                            id,
                            WindowEvent::MouseWheel {
                                device_id: DeviceId::dummy(),
                                delta: MouseScrollDelta::LineDelta(0.0, steps),
                                phase: TouchPhase::Moved,
                            },
                        );
                        Finger::Menu {
                            down,
                            at: (at.0, at.1 + steps * SCROLL_STEP),
                            drag,
                        }
                    }
                }
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
            Finger::Pad { dir } => {
                if dir.is_some_and(Dir::forward) {
                    self.touch.forward_ended = Some(Instant::now());
                }
                self.touch.dashing = false;
                self.touch_sprint_key();
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
            Finger::Menu { down, drag, .. } => match drag {
                // A tap: a click where the finger went down.
                MenuDrag::Undecided if ended => {
                    self.touch_pointer(event_loop, id, down, density, Some(ElementState::Pressed));
                    self.touch_pointer(event_loop, id, down, density, Some(ElementState::Released));
                }
                MenuDrag::Pointer => {
                    self.touch_pointer(event_loop, id, p, density, Some(ElementState::Released));
                }
                _ => {}
            },
            Finger::Hotbar { .. } | Finger::Done => {}
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
        let mut drop = false;
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
            if let Finger::Hotbar { since, dropped } = f
                && !*dropped
                && playing
                && since.elapsed() >= DROP_TIME
            {
                *dropped = true;
                drop = true;
            }
        }
        if drop && let Err(e) = self.key_pressed(KeyCode::KeyQ) {
            log::warn!("Horizon: touch drop: {e}");
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
                Finger::Pad { .. } | Finger::Hotbar { .. } | Finger::Look { mining: false, .. } => {
                    *f = Finger::Done
                }
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
        self.touch.dashing = false;
        self.touch_sprint_key();
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
            (Button::Sprint, true) => {
                self.touch.sprinting = !self.touch.sprinting;
                self.touch_sprint_key();
                Ok(())
            }
            (Button::Attack, _) => self.play_mouse(MouseButton::Left, down),
            (Button::Use, _) => self.play_mouse(MouseButton::Right, down),
            (_, false) => Ok(()),
            (Button::Inventory, true) => self.key_pressed(KeyCode::KeyE),
            (Button::Chat, true) => self.key_pressed(KeyCode::KeyT),
            (Button::Perspective, true) => self.key_pressed(KeyCode::F5),
            (Button::Pause, true) => self.pad_back(),
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

    /// The D-pad and the buttons, over everything but the on-screen keyboard.
    pub(super) fn draw_touch(&self, batch: &mut HudBatch) {
        if !ENABLED {
            return;
        }
        let Some(layout) = self.touch_layout() else {
            return;
        };
        let mut overlay = Overlay {
            playing: self.touch_playing(),
            sneak: self.touch.sneaking,
            sprint: self.touch.sprinting || self.touch.dashing,
            jump: self.keys.contains(&KeyCode::Space),
            ..Overlay::default()
        };
        for (_, f) in &self.touch.fingers {
            match *f {
                Finger::Pad { dir } => overlay.walking = dir,
                Finger::Look { mining: true, .. } | Finger::Button(Button::Attack) => {
                    overlay.attack = true;
                }
                Finger::Button(Button::Use) => overlay.using = true,
                _ => {}
            }
        }
        let mut ui = horizon_client::ui_theme::Ui::new(&self.gui.atlas, batch, self.cursor);
        touch::draw(&mut ui, &layout, &overlay);
        ui.finish();
    }
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}
