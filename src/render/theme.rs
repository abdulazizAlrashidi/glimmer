//! Colour themes. Each maze has its own palette; later loops shift hue.
//!
//! Four hand-tuned palettes share one mood ("neon garden at night"): a near
//! black, slightly tinted backdrop, glowing neon wall tubes, warm or cold
//! points of light for the dots.
//!
//! | maze | palette  | walls / glow                 | door        | backdrop     |
//! |------|----------|------------------------------|-------------|--------------|
//! | 0    | GARDEN   | luminous jade / deep emerald | honey gold  | green-black  |
//! | 1    | CRYSTAL  | ice blue / indigo violet     | aqua        | violet-black |
//! | 2    | MOONLIT  | silver / cerulean            | pale gold   | indigo-black |
//! | 3    | EMBER    | hot rose / vermilion         | sky cyan    | wine-black   |
//!
//! Shade signature colours (crimson, violet, amber, lime) and the frightened
//! blue are not part of a theme. Every vivid wall and wall-glow hue stays at
//! least 0.06 of a turn away from all of them, so the shades stay readable
//! against the walls.
//!
//! Once the player has seen all four mazes, later loops alternate each maze
//! between its base palette and a hand-picked hue-rotated variant (walls, glow,
//! backdrop and HUD labels; doors, motes and flares keep their colours). The
//! variants are picked from the hue bands that keep the walls clear of every
//! signature colour; a test checks every maze at every loop.

use macroquad::color::Color;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub wall: Color,
    pub wall_glow: Color,
    pub wall_flash: Color,
    pub door: Color,
    pub mote: Color,
    pub flare: Color,
    pub backdrop_top: Color,
    pub backdrop_bottom: Color,
    pub hud_label: Color,
    pub hud_value: Color,
}

/// Number of distinct base palettes.
pub const THEME_COUNT: usize = 4;

const fn rgb(r: f32, g: f32, b: f32) -> Color {
    Color { r, g, b, a: 1.0 }
}

/// Maze 0: jade hedges, a honey-gold gate and warm firefly motes on a deep
/// green-black night.
const GARDEN: Theme = Theme {
    wall: rgb(0.20, 0.92, 0.50),
    wall_glow: rgb(0.02, 0.62, 0.30),
    wall_flash: rgb(0.94, 1.0, 0.94),
    door: rgb(1.0, 0.76, 0.30),
    mote: rgb(1.0, 0.88, 0.58),
    flare: rgb(1.0, 0.86, 0.46),
    backdrop_top: rgb(0.012, 0.052, 0.034),
    backdrop_bottom: rgb(0.003, 0.016, 0.011),
    hud_label: rgb(0.55, 0.95, 0.66),
    hud_value: rgb(0.95, 1.0, 0.95),
};

/// Maze 1: ice blue / indigo-violet crystal.
const CRYSTAL: Theme = Theme {
    wall: rgb(0.56, 0.86, 1.0),
    wall_glow: rgb(0.40, 0.30, 1.0),
    wall_flash: rgb(1.0, 1.0, 1.0),
    door: rgb(0.40, 1.0, 0.94),
    mote: rgb(0.80, 0.94, 1.0),
    flare: rgb(0.78, 0.90, 1.0),
    backdrop_top: rgb(0.034, 0.026, 0.096),
    backdrop_bottom: rgb(0.008, 0.005, 0.030),
    hud_label: rgb(0.72, 0.62, 1.0),
    hud_value: rgb(0.95, 0.96, 1.0),
};

/// Maze 2: moonlit silver with a cerulean haze.
const MOONLIT: Theme = Theme {
    wall: rgb(0.80, 0.84, 0.98),
    wall_glow: rgb(0.18, 0.68, 1.0),
    wall_flash: rgb(1.0, 1.0, 1.0),
    door: rgb(1.0, 0.84, 0.46),
    mote: rgb(1.0, 0.95, 0.80),
    flare: rgb(1.0, 0.92, 0.64),
    backdrop_top: rgb(0.030, 0.038, 0.110),
    backdrop_bottom: rgb(0.006, 0.008, 0.034),
    hud_label: rgb(0.62, 0.74, 1.0),
    hud_value: rgb(0.96, 0.96, 1.0),
};

/// Maze 3: hot rose walls with a vermilion ember glow.
const EMBER: Theme = Theme {
    wall: rgb(1.0, 0.42, 0.76),
    wall_glow: rgb(1.0, 0.36, 0.16),
    wall_flash: rgb(1.0, 0.98, 0.92),
    door: rgb(0.46, 0.90, 1.0),
    mote: rgb(1.0, 0.86, 0.72),
    flare: rgb(1.0, 0.80, 0.46),
    backdrop_top: rgb(0.080, 0.020, 0.052),
    backdrop_bottom: rgb(0.020, 0.004, 0.014),
    hud_label: rgb(1.0, 0.62, 0.48),
    hud_value: rgb(1.0, 0.97, 0.94),
};

const BASE: [Theme; THEME_COUNT] = [GARDEN, CRYSTAL, MOONLIT, EMBER];

/// Hue rotation (turns) of each palette, indexed by `loop_index % len`.
/// Entry 0 is the base palette. The variants: an orchid night garden, a
/// green-and-cyan crystal cave, a rose moon and a jade-and-cyan citadel.
const LOOP_TURNS: [&[f32]; THEME_COUNT] = [&[0.0, 0.455], &[0.0, 0.82], &[0.0, 0.30], &[0.0, 0.46]];

/// How many times the player has already been through the whole maze
/// rotation at `level` (see `config::maze_for_level`): 0 for levels 1-14,
/// then +1 every 16 levels.
pub fn loop_index(level: u32) -> u32 {
    if level < 15 {
        0
    } else {
        1 + (level - 15) / 16
    }
}

/// Hue rotation (fraction of a turn) applied to maze `maze_index`'s palette
/// at `level`.
pub fn loop_turns(maze_index: usize, level: u32) -> f32 {
    let turns = LOOP_TURNS[maze_index % THEME_COUNT];
    turns[loop_index(level) as usize % turns.len()]
}

/// Theme for a maze index and level.
pub fn theme_for(maze_index: usize, level: u32) -> Theme {
    let base = BASE[maze_index % THEME_COUNT];
    let turns = loop_turns(maze_index, level);
    if turns == 0.0 {
        return base;
    }
    Theme {
        wall: rotate_hue(base.wall, turns),
        wall_glow: rotate_hue(base.wall_glow, turns),
        backdrop_top: rotate_hue(base.backdrop_top, turns),
        backdrop_bottom: rotate_hue(base.backdrop_bottom, turns),
        hud_label: rotate_hue(base.hud_label, turns),
        ..base
    }
}

/// Rotate the hue of `c` by `turns` (fraction of a full circle), keeping
/// saturation, value and alpha.
pub fn rotate_hue(c: Color, turns: f32) -> Color {
    let (h, s, v) = rgb_to_hsv(c);
    let (r, g, b) = hsv_to_rgb((h + turns).rem_euclid(1.0), s, v);
    Color::new(r, g, b, c.a)
}

/// RGB to (hue in turns `[0, 1)`, saturation, value).
fn rgb_to_hsv(c: Color) -> (f32, f32, f32) {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let d = max - min;
    let h = if d <= f32::EPSILON {
        0.0
    } else if max == c.r {
        ((c.g - c.b) / d).rem_euclid(6.0) / 6.0
    } else if max == c.g {
        ((c.b - c.r) / d + 2.0) / 6.0
    } else {
        ((c.r - c.g) / d + 4.0) / 6.0
    };
    let s = if max <= f32::EPSILON { 0.0 } else { d / max };
    (h, s, max)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h6 = h.rem_euclid(1.0) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h6 % 2.0 - 1.0).abs());
    let (r, g, b) = match h6 as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (r + m, g + m, b + m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::sprites;
    use crate::shade::ShadeKind;

    /// Smallest hue distance (fraction of a turn) between a vivid wall colour
    /// and any shade signature colour or the frightened colour.
    const MIN_SIGNATURE_GAP: f32 = 0.06;

    /// Colours below this HSV saturation read as tinted white (e.g. MOONLIT's
    /// silver walls) and cannot be mistaken for a pursuer's colour.
    const VIVID_SATURATION: f32 = 0.3;

    /// Would `c` (a wall or wall-glow colour) be mistaken for a shade's
    /// signature colour or the frightened colour? True when `c` is vivid and
    /// its hue is within [`MIN_SIGNATURE_GAP`] of one of them.
    fn resembles_signature(c: Color) -> bool {
        let (h, s, _) = rgb_to_hsv(c);
        if s < VIVID_SATURATION {
            return false;
        }
        let mut signatures = ShadeKind::ALL
            .map(sprites::shade_color)
            .into_iter()
            .chain([sprites::fright_color(false)]);
        signatures.any(|sig| hue_gap(h, rgb_to_hsv(sig).0) < MIN_SIGNATURE_GAP)
    }

    /// Distance between two hues (turns), in `[0, 0.5]`.
    fn hue_gap(a: f32, b: f32) -> f32 {
        let d = (a - b).rem_euclid(1.0);
        d.min(1.0 - d)
    }

    fn close(a: Color, b: Color) -> bool {
        (a.r - b.r).abs() < 1e-4 && (a.g - b.g).abs() < 1e-4 && (a.b - b.b).abs() < 1e-4
    }

    fn hue(c: Color) -> f32 {
        rgb_to_hsv(c).0
    }

    fn all_colours(t: &Theme) -> [Color; 10] {
        [
            t.wall,
            t.wall_glow,
            t.wall_flash,
            t.door,
            t.mote,
            t.flare,
            t.backdrop_top,
            t.backdrop_bottom,
            t.hud_label,
            t.hud_value,
        ]
    }

    #[test]
    fn four_distinct_palettes_wrapping_by_index() {
        for i in 0..THEME_COUNT {
            for j in 0..i {
                let (a, b) = (theme_for(i, 1), theme_for(j, 1));
                assert_ne!(a.wall, b.wall);
                // Each pair differs clearly in wall hue or in glow hue.
                assert!(
                    hue_gap(hue(a.wall), hue(b.wall)) > 0.08
                        || hue_gap(hue(a.wall_glow), hue(b.wall_glow)) > 0.08,
                    "palettes {i} and {j} look alike"
                );
            }
            assert_eq!(theme_for(i, 1), theme_for(i + THEME_COUNT, 1));
        }
    }

    #[test]
    fn garden_has_its_own_identity() {
        let g = theme_for(0, 1);
        // Jade-green walls (between lime and Glim's mint), a warm gold door
        // and a green-tinted night backdrop.
        assert!((0.33..0.44).contains(&hue(g.wall)), "{}", hue(g.wall));
        assert!((0.05..0.17).contains(&hue(g.door)), "{}", hue(g.door));
        let b = g.backdrop_top;
        assert!(b.g > b.r && b.g > b.b);
        // Lime Moss stays a clearly different green from the hedges.
        let moss = sprites::shade_color(ShadeKind::Moss);
        assert!(hue_gap(hue(g.wall), hue(moss)) > 0.1);
        assert!(hue_gap(hue(g.wall), hue(sprites::glim_color())) > 0.04);
    }

    #[test]
    fn no_palette_pairs_light_blue_walls_with_a_pink_door() {
        for i in 0..THEME_COUNT {
            for level in [1, 14, 30, 46, 62] {
                let t = theme_for(i, level);
                let blue_walls = (0.5..0.64).contains(&hue(t.wall)) && rgb_to_hsv(t.wall).1 > 0.5;
                let pink_door = (0.83..0.98).contains(&hue(t.door));
                assert!(!(blue_walls && pink_door), "maze {i} level {level}");
            }
        }
    }

    #[test]
    fn walls_never_resemble_a_shade_or_the_fright_colour() {
        // Every maze, every loop (the variants cycle, so 0..400 covers all).
        for i in 0..THEME_COUNT {
            for level in 1..=400 {
                let t = theme_for(i, level);
                for (part, c) in [("wall", t.wall), ("wall_glow", t.wall_glow)] {
                    assert!(
                        !resembles_signature(c),
                        "maze {i} level {level}: {part} {c:?} (hue {}) too close to a signature colour",
                        hue(c)
                    );
                }
            }
        }
        // The check itself: a shade's own colour resembles it, pale silver
        // and a far hue do not.
        for k in ShadeKind::ALL {
            assert!(resembles_signature(sprites::shade_color(k)), "{k:?}");
        }
        assert!(resembles_signature(sprites::fright_color(false)));
        assert!(resembles_signature(rotate_hue(
            sprites::shade_color(ShadeKind::Rook),
            0.05
        )));
        assert!(!resembles_signature(rgb(0.80, 0.84, 0.98)));
        assert!(!resembles_signature(rgb(0.20, 0.92, 0.50)));
    }

    #[test]
    fn later_loops_shift_hue_but_keep_brightness() {
        assert_eq!(loop_index(1), 0);
        assert_eq!(loop_index(14), 0);
        assert_eq!(loop_index(15), 1);
        assert_eq!(loop_index(30), 1);
        assert_eq!(loop_index(31), 2);
        for i in 0..THEME_COUNT {
            let a = theme_for(i, 1);
            let b = theme_for(i, 20);
            assert_ne!(a.wall, b.wall);
            assert_ne!(a.wall_glow, b.wall_glow);
            let va = a.wall.r.max(a.wall.g).max(a.wall.b);
            let vb = b.wall.r.max(b.wall.g).max(b.wall.b);
            assert!((va - vb).abs() < 1e-4);
            // Gameplay colours stay put.
            assert_eq!((a.door, a.mote, a.flare), (b.door, b.mote, b.flare));
            // The cycle comes back to the base palette.
            assert_eq!(theme_for(i, 31), a);
            assert_eq!(loop_turns(i, 1), 0.0);
        }
    }

    #[test]
    fn hsv_roundtrip() {
        for c in [
            GARDEN.wall,
            CRYSTAL.wall_glow,
            EMBER.door,
            MOONLIT.mote,
            rgb(0.5, 0.5, 0.5),
        ] {
            assert!(close(rotate_hue(c, 0.0), c), "{c:?}");
            assert!(close(rotate_hue(rotate_hue(c, 0.3), 0.7), c), "{c:?}");
        }
        let red = rgb(1.0, 0.0, 0.0);
        assert!(close(rotate_hue(red, 1.0 / 3.0), rgb(0.0, 1.0, 0.0)));
        assert!((hue_gap(0.98, 0.02) - 0.04).abs() < 1e-6);
        assert!((hue_gap(0.1, 0.6) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn colours_are_in_range() {
        for i in 0..THEME_COUNT {
            for level in [1, 14, 30, 46, 200] {
                for c in all_colours(&theme_for(i, level)) {
                    for v in [c.r, c.g, c.b, c.a] {
                        assert!((0.0..=1.0 + 1e-5).contains(&v), "{c:?}");
                    }
                }
            }
        }
    }
}
