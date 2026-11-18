//! Overlays drawn on top of other screens: the pause panel (with its
//! confirm-before-discarding model), the demo banner, toast messages, the
//! mute badge, the screen-change fade and the FPS counter.

use super::menu::{self, ListLayout, Menu, MenuEvent};
use super::ui::{self, pal};
use crate::config::CANVAS_W;
use crate::font::{self, Align};
use crate::input::Actions;
use crate::view::Layout;
use macroquad::math::{Rect, Vec2};
use macroquad::shapes::{draw_line, draw_rectangle, draw_triangle};
use macroquad::time::get_fps;

/// Pause menu entries, in order.
pub const PAUSE_ITEMS: [&str; 4] = ["RESUME", "RESTART", "OPTIONS", "QUIT TO TITLE"];
/// Index of each pause entry in [`PAUSE_ITEMS`].
pub const PAUSE_RESUME: usize = 0;
pub const PAUSE_RESTART: usize = 1;
pub const PAUSE_OPTIONS: usize = 2;
pub const PAUSE_QUIT: usize = 3;
/// Labels shown while an entry that throws the game away waits for a
/// second Enter.
const CONFIRM_RESTART: &str = "CONFIRM RESTART?";
const CONFIRM_QUIT: &str = "CONFIRM QUIT?";

pub const PAUSE_LIST: ListLayout = ListLayout {
    cx: ui::CX,
    y0: 14.9,
    spacing: 1.7,
    size: 0.85,
};
const PAUSE_PANEL: Rect = Rect {
    x: 4.3,
    y: 10.4,
    w: 18.4,
    h: 13.6,
};

/// The open pause menu. RESTART and QUIT TO TITLE throw the running game
/// away, so the first Enter (or click) only *arms* them: the label turns into
/// a question and a second Enter on the same entry confirms. Moving to
/// another entry, Esc or P cancels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PauseMenu {
    pub menu: Menu,
    /// The entry waiting for confirmation, if any.
    pub armed: Option<usize>,
}

/// What one frame of pause-menu input asks the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseAction {
    None,
    /// The selection moved (any pending confirmation was cancelled).
    Moved,
    /// Close the menu and carry on.
    Resume,
    /// RESTART / QUIT TO TITLE now asks for confirmation.
    Armed,
    /// A pending confirmation was cancelled with Esc / P.
    Disarmed,
    /// Confirmed: start a new game.
    Restart,
    Options,
    /// Confirmed: abandon the game and go back to the title.
    QuitToTitle,
}

impl Default for PauseMenu {
    fn default() -> Self {
        PauseMenu::new()
    }
}

impl PauseMenu {
    pub fn new() -> PauseMenu {
        PauseMenu {
            menu: Menu::new(PAUSE_ITEMS.len()),
            armed: None,
        }
    }

    /// Entry labels as shown right now (an armed entry reads as a question).
    pub fn labels(&self) -> [&'static str; PAUSE_ITEMS.len()] {
        let mut labels = PAUSE_ITEMS;
        match self.armed {
            Some(PAUSE_RESTART) => labels[PAUSE_RESTART] = CONFIRM_RESTART,
            Some(PAUSE_QUIT) => labels[PAUSE_QUIT] = CONFIRM_QUIT,
            _ => {}
        }
        labels
    }

    /// One frame of input. `mouse` is the pointer in canvas tiles.
    pub fn update(&mut self, act: &Actions, mouse: Vec2) -> PauseAction {
        if act.pause {
            return if self.armed.take().is_some() {
                PauseAction::Disarmed
            } else {
                PauseAction::Resume
            };
        }
        let hover = PAUSE_LIST.hit(&self.labels(), mouse);
        match menu::navigate(&mut self.menu, act, hover) {
            MenuEvent::None => PauseAction::None,
            MenuEvent::Moved => {
                if self.armed != Some(self.menu.selected) {
                    self.armed = None;
                }
                PauseAction::Moved
            }
            MenuEvent::Activated(i) => {
                let confirmed = self.armed == Some(i);
                self.armed = None;
                match i {
                    PAUSE_RESUME => PauseAction::Resume,
                    PAUSE_OPTIONS => PauseAction::Options,
                    PAUSE_RESTART | PAUSE_QUIT if !confirmed => {
                        self.armed = Some(i);
                        PauseAction::Armed
                    }
                    PAUSE_RESTART => PauseAction::Restart,
                    _ => PauseAction::QuitToTitle,
                }
            }
        }
    }
}

/// Draw the dimmed pause overlay with its menu.
pub fn draw_pause(l: &Layout, pause: &PauseMenu, t: f32) {
    ui::dim_screen(l, 0.62);
    ui::panel(l, PAUSE_PANEL, pal::VIOLET);
    ui::text_glow(l, "PAUSED", ui::CX, 11.6, ui::H1, pal::MINT, Align::Center);
    let labels = pause.labels();
    let (accent, hint, hint_color) = if pause.armed.is_some() {
        (
            pal::AMBER,
            "ENTER AGAIN TO CONFIRM \u{b7} ESC CANCELS",
            pal::AMBER,
        )
    } else {
        (pal::MINT, "ESC OR P TO RESUME", pal::DIM)
    };
    menu::draw_list_with(l, &labels, &pause.menu, &PAUSE_LIST, t, accent);
    ui::text(l, hint, ui::CX, 22.5, ui::TINY, hint_color, Align::Center);
}

/// Small speaker-with-a-cross badge in the window's top-right corner, shown
/// for as long as the session mute (M or `--mute`) is on.
pub fn draw_mute_badge(l: &Layout, t: f32) {
    let s = l.px(0.62).clamp(11.0, 30.0);
    let pad = (s * 0.55).round();
    let (x0, y0) = (l.screen_w - pad - s * 1.6, pad);
    let a = 0.75 + 0.25 * ui::pulse(t, 0.5);
    let ink = ui::alpha(pal::INK, 0.9 * a);
    let cross = ui::alpha(pal::CRIMSON, a);
    // Backing plate for contrast on bright scenes.
    draw_rectangle(
        x0 - s * 0.25,
        y0 - s * 0.25,
        s * 1.92,
        s * 1.5,
        ui::alpha(pal::SHADOW, 0.55),
    );
    let cy = y0 + s * 0.5;
    // Speaker: a box and a cone.
    draw_rectangle(x0, cy - s * 0.18, s * 0.26, s * 0.36, ink);
    draw_triangle(
        Vec2::new(x0 + s * 0.2, cy - s * 0.18),
        Vec2::new(x0 + s * 0.62, cy - s * 0.48),
        Vec2::new(x0 + s * 0.62, cy + s * 0.48),
        ink,
    );
    draw_triangle(
        Vec2::new(x0 + s * 0.2, cy - s * 0.18),
        Vec2::new(x0 + s * 0.62, cy + s * 0.48),
        Vec2::new(x0 + s * 0.2, cy + s * 0.18),
        ink,
    );
    // The cross.
    let (cx, r) = (x0 + s * 1.15, s * 0.27);
    let th = (s * 0.13).max(1.5);
    draw_line(cx - r, cy - r, cx + r, cy + r, th, cross);
    draw_line(cx - r, cy + r, cx + r, cy - r, th, cross);
}

/// Attract-mode banner across the bottom HUD rows.
pub fn draw_demo_banner(l: &Layout, t: f32) {
    let band = Rect::new(0.0, 34.05, CANVAS_W, 1.9);
    ui::rect(l, band, ui::alpha(pal::SHADOW, 0.82));
    ui::line(
        l,
        0.0,
        band.y,
        CANVAS_W,
        band.y,
        0.05,
        ui::alpha(pal::VIOLET, 0.6),
    );
    let a = 0.55 + 0.45 * ui::pulse(t, 0.8);
    ui::text_glow(
        l,
        "DEMO - PRESS ANY KEY",
        ui::CX,
        34.65,
        ui::SMALL + 0.08,
        ui::alpha(pal::AMBER, a),
        Align::Center,
    );
}

/// A short status message ("SOUND OFF", ...).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Toast {
    text: &'static str,
    /// Seconds remaining.
    left: f32,
}

impl Toast {
    const LIFE: f32 = 1.6;

    pub fn show(&mut self, text: &'static str) {
        self.text = text;
        self.left = Self::LIFE;
    }

    pub fn update(&mut self, dt: f32) {
        self.left = (self.left - dt).max(0.0);
    }

    pub fn draw(&self, l: &Layout) {
        if self.left <= 0.0 || self.text.is_empty() {
            return;
        }
        let a = (self.left / 0.3).min(1.0) * ui::progress(Self::LIFE - self.left, 0.0, 0.15);
        let size = ui::SMALL;
        let w = ui::width(self.text, size).min(CANVAS_W - 2.0) + 1.6;
        let r = Rect::new(ui::CX - w * 0.5, 31.0, w, size + 0.9);
        ui::rect(l, r, ui::alpha(pal::PANEL, a));
        ui::line(
            l,
            r.x,
            r.y + r.h,
            r.x + r.w,
            r.y + r.h,
            0.06,
            ui::alpha(pal::MINT, 0.8 * a),
        );
        ui::text(
            l,
            self.text,
            ui::CX,
            r.y + 0.45,
            size,
            ui::alpha(pal::INK, a),
            Align::Center,
        );
    }
}

/// Fade-from-black after a screen change.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fade {
    left: f32,
}

impl Fade {
    const LEN: f32 = 0.28;

    pub fn start(&mut self) {
        self.left = Self::LEN;
    }

    pub fn update(&mut self, dt: f32) {
        self.left = (self.left - dt).max(0.0);
    }

    pub fn draw(&self, l: &Layout) {
        if self.left > 0.0 {
            ui::dim_screen(l, (self.left / Self::LEN).powi(2));
        }
    }
}

/// FPS readout in the window's top-left corner; formats only on change.
#[derive(Clone, Debug, Default)]
pub struct FpsCounter {
    value: i32,
    label: String,
}

impl FpsCounter {
    pub fn draw(&mut self, l: &Layout) {
        let fps = get_fps();
        if fps != self.value || self.label.is_empty() {
            self.value = fps;
            self.label.clear();
            self.label.push_str(&fps.max(0).to_string());
            self.label.push_str(" FPS");
        }
        let size = l.px(0.42).clamp(9.0, 22.0);
        let pad = (size * 0.5).round();
        font::draw_text(
            &self.label,
            pad + 1.0,
            pad + 1.0,
            size,
            ui::alpha(pal::SHADOW, 0.8),
            Align::Left,
        );
        font::draw_text(&self.label, pad, pad, size, pal::GREEN, Align::Left);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_and_fade_expire() {
        let mut t = Toast::default();
        t.show("SOUND OFF");
        t.update(0.5);
        assert!(t.left > 0.0);
        t.update(10.0);
        assert_eq!(t.left, 0.0);
        let mut f = Fade::default();
        f.start();
        f.update(1.0);
        assert_eq!(f.left, 0.0);
    }

    #[test]
    fn pause_items_fit_inside_the_panel() {
        let last = PAUSE_LIST.item_y(PAUSE_ITEMS.len() - 1) + PAUSE_LIST.size;
        assert!(last < PAUSE_PANEL.y + PAUSE_PANEL.h);
        let mut labels: Vec<(usize, &str)> = PAUSE_ITEMS.iter().copied().enumerate().collect();
        labels.push((PAUSE_RESTART, CONFIRM_RESTART));
        labels.push((PAUSE_QUIT, CONFIRM_QUIT));
        for (i, label) in labels {
            let r = PAUSE_LIST.item_rect(i, label);
            assert!(
                r.x >= PAUSE_PANEL.x && r.x + r.w <= PAUSE_PANEL.x + PAUSE_PANEL.w,
                "{label}"
            );
        }
    }

    fn act(f: impl FnOnce(&mut Actions)) -> Actions {
        let mut a = Actions::default();
        f(&mut a);
        a
    }

    const FAR: Vec2 = Vec2::new(-50.0, -50.0);

    #[test]
    fn restart_and_quit_need_a_second_enter() {
        let enter = act(|a| a.confirm = true);
        let down = act(|a| a.down = true);
        for (index, done) in [
            (PAUSE_RESTART, PauseAction::Restart),
            (PAUSE_QUIT, PauseAction::QuitToTitle),
        ] {
            let mut p = PauseMenu::new();
            p.menu.select(index);
            assert_eq!(p.update(&enter, FAR), PauseAction::Armed);
            assert_eq!(p.armed, Some(index));
            assert!(p.labels()[index].starts_with("CONFIRM"));
            assert_eq!(p.update(&Actions::default(), FAR), PauseAction::None);
            assert_eq!(p.update(&enter, FAR), done);
            assert_eq!(p.armed, None);
            // Moving away cancels: the next Enter only arms again.
            p.menu.select(index);
            assert_eq!(p.update(&enter, FAR), PauseAction::Armed);
            assert_eq!(p.update(&down, FAR), PauseAction::Moved);
            assert_eq!(p.armed, None);
            assert_eq!(p.labels(), PAUSE_ITEMS);
        }
    }

    #[test]
    fn escape_cancels_a_confirmation_before_resuming() {
        let enter = act(|a| a.confirm = true);
        let esc = act(|a| {
            a.pause = true;
            a.escape = true;
        });
        let mut p = PauseMenu::new();
        p.menu.select(PAUSE_QUIT);
        assert_eq!(p.update(&enter, FAR), PauseAction::Armed);
        assert_eq!(p.update(&esc, FAR), PauseAction::Disarmed);
        assert_eq!(p.armed, None);
        assert_eq!(p.update(&esc, FAR), PauseAction::Resume);
    }

    #[test]
    fn resume_and_options_act_at_once() {
        let enter = act(|a| a.confirm = true);
        let mut p = PauseMenu::new();
        assert_eq!(p.update(&enter, FAR), PauseAction::Resume);
        p.menu.select(PAUSE_OPTIONS);
        assert_eq!(p.update(&enter, FAR), PauseAction::Options);
        // Clicking an armed entry confirms it; clicking another one does not.
        let click_on = |i: usize| {
            let r = PAUSE_LIST.item_rect(i, PauseMenu::new().labels()[i]);
            (
                act(|a| a.click = true),
                Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5),
            )
        };
        let (click, at_restart) = click_on(PAUSE_RESTART);
        assert_eq!(p.update(&click, at_restart), PauseAction::Armed);
        let (_, at_quit) = click_on(PAUSE_QUIT);
        assert_eq!(p.update(&click, at_quit), PauseAction::Armed);
        assert_eq!(p.armed, Some(PAUSE_QUIT));
        assert_eq!(p.update(&click, at_quit), PauseAction::QuitToTitle);
    }
}
