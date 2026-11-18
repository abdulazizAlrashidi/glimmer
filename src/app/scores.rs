//! The HIGH SCORES table screen.

use super::ui::{self, pal};
use crate::font::Align;
use crate::input::Actions;
use crate::save::HighScores;
use crate::view::Layout;

/// Pre-formatted row text (built once when the screen opens).
#[derive(Clone, Debug)]
struct Row {
    rank: &'static str,
    name: String,
    score: String,
    level: String,
}

#[derive(Clone, Debug)]
pub struct ScoresState {
    /// Row to celebrate (a freshly entered score).
    pub highlight: Option<usize>,
    pub t: f32,
    /// Arrived here straight from a finished game.
    pub from_game: bool,
    footer: Option<String>,
    rows: Vec<Row>,
}

const RANKS: [&str; HighScores::MAX] = [
    "1ST", "2ND", "3RD", "4TH", "5TH", "6TH", "7TH", "8TH", "9TH", "10TH",
];

const ROW_Y0: f32 = 8.2;
const ROW_SPACING: f32 = 1.95;
const ROW_SIZE: f32 = 0.78;
const RANK_R: f32 = 5.6;
const NAME_X: f32 = 7.4;
const SCORE_R: f32 = 19.4;
const LEVEL_R: f32 = 24.4;

impl ScoresState {
    /// `your_score`: a final score that did not qualify, shown as a footer.
    pub fn new(
        scores: &HighScores,
        highlight: Option<usize>,
        your_score: Option<u32>,
        from_game: bool,
    ) -> ScoresState {
        let rows = scores
            .entries
            .iter()
            .take(HighScores::MAX)
            .enumerate()
            .map(|(i, e)| Row {
                rank: RANKS.get(i).copied().unwrap_or(""),
                name: e.name.clone(),
                score: e.score.to_string(),
                level: e.level.to_string(),
            })
            .collect();
        ScoresState {
            highlight,
            t: 0.0,
            from_game,
            footer: your_score.map(|s| format!("YOUR SCORE  {s}")),
            rows,
        }
    }

    /// Returns true when the player leaves the screen.
    pub fn update(&mut self, dt: f32, act: &Actions) -> bool {
        ui::tick_clock(&mut self.t, dt);
        // Ignore input for a moment so a key still held from the game (or the
        // name-entry Enter) does not skip the table instantly.
        self.t > 0.35 && (act.confirm || act.back || act.click)
    }

    pub fn draw(&self, l: &Layout) {
        let t = self.t;
        ui::heading(l, "HIGH SCORES", 2.2, pal::AMBER, t);
        let hy = 6.3;
        ui::text(l, "RANK", RANK_R, hy, ui::TINY, pal::FAINT, Align::Right);
        ui::text(l, "NAME", NAME_X, hy, ui::TINY, pal::FAINT, Align::Left);
        ui::text(l, "SCORE", SCORE_R, hy, ui::TINY, pal::FAINT, Align::Right);
        ui::text(l, "LEVEL", LEVEL_R, hy, ui::TINY, pal::FAINT, Align::Right);

        for (i, row) in self.rows.iter().enumerate() {
            let appear = ui::ease_out(ui::progress(t, 0.05 + i as f32 * 0.06, 0.35));
            if appear <= 0.0 {
                continue;
            }
            let y = ROW_Y0 + i as f32 * ROW_SPACING;
            let slide = (1.0 - appear) * 1.5;
            let highlighted = self.highlight == Some(i);
            let base = match i {
                0 => pal::AMBER,
                1 | 2 => ui::mix(pal::AMBER, pal::INK, 0.6),
                _ => pal::INK,
            };
            if highlighted {
                let p = ui::pulse(t, 1.4);
                let c = ui::mix(pal::MINT, pal::INK, 0.35 * p);
                let band = macroquad::math::Rect::new(1.0, y - 0.45, 25.0, ROW_SIZE + 0.9);
                ui::rect(l, band, ui::alpha(pal::MINT, 0.07 + 0.05 * p));
                ui::marker(l, 1.9, y + ROW_SIZE * 0.5, 0.36, t);
                ui::text_glow(l, row.rank, RANK_R + slide, y, ROW_SIZE, c, Align::Right);
                ui::text_glow(l, &row.name, NAME_X + slide, y, ROW_SIZE, c, Align::Left);
                ui::text_glow(l, &row.score, SCORE_R + slide, y, ROW_SIZE, c, Align::Right);
                ui::text_glow(l, &row.level, LEVEL_R + slide, y, ROW_SIZE, c, Align::Right);
            } else {
                let c = ui::alpha(base, appear);
                let dim = ui::alpha(pal::DIM, appear);
                ui::text(l, row.rank, RANK_R + slide, y, ROW_SIZE, dim, Align::Right);
                ui::text(l, &row.name, NAME_X + slide, y, ROW_SIZE, c, Align::Left);
                ui::text(l, &row.score, SCORE_R + slide, y, ROW_SIZE, c, Align::Right);
                ui::text(
                    l,
                    &row.level,
                    LEVEL_R + slide,
                    y,
                    ROW_SIZE,
                    dim,
                    Align::Right,
                );
            }
        }
        if self.rows.is_empty() {
            ui::text(
                l,
                "NO SCORES YET",
                ui::CX,
                15.0,
                ui::BODY,
                pal::DIM,
                Align::Center,
            );
        }
        if let Some(f) = &self.footer {
            ui::text_glow(l, f, ui::CX, 28.4, ui::BODY, pal::VIOLET, Align::Center);
        }
        ui::hint(l, "ENTER OR ESC TO CONTINUE", 33.6);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_preformatted_from_the_table() {
        let table = HighScores::defaults();
        let st = ScoresState::new(&table, Some(2), Some(1234), true);
        assert_eq!(st.rows.len(), table.entries.len().min(HighScores::MAX));
        assert_eq!(st.rows[0].rank, "1ST");
        assert_eq!(st.rows[0].score, table.entries[0].score.to_string());
        assert_eq!(st.footer.as_deref(), Some("YOUR SCORE  1234"));
    }

    #[test]
    fn leaving_waits_for_a_short_grace_period() {
        let mut st = ScoresState::new(&HighScores::defaults(), None, None, false);
        let ok = Actions {
            confirm: true,
            ..Actions::default()
        };
        assert!(!st.update(0.1, &ok));
        assert!(st.update(0.3, &ok));
    }
}
