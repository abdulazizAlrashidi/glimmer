//! Game-design constants and per-level difficulty tables.
//!
//! All speeds are expressed as a fraction of [`BASE_SPEED`] (tiles/second).
//! All durations are in seconds of simulation time.

use crate::geom::Tile;

// ---------------------------------------------------------------------------
// Board & canvas
// ---------------------------------------------------------------------------

/// Maze width in tiles (odd, so the maze has a true centre column).
pub const MAZE_W: i32 = 27;
/// Maze height in tiles.
pub const MAZE_H: i32 = 31;

/// HUD rows above the maze on the virtual canvas.
pub const HUD_TOP_ROWS: f32 = 3.0;
/// HUD rows below the maze on the virtual canvas.
pub const HUD_BOTTOM_ROWS: f32 = 2.0;
/// Virtual canvas size in tiles (the renderer letterboxes this into the window).
pub const CANVAS_W: f32 = MAZE_W as f32;
pub const CANVAS_H: f32 = HUD_TOP_ROWS + MAZE_H as f32 + HUD_BOTTOM_ROWS;

// ---------------------------------------------------------------------------
// Simulation clock
// ---------------------------------------------------------------------------

/// Fixed simulation rate. The app runs `Game::update(SIM_DT, ..)` this many
/// times per second regardless of display refresh rate and interpolates.
pub const SIM_HZ: f64 = 120.0;
pub const SIM_DT: f32 = (1.0 / SIM_HZ) as f32;
/// Upper bound on wall-clock time consumed in one frame (avoids a spiral of
/// death after the window was dragged or the machine slept).
pub const MAX_FRAME_TIME: f64 = 0.25;

// ---------------------------------------------------------------------------
// Movement
// ---------------------------------------------------------------------------

/// 100% speed in tiles per second.
pub const BASE_SPEED: f32 = 9.0;
/// Player/shade centres closer than this (in tiles) collide.
pub const COLLIDE_DIST: f32 = 0.6;
/// A tapped direction is remembered this long while waiting for an opening.
pub const INPUT_BUFFER_TIME: f32 = 0.30;
/// The player may start a perpendicular turn up to this many tiles before
/// reaching a junction's centre ("pre-turn" cornering); the off-lane error is
/// then corrected diagonally at the same speed (never snapped).
pub const CORNER_WINDOW: f32 = 0.30;
/// Post-turn grace: a perpendicular turn requested up to this many tiles
/// *after* passing a junction's centre is still taken at that junction. The
/// overshoot is corrected exactly like a pre-turn (diagonally, at the same
/// speed), so a key pressed a display frame late is not lost.
pub const POST_TURN_WINDOW: f32 = 0.15;
// Both windows fit inside the junction tile, and the post-turn one is shorter.
const _: () =
    assert!(POST_TURN_WINDOW > 0.0 && POST_TURN_WINDOW < CORNER_WINDOW && CORNER_WINDOW < 0.5);
/// The player pauses this long after eating a mote / a flare.
pub const MOTE_STALL: f32 = 1.0 / 90.0;
pub const FLARE_STALL: f32 = 1.0 / 30.0;

/// Speed of shades bobbing inside / climbing out of the nest (fraction of base).
pub const NEST_SPEED: f32 = 0.45;
/// Vertical half-amplitude of the idle bob inside the nest (tiles).
pub const NEST_BOB: f32 = 0.45;
/// Speed of returning eyes (fraction of base).
pub const EYES_SPEED: f32 = 1.9;

// ---------------------------------------------------------------------------
// Phase timings
// ---------------------------------------------------------------------------

/// "GET SET!" hold at the start of a new game (intro jingle must fit in it).
pub const READY_TIME_FIRST: f32 = 4.2;
/// "GET SET!" hold at the start of a level or after losing a life.
pub const READY_TIME: f32 = 2.2;
/// Freeze after the player is caught, before the death animation.
pub const CAUGHT_TIME: f32 = 1.0;
/// Length of the player's death animation.
pub const DEATH_TIME: f32 = 1.8;
/// Freeze after a shade is eaten (points shown at its position).
pub const EAT_PAUSE_TIME: f32 = 0.9;
/// After the last dot: everything holds still for this long...
pub const LEVEL_CLEAR_HOLD: f32 = 1.0;
/// ...then shades vanish and the maze walls flash for this long.
pub const LEVEL_CLEAR_FLASH: f32 = 2.2;
/// Number of wall flashes during LEVEL_CLEAR_FLASH.
pub const LEVEL_CLEAR_FLASHES: u32 = 4;
/// "GAME OVER" hold before `Game::is_over()` becomes true.
pub const GAME_OVER_TIME: f32 = 3.5;
/// Lifetime of floating score popups.
pub const POPUP_TIME: f32 = 1.6;
/// Fright flashing: in the final `fright_flashes * FRIGHT_FLASH_PERIOD`
/// seconds, frightened shades alternate between blue and white every half period.
pub const FRIGHT_FLASH_PERIOD: f32 = 0.4;

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

pub const MOTE_POINTS: u32 = 10;
pub const FLARE_POINTS: u32 = 50;
/// Points for the 1st..4th shade eaten during a single fright.
pub const SHADE_CHAIN_POINTS: [u32; 4] = [250, 500, 1000, 2000];
/// Extra reward when all four shades are eaten during one fright.
pub const FULL_CHAIN_BONUS: u32 = 3000;
/// Bonus relics appear after this fraction of the level's dots are eaten.
pub const BONUS_TRIGGER_FRACTIONS: [f32; 2] = [0.35, 0.75];
/// A bonus relic stays for a random time in this range.
pub const BONUS_TIME_MIN: f32 = 8.0;
pub const BONUS_TIME_MAX: f32 = 9.5;
/// Reserve-life icons drawn in the HUD at most.
pub const MAX_LIVES_SHOWN: u32 = 6;
/// Number of recent relic icons shown in the HUD.
pub const BONUS_HISTORY_LEN: usize = 5;

// ---------------------------------------------------------------------------
// Shade AI
// ---------------------------------------------------------------------------

/// Vex (ambusher) targets this many tiles ahead of the player.
pub const VEX_LOOKAHEAD: i32 = 3;
/// Flint (flanker) pivots around the tile this many tiles ahead of the player
/// and doubles the vector from Rook to that pivot.
pub const FLINT_PIVOT: i32 = 1;
/// Moss (drifter) chases only while farther than this (tiles) from the player.
pub const MOSS_SHY_RADIUS: f32 = 6.5;

/// Scatter corners (outside the board so the shades orbit a corner block).
/// Index = `ShadeKind::index()` (Rook, Vex, Flint, Moss).
pub const SCATTER_TARGETS: [Tile; 4] = [
    Tile::new(MAZE_W - 2, -2),
    Tile::new(1, -2),
    Tile::new(MAZE_W, MAZE_H),
    Tile::new(-1, MAZE_H),
];

/// After losing a life, waiting shades are released when the level's
/// "global" dot counter reaches these values (Vex, Flint, Moss).
pub const GLOBAL_RELEASE_DOTS: [u32; 3] = [6, 14, 26];

// ---------------------------------------------------------------------------
// Demo / attract mode
// ---------------------------------------------------------------------------

/// Seconds of idle title screen before the attract-mode demo starts.
pub const ATTRACT_IDLE_TIME: f32 = 20.0;
/// Maximum length of one demo run.
pub const DEMO_MAX_TIME: f32 = 45.0;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Global shade behaviour mode (frightened is tracked separately).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Scatter,
    Chase,
}

/// Bonus relics that appear twice per level beneath the nest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BonusKind {
    Shard,
    Prism,
    Lotus,
    Crown,
    Comet,
    Star,
    Nova,
    Singularity,
}

impl BonusKind {
    pub const ALL: [BonusKind; 8] = [
        BonusKind::Shard,
        BonusKind::Prism,
        BonusKind::Lotus,
        BonusKind::Crown,
        BonusKind::Comet,
        BonusKind::Star,
        BonusKind::Nova,
        BonusKind::Singularity,
    ];

    pub const fn points(self) -> u32 {
        match self {
            BonusKind::Shard => 150,
            BonusKind::Prism => 250,
            BonusKind::Lotus => 400,
            BonusKind::Crown => 600,
            BonusKind::Comet => 900,
            BonusKind::Star => 1500,
            BonusKind::Nova => 2500,
            BonusKind::Singularity => 4000,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            BonusKind::Shard => "SHARD",
            BonusKind::Prism => "PRISM",
            BonusKind::Lotus => "LOTUS",
            BonusKind::Crown => "CROWN",
            BonusKind::Comet => "COMET",
            BonusKind::Star => "STAR",
            BonusKind::Nova => "NOVA",
            BonusKind::Singularity => "SINGULARITY",
        }
    }
}

/// Difficulty parameters for one level.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelSpec {
    /// Player speed (fraction of base) normally / while shades are frightened.
    pub player_speed: f32,
    pub player_fright_speed: f32,
    /// Shade speed normally / on slow (tunnel) tiles / while frightened.
    pub shade_speed: f32,
    pub shade_tunnel_speed: f32,
    pub shade_fright_speed: f32,
    /// Fright duration in seconds (0 = shades only reverse).
    pub fright_time: f32,
    /// Number of blue/white flashes at the end of a fright.
    pub fright_flashes: u32,
    /// Rook's "fury": when `dots_left <= fury1_dots` Rook moves at
    /// `fury1_speed` (`shade_speed + FURY1_BOOST`) and keeps chasing during
    /// scatter; stage 2 likewise with `FURY2_BOOST`.
    pub fury1_dots: u32,
    pub fury1_speed: f32,
    pub fury2_dots: u32,
    pub fury2_speed: f32,
    /// Relic offered on this level.
    pub bonus: BonusKind,
    /// Scatter/chase schedule; the final entry lasts forever (`f32::INFINITY`).
    pub schedule: &'static [(Mode, f32)],
    /// Per-shade personal dot counters for leaving the nest, indexed by
    /// `ShadeKind::index()`. Rook's entry is unused (Rook starts at its lair,
    /// outside the nest).
    pub release_dots: [u32; 4],
    /// If the player eats nothing for this long, the next waiting shade leaves.
    pub idle_release: f32,
}

const INF: f32 = f32::INFINITY;

/// Level 1: long, gentle scatters early on.
static SCHEDULE_1: [(Mode, f32); 8] = [
    (Mode::Scatter, 6.0),
    (Mode::Chase, 22.0),
    (Mode::Scatter, 6.0),
    (Mode::Chase, 22.0),
    (Mode::Scatter, 4.0),
    (Mode::Chase, 24.0),
    (Mode::Scatter, 4.0),
    (Mode::Chase, INF),
];
/// Levels 2-4.
static SCHEDULE_2: [(Mode, f32); 8] = [
    (Mode::Scatter, 6.0),
    (Mode::Chase, 24.0),
    (Mode::Scatter, 5.0),
    (Mode::Chase, 24.0),
    (Mode::Scatter, 4.0),
    (Mode::Chase, 45.0),
    (Mode::Scatter, 2.0),
    (Mode::Chase, INF),
];
/// Levels 5 and up: the last scatter is only a brief breather.
static SCHEDULE_5: [(Mode, f32); 8] = [
    (Mode::Scatter, 5.0),
    (Mode::Chase, 25.0),
    (Mode::Scatter, 4.0),
    (Mode::Chase, 25.0),
    (Mode::Scatter, 3.0),
    (Mode::Chase, 55.0),
    (Mode::Scatter, 1.0),
    (Mode::Chase, INF),
];

/// Rook's fury speed boosts over the level's `shade_speed` (stage 1 / 2).
pub const FURY1_BOOST: f32 = 0.04;
pub const FURY2_BOOST: f32 = 0.08;

/// Difficulty parameters for `level` (1-based; 0 is treated as 1).
///
/// The curve has four tiers (level 1, 2-4, 5-20, 21+). Glim keeps a small
/// speed edge over un-furious shades until the last tier, frights get half a
/// second shorter per level (never below one second) and the shades leave
/// the nest sooner as levels rise.
pub fn level_spec(level: u32) -> LevelSpec {
    let level = level.max(1);
    let fright_time = (6.5 - 0.5 * level as f32).max(1.0);
    let fright_flashes = if fright_time >= 3.0 { 5 } else { 3 };
    let bonus = match level {
        1 | 2 => BonusKind::Shard,
        3 => BonusKind::Prism,
        4 | 5 => BonusKind::Lotus,
        6 | 7 => BonusKind::Crown,
        8 | 9 => BonusKind::Comet,
        10 | 11 => BonusKind::Star,
        12..=14 => BonusKind::Nova,
        _ => BonusKind::Singularity,
    };
    let t = tier(level);
    LevelSpec {
        player_speed: t.player,
        player_fright_speed: t.player_fright,
        shade_speed: t.shade,
        shade_tunnel_speed: t.shade_tunnel,
        shade_fright_speed: t.shade_fright,
        fright_time,
        fright_flashes,
        fury1_dots: t.fury1_dots,
        fury1_speed: t.shade + FURY1_BOOST,
        fury2_dots: t.fury2_dots,
        fury2_speed: t.shade + FURY2_BOOST,
        bonus,
        schedule: t.schedule,
        release_dots: t.release_dots,
        idle_release: t.idle_release,
    }
}

/// The difficulty-tier part of a [`LevelSpec`] (speeds, fury thresholds,
/// schedule and nest release); fright and relic are computed per level.
struct Tier {
    player: f32,
    player_fright: f32,
    shade: f32,
    shade_tunnel: f32,
    shade_fright: f32,
    fury1_dots: u32,
    fury2_dots: u32,
    schedule: &'static [(Mode, f32)],
    release_dots: [u32; 4],
    idle_release: f32,
}

fn tier(level: u32) -> Tier {
    match level {
        1 => Tier {
            player: 0.82,
            player_fright: 0.92,
            shade: 0.76,
            shade_tunnel: 0.42,
            shade_fright: 0.52,
            fury1_dots: 24,
            fury2_dots: 12,
            schedule: &SCHEDULE_1,
            release_dots: [0, 0, 24, 48],
            idle_release: 3.5,
        },
        2..=4 => Tier {
            player: 0.88,
            player_fright: 0.94,
            shade: 0.84,
            shade_tunnel: 0.46,
            shade_fright: 0.56,
            fury1_dots: if level == 2 { 32 } else { 40 },
            fury2_dots: if level == 2 { 16 } else { 20 },
            schedule: &SCHEDULE_2,
            release_dots: if level == 2 { [0, 0, 0, 36] } else { [0; 4] },
            idle_release: 3.5,
        },
        5..=20 => Tier {
            player: 0.98,
            player_fright: 1.00,
            shade: 0.94,
            shade_tunnel: 0.50,
            shade_fright: 0.60,
            fury1_dots: 48,
            fury2_dots: 24,
            schedule: &SCHEDULE_5,
            release_dots: [0; 4],
            idle_release: 2.75,
        },
        _ => Tier {
            player: 0.93,
            player_fright: 0.93,
            shade: 0.95,
            shade_tunnel: 0.50,
            shade_fright: 0.60,
            fury1_dots: 56,
            fury2_dots: 28,
            schedule: &SCHEDULE_5,
            release_dots: [0; 4],
            idle_release: 2.75,
        },
    }
}

/// Which built-in maze (index into `mazes::ALL`, taken modulo its length)
/// a level is played on: levels 1-3 maze 0, 4-6 maze 1, 7-10 maze 2,
/// 11-14 maze 3, then the rotation repeats with four levels per maze
/// (15-18 maze 0, 19-22 maze 1, ...).
pub fn maze_for_level(level: u32) -> usize {
    match level.max(1) {
        1..=3 => 0,
        4..=6 => 1,
        7..=10 => 2,
        11..=14 => 3,
        l => (((l - 15) / 4) % 4) as usize,
    }
}

/// Interlude (short animated intermission) to show after clearing `level`,
/// if any: after levels 3, 7 and 11, then after every fifth level (16, 21,
/// 26, ...). Variants are `0..INTERLUDE_VARIANTS` and cycle in order.
pub fn interlude_after(level: u32) -> Option<u8> {
    match level {
        3 => Some(0),
        7 => Some(1),
        11 => Some(2),
        l if l >= 16 && (l - 16) % 5 == 0 => Some(((l - 16) / 5 % 3) as u8),
        _ => None,
    }
}

/// Number of distinct interlude scenes.
pub const INTERLUDE_VARIANTS: u8 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn schedules_end_in_infinite_chase() {
        for level in 1..40 {
            let s = level_spec(level);
            let last = s.schedule.last().unwrap();
            assert_eq!(last.0, Mode::Chase);
            assert!(last.1.is_infinite());
            assert!(s.fright_time >= 1.0);
            assert!(s.fury2_dots < s.fury1_dots);
            // Scatter and chase alternate, starting with a scatter.
            for (i, &(mode, secs)) in s.schedule.iter().enumerate() {
                let want = if i % 2 == 0 {
                    Mode::Scatter
                } else {
                    Mode::Chase
                };
                assert_eq!(mode, want, "level {level} entry {i}");
                assert!(secs > 0.0);
            }
        }
    }

    #[test]
    fn schedules_per_tier() {
        let secs = |level: u32| -> Vec<f32> {
            level_spec(level).schedule[..7]
                .iter()
                .map(|e| e.1)
                .collect()
        };
        assert_eq!(secs(1), [6.0, 22.0, 6.0, 22.0, 4.0, 24.0, 4.0]);
        for level in 2..=4 {
            assert_eq!(secs(level), [6.0, 24.0, 5.0, 24.0, 4.0, 45.0, 2.0]);
        }
        for level in [5, 12, 20, 21, 99] {
            assert_eq!(secs(level), [5.0, 25.0, 4.0, 25.0, 3.0, 55.0, 1.0]);
        }
        assert_eq!(level_spec(0), level_spec(1), "level 0 is treated as 1");
    }

    #[test]
    fn speed_tables() {
        // (level, player, player fright, shade, tunnel, shade fright)
        for (level, p, pf, sh, tu, sf) in [
            (1, 0.82, 0.92, 0.76, 0.42, 0.52),
            (2, 0.88, 0.94, 0.84, 0.46, 0.56),
            (4, 0.88, 0.94, 0.84, 0.46, 0.56),
            (5, 0.98, 1.00, 0.94, 0.50, 0.60),
            (20, 0.98, 1.00, 0.94, 0.50, 0.60),
            (21, 0.93, 0.93, 0.95, 0.50, 0.60),
            (60, 0.93, 0.93, 0.95, 0.50, 0.60),
        ] {
            let s = level_spec(level);
            assert!(close(s.player_speed, p), "level {level}");
            assert!(close(s.player_fright_speed, pf), "level {level}");
            assert!(close(s.shade_speed, sh), "level {level}");
            assert!(close(s.shade_tunnel_speed, tu), "level {level}");
            assert!(close(s.shade_fright_speed, sf), "level {level}");
        }
    }

    #[test]
    fn difficulty_curve_is_sane() {
        let mut prev = level_spec(1);
        for level in 1..=40 {
            let s = level_spec(level);
            // Glim outruns un-furious shades until the final tier, frightened
            // and tunnelled shades are slower still, fury makes Rook faster.
            if level <= 20 {
                assert!(s.player_speed > s.shade_speed, "level {level}");
            }
            assert!(s.player_fright_speed >= s.player_speed);
            assert!(s.shade_fright_speed < s.shade_speed);
            assert!(s.shade_tunnel_speed < s.shade_fright_speed);
            assert!(s.fury1_speed > s.shade_speed && s.fury2_speed > s.fury1_speed);
            // Shades never slow down and frights never lengthen.
            assert!(s.shade_speed >= prev.shade_speed, "level {level}");
            assert!(s.fright_time <= prev.fright_time, "level {level}");
            assert!(s.idle_release <= prev.idle_release, "level {level}");
            prev = s;
        }
    }

    #[test]
    fn fury_and_release_tables() {
        // (level, fury1 dots, fury2 dots, release dots, idle release)
        for (level, f1, f2, rel, idle) in [
            (1, 24, 12, [0, 0, 24, 48], 3.5),
            (2, 32, 16, [0, 0, 0, 36], 3.5),
            (3, 40, 20, [0; 4], 3.5),
            (4, 40, 20, [0; 4], 3.5),
            (5, 48, 24, [0; 4], 2.75),
            (20, 48, 24, [0; 4], 2.75),
            (21, 56, 28, [0; 4], 2.75),
        ] {
            let s = level_spec(level);
            assert_eq!((s.fury1_dots, s.fury2_dots), (f1, f2), "level {level}");
            assert_eq!(s.release_dots, rel, "level {level}");
            assert!(close(s.idle_release, idle), "level {level}");
            assert!(close(s.fury1_speed, s.shade_speed + 0.04));
            assert!(close(s.fury2_speed, s.shade_speed + 0.08));
        }
        assert_eq!(GLOBAL_RELEASE_DOTS, [6, 14, 26]);
    }

    #[test]
    fn relic_table() {
        let points: Vec<u32> = BonusKind::ALL.iter().map(|k| k.points()).collect();
        assert_eq!(points, [150, 250, 400, 600, 900, 1500, 2500, 4000]);
        let expect = |levels: std::ops::RangeInclusive<u32>, kind: BonusKind| {
            for level in levels {
                assert_eq!(level_spec(level).bonus, kind, "level {level}");
            }
        };
        expect(1..=2, BonusKind::Shard);
        expect(3..=3, BonusKind::Prism);
        expect(4..=5, BonusKind::Lotus);
        expect(6..=7, BonusKind::Crown);
        expect(8..=9, BonusKind::Comet);
        expect(10..=11, BonusKind::Star);
        expect(12..=14, BonusKind::Nova);
        expect(15..=60, BonusKind::Singularity);
        for k in BonusKind::ALL {
            assert!(!k.name().is_empty());
        }
    }

    #[test]
    fn scoring_constants() {
        assert_eq!(SHADE_CHAIN_POINTS, [250, 500, 1000, 2000]);
        assert_eq!(FULL_CHAIN_BONUS, 3000);
        assert_eq!(BONUS_HISTORY_LEN, 5);
        assert_eq!(BONUS_TRIGGER_FRACTIONS, [0.35, 0.75]);
        assert!(close(BONUS_TIME_MIN, 8.0) && close(BONUS_TIME_MAX, 9.5));
        assert!(close(MOTE_STALL, 1.0 / 90.0) && close(FLARE_STALL, 1.0 / 30.0));
        assert!(SHADE_CHAIN_POINTS.windows(2).all(|w| w[1] > w[0]));
    }

    #[test]
    fn shade_personality_constants() {
        assert_eq!(VEX_LOOKAHEAD, 3);
        assert_eq!(FLINT_PIVOT, 1);
        assert!(close(MOSS_SHY_RADIUS, 6.5));
        assert_eq!(
            SCATTER_TARGETS,
            [
                Tile::new(MAZE_W - 2, -2),
                Tile::new(1, -2),
                Tile::new(MAZE_W, MAZE_H),
                Tile::new(-1, MAZE_H),
            ]
        );
        // Every scatter corner lies outside the board.
        for t in SCATTER_TARGETS {
            assert!(
                t.x < 0 || t.y < 0 || t.x >= MAZE_W || t.y >= MAZE_H,
                "{t:?}"
            );
        }
    }

    #[test]
    fn maze_rotation() {
        let expect = |levels: std::ops::RangeInclusive<u32>, maze: usize| {
            for level in levels {
                assert_eq!(maze_for_level(level), maze, "level {level}");
            }
        };
        expect(0..=3, 0);
        expect(4..=6, 1);
        expect(7..=10, 2);
        expect(11..=14, 3);
        // Then four levels per maze, in order, forever.
        expect(15..=18, 0);
        expect(19..=22, 1);
        expect(23..=26, 2);
        expect(27..=30, 3);
        expect(31..=34, 0);
        for level in 1..500 {
            assert!(maze_for_level(level) < 4);
        }
    }

    #[test]
    fn interlude_cadence() {
        let shown: Vec<(u32, u8)> = (1..=40)
            .filter_map(|l| interlude_after(l).map(|v| (l, v)))
            .collect();
        assert_eq!(
            shown,
            [
                (3, 0),
                (7, 1),
                (11, 2),
                (16, 0),
                (21, 1),
                (26, 2),
                (31, 0),
                (36, 1)
            ]
        );
        for l in 1..500 {
            if let Some(v) = interlude_after(l) {
                assert!(v < INTERLUDE_VARIANTS);
            }
        }
    }
}
