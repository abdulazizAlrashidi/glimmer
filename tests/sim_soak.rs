//! Headless soak test: the autopilot plays many seeded games from several
//! start levels while every simulation invariant is checked on every tick.

use glimmer::autopilot::Autopilot;
use glimmer::config::{maze_for_level, BASE_SPEED, SIM_DT};
use glimmer::game::{Game, GameEvent, GameOptions, Phase};
use glimmer::maze::Maze;
use glimmer::shade::ShadeState;

/// Simulated time budget per run.
const RUN_SECONDS: f32 = 240.0;
const SEEDS: [u64; 6] = [1, 2, 3, 4, 5, 6];
/// The first level of each maze in the rotation, plus the final speed tier.
const START_LEVELS: [u32; 5] = [1, 4, 7, 11, 21];

#[derive(Default, Debug)]
struct Stats {
    runs: u32,
    ticks: u64,
    levels_cleared: u32,
    /// Levels cleared per built-in maze.
    cleared_by_maze: Vec<u32>,
    shades_eaten: u32,
    relics_eaten: u32,
    deaths: u32,
}

fn check_invariants(g: &Game, ctx: &str) {
    let w = g.maze.w as f32;
    let movers = std::iter::once(&g.player.mover).chain(g.shades.iter().map(|s| &s.mover));
    for m in movers {
        for p in [m.pos, m.prev_pos] {
            assert!(
                p.x.is_finite() && p.y.is_finite(),
                "{ctx}: non-finite position {p:?}"
            );
            assert!(
                (-1.0..w + 1.0).contains(&p.x),
                "{ctx}: x out of range {p:?}"
            );
            assert!(
                (0.0..g.maze.h as f32).contains(&p.y),
                "{ctx}: y out of range {p:?}"
            );
        }
    }
    let pt = g.player.mover.tile();
    assert!(
        g.maze.is_walkable(pt),
        "{ctx}: player on non-walkable tile {pt:?} at {:?}",
        g.player.mover.pos
    );
    for s in &g.shades {
        if s.state == ShadeState::Active || s.state == ShadeState::Eyes {
            let t = s.mover.tile();
            assert!(
                g.maze.is_walkable(t),
                "{ctx}: {:?} ({:?}) on non-walkable tile {t:?} at {:?}",
                s.kind,
                s.state,
                s.mover.pos
            );
        }
    }
    assert!(g.high_score >= g.score, "{ctx}: high score below score");
    assert!(
        g.fright_timer >= 0.0 && g.fright_timer.is_finite(),
        "{ctx}: bad fright timer"
    );
    for i in 0..g.shades.len() {
        let _ = g.shade_look(i);
    }
    let _ = g.loop_hint();
    // One shade at a time in the door column.
    let in_door = g
        .shades
        .iter()
        .filter(|s| matches!(s.state, ShadeState::Leaving | ShadeState::Entering))
        .count();
    assert!(in_door <= 1, "{ctx}: {in_door} shades in the door");
}

/// While playing, Glim never moves farther than his speed allows on either
/// axis in one tick (cornering moves both axes at once, never one twice).
fn check_player_step(g: &Game, was_playing: bool, ctx: &str) {
    if !was_playing || g.phase != Phase::Playing {
        return;
    }
    let m = &g.player.mover;
    let d = m.pos - m.prev_pos;
    if d.x.abs() > 1.5 {
        return; // tunnel wrap
    }
    let max = g.spec.player_speed.max(g.spec.player_fright_speed) * BASE_SPEED * SIM_DT;
    assert!(
        d.x.abs() <= max + 1e-4 && d.y.abs() <= max + 1e-4,
        "{ctx}: player hopped {d:?} (max {max}) to {:?}",
        m.pos
    );
}

fn soak_run(seed: u64, start_level: u32, stats: &mut Stats) {
    let opts = GameOptions {
        seed,
        start_level,
        ..GameOptions::default()
    };
    let mut g = Game::new(opts);
    let mut ap = Autopilot::new(seed);
    let ticks = (RUN_SECONDS / SIM_DT) as u64;
    let mut last_score = 0;
    let mut level = g.level;
    let mut dots_left = g.maze.dots_left();
    let mut eyes_time = [0.0f32; 4];
    stats.runs += 1;
    for tick in 0..ticks {
        let ctx = format!(
            "seed {seed} start {start_level} tick {tick} level {}",
            g.level
        );
        let input = ap.choose(&g);
        let was_playing = g.phase == Phase::Playing;
        g.update(SIM_DT, input);
        stats.ticks += 1;
        check_invariants(&g, &ctx);
        check_player_step(&g, was_playing, &ctx);

        assert!(g.score >= last_score, "{ctx}: score decreased");
        last_score = g.score;
        if g.level != level {
            assert_eq!(g.level, level + 1, "{ctx}: skipped a level");
            level = g.level;
        } else {
            assert!(g.maze.dots_left() <= dots_left, "{ctx}: dots reappeared");
        }
        dots_left = g.maze.dots_left();

        // Eyes always make it home in reasonable time.
        for (i, s) in g.shades.iter().enumerate() {
            if s.is_eyes() {
                eyes_time[i] += SIM_DT;
                assert!(eyes_time[i] < 20.0, "{ctx}: {:?} eyes lost", s.kind);
            } else {
                eyes_time[i] = 0.0;
            }
        }

        for ev in g.take_events() {
            match ev {
                GameEvent::LevelCleared { level } => {
                    stats.levels_cleared += 1;
                    stats.cleared_by_maze[maze_for_level(level) % Maze::builtin_count()] += 1;
                }
                GameEvent::ShadeEaten { .. } => stats.shades_eaten += 1,
                GameEvent::BonusEaten { .. } => stats.relics_eaten += 1,
                GameEvent::PlayerCaught { .. } => stats.deaths += 1,
                _ => {}
            }
        }
        match g.phase {
            Phase::Interlude { .. } => g.end_interlude(),
            Phase::GameOver { .. } => break,
            _ => {}
        }
    }
}

#[test]
fn autopilot_soak() {
    let mut stats = Stats {
        cleared_by_maze: vec![0; Maze::builtin_count()],
        ..Stats::default()
    };
    for level in START_LEVELS {
        for seed in SEEDS {
            soak_run(seed, level, &mut stats);
        }
    }
    println!("{stats:?}");
    assert!(
        stats.levels_cleared > 0,
        "autopilot never cleared a level: {stats:?}"
    );
    for (maze, &n) in stats.cleared_by_maze.iter().enumerate() {
        assert!(n > 0, "autopilot never cleared maze {maze}: {stats:?}");
    }
    assert!(
        stats.shades_eaten > 0,
        "autopilot never ate a shade: {stats:?}"
    );
}

#[test]
fn demo_games_end_after_one_life() {
    let opts = GameOptions {
        seed: 11,
        demo: true,
        ..GameOptions::default()
    };
    let mut g = Game::new(opts);
    let mut ap = Autopilot::new(11);
    assert_eq!(g.lives, 0);
    let mut caught = 0;
    for _ in 0..(600.0 / SIM_DT) as u32 {
        let input = ap.choose(&g);
        g.update(SIM_DT, input);
        for ev in g.take_events() {
            match ev {
                GameEvent::PlayerCaught { .. } => caught += 1,
                GameEvent::InterludeStarted { .. } | GameEvent::ExtraLife => {
                    panic!("demo games have no interludes or extra lives")
                }
                _ => {}
            }
        }
        if g.is_over() {
            break;
        }
    }
    assert!(g.is_over(), "demo never ended");
    assert_eq!(caught, 1);
}
