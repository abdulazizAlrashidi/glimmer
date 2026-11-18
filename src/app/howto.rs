//! HOW TO PLAY: four pages (basics & controls, scoring, the Gloom, tips).
//!
//! Wording: one pursuer is a *shade*, the four together are *the Gloom*.
//! Every number shown (points, chain values, bonus, relic values, how often
//! a relic appears) is read from `config`, never typed into the text.

use super::ui::{self, pal};
use crate::config::{
    BonusKind, BONUS_TRIGGER_FRACTIONS, FLARE_POINTS, FULL_CHAIN_BONUS, MOTE_POINTS,
    SHADE_CHAIN_POINTS,
};
use crate::font::Align;
use crate::geom::Dir;
use crate::input::Actions;
use crate::render::sprites;
use crate::save::Settings;
use crate::shade::{ShadeKind, ShadeLook};
use crate::view::Layout;
use macroquad::math::Rect;

/// Number of pages.
pub const PAGES: usize = 4;

const PAGE_TITLES: [&str; PAGES] = ["HOW TO PLAY", "SCORING", "THE GLOOM", "TIPS"];

#[derive(Clone, Debug, PartialEq)]
pub struct HowToState {
    pub page: usize,
    /// Seconds since the screen opened.
    pub t: f32,
    /// Seconds since the current page appeared.
    pub page_t: f32,
    /// Pre-formatted values that depend on settings / config.
    labels: Labels,
}

#[derive(Clone, Debug, PartialEq)]
struct Labels {
    mote: String,
    flare: String,
    chain: String,
    chain_top: String,
    full_chain: String,
    relics: [String; 8],
    relic_note: String,
}

/// "ONCE", "TWICE" or "N TIMES".
fn times(n: usize) -> String {
    match n {
        1 => "ONCE".to_owned(),
        2 => "TWICE".to_owned(),
        n => format!("{n} TIMES"),
    }
}

impl Labels {
    fn new() -> Labels {
        let chain = SHADE_CHAIN_POINTS
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(" \u{2192} ");
        let top = SHADE_CHAIN_POINTS.last().copied().unwrap_or(0);
        Labels {
            mote: MOTE_POINTS.to_string(),
            flare: FLARE_POINTS.to_string(),
            chain,
            chain_top: format!("UP TO {top}"),
            full_chain: format!("+{FULL_CHAIN_BONUS}"),
            relics: BonusKind::ALL.map(|k| k.points().to_string()),
            relic_note: format!(
                "A RELIC APPEARS BELOW THE NEST {} A LEVEL",
                times(BONUS_TRIGGER_FRACTIONS.len())
            ),
        }
    }
}

/// Result of one frame of input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HowToAction {
    None,
    Turned,
    Leave,
}

impl HowToState {
    pub fn new(page: usize) -> HowToState {
        HowToState {
            page: page.min(PAGES - 1),
            t: 0.0,
            page_t: 0.0,
            labels: Labels::new(),
        }
    }

    pub fn update(&mut self, dt: f32, act: &Actions) -> HowToAction {
        ui::tick_clock(&mut self.t, dt);
        ui::tick_clock(&mut self.page_t, dt);
        if act.back {
            return HowToAction::Leave;
        }
        let next = act.right || act.down || act.confirm || act.click;
        let prev = act.left || act.up;
        if next {
            if self.page + 1 >= PAGES {
                if act.confirm || act.click {
                    return HowToAction::Leave;
                }
                return HowToAction::None;
            }
            self.page += 1;
            self.page_t = 0.0;
            return HowToAction::Turned;
        }
        if prev && self.page > 0 {
            self.page -= 1;
            self.page_t = 0.0;
            return HowToAction::Turned;
        }
        HowToAction::None
    }

    pub fn draw(&self, l: &Layout, settings: &Settings) {
        let t = self.t;
        ui::heading(l, PAGE_TITLES[self.page.min(PAGES - 1)], 2.2, pal::MINT, t);
        match self.page {
            0 => self.draw_basics(l),
            1 => self.draw_scoring(l, settings),
            2 => self.draw_gloom(l),
            _ => self.draw_tips(l),
        }
        // Page-turn flash: briefly veil the content area.
        let veil = 1.0 - ui::ease_out(ui::progress(self.page_t, 0.0, 0.22));
        if veil > 0.0 {
            ui::rect(
                l,
                Rect::new(0.0, 5.0, 27.0, 25.5),
                ui::alpha(pal::SHADOW, 0.85 * veil),
            );
        }
        // Page indicator.
        for i in 0..PAGES {
            let x = ui::CX + (i as f32 - (PAGES as f32 - 1.0) * 0.5) * 1.1;
            let (r, c) = if i == self.page {
                (0.24, pal::MINT)
            } else {
                (0.14, pal::FAINT)
            };
            ui::diamond(l, x, 31.6, r, c);
        }
        let hint = if self.page + 1 < PAGES {
            "\u{2190}\u{2192} TURN PAGE   ESC BACK"
        } else {
            "\u{2190} PREVIOUS   ENTER OR ESC BACK"
        };
        ui::hint(l, hint, 33.6);
    }

    fn draw_basics(&self, l: &Layout) {
        let t = self.t;
        sprites::draw_glim(l.canvas(ui::CX, 7.6), l.px(1.15), Dir::Right, t, 1.0);
        let lines = [
            "GUIDE GLIM THROUGH THE NIGHT GARDEN",
            "AND GATHER EVERY MOTE TO CLEAR IT.",
            "THE GLOOM - FOUR SHADES - HUNTS YOU.",
            "EAT A FLARE AND THE SHADES FLEE.",
        ];
        for (i, s) in lines.iter().enumerate() {
            ui::text(
                l,
                s,
                ui::CX,
                10.0 + i as f32 * 1.2,
                ui::SMALL + 0.06,
                pal::INK,
                Align::Center,
            );
        }
        ui::text_glow(
            l,
            "CONTROLS",
            ui::CX,
            16.0,
            ui::BODY,
            pal::VIOLET,
            Align::Center,
        );
        let rows = [
            ("ARROWS / WASD", "MOVE"),
            ("ESC / P", "PAUSE"),
            ("ENTER / SPACE", "SELECT"),
            ("M", "MUTE SOUND"),
            ("F11 / ALT+ENTER", "FULLSCREEN"),
            ("F3", "DEBUG VIEW"),
        ];
        for (i, (key, what)) in rows.iter().enumerate() {
            let y = 18.0 + i as f32 * 1.45;
            ui::text(l, key, 12.6, y, ui::SMALL, pal::MINT, Align::Right);
            ui::text(l, what, 14.2, y, ui::SMALL, pal::INK, Align::Left);
        }
        ui::text(
            l,
            "MOUSE: POINT AND CLICK IN MENUS",
            ui::CX,
            27.6,
            ui::SMALL,
            pal::DIM,
            Align::Center,
        );
    }

    fn draw_scoring(&self, l: &Layout, settings: &Settings) {
        let t = self.t;
        let lb = &self.labels;
        let icon_x = 3.6;
        let label_x = 5.6;
        let value_r = 24.6;
        let row = |y: f32, label: &str, value: &str| {
            ui::text_mid(l, label, label_x, y, ui::BODY, pal::INK, Align::Left);
            ui::text_mid(l, value, value_r, y, ui::BODY, pal::AMBER, Align::Right);
        };

        let y = 6.9;
        sprites::draw_mote(l.canvas(icon_x, y), l.px(0.14), pal::INK);
        row(y, "MOTE", &lb.mote);

        let y = 8.7;
        sprites::draw_flare(l.canvas(icon_x, y), l.px(0.36), t, pal::INK);
        row(y, "FLARE", &lb.flare);

        let y = 10.6;
        let flash = ((t * 2.5) as i32) % 2 == 0;
        sprites::draw_shade(
            l.canvas(icon_x, y),
            l.px(0.62),
            ShadeKind::Vex,
            ShadeLook::Frightened { flash },
            Dir::Left,
            t,
        );
        row(y, "SHADE CHAIN", &lb.chain_top);
        ui::text(
            l,
            &lb.chain,
            label_x,
            y + 0.75,
            ui::TINY,
            pal::DIM,
            Align::Left,
        );

        let y = 13.0;
        for k in 0..4 {
            let x = icon_x - 0.9 + k as f32 * 0.6;
            sprites::draw_shade(
                l.canvas(x, y),
                l.px(0.3),
                ShadeKind::ALL[k],
                ShadeLook::Eyes,
                Dir::Right,
                t,
            );
        }
        row(y, "ALL FOUR, ONE FLARE", &lb.full_chain);

        let y = 14.9;
        sprites::draw_glim(l.canvas(icon_x, y), l.px(0.45), Dir::Right, t, 1.0);
        let extra = extra_life_text(settings.extra_life_at);
        row(y, "EXTRA LIFE", extra);

        ui::text_glow(
            l,
            "RELICS",
            ui::CX,
            17.0,
            ui::BODY,
            pal::VIOLET,
            Align::Center,
        );
        for (i, kind) in BonusKind::ALL.iter().enumerate() {
            let col = i / 4;
            let r = i % 4;
            let y = 19.4 + r as f32 * 2.05;
            let (ix, nx, pr) = if col == 0 {
                (2.4, 3.8, 12.4)
            } else {
                (14.8, 16.2, 25.0)
            };
            sprites::draw_bonus(l.canvas(ix, y), l.px(0.52), *kind, t);
            ui::text_mid(l, kind.name(), nx, y, ui::SMALL, pal::INK, Align::Left);
            ui::text_mid(l, &lb.relics[i], pr, y, ui::SMALL, pal::AMBER, Align::Right);
        }
        ui::text(
            l,
            &lb.relic_note,
            ui::CX,
            27.9,
            ui::TINY,
            pal::DIM,
            Align::Center,
        );
    }

    fn draw_gloom(&self, l: &Layout) {
        let t = self.t;
        let hints: [[&str; 2]; 4] = [
            [
                "CHASES YOU STRAIGHT DOWN.",
                "GROWS FASTER AS MOTES RUN OUT.",
            ],
            ["AIMS AHEAD OF YOU TO CUT", "YOU OFF AT THE NEXT TURN."],
            ["WORKS WITH ROOK TO PINCH", "YOU FROM THE OTHER SIDE."],
            ["CLOSES IN FROM AFAR, THEN", "LOSES INTEREST UP CLOSE."],
        ];
        ui::text(
            l,
            GLOOM_SUBTITLE,
            ui::CX,
            4.75,
            ui::SMALL,
            pal::VIOLET,
            Align::Center,
        );
        for (i, kind) in ShadeKind::ALL.iter().enumerate() {
            let y = 6.9 + i as f32 * 5.0;
            let bob = (t * 2.2 + i as f32).sin() * 0.12;
            sprites::draw_shade(
                l.canvas(3.6, y + 1.0 + bob),
                l.px(1.0),
                *kind,
                ShadeLook::Normal,
                Dir::Right,
                t,
            );
            let color = sprites::shade_color(*kind);
            ui::text_glow(l, kind.name(), 6.2, y, ui::BODY, color, Align::Left);
            ui::text(
                l,
                kind.title(),
                24.8,
                y + 0.12,
                ui::TINY,
                ui::mix(color, pal::DIM, 0.5),
                Align::Right,
            );
            for (j, h) in hints[i].iter().enumerate() {
                ui::text(
                    l,
                    h,
                    6.2,
                    y + 1.25 + j as f32 * 1.0,
                    ui::SMALL,
                    pal::INK,
                    Align::Left,
                );
            }
        }
        ui::text(
            l,
            "A FLARE TURNS EVERY SHADE HOLLOW, SLOW AND EDIBLE.",
            ui::CX,
            27.8,
            ui::TINY,
            pal::DIM,
            Align::Center,
        );
    }

    fn draw_tips(&self, l: &Layout) {
        let tips: [&[&str]; 7] = [
            &[
                "TURN EARLY: PRESS THE NEXT DIRECTION",
                "BEFORE YOU REACH THE CORNER.",
            ],
            &["THE GLOOM SLOWS DOWN IN SIDE TUNNELS."],
            &["SAVE FLARES FOR WHEN YOU ARE TRAPPED."],
            &["FLASHING SHADES ARE ABOUT TO RECOVER."],
            &[
                "THE GLOOM TAKES TURNS ROAMING AND",
                "HUNTING - WATCH THEM DOUBLE BACK.",
            ],
            &["FRIGHTS GET SHORTER ON LATER LEVELS."],
            &["EVERY FEW LEVELS THE GARDEN CHANGES."],
        ];
        let size = 0.6;
        let mut y = 6.6;
        for tip in tips {
            for (j, line) in tip.iter().enumerate() {
                if j == 0 {
                    ui::diamond(l, 2.0, y + size * 0.5, 0.16, pal::MINT);
                }
                ui::text(l, line, 2.8, y, size, pal::INK, Align::Left);
                y += 1.1;
            }
            y += 1.05;
        }
        // A small chase along the bottom for flavour.
        let t = self.t;
        let x = 6.0 + (t * 1.5).sin() * 1.2;
        sprites::draw_glim(l.canvas(x, 28.0), l.px(0.55), Dir::Right, t, 1.0);
        for (k, kind) in ShadeKind::ALL.iter().enumerate() {
            let sx = x + 4.0 + k as f32 * 2.0;
            sprites::draw_shade(
                l.canvas(sx, 28.0),
                l.px(0.55),
                *kind,
                ShadeLook::Frightened { flash: false },
                Dir::Right,
                t,
            );
        }
    }
}

/// Introduces the word "shade" on THE GLOOM page.
const GLOOM_SUBTITLE: &str = "FOUR SHADES HUNT THE GARDEN";

fn extra_life_text(at: u32) -> &'static str {
    match at {
        0 => "OFF",
        10_000 => "10000",
        15_000 => "15000",
        20_000 => "20000",
        30_000 => "30000",
        _ => "ON",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_turn_and_leave_at_the_end() {
        let mut h = HowToState::new(0);
        let right = Actions {
            right: true,
            ..Actions::default()
        };
        let left = Actions {
            left: true,
            ..Actions::default()
        };
        let ok = Actions {
            confirm: true,
            ..Actions::default()
        };
        assert_eq!(h.update(0.0, &left,), HowToAction::None);
        for p in 1..PAGES {
            assert_eq!(h.update(0.0, &right), HowToAction::Turned);
            assert_eq!(h.page, p);
        }
        assert_eq!(h.update(0.0, &right), HowToAction::None);
        assert_eq!(h.update(0.0, &ok), HowToAction::Leave);
        assert_eq!(h.update(0.0, &left), HowToAction::Turned);
        assert_eq!(h.page, PAGES - 2);
        let back = Actions {
            back: true,
            ..Actions::default()
        };
        assert_eq!(h.update(0.0, &back), HowToAction::Leave);
        assert_eq!(HowToState::new(99).page, PAGES - 1);
    }

    #[test]
    fn labels_follow_the_config() {
        let lb = Labels::new();
        assert_eq!(lb.mote, MOTE_POINTS.to_string());
        assert!(lb.chain.contains(&SHADE_CHAIN_POINTS[0].to_string()));
        assert_eq!(lb.relics[7], BonusKind::ALL[7].points().to_string());
        assert_eq!(lb.flare, FLARE_POINTS.to_string());
        assert_eq!(lb.full_chain, format!("+{FULL_CHAIN_BONUS}"));
        for p in SHADE_CHAIN_POINTS {
            assert!(lb.chain.contains(&p.to_string()));
        }
        let top = SHADE_CHAIN_POINTS[SHADE_CHAIN_POINTS.len() - 1];
        assert!(lb.chain_top.ends_with(&top.to_string()));
        assert!(lb
            .relic_note
            .contains(&times(BONUS_TRIGGER_FRACTIONS.len())));
        assert_eq!(
            (times(1), times(2), times(3)),
            ("ONCE".into(), "TWICE".into(), "3 TIMES".into())
        );
    }

    #[test]
    fn the_gloom_page_introduces_the_word_shade() {
        assert!(GLOOM_SUBTITLE.contains("SHADES"));
        assert_eq!(PAGE_TITLES[2], "THE GLOOM");
    }
}
