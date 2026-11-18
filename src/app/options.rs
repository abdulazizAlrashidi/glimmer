//! The OPTIONS screen: item model, value adjustment (pure, tested) and drawing.

use super::menu::Menu;
use super::ui::{self, pal};
use crate::config::CANVAS_W;
use crate::font::Align;
use crate::input::Actions;
use crate::save::Settings;
use crate::view::Layout;
use macroquad::math::{Rect, Vec2};

/// Selectable extra-life thresholds (0 = off).
pub const EXTRA_LIFE_STEPS: [u32; 5] = [0, 10_000, 15_000, 20_000, 30_000];
/// Inclusive range of starting lives.
pub const LIVES_RANGE: (u8, u8) = (1, 5);
/// Volume steps.
pub const VOLUME_MAX: u8 = 10;

/// One row of the options screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptItem {
    Master,
    Music,
    Sfx,
    Fullscreen,
    Shake,
    Scanlines,
    ShowFps,
    Lives,
    ExtraLife,
    ResetScores,
    Back,
}

impl OptItem {
    pub const ALL: [OptItem; 11] = [
        OptItem::Master,
        OptItem::Music,
        OptItem::Sfx,
        OptItem::Fullscreen,
        OptItem::Shake,
        OptItem::Scanlines,
        OptItem::ShowFps,
        OptItem::Lives,
        OptItem::ExtraLife,
        OptItem::ResetScores,
        OptItem::Back,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            OptItem::Master => "MASTER VOLUME",
            OptItem::Music => "MUSIC VOLUME",
            OptItem::Sfx => "SFX VOLUME",
            OptItem::Fullscreen => "FULLSCREEN",
            OptItem::Shake => "SCREEN SHAKE",
            OptItem::Scanlines => "CRT SCANLINES",
            OptItem::ShowFps => "SHOW FPS",
            OptItem::Lives => "STARTING LIVES",
            OptItem::ExtraLife => "EXTRA LIFE",
            OptItem::ResetScores => "RESET HIGH SCORES",
            OptItem::Back => "BACK",
        }
    }

    /// One-line explanation shown under the list.
    pub const fn help(self) -> &'static str {
        match self {
            OptItem::Master => "OVERALL LOUDNESS OF THE GAME",
            OptItem::Music => "TITLE MUSIC, JINGLES AND INTERLUDES",
            OptItem::Sfx => "MOTES, FLARES, MENUS AND THE GLOOM",
            OptItem::Fullscreen => "ALSO F11 OR ALT+ENTER AT ANY TIME",
            OptItem::Shake => "CAMERA SHAKE ON BIG MOMENTS",
            OptItem::Scanlines => "SUBTLE RETRO SCANLINES AND VIGNETTE",
            OptItem::ShowFps => "FRAME RATE IN THE CORNER",
            OptItem::Lives => "LIVES AT THE START OF A NEW GAME",
            OptItem::ExtraLife => "SCORE THAT AWARDS ONE BONUS LIFE",
            OptItem::ResetScores => "RESTORE THE DEFAULT HIGH SCORE TABLE",
            OptItem::Back => "SAVE AND RETURN",
        }
    }

    /// Items adjusted with Left/Right.
    pub const fn is_adjustable(self) -> bool {
        !matches!(self, OptItem::ResetScores | OptItem::Back)
    }

    const fn volume(self) -> bool {
        matches!(self, OptItem::Master | OptItem::Music | OptItem::Sfx)
    }
}

/// Next extra-life threshold in [`EXTRA_LIFE_STEPS`] from `cur` in direction
/// `delta` (clamped at the ends unless `wrap`). Values not in the table snap
/// to the nearest step in the requested direction.
pub fn step_extra_life(cur: u32, delta: i32, wrap: bool) -> u32 {
    let n = EXTRA_LIFE_STEPS.len();
    let idx = match EXTRA_LIFE_STEPS.iter().position(|&s| s == cur) {
        Some(i) => i as i32 + delta.signum(),
        None if delta >= 0 => EXTRA_LIFE_STEPS
            .iter()
            .position(|&s| s > cur)
            .map_or(n as i32, |i| i as i32),
        None => EXTRA_LIFE_STEPS
            .iter()
            .rposition(|&s| s < cur)
            .map_or(-1, |i| i as i32),
    };
    let idx = if wrap {
        idx.rem_euclid(n as i32)
    } else {
        idx.clamp(0, n as i32 - 1)
    };
    EXTRA_LIFE_STEPS[idx as usize]
}

fn step_u8(v: u8, delta: i32, lo: u8, hi: u8, wrap: bool) -> u8 {
    let v = i32::from(v.clamp(lo, hi)) + delta.signum();
    let (lo, hi) = (i32::from(lo), i32::from(hi));
    let v = if wrap {
        if v > hi {
            lo
        } else if v < lo {
            hi
        } else {
            v
        }
    } else {
        v.clamp(lo, hi)
    };
    // Clamped into a u8 range above.
    v as u8
}

/// New value of an ON/OFF setting: Enter (`wrap`) toggles it, Left turns it
/// OFF and Right turns it ON. Left/Right auto-repeat while held, so they must
/// *set* the value; toggling would flip it back and forth (and flip the real
/// window in and out of fullscreen several times a second).
fn step_bool(v: bool, delta: i32, wrap: bool) -> bool {
    if wrap {
        !v
    } else {
        match delta.signum() {
            -1 => false,
            1 => true,
            _ => v,
        }
    }
}

/// Change `item` by one step (`delta` < 0 = left, > 0 = right; `wrap` cycles
/// at the ends, used when activating with Enter). ON/OFF items follow
/// [`step_bool`]. Returns whether anything changed.
pub fn adjust(s: &mut Settings, item: OptItem, delta: i32, wrap: bool) -> bool {
    let before = s.clone();
    match item {
        OptItem::Master => s.master_volume = step_u8(s.master_volume, delta, 0, VOLUME_MAX, wrap),
        OptItem::Music => s.music_volume = step_u8(s.music_volume, delta, 0, VOLUME_MAX, wrap),
        OptItem::Sfx => s.sfx_volume = step_u8(s.sfx_volume, delta, 0, VOLUME_MAX, wrap),
        OptItem::Fullscreen => s.fullscreen = step_bool(s.fullscreen, delta, wrap),
        OptItem::Shake => s.screen_shake = step_bool(s.screen_shake, delta, wrap),
        OptItem::Scanlines => s.scanlines = step_bool(s.scanlines, delta, wrap),
        OptItem::ShowFps => s.show_fps = step_bool(s.show_fps, delta, wrap),
        OptItem::Lives => {
            s.start_lives = step_u8(s.start_lives, delta, LIVES_RANGE.0, LIVES_RANGE.1, wrap)
        }
        OptItem::ExtraLife => s.extra_life_at = step_extra_life(s.extra_life_at, delta, wrap),
        OptItem::ResetScores | OptItem::Back => {}
    }
    *s != before
}

/// Where the options screen returns to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Back {
    Title,
    Pause,
}

/// State of an open options screen.
#[derive(Clone, Debug)]
pub struct OptionsState {
    pub menu: Menu,
    pub back: Back,
    /// Reset was pressed once; the next press confirms.
    pub reset_armed: bool,
    /// Settings changed since the screen opened (save on leave).
    pub dirty: bool,
    /// Feedback line after a reset (seconds remaining).
    pub reset_done: f32,
    pub t: f32,
}

/// What the app must do after a frame of options input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsAction {
    None,
    Moved,
    /// A setting changed: apply it live.
    Changed,
    /// Reset armed (play a warning blip).
    Armed,
    /// Reset confirmed: restore and save the default table.
    ResetScores,
    Leave,
}

/// Row geometry (canvas tiles). Every value (text or volume bar) is
/// right-aligned to `VALUE_R`; the selection band is centred on the canvas
/// and wide enough for the marker on the left and the value arrows on the
/// right.
const ROW_Y0: f32 = 6.8;
const ROW_SPACING: f32 = 1.9;
const LABEL_X: f32 = 2.3;
const VALUE_R: f32 = 24.3;
const BAR_W: f32 = 8.6;
const BAR_X: f32 = VALUE_R - BAR_W;
const ROW_SIZE: f32 = ui::BODY;
/// Gap between a value and each of its arrow hints.
const ARROW_GAP: f32 = 0.45;
/// Selection band: from just left of the marker, symmetric about the centre.
const BAND_X: f32 = LABEL_X - 1.6;
const BAND_W: f32 = CANVAS_W - 2.0 * BAND_X;
/// Centre of the selection marker (left of the label).
const MARKER_X: f32 = LABEL_X - 0.95;
const MARKER_R: f32 = 0.32;

impl OptionsState {
    pub fn new(back: Back) -> OptionsState {
        OptionsState {
            menu: Menu::new(OptItem::ALL.len()),
            back,
            reset_armed: false,
            dirty: false,
            reset_done: 0.0,
            t: 0.0,
        }
    }

    pub fn item(&self) -> OptItem {
        OptItem::ALL
            .get(self.menu.selected)
            .copied()
            .unwrap_or(OptItem::Back)
    }

    fn row_y(i: usize) -> f32 {
        ROW_Y0 + i as f32 * ROW_SPACING
    }

    /// Hover / click area of row `i` (the full band width).
    fn row_rect(i: usize) -> Rect {
        Rect::new(BAND_X, Self::row_y(i) - 0.55, BAND_W, ROW_SPACING)
    }

    /// The highlight drawn behind the selected row `i`.
    fn band_rect(i: usize) -> Rect {
        Rect::new(BAND_X, Self::row_y(i) - 0.38, BAND_W, ROW_SIZE + 0.76)
    }

    fn hover(p: Vec2) -> Option<usize> {
        (0..OptItem::ALL.len()).find(|&i| Self::row_rect(i).contains(p))
    }

    /// Handle one frame of input. `mouse` is the pointer in canvas tiles.
    pub fn update(
        &mut self,
        dt: f32,
        act: &Actions,
        mouse: Vec2,
        settings: &mut Settings,
    ) -> OptionsAction {
        ui::tick_clock(&mut self.t, dt);
        self.reset_done = (self.reset_done - dt).max(0.0);
        if act.back {
            return OptionsAction::Leave;
        }
        let hover = Self::hover(mouse);
        if act.click {
            let Some(i) = hover else {
                return OptionsAction::None;
            };
            if self.menu.select(i) {
                self.reset_armed = false;
            }
            let item = self.item();
            if item.volume() {
                if let Some(v) = ui::bar_hit(mouse.x, BAR_X, BAR_W, VOLUME_MAX) {
                    return self.set_volume(item, v, settings);
                }
            }
            return self.activate(item, settings);
        }
        let before = self.menu.selected;
        if act.up {
            self.menu.prev();
        } else if act.down {
            self.menu.next();
        } else if act.mouse_moved {
            if let Some(i) = hover {
                self.menu.select(i);
            }
        }
        if self.menu.selected != before {
            self.reset_armed = false;
            return OptionsAction::Moved;
        }

        let item = self.item();
        if act.confirm {
            return self.activate(item, settings);
        }
        let delta = if act.left {
            -1
        } else if act.right {
            1
        } else {
            0
        };
        if delta != 0 && item.is_adjustable() && adjust(settings, item, delta, false) {
            self.dirty = true;
            return OptionsAction::Changed;
        }
        OptionsAction::None
    }

    fn set_volume(&mut self, item: OptItem, v: u8, s: &mut Settings) -> OptionsAction {
        let slot = match item {
            OptItem::Master => &mut s.master_volume,
            OptItem::Music => &mut s.music_volume,
            _ => &mut s.sfx_volume,
        };
        if *slot == v {
            return OptionsAction::None;
        }
        *slot = v;
        self.dirty = true;
        OptionsAction::Changed
    }

    fn activate(&mut self, item: OptItem, settings: &mut Settings) -> OptionsAction {
        match item {
            OptItem::Back => OptionsAction::Leave,
            OptItem::ResetScores => {
                if self.reset_armed {
                    self.reset_armed = false;
                    self.reset_done = 2.0;
                    OptionsAction::ResetScores
                } else {
                    self.reset_armed = true;
                    OptionsAction::Armed
                }
            }
            _ => {
                if adjust(settings, item, 1, true) {
                    self.dirty = true;
                    OptionsAction::Changed
                } else {
                    OptionsAction::None
                }
            }
        }
    }

    /// Draw the screen. `muted`: the session mute (M / `--mute`) is on, so
    /// the volume rows explain why nothing is audible.
    pub fn draw(&self, l: &Layout, s: &Settings, muted: bool) {
        let t = self.t;
        ui::heading(l, "OPTIONS", 2.2, pal::MINT, t);
        for (i, &item) in OptItem::ALL.iter().enumerate() {
            let y = Self::row_y(i);
            let selected = i == self.menu.selected;
            let label_color = if selected { pal::MINT } else { pal::INK };
            if selected {
                ui::rect(l, Self::band_rect(i), ui::alpha(pal::MINT, 0.06));
                ui::text_glow(
                    l,
                    item.label(),
                    LABEL_X,
                    y,
                    ROW_SIZE,
                    label_color,
                    Align::Left,
                );
                ui::marker(l, MARKER_X, y + ROW_SIZE * 0.5, MARKER_R, t);
            } else {
                ui::text(
                    l,
                    item.label(),
                    LABEL_X,
                    y,
                    ROW_SIZE,
                    ui::alpha(label_color, 0.85),
                    Align::Left,
                );
            }
            self.draw_value(l, s, item, y, selected, muted);
        }
        let help_y = Self::row_y(OptItem::ALL.len()) + 0.6;
        let item = self.item();
        let (help, color) = if self.reset_done > 0.0 {
            ("HIGH SCORES RESET", pal::GREEN)
        } else if self.reset_armed && item == OptItem::ResetScores {
            ("PRESS ENTER AGAIN TO ERASE ALL SCORES", pal::CRIMSON)
        } else if muted && item.volume() {
            ("SOUND IS MUTED - PRESS M TO TURN IT BACK ON", pal::AMBER)
        } else {
            (item.help(), pal::DIM)
        };
        ui::text(l, help, ui::CX, help_y, ui::SMALL, color, Align::Center);
        ui::hint(
            l,
            "\u{2191}\u{2193} CHOOSE  \u{2190}\u{2192} CHANGE  ESC BACK",
            33.6,
        );
    }

    fn draw_value(
        &self,
        l: &Layout,
        s: &Settings,
        item: OptItem,
        y: f32,
        selected: bool,
        muted: bool,
    ) {
        let on_off = |b: bool| if b { "ON" } else { "OFF" };
        let value_color = if selected { pal::INK } else { pal::DIM };
        let text: &str = match item {
            OptItem::Master | OptItem::Music | OptItem::Sfx => {
                let v = match item {
                    OptItem::Master => s.master_volume,
                    OptItem::Music => s.music_volume,
                    _ => s.sfx_volume,
                };
                let (color, active) = if muted {
                    (pal::FAINT, false)
                } else {
                    (pal::MINT, selected)
                };
                ui::segmented_bar(
                    l,
                    BAR_X,
                    y - 0.05,
                    BAR_W,
                    ROW_SIZE + 0.1,
                    v.min(VOLUME_MAX),
                    VOLUME_MAX,
                    color,
                    active,
                );
                if muted {
                    ui::text_mid(
                        l,
                        MUTED_LABEL,
                        BAR_X - ARROW_GAP,
                        y + ROW_SIZE * 0.5,
                        ui::SMALL,
                        ui::alpha(pal::CRIMSON, if selected { 1.0 } else { 0.75 }),
                        Align::Right,
                    );
                }
                return;
            }
            OptItem::Fullscreen => on_off(s.fullscreen),
            OptItem::Shake => on_off(s.screen_shake),
            OptItem::Scanlines => on_off(s.scanlines),
            OptItem::ShowFps => on_off(s.show_fps),
            OptItem::Lives => lives_label(s.start_lives),
            OptItem::ExtraLife => extra_life_label(s.extra_life_at),
            OptItem::ResetScores => {
                if self.reset_armed {
                    "CONFIRM?"
                } else {
                    ""
                }
            }
            OptItem::Back => "",
        };
        if text.is_empty() {
            return;
        }
        let color = match (item, text) {
            (OptItem::ResetScores, _) => pal::CRIMSON,
            (_, "OFF") => ui::alpha(value_color, 0.7),
            (_, "ON") if selected => pal::MINT,
            _ => value_color,
        };
        if selected && item.is_adjustable() {
            // Arrow hints around the value, the same size and gap each side.
            let w = ui::width(text, ROW_SIZE);
            let arrow = ui::alpha(pal::MINT, 0.8);
            let (left, right) = arrow_xs(w);
            ui::text(l, "\u{2190}", left, y, ROW_SIZE, arrow, Align::Right);
            ui::text(l, "\u{2192}", right, y, ROW_SIZE, arrow, Align::Left);
        }
        ui::text(l, text, VALUE_R, y, ROW_SIZE, color, Align::Right);
    }
}

/// Shown beside each volume bar while the session mute is on.
const MUTED_LABEL: &str = "MUTED (M)";

/// Anchor x of the left arrow (right-aligned) and the right arrow
/// (left-aligned) around a value `w` tiles wide that ends at `VALUE_R`.
fn arrow_xs(w: f32) -> (f32, f32) {
    (VALUE_R - w - ARROW_GAP, VALUE_R + ARROW_GAP)
}

/// Static label for a lives count.
fn lives_label(n: u8) -> &'static str {
    match n {
        0 | 1 => "1",
        2 => "2",
        3 => "3",
        4 => "4",
        _ => "5",
    }
}

/// Static label for an extra-life threshold.
fn extra_life_label(at: u32) -> &'static str {
    match at {
        0 => "OFF",
        10_000 => "10000",
        15_000 => "15000",
        20_000 => "20000",
        30_000 => "30000",
        // Hand-edited settings files may hold other values.
        a if a < 10_000 => "5000+",
        a if a < 50_000 => "CUSTOM",
        _ => "50000+",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_life_steps_clamp_and_wrap() {
        assert_eq!(step_extra_life(0, 1, false), 10_000);
        assert_eq!(step_extra_life(30_000, 1, false), 30_000);
        assert_eq!(step_extra_life(30_000, 1, true), 0);
        assert_eq!(step_extra_life(0, -1, false), 0);
        assert_eq!(step_extra_life(0, -1, true), 30_000);
        // Off-table values snap in the requested direction.
        assert_eq!(step_extra_life(12_000, 1, false), 15_000);
        assert_eq!(step_extra_life(12_000, -1, false), 10_000);
        assert_eq!(step_extra_life(99_000, 1, false), 30_000);
        assert_eq!(step_extra_life(99_000, -1, false), 30_000);
        assert_eq!(step_extra_life(5_000, -1, false), 0);
    }

    #[test]
    fn adjust_volume_and_lives() {
        let mut s = Settings {
            master_volume: 10,
            start_lives: 1,
            ..Settings::default()
        };
        assert!(!adjust(&mut s, OptItem::Master, 1, false));
        assert!(adjust(&mut s, OptItem::Master, 1, true));
        assert_eq!(s.master_volume, 0);
        assert!(!adjust(&mut s, OptItem::Master, -1, false));
        assert!(adjust(&mut s, OptItem::Lives, -1, true));
        assert_eq!(s.start_lives, 5);
        assert!(!adjust(&mut s, OptItem::Lives, 1, false));
        assert!(!adjust(&mut s, OptItem::Back, 1, true));
        assert!(!adjust(&mut s, OptItem::ResetScores, 1, true));
    }

    #[test]
    fn left_sets_off_right_sets_on_and_enter_toggles() {
        let mut s = Settings {
            fullscreen: false,
            screen_shake: true,
            ..Settings::default()
        };
        assert!(!adjust(&mut s, OptItem::Fullscreen, -1, false));
        assert!(!s.fullscreen);
        assert!(adjust(&mut s, OptItem::Fullscreen, 1, false));
        assert!(s.fullscreen);
        assert!(!adjust(&mut s, OptItem::Fullscreen, 1, false));
        assert!(s.fullscreen);
        assert!(adjust(&mut s, OptItem::Fullscreen, 1, true));
        assert!(!s.fullscreen);
        assert!(adjust(&mut s, OptItem::Shake, -1, false));
        assert!(!s.screen_shake);
        assert!(!adjust(&mut s, OptItem::Shake, 0, false));
        for item in [OptItem::Scanlines, OptItem::ShowFps] {
            adjust(&mut s, item, -1, false);
            assert!(adjust(&mut s, item, 1, false));
            assert!(!adjust(&mut s, item, 1, false));
        }
    }

    #[test]
    fn holding_an_arrow_on_a_toggle_row_changes_it_once() {
        use crate::input::KeyRepeat;
        let far = Vec2::new(-10.0, -10.0);
        let dt = 1.0 / 60.0;
        for item in [OptItem::Fullscreen, OptItem::Shake, OptItem::ShowFps] {
            let row = OptItem::ALL.iter().position(|&i| i == item).unwrap();
            for (right, start) in [(true, false), (false, true)] {
                let mut st = OptionsState::new(Back::Title);
                st.menu.select(row);
                let mut s = Settings {
                    fullscreen: start,
                    screen_shake: start,
                    show_fps: start,
                    ..Settings::default()
                };
                let mut rep = KeyRepeat::new();
                let mut changes = 0;
                // Two seconds of a held key, auto-repeat included.
                for f in 0..120 {
                    let fire = rep.update(f == 0, true, dt);
                    let act = Actions {
                        left: fire && !right,
                        right: fire && right,
                        ..Actions::default()
                    };
                    if st.update(dt, &act, far, &mut s) == OptionsAction::Changed {
                        changes += 1;
                    }
                }
                assert_eq!(changes, 1, "{item:?} right={right}");
                let now = match item {
                    OptItem::Fullscreen => s.fullscreen,
                    OptItem::Shake => s.screen_shake,
                    _ => s.show_fps,
                };
                assert_eq!(now, right, "{item:?}");
            }
        }
    }

    #[test]
    fn rows_line_up() {
        // Volume bars end on the same right edge as every other value.
        assert!((BAR_X + BAR_W - VALUE_R).abs() < 1e-4);
        for i in 0..OptItem::ALL.len() {
            let band = OptionsState::band_rect(i);
            // Centred band that holds the marker and both value arrows.
            assert!((band.x + band.w * 0.5 - ui::CX).abs() < 1e-4);
            assert!(band.x <= MARKER_X - MARKER_R);
            let w = ui::width("10000", ROW_SIZE);
            let (left, right) = arrow_xs(w);
            let arrow_w = ui::width("\u{2192}", ROW_SIZE);
            assert!((VALUE_R - w - left - (right - VALUE_R)).abs() < 1e-4);
            assert!(right + arrow_w <= band.x + band.w);
            assert!(left - arrow_w >= LABEL_X);
            // The click area matches the visible band.
            let r = OptionsState::row_rect(i);
            assert_eq!((r.x, r.w), (band.x, band.w));
        }
        // The muted label fits between the longest volume label and its bar.
        let label_end = [OptItem::Master, OptItem::Music, OptItem::Sfx]
            .iter()
            .map(|i| LABEL_X + ui::width(i.label(), ROW_SIZE))
            .fold(0.0, f32::max);
        assert!(label_end + 0.3 < BAR_X - ARROW_GAP - ui::width(MUTED_LABEL, ui::SMALL));
    }

    #[test]
    fn reset_requires_confirmation_and_moving_disarms() {
        let mut st = OptionsState::new(Back::Title);
        let mut s = Settings::default();
        let far = Vec2::new(-10.0, -10.0);
        st.menu.select(
            OptItem::ALL
                .iter()
                .position(|&i| i == OptItem::ResetScores)
                .unwrap(),
        );
        let enter = Actions {
            confirm: true,
            ..Actions::default()
        };
        assert_eq!(st.update(0.0, &enter, far, &mut s), OptionsAction::Armed);
        let down = Actions {
            down: true,
            ..Actions::default()
        };
        assert_eq!(st.update(0.0, &down, far, &mut s), OptionsAction::Moved);
        assert!(!st.reset_armed);
        let up = Actions {
            up: true,
            ..Actions::default()
        };
        st.update(0.0, &up, far, &mut s);
        assert_eq!(st.update(0.0, &enter, far, &mut s), OptionsAction::Armed);
        assert_eq!(
            st.update(0.0, &enter, far, &mut s),
            OptionsAction::ResetScores
        );
        assert!(!st.dirty);
    }

    #[test]
    fn left_right_change_and_mark_dirty() {
        let mut st = OptionsState::new(Back::Pause);
        let mut s = Settings {
            master_volume: 5,
            ..Settings::default()
        };
        let far = Vec2::new(-10.0, -10.0);
        let right = Actions {
            right: true,
            ..Actions::default()
        };
        assert_eq!(st.update(0.0, &right, far, &mut s), OptionsAction::Changed);
        assert_eq!(s.master_volume, 6);
        assert!(st.dirty);
        let back = Actions {
            back: true,
            ..Actions::default()
        };
        assert_eq!(st.update(0.0, &back, far, &mut s), OptionsAction::Leave);
    }

    #[test]
    fn clicking_a_volume_bar_sets_the_value() {
        let mut st = OptionsState::new(Back::Title);
        let mut s = Settings {
            music_volume: 2,
            ..Settings::default()
        };
        let row = OptItem::ALL
            .iter()
            .position(|&i| i == OptItem::Music)
            .unwrap();
        let p = Vec2::new(BAR_X + BAR_W - 0.1, OptionsState::row_y(row) + 0.2);
        let click = Actions {
            click: true,
            ..Actions::default()
        };
        assert_eq!(st.update(0.0, &click, p, &mut s), OptionsAction::Changed);
        assert_eq!(s.music_volume, 10);
        assert_eq!(st.menu.selected, row);
    }

    #[test]
    fn labels_exist_for_every_value() {
        for n in 0..=10 {
            assert!(!lives_label(n).is_empty());
        }
        for at in [0, 1, 5_000, 10_000, 12_345, 15_000, 20_000, 30_000, 99_999] {
            assert!(!extra_life_label(at).is_empty());
        }
    }
}
