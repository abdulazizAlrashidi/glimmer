//! Loading frame, splash, and the title screen with its menu and cast roll.

use super::menu::{self, ListLayout, Menu, MenuEvent};
use super::ui::{self, pal};
use crate::config::ATTRACT_IDLE_TIME;
use crate::font::Align;
use crate::geom::Dir;
use crate::input::Actions;
use crate::render::sprites;
use crate::shade::{ShadeKind, ShadeLook};
use crate::view::Layout;
use macroquad::math::{Rect, Vec2};

/// Total splash length before the title appears on its own.
pub const SPLASH_TIME: f32 = 2.7;
/// Logo fade-in length on the splash.
const SPLASH_FADE: f32 = 1.6;

/// "LOADING" frame shown while sounds are synthesised.
pub fn draw_loading(l: &Layout) {
    ui::text(
        l,
        "LOADING",
        ui::CX,
        17.2,
        ui::BODY,
        pal::DIM,
        Align::Center,
    );
    let w = ui::width("LOADING", ui::BODY);
    ui::marker(l, ui::CX - w * 0.5 - 1.0, 17.2 + ui::BODY * 0.5, 0.3, 0.0);
}

/// The splash: logo fades in from the dark amid sparkles.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplashState {
    pub t: f32,
}

impl SplashState {
    /// Returns true when the splash is over (timed out or skipped).
    pub fn update(&mut self, dt: f32, act: &Actions) -> bool {
        ui::tick_clock(&mut self.t, dt);
        self.t >= SPLASH_TIME || (self.t > 0.15 && act.any)
    }

    pub fn draw(&self, l: &Layout) {
        let t = self.t;
        let fade = ui::ease_out(ui::progress(t, 0.0, SPLASH_FADE));
        sprites::draw_logo(l.canvas(ui::CX, 15.6), l.px(21.0), t);
        // Veil over everything drawn so far: the scene fades in from black.
        ui::dim_screen(l, 1.0 - fade);
        let field = Rect::new(2.0, 10.5, 23.0, 10.0);
        ui::sparkle_field(l, field, 34, 11, t * 1.3, 0.35 + 0.65 * fade);
        let sub = ui::progress(t, 1.0, 0.8);
        ui::text(
            l,
            "A NEON MAZE CHASE",
            ui::CX,
            20.6,
            ui::SMALL,
            ui::alpha(pal::VIOLET, sub),
            Align::Center,
        );
    }
}

/// Footer key hints. F11 alone is not enough: on a default Mac keyboard the
/// F-keys are media keys and fn+F11 shows the desktop, so the game never
/// sees it; Alt(Option)+Enter always works.
pub const TITLE_HINT: &str =
    "\u{2191}\u{2193} CHOOSE \u{b7} ENTER SELECT \u{b7} M MUTE \u{b7} F11 OR ALT+ENTER FULLSCREEN";

/// Title menu entries, in order.
pub const TITLE_ITEMS: [&str; 5] = ["PLAY", "HIGH SCORES", "OPTIONS", "HOW TO PLAY", "QUIT"];

const TITLE_LIST: ListLayout = ListLayout {
    cx: ui::CX,
    y0: 10.05,
    spacing: 1.6,
    size: ui::MENU,
};

/// What the player chose on the title screen this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleAction {
    None,
    Moved,
    Play,
    Scores,
    Options,
    HowTo,
    Quit,
}

/// Persistent title-screen state (kept while visiting sub-screens).
#[derive(Clone, Debug)]
pub struct TitleState {
    pub menu: Menu,
    /// Seconds since the title (re)appeared; drives the cast roll.
    pub t: f32,
    /// Seconds without any input (starts the attract demo).
    pub idle: f32,
    version: String,
}

/// Cast roll timing: row `i` pops in at `CAST_START + i * CAST_STEP`.
const CAST_START: f32 = 0.5;
const CAST_STEP: f32 = 0.5;
const CAST_Y0: f32 = 20.1;
const CAST_SPACING: f32 = 2.55;
/// Cast roll columns, relative to the sprite centre: names start
/// `CAST_NAME_DX` to the right of it, epithets end `CAST_TITLE_DX` to the
/// right of it. The block (sprite edge to epithet end) is centred on the
/// canvas.
const CAST_SPRITE_R: f32 = 0.8;
const CAST_NAME_DX: f32 = 2.0;
const CAST_TITLE_DX: f32 = 15.4;
const CAST_SPRITE_X: f32 = ui::CX - (CAST_TITLE_DX + CAST_SPRITE_R) * 0.5 + CAST_SPRITE_R;

impl TitleState {
    pub fn new() -> TitleState {
        TitleState {
            menu: Menu::new(TITLE_ITEMS.len()),
            t: 0.0,
            idle: 0.0,
            version: format!("V{}", env!("CARGO_PKG_VERSION")),
        }
    }

    /// Restart the cast roll and the idle timer (e.g. after the demo).
    pub fn replay(&mut self) {
        self.t = 0.0;
        self.idle = 0.0;
    }

    /// Time to start the attract-mode demo?
    pub fn wants_demo(&self) -> bool {
        self.idle >= ATTRACT_IDLE_TIME
    }

    /// One frame of title input. `mouse` is the pointer in canvas tiles.
    pub fn update(&mut self, dt: f32, act: &Actions, mouse: Vec2) -> TitleAction {
        ui::tick_clock(&mut self.t, dt);
        self.idle += dt;
        if act.any || act.mouse_moved {
            self.idle = 0.0;
        }
        if act.quit {
            return TitleAction::Quit;
        }
        let hover = TITLE_LIST.hit(&TITLE_ITEMS, mouse);
        match menu::navigate(&mut self.menu, act, hover) {
            MenuEvent::None => TitleAction::None,
            MenuEvent::Moved => TitleAction::Moved,
            MenuEvent::Activated(i) => match i {
                0 => TitleAction::Play,
                1 => TitleAction::Scores,
                2 => TitleAction::Options,
                3 => TitleAction::HowTo,
                _ => TitleAction::Quit,
            },
        }
    }

    pub fn draw(&self, l: &Layout) {
        let t = self.t;
        sprites::draw_logo(l.canvas(ui::CX, 4.1), l.px(19.5), t);
        ui::text(
            l,
            "A NEON MAZE CHASE",
            ui::CX,
            8.15,
            ui::SMALL,
            pal::VIOLET,
            Align::Center,
        );
        menu::draw_list(l, &TITLE_ITEMS, &self.menu, &TITLE_LIST, t);
        self.draw_cast(l);
        ui::hint(l, TITLE_HINT, 33.5);
        ui::text(
            l,
            &self.version,
            0.9,
            34.85,
            ui::TINY,
            pal::FAINT,
            Align::Left,
        );
        ui::text(l, "Q QUIT", 26.1, 34.85, ui::TINY, pal::FAINT, Align::Right);
    }

    fn draw_cast(&self, l: &Layout) {
        let t = self.t;
        let heading = ui::progress(t, 0.15, 0.5);
        ui::text(
            l,
            "- MEET THE CAST -",
            ui::CX,
            18.35,
            ui::SMALL,
            ui::alpha(pal::DIM, heading),
            Align::Center,
        );

        let sprite_x = CAST_SPRITE_X;
        let name_x = sprite_x + CAST_NAME_DX;
        let title_r = sprite_x + CAST_TITLE_DX;
        for row in 0..5 {
            let start = CAST_START + row as f32 * CAST_STEP;
            if t < start {
                continue;
            }
            let k = t - start;
            let y = CAST_Y0 + row as f32 * CAST_SPACING;
            let pop = ui::ease_back(ui::progress(k, 0.0, 0.45));
            let radius = if row == 0 { 0.75 } else { CAST_SPRITE_R };
            let bob = (t * 2.4 + row as f32 * 0.9).sin() * 0.1;
            let centre = l.canvas(sprite_x, y + bob);
            let (name, epithet, color) = if row == 0 {
                sprites::draw_glim(centre, l.px(radius * pop), Dir::Right, t, 1.0);
                ("GLIM", "THE FIREFLY", sprites::glim_color())
            } else {
                let kind = ShadeKind::ALL[row - 1];
                sprites::draw_shade(
                    centre,
                    l.px(radius * pop),
                    kind,
                    ShadeLook::Normal,
                    Dir::Right,
                    t,
                );
                (kind.name(), kind.title(), sprites::shade_color(kind))
            };
            // A short burst of sparkles as each character arrives.
            let burst = 1.0 - ui::progress(k, 0.0, 0.6);
            if burst > 0.0 {
                for s in 0..6u32 {
                    let a = s as f32 * std::f32::consts::TAU / 6.0 + row as f32;
                    let d = 0.6 + 1.2 * (1.0 - burst);
                    ui::sparkle(
                        l,
                        sprite_x + a.cos() * d,
                        y + a.sin() * d,
                        0.2 * burst,
                        ui::alpha(color, burst),
                    );
                }
            }
            // Name types in letter by letter (names are ASCII).
            let letters = ((k - 0.2) / 0.06).max(0.0) as usize;
            let shown = &name[..letters.min(name.len())];
            if !shown.is_empty() {
                ui::text_glow(l, shown, name_x, y - 0.42, 0.85, color, Align::Left);
            }
            let ep = ui::progress(k, 0.45, 0.4);
            if ep > 0.0 {
                let dim = ui::mix(color, pal::DIM, 0.45);
                ui::text(
                    l,
                    epithet,
                    title_r,
                    y - 0.28,
                    ui::SMALL,
                    ui::alpha(dim, ep),
                    Align::Right,
                );
            }
        }
    }
}

impl Default for TitleState {
    fn default() -> Self {
        TitleState::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_timer_triggers_demo_and_input_resets_it() {
        let mut st = TitleState::new();
        let none = Actions::default();
        let far = Vec2::new(-50.0, -50.0);
        st.update(ATTRACT_IDLE_TIME * 0.6, &none, far);
        assert!(!st.wants_demo());
        let key = Actions {
            any: true,
            ..Actions::default()
        };
        st.update(0.0, &key, far);
        st.update(ATTRACT_IDLE_TIME * 0.6, &none, far);
        assert!(!st.wants_demo());
        st.update(ATTRACT_IDLE_TIME * 0.6, &none, far);
        assert!(st.wants_demo());
        st.replay();
        assert!(!st.wants_demo());
    }

    #[test]
    fn menu_choices_map_to_actions() {
        let mut st = TitleState::new();
        let far = Vec2::new(-50.0, -50.0);
        let ok = Actions {
            confirm: true,
            ..Actions::default()
        };
        let down = Actions {
            down: true,
            ..Actions::default()
        };
        let expected = [
            TitleAction::Play,
            TitleAction::Scores,
            TitleAction::Options,
            TitleAction::HowTo,
            TitleAction::Quit,
        ];
        for e in expected {
            assert_eq!(st.update(0.0, &ok, far), e);
            assert_eq!(st.update(0.0, &down, far), TitleAction::Moved);
        }
        let q = Actions {
            quit: true,
            ..Actions::default()
        };
        assert_eq!(st.update(0.0, &q, far), TitleAction::Quit);
    }

    #[test]
    fn splash_ends_on_time_or_key() {
        let mut s = SplashState::default();
        assert!(!s.update(0.1, &Actions::default()));
        let key = Actions {
            any: true,
            ..Actions::default()
        };
        assert!(s.update(0.1, &key));
        let mut s = SplashState::default();
        assert!(s.update(SPLASH_TIME, &Actions::default()));
    }

    #[test]
    fn cast_block_is_centred_and_columns_do_not_collide() {
        let left = CAST_SPRITE_X - CAST_SPRITE_R;
        let right = CAST_SPRITE_X + CAST_TITLE_DX;
        assert!(((left + right) * 0.5 - ui::CX).abs() < 1e-4);
        let name_x = CAST_SPRITE_X + CAST_NAME_DX;
        let names = ShadeKind::ALL
            .iter()
            .map(|k| (k.name(), k.title()))
            .chain(std::iter::once(("GLIM", "THE FIREFLY")));
        for (name, epithet) in names {
            let name_end = name_x + ui::width(name, 0.85);
            let epithet_start = right - ui::width(epithet, ui::SMALL);
            assert!(name_end + 0.5 < epithet_start, "{name} / {epithet}");
        }
    }

    #[test]
    fn title_hint_names_both_fullscreen_keys() {
        assert!(TITLE_HINT.contains("F11") && TITLE_HINT.contains("ALT+ENTER"));
    }

    #[test]
    fn cast_names_fit_the_letter_slicing() {
        for k in ShadeKind::ALL {
            assert!(k.name().is_ascii());
        }
    }
}
