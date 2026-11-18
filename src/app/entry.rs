//! NEW HIGH SCORE name entry: three letter slots (pure model, tested) and its
//! screen.

use super::ui::{self, pal};
use crate::config::CANVAS_W;
use crate::font::Align;
use crate::geom::Dir;
use crate::input::Actions;
use crate::render::sprites;
use crate::save::{is_name_char, NAME_CHARSET, NAME_LEN};
use crate::shade::{ShadeKind, ShadeLook};
use crate::view::Layout;
use macroquad::math::Rect;

/// Seconds after the screen opens during which all input is ignored, so a
/// player still mashing Enter/Space to get past GAME OVER does not confirm
/// the pre-filled name (or type blanks) before seeing this screen.
pub const INPUT_GRACE: f32 = 0.45;

/// The editable name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameEntry {
    pub chars: [char; NAME_LEN],
    pub cursor: usize,
    /// The last action typed into the final slot (so Backspace clears it
    /// rather than stepping back).
    typed_last: bool,
}

impl NameEntry {
    /// Start from `initial` (sanitised; short names are padded with `A`).
    pub fn new(initial: &str) -> NameEntry {
        let mut chars = ['A'; NAME_LEN];
        for (slot, c) in chars.iter_mut().zip(initial.chars()) {
            let c = c.to_ascii_uppercase();
            *slot = if is_name_char(c) { c } else { 'A' };
        }
        NameEntry {
            chars,
            cursor: 0,
            typed_last: false,
        }
    }

    /// Step the character under the cursor through [`NAME_CHARSET`].
    pub fn cycle(&mut self, delta: i32) {
        let set: &[u8] = NAME_CHARSET.as_bytes();
        if set.is_empty() {
            return;
        }
        let cur = self.chars[self.cursor];
        let idx = set.iter().position(|&b| char::from(b) == cur).unwrap_or(0) as i32;
        let next = (idx + delta).rem_euclid(set.len() as i32) as usize;
        self.chars[self.cursor] = char::from(set[next]);
        self.typed_last = false;
    }

    /// Move the cursor (clamped). Returns whether it moved.
    pub fn move_cursor(&mut self, delta: i32) -> bool {
        let max = NAME_LEN as i32 - 1;
        let next = (self.cursor as i32 + delta).clamp(0, max) as usize;
        self.typed_last = false;
        let moved = next != self.cursor;
        self.cursor = next;
        moved
    }

    /// Type `c` into the current slot and advance. Returns false (and changes
    /// nothing) for characters a name may not contain.
    pub fn type_char(&mut self, c: char) -> bool {
        let c = c.to_ascii_uppercase();
        if !is_name_char(c) {
            return false;
        }
        self.chars[self.cursor] = c;
        if self.cursor + 1 < NAME_LEN {
            self.cursor += 1;
            self.typed_last = false;
        } else {
            self.typed_last = true;
        }
        true
    }

    /// Delete like a text field: clear the slot just typed, otherwise step
    /// back and clear that one.
    pub fn backspace(&mut self) {
        if self.typed_last {
            self.typed_last = false;
        } else if self.cursor > 0 {
            self.cursor -= 1;
        }
        self.chars[self.cursor] = ' ';
    }

    /// The final name; an all-blank name becomes `???`.
    pub fn name(&self) -> String {
        if self.chars.iter().all(|&c| c == ' ') {
            "???".to_owned()
        } else {
            self.chars.iter().collect()
        }
    }
}

/// The open entry screen.
#[derive(Clone, Debug)]
pub struct EntryState {
    pub entry: NameEntry,
    pub score: u32,
    pub level: u32,
    pub t: f32,
    score_label: String,
    rank_label: String,
}

/// Result of one frame on the entry screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryAction {
    None,
    /// A letter changed or the cursor moved (play a key tick).
    Edited,
    Confirmed,
}

impl EntryState {
    /// `rank` is the 0-based table position the score will take.
    pub fn new(initial_name: &str, score: u32, level: u32, rank: usize) -> EntryState {
        EntryState {
            entry: NameEntry::new(initial_name),
            score,
            level,
            t: 0.0,
            score_label: format!("SCORE  {score}"),
            rank_label: format!("RANK {}   LEVEL {level}", rank + 1),
        }
    }

    pub fn update(&mut self, dt: f32, act: &Actions, typed: &[char]) -> EntryAction {
        ui::tick_clock(&mut self.t, dt);
        if self.t < INPUT_GRACE {
            // Keys and characters that arrive now are dropped, not queued.
            return EntryAction::None;
        }
        if act.enter {
            return EntryAction::Confirmed;
        }
        let mut edited = false;
        for &c in typed {
            edited |= self.entry.type_char(c);
        }
        if act.backspace {
            self.entry.backspace();
            edited = true;
        }
        if act.arrow_up {
            self.entry.cycle(1);
            edited = true;
        }
        if act.arrow_down {
            self.entry.cycle(-1);
            edited = true;
        }
        if act.arrow_left {
            edited |= self.entry.move_cursor(-1);
        }
        if act.arrow_right {
            edited |= self.entry.move_cursor(1);
        }
        if edited {
            EntryAction::Edited
        } else {
            EntryAction::None
        }
    }

    pub fn draw(&self, l: &Layout) {
        let t = self.t;
        let pop = ui::ease_back(ui::progress(t, 0.0, 0.5));
        ui::text_glow(
            l,
            "NEW HIGH SCORE!",
            ui::CX,
            3.0,
            ui::H1 * (0.6 + 0.4 * pop),
            pal::AMBER,
            Align::Center,
        );
        ui::sparkle_field(l, Rect::new(2.0, 1.5, 23.0, 4.5), 18, 7, t, 0.9);

        ui::text(
            l,
            &self.score_label,
            ui::CX,
            7.0,
            ui::BODY,
            pal::INK,
            Align::Center,
        );
        ui::text(
            l,
            &self.rank_label,
            ui::CX,
            8.4,
            ui::SMALL,
            pal::DIM,
            Align::Center,
        );

        ui::text(
            l,
            "ENTER YOUR NAME",
            ui::CX,
            11.2,
            ui::BODY,
            pal::VIOLET,
            Align::Center,
        );

        // The three big slots.
        let size = 3.0;
        let pitch = 4.6;
        let y = 14.0;
        let mut buf = [0u8; 4];
        for (i, &c) in self.entry.chars.iter().enumerate() {
            let cx = ui::CX + (i as f32 - (NAME_LEN as f32 - 1.0) * 0.5) * pitch;
            let selected = i == self.entry.cursor;
            let glyph: &str = c.encode_utf8(&mut buf);
            let underline_y = y + size + 0.5;
            if selected {
                let p = ui::pulse(t, 1.6);
                ui::text_glow(
                    l,
                    glyph,
                    cx,
                    y,
                    size,
                    ui::mix(pal::MINT, pal::INK, 0.3 * p),
                    Align::Center,
                );
                ui::line(
                    l,
                    cx - 1.3,
                    underline_y,
                    cx + 1.3,
                    underline_y,
                    0.14,
                    ui::alpha(pal::MINT, 0.6 + 0.4 * p),
                );
                ui::text(
                    l,
                    "\u{2191}",
                    cx,
                    y - 1.3,
                    ui::SMALL,
                    ui::alpha(pal::MINT, 0.7),
                    Align::Center,
                );
                ui::text(
                    l,
                    "\u{2193}",
                    cx,
                    underline_y + 0.45,
                    ui::SMALL,
                    ui::alpha(pal::MINT, 0.7),
                    Align::Center,
                );
            } else {
                ui::text(l, glyph, cx, y, size, pal::INK, Align::Center);
                ui::line(
                    l,
                    cx - 1.3,
                    underline_y,
                    cx + 1.3,
                    underline_y,
                    0.08,
                    ui::alpha(pal::DIM, 0.6),
                );
            }
        }

        let hints = [
            "\u{2191}\u{2193} CHANGE LETTER",
            "\u{2190}\u{2192} MOVE",
            "OR JUST TYPE A-Z 0-9",
            "ENTER TO CONFIRM",
        ];
        for (i, h) in hints.iter().enumerate() {
            let color = if i == hints.len() - 1 {
                pal::MINT
            } else {
                pal::DIM
            };
            ui::text(
                l,
                h,
                ui::CX,
                21.4 + i as f32 * 1.3,
                ui::SMALL,
                color,
                Align::Center,
            );
        }
        draw_parade(l, t);
    }
}

/// Parade lane (canvas tiles) and motion.
const PARADE_Y: f32 = 28.6;
const PARADE_SPEED: f32 = 4.0;
const PARADE_R: f32 = 0.62;
/// Spacing between the fleeing shades, and how far Glim trails the last one.
const PARADE_GAP: f32 = 1.9;
const PARADE_GLIM_GAP: f32 = 2.5;
/// Actors grow in / shrink out over this many tiles at the canvas edges.
const PARADE_FADE: f32 = 1.5;
/// Extra run-out beyond each edge so the wrap happens off-stage.
const PARADE_MARGIN: f32 = 0.5;

/// One actor of the parade this frame. `scale` is 0 (hidden) at or beyond a
/// canvas edge and grows to 1 over [`PARADE_FADE`] tiles, so nothing ever
/// pops in or out mid-screen.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ParadeActor {
    /// `None` = Glim, otherwise the shade.
    shade: Option<ShadeKind>,
    x: f32,
    y: f32,
    scale: f32,
}

/// Distance from the lead shade back to Glim.
const PARADE_LEN: f32 = 3.0 * PARADE_GAP + PARADE_GLIM_GAP;

/// The celebratory parade at time `t`: Glim chasing the four frightened
/// shades left to right. The whole group crosses the canvas and leaves it
/// before the loop wraps.
fn parade_actors(t: f32) -> [ParadeActor; 5] {
    let span = CANVAS_W + PARADE_LEN + 2.0 * PARADE_MARGIN;
    let lead = (t.max(0.0) * PARADE_SPEED) % span - PARADE_MARGIN;
    let fade = |x: f32| (x.min(CANVAS_W - x) / PARADE_FADE).clamp(0.0, 1.0);
    let mut out = [ParadeActor {
        shade: None,
        x: lead - PARADE_LEN,
        y: PARADE_Y,
        scale: fade(lead - PARADE_LEN),
    }; 5];
    for (k, kind) in ShadeKind::ALL.iter().enumerate() {
        let x = lead - k as f32 * PARADE_GAP;
        out[k + 1] = ParadeActor {
            shade: Some(*kind),
            x,
            y: PARADE_Y + (t * 20.0 + k as f32).sin() * 0.06,
            scale: fade(x),
        };
    }
    out
}

/// Draw the parade along the bottom of the entry screen.
fn draw_parade(l: &Layout, t: f32) {
    for a in parade_actors(t) {
        if a.scale <= 0.0 {
            continue;
        }
        let c = l.canvas(a.x, a.y);
        let r = l.px(PARADE_R * a.scale);
        match a.shade {
            None => sprites::draw_glim(c, r, Dir::Right, t, a.scale),
            Some(kind) => {
                let look = ShadeLook::Frightened {
                    flash: ((t * 4.0) as i32) % 2 == 0,
                };
                sprites::draw_shade(c, r, kind, look, Dir::Right, t);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_fills_and_advances() {
        let mut e = NameEntry::new("");
        assert_eq!(e.name(), "AAA");
        assert!(e.type_char('b'));
        assert!(e.type_char('o'));
        assert!(e.type_char('B'));
        assert_eq!(e.name(), "BOB");
        assert_eq!(e.cursor, 2);
        assert!(!e.type_char('#'));
        assert!(!e.type_char('\u{f700}'));
        assert_eq!(e.name(), "BOB");
    }

    #[test]
    fn backspace_behaves_like_a_text_field() {
        let mut e = NameEntry::new("");
        for c in "ZED".chars() {
            e.type_char(c);
        }
        e.backspace();
        assert_eq!(e.name(), "ZE ");
        assert_eq!(e.cursor, 2);
        e.backspace();
        assert_eq!(e.name(), "Z  ");
        assert_eq!(e.cursor, 1);
        e.backspace();
        e.backspace();
        assert_eq!(e.cursor, 0);
        assert_eq!(e.name(), "???");
    }

    #[test]
    fn cycling_wraps_through_the_charset() {
        let mut e = NameEntry::new("A");
        e.cycle(-1);
        let last = NAME_CHARSET.chars().last().unwrap();
        assert_eq!(e.chars[0], last);
        e.cycle(1);
        assert_eq!(e.chars[0], 'A');
        e.cycle(1);
        assert_eq!(e.chars[0], 'B');
        // Unknown characters cycle from the start of the set.
        e.chars[0] = '#';
        e.cycle(1);
        assert_eq!(e.chars[0], 'B');
    }

    #[test]
    fn cursor_is_clamped() {
        let mut e = NameEntry::new("ABC");
        assert!(!e.move_cursor(-1));
        assert!(e.move_cursor(5));
        assert_eq!(e.cursor, NAME_LEN - 1);
    }

    #[test]
    fn invalid_initial_characters_are_replaced() {
        let e = NameEntry::new("a#zq");
        assert_eq!(e.name(), "AAZ");
    }

    #[test]
    fn screen_confirms_on_enter_only() {
        let mut st = EntryState::new("ABC", 10, 1, 0);
        assert_eq!(
            st.update(INPUT_GRACE, &Actions::default(), &[]),
            EntryAction::None
        );
        let space = Actions {
            confirm: true,
            ..Actions::default()
        };
        assert_eq!(st.update(0.0, &space, &[' ']), EntryAction::Edited);
        assert_eq!(st.entry.name(), " BC");
        let enter = Actions {
            enter: true,
            confirm: true,
            ..Actions::default()
        };
        assert_eq!(st.update(0.0, &enter, &[]), EntryAction::Confirmed);
        let up = Actions {
            arrow_up: true,
            ..Actions::default()
        };
        assert_eq!(st.update(0.0, &up, &[]), EntryAction::Edited);
    }

    #[test]
    fn input_is_ignored_while_the_screen_opens() {
        let mut st = EntryState::new("ZED", 10, 1, 0);
        let enter = Actions {
            enter: true,
            confirm: true,
            backspace: true,
            arrow_up: true,
            arrow_right: true,
            ..Actions::default()
        };
        let dt = 1.0 / 60.0;
        // Mashing Enter / Space right after skipping GAME OVER does nothing.
        while st.t + dt < INPUT_GRACE {
            assert_eq!(st.update(dt, &enter, &[' ', 'Q']), EntryAction::None);
        }
        assert_eq!(st.entry.name(), "ZED");
        assert_eq!(st.entry.cursor, 0);
        st.update(dt, &Actions::default(), &[]);
        assert_eq!(st.update(dt, &enter, &[]), EntryAction::Confirmed);
    }

    #[test]
    fn parade_never_pops_in_or_out_mid_screen() {
        let dt = 1.0 / 60.0;
        let mut prev = parade_actors(0.0);
        let mut all_visible = false;
        for f in 1..(60 * 40) {
            let t = f as f32 * dt;
            let now = parade_actors(t);
            all_visible |= now.iter().all(|a| a.scale >= 1.0);
            for (a, b) in prev.iter().zip(now.iter()) {
                assert!(b.x.is_finite() && b.y.is_finite());
                if !(0.0..=CANVAS_W).contains(&b.x) {
                    assert_eq!(b.scale, 0.0, "t {t}: {b:?}");
                }
                // Sizes change gradually; a visible actor never teleports.
                assert!((a.scale - b.scale).abs() < 0.1, "t {t}: {a:?} -> {b:?}");
                if a.scale > 0.0 && b.scale > 0.0 {
                    assert!((a.x - b.x).abs() < 0.2, "t {t}: {a:?} -> {b:?}");
                }
            }
            prev = now;
        }
        assert!(all_visible, "the whole parade is on screen at some point");
    }
}
