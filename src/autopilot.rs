//! Computer player used by the attract-mode demo and by headless soak tests.
//!
//! It aims to look plausible on screen: it hunts the nearest dots, avoids
//! non-frightened shades (tiles near them count as dangerous), chases
//! frightened shades when close, and grabs relics.
//!
//! # How it plays
//! Glim can only change course at tile centres (or reverse at any time), so
//! the autopilot plans from the *decision tile* (the centre Glim reaches next)
//! and, mid-tile, from the centre behind it (a reversal).
//!
//! 1. Threat map: for every harmful shade a breadth-first search over
//!    (tile, heading) states (shades never reverse on their own) gives the
//!    earliest time it could reach each nearby tile. Times are measured in
//!    "player tiles" (how far Glim travels meanwhile), so speed differences,
//!    shades still climbing out of the nest and frightened shades about to
//!    recover are all accounted for.
//! 2. Search: Dijkstra over the tunnel-aware movement graph, where each step
//!    costs one tile plus a penalty when a shade could be on that tile at
//!    about the time Glim gets there (a shade that passed long before is no
//!    threat). Tiles a shade reaches first are effectively forbidden.
//! 3. Goal: the cheapest of the nearest dot (flares are saved for emergencies
//!    and favoured when threatened), the relic, or a nearby frightened shade
//!    with enough fright time left. The first step of its path is the
//!    requested direction.
//!
//! Plans are cached per decision tile and recomputed when Glim reaches a new
//! decision tile, eats a dot, stops, or a harmful shade comes close, so the
//! request stays steady between centres. Just past a junction Glim would
//! take a perpendicular request at *that* junction (the post-turn window), so
//! while he is inside it a turn meant for the decision tile ahead is held
//! back and the current heading is requested instead. Voluntary turn-backs
//! are rate-limited (unless a shade is about to touch Glim) to prevent
//! dithering. Everything is deterministic; the seed only salts tie-breaks so
//! different demos take different routes.

use crate::actor::{nav_index, nav_len, nav_tile, wrap_step, wrapped_distance};
use crate::config::{BASE_SPEED, NEST_SPEED, POST_TURN_WINDOW};
use crate::game::{Game, Phase};
use crate::geom::{Dir, Tile};
use crate::maze::{Cell, Maze};
use crate::shade::{Shade, ShadeKind, ShadeState};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

/// Cost of travelling one tile.
const STEP: u32 = 10;
/// Penalty for tiles a harmful shade reaches before (or together with) Glim.
const BLOCK: u32 = 4_000;
/// Threat penalties halve for dangers this many tiles in the future.
const DISCOUNT_HORIZON: f32 = 6.0;
/// Shade reachability is explored this many tiles deep.
const MAX_SHADE_DEPTH: u16 = 24;
/// A shade that reached a tile this many player-tiles before Glim has moved on.
const PASSED_WINDOW: f32 = 4.0;
/// A harmful shade able to reach Glim's tile within this many player-tiles
/// counts as a threat (flares become attractive).
const THREAT_ETA: f32 = 7.0;
/// Replan every tick while a harmful shade is this close (tiles) to Glim or
/// to the decision tile.
const ALERT_RADIUS: f32 = 6.0;
/// Minimum time between two voluntary turn-backs (seconds).
const REVERSE_COOLDOWN: f32 = 0.6;
/// Fright must last at least this long for frightened shades to be chased.
const CHASE_MIN_FRIGHT: f32 = 1.5;
/// Goal adjustments (negative = attractive), in cost units.
const RELIC_BONUS: i64 = -200;
const CHASE_BONUS: i64 = -150;
const CHASE_MAX_COST: u32 = 14 * STEP;
const FLARE_WHEN_THREATENED: i64 = -80;
const FLARE_WHEN_SAFE: i64 = 150;
/// Largest attraction of any goal (bounds the search once a goal is found).
const MAX_ATTRACTION: i64 = 200;

/// Sentinel values in `first`: no path / the source itself / via reversal.
const FIRST_NONE: u8 = u8::MAX;
const FIRST_HERE: u8 = 4;
const FIRST_REVERSE: u8 = 5;

#[derive(Clone, Debug)]
pub struct Autopilot {
    seed: u64,
    /// Decision tile the cached plan belongs to.
    plan_from: Option<Tile>,
    plan: Option<Dir>,
    /// Dots left when the cached plan was made (eating one forces a replan).
    plan_dots: u32,
    /// Game time of the last turn-back (reversal hysteresis).
    last_reverse: f32,
    // Scratch buffers, sized to the extended navigation grid.
    /// Earliest harmful-shade arrival per tile, in player-tiles.
    eta: Vec<f32>,
    /// Per-tile bitmask of headings already explored by the shade search.
    seen: Vec<u8>,
    cost: Vec<u32>,
    /// Glim's arrival along the best path, in tiles.
    arrive: Vec<f32>,
    first: Vec<u8>,
    heap: BinaryHeap<Reverse<(u32, u32)>>,
    queue: VecDeque<(Tile, Dir, u16)>,
    prey: Vec<Tile>,
}

impl Autopilot {
    pub fn new(seed: u64) -> Autopilot {
        Autopilot {
            seed,
            plan_from: None,
            plan: None,
            plan_dots: 0,
            last_reverse: f32::NEG_INFINITY,
            eta: Vec::new(),
            seen: Vec::new(),
            cost: Vec::new(),
            arrive: Vec::new(),
            first: Vec::new(),
            heap: BinaryHeap::new(),
            queue: VecDeque::new(),
            prey: Vec::with_capacity(4),
        }
    }

    /// Direction to request this tick (fed to `Game::update`).
    pub fn choose(&mut self, game: &Game) -> Option<Dir> {
        if game.phase != Phase::Playing {
            return None;
        }
        let maze = &game.maze;
        let m = &game.player.mover;
        let at_center = m.at_center() && m.lane_error() == 0.0;
        let n = if at_center {
            m.tile()
        } else {
            m.next_center_tile(maze)
        };
        let alert = game.shades.iter().any(|s| {
            is_threat(game, s)
                && (wrapped_distance(maze, s.mover.pos, m.pos) < ALERT_RADIUS
                    || wrapped_distance(maze, s.mover.pos, n.center()) < ALERT_RADIUS)
        });
        let dots = maze.dots_left();
        if self.plan_from == Some(n) && self.plan_dots == dots && m.moving && !alert {
            if let Some(d) = self.plan {
                return Some(hold_post_turn(game, d));
            }
        }
        let d = self.replan(game, n, at_center);
        if m.moving && d == m.dir.opposite() {
            self.last_reverse = game.time;
        }
        self.plan_from = Some(n);
        self.plan_dots = dots;
        self.plan = Some(d);
        Some(hold_post_turn(game, d))
    }

    fn ensure_buffers(&mut self, maze: &Maze) {
        let len = nav_len(maze);
        if self.eta.len() != len {
            self.eta = vec![f32::INFINITY; len];
            self.seen = vec![0; len];
            self.cost = vec![u32::MAX; len];
            self.arrive = vec![0.0; len];
            self.first = vec![FIRST_NONE; len];
        }
    }

    /// Fill `eta` with the earliest time (in player-tiles) any shade that is
    /// or will soon be harmful could reach each tile.
    fn compute_threats(&mut self, game: &Game, player_tiles_per_sec: f32) {
        let maze = &game.maze;
        let spec = &game.spec;
        self.eta.fill(f32::INFINITY);
        let vp = player_tiles_per_sec.max(0.1);
        for s in &game.shades {
            let m = &s.mover;
            // (first decision tile, heading, player-tiles until it gets there,
            //  shade speed in tiles/s, earliest harmful time in player-tiles)
            let (start, heading, offset, speed, harmless_until) = match s.state {
                ShadeState::Active => {
                    let at_c = m.at_center();
                    let start = if at_c {
                        m.tile()
                    } else {
                        m.next_center_tile(maze)
                    };
                    let (speed, until) = if s.frightened {
                        (spec.shade_fright_speed, game.fright_timer * vp)
                    } else {
                        (active_speed(game, s), 0.0)
                    };
                    let speed = speed * BASE_SPEED;
                    // The shade is already on its current tile.
                    if let Some(i) = nav_index(maze, m.tile()) {
                        self.eta[i] = self.eta[i].min(until);
                    }
                    (start, m.dir, m.dist_to_center() / speed * vp, speed, until)
                }
                ShadeState::Leaving => {
                    let climb = (m.pos - maze.nest_exit.center()).length()
                        + (m.pos.x - maze.nest_exit.center().x).abs();
                    let offset = climb / (NEST_SPEED * BASE_SPEED) * vp;
                    let until = if s.frightened {
                        game.fright_timer * vp
                    } else {
                        0.0
                    };
                    (
                        maze.nest_exit,
                        Dir::Left,
                        offset,
                        spec.shade_speed * BASE_SPEED,
                        until,
                    )
                }
                _ => continue,
            };
            let Some(si) = nav_index(maze, start) else {
                continue;
            };
            let scale = vp / speed.max(0.1);
            self.seen.fill(0);
            self.queue.clear();
            self.seen[si] = dir_bit(heading);
            self.eta[si] = self.eta[si].min(offset.max(harmless_until));
            self.queue.push_back((start, heading, 0));
            while let Some((t, h, k)) = self.queue.pop_front() {
                if k >= MAX_SHADE_DEPTH {
                    continue;
                }
                for d in Dir::ALL {
                    if d == h.opposite() {
                        continue;
                    }
                    let nt = wrap_step(maze, t, d);
                    if !maze.is_walkable(nt) {
                        continue;
                    }
                    let Some(ni) = nav_index(maze, nt) else {
                        continue;
                    };
                    if self.seen[ni] & dir_bit(d) != 0 {
                        continue;
                    }
                    self.seen[ni] |= dir_bit(d);
                    let e = (offset + f32::from(k + 1) * scale).max(harmless_until);
                    self.eta[ni] = self.eta[ni].min(e);
                    self.queue.push_back((nt, d, k + 1));
                }
            }
        }
    }

    fn replan(&mut self, game: &Game, n: Tile, at_center: bool) -> Dir {
        let maze = &game.maze;
        self.ensure_buffers(maze);
        let frac = if game.fright_active() {
            game.spec.player_fright_speed
        } else {
            game.spec.player_speed
        };
        self.compute_threats(game, frac * BASE_SPEED);
        let m = &game.player.mover;
        let dir = m.dir;
        let Some(ni) = nav_index(maze, n) else {
            return dir;
        };

        let threatened = nav_index(maze, m.tile()).is_some_and(|i| self.eta[i] < THREAT_ETA);
        self.prey.clear();
        if game.fright_timer > CHASE_MIN_FRIGHT {
            self.prey.extend(
                game.shades
                    .iter()
                    .filter(|s| s.state == ShadeState::Active && s.frightened)
                    .map(|s| s.mover.tile()),
            );
        }

        // Sources: the decision tile ahead and, mid-tile, the centre behind.
        // Turning back is rate-limited unless a shade is about to touch Glim.
        let behind = (!at_center).then(|| wrap_step(maze, n, dir.opposite()));
        let to_c = m.dist_to_center();
        let margin = self.eta[ni] - to_c;
        let ahead_blocked = (-PASSED_WINDOW..1.0).contains(&margin);
        let may_turn_back =
            !m.moving || game.time - self.last_reverse >= REVERSE_COOLDOWN || ahead_blocked;
        let allow_reverse = behind.is_some() && may_turn_back;
        self.cost.fill(u32::MAX);
        self.first.fill(FIRST_NONE);
        self.heap.clear();
        let c0 = (to_c * STEP as f32) as u32 + penalty(self.eta[ni], to_c);
        self.relax(ni, c0, to_c, FIRST_HERE);
        if let (Some(b), true) = (behind, allow_reverse) {
            if let Some(bi) = nav_index(maze, b) {
                let back = 1.0 - to_c;
                let c = (back * STEP as f32) as u32 + penalty(self.eta[bi], back);
                self.relax(bi, c, back, FIRST_REVERSE);
            }
        }

        let mut best: Option<(i64, u64, u8)> = None;
        while let Some(Reverse((c, i))) = self.heap.pop() {
            let i = i as usize;
            if c > self.cost[i] {
                continue;
            }
            if let Some((b, _, _)) = best {
                if c as i64 > b + MAX_ATTRACTION {
                    break;
                }
            }
            let t = nav_tile(maze, i);
            if let Some(adj) = self.goal_value(game, t, c, threatened) {
                let score = c as i64 + adj;
                let key = salt(self.seed, t);
                if best.is_none_or(|(b, k, _)| (score, key) < (b, k)) {
                    best = Some((score, key, self.first[i]));
                }
            }
            let arrive = self.arrive[i] + 1.0;
            for d in Dir::ALL {
                // Turning back at the decision tile is a reversal: mid-tile it
                // is covered by the `behind` source; at a centre it obeys the
                // cooldown.
                if i == ni && d == dir.opposite() && (behind.is_some() || !may_turn_back) {
                    continue;
                }
                let nt = wrap_step(maze, t, d);
                if !maze.is_walkable(nt) {
                    continue;
                }
                let Some(j) = nav_index(maze, nt) else {
                    continue;
                };
                let first = if self.first[i] == FIRST_HERE {
                    dir_index(d)
                } else {
                    self.first[i]
                };
                let step = STEP + penalty(self.eta[j], arrive);
                self.relax(j, c.saturating_add(step), arrive, first);
            }
        }

        let first = best.map_or(FIRST_NONE, |(_, _, f)| f);
        match first {
            FIRST_REVERSE => dir.opposite(),
            f if (f as usize) < Dir::ALL.len() => Dir::ALL[f as usize],
            FIRST_HERE if maze.is_walkable(wrap_step(maze, n, dir)) => dir,
            _ => self.safest_exit(maze, n, dir, !may_turn_back),
        }
    }

    fn relax(&mut self, i: usize, c: u32, arrive: f32, first: u8) {
        if c < self.cost[i] {
            self.cost[i] = c;
            self.arrive[i] = arrive;
            self.first[i] = first;
            self.heap.push(Reverse((c, i as u32)));
        }
    }

    /// Attraction adjustment if `t` is a goal (lower is better).
    fn goal_value(&self, game: &Game, t: Tile, cost: u32, threatened: bool) -> Option<i64> {
        let mut value: Option<i64> = None;
        let mut offer = |v: i64| value = Some(value.map_or(v, |old: i64| old.min(v)));
        match game.maze.dot_at(t) {
            Some(Cell::Flare) => {
                if threatened && !game.fright_active() {
                    offer(FLARE_WHEN_THREATENED);
                } else if game.maze.dots_left() > 12 {
                    offer(FLARE_WHEN_SAFE);
                } else {
                    offer(0);
                }
            }
            Some(_) => offer(0),
            None => {}
        }
        if game.bonus.is_some() && t == game.maze.bonus_tile {
            offer(RELIC_BONUS);
        }
        if cost <= CHASE_MAX_COST && self.prey.contains(&t) {
            offer(CHASE_BONUS);
        }
        value
    }

    /// No goal reachable: take the open exit whose tile shades reach last.
    fn safest_exit(&self, maze: &Maze, n: Tile, dir: Dir, no_reverse: bool) -> Dir {
        let mut best: Option<(f32, Dir)> = None;
        for d in Dir::ALL {
            if no_reverse && d == dir.opposite() {
                continue;
            }
            let nt = wrap_step(maze, n, d);
            if !maze.is_walkable(nt) {
                continue;
            }
            let eta = nav_index(maze, nt).map_or(0.0, |i| self.eta[i]);
            if best.is_none_or(|(b, _)| eta > b) {
                best = Some((eta, d));
            }
        }
        best.map_or(dir, |(_, d)| d)
    }
}

/// True while Glim is inside the post-turn window of the junction he has just
/// passed and would take the perpendicular `d` there instead of at the
/// decision tile ahead.
fn in_post_turn_window(game: &Game, d: Dir) -> bool {
    let m = &game.player.mover;
    if !m.moving || m.lane_error() != 0.0 || d.is_horizontal() == m.dir.is_horizontal() {
        return false;
    }
    let to_c = m.dist_to_center();
    to_c > 0.0 && 1.0 - to_c <= POST_TURN_WINDOW && game.maze.is_walkable(m.tile().step(d))
}

/// Request the current heading instead of a turn meant for the decision tile
/// ahead while that turn would be taken at the junction just passed. The
/// window is far shorter than the way to the next centre, so the planned
/// turn is requested again in good time.
fn hold_post_turn(game: &Game, d: Dir) -> Dir {
    if in_post_turn_window(game, d) {
        game.player.mover.dir
    } else {
        d
    }
}

/// Is this shade dangerous now or soon (worth replanning every tick for)?
fn is_threat(game: &Game, s: &Shade) -> bool {
    match s.state {
        ShadeState::Active => !s.frightened || game.fright_timer < CHASE_MIN_FRIGHT,
        ShadeState::Leaving => true,
        _ => false,
    }
}

/// Roaming speed of an un-frightened shade (fraction of base).
fn active_speed(game: &Game, s: &Shade) -> f32 {
    let spec = &game.spec;
    match (s.kind, game.fury) {
        (ShadeKind::Rook, 2) => spec.fury2_speed,
        (ShadeKind::Rook, 1) => spec.fury1_speed,
        _ => spec.shade_speed,
    }
}

/// Step penalty for entering a tile Glim reaches at `arrive` (player-tiles)
/// when a shade can first be there at `eta`. Threats further in the future
/// are discounted: they are less certain and leave time to re-plan, so when
/// every route is dangerous the one whose danger is furthest away wins.
fn penalty(eta: f32, arrive: f32) -> u32 {
    let margin = eta - arrive;
    let base = if margin < -PASSED_WINDOW {
        return 0;
    } else if margin < 1.0 {
        BLOCK
    } else if margin < 2.5 {
        400
    } else if margin < 4.0 {
        80
    } else {
        return 0;
    };
    let discount = DISCOUNT_HORIZON / (DISCOUNT_HORIZON + arrive.max(0.0));
    (base as f32 * discount) as u32
}

fn dir_index(d: Dir) -> u8 {
    match d {
        Dir::Up => 0,
        Dir::Left => 1,
        Dir::Down => 2,
        Dir::Right => 3,
    }
}

fn dir_bit(d: Dir) -> u8 {
    1 << dir_index(d)
}

/// Seeded tie-break key for a tile (SplitMix64 finaliser).
fn salt(seed: u64, t: Tile) -> u64 {
    let mut z = seed
        ^ (((t.x as u32 as u64) << 32) | t.y as u32 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SIM_DT;
    use crate::game::GameOptions;
    use crate::shade::ShadeKind;
    use macroquad::math::Vec2;

    fn playing(seed: u64) -> Game {
        let mut g = Game::new(GameOptions {
            seed,
            ..GameOptions::default()
        });
        g.debug_skip_ready();
        g
    }

    #[test]
    fn idle_outside_play() {
        let g = Game::new(GameOptions::default());
        assert_eq!(Autopilot::new(1).choose(&g), None);
    }

    #[test]
    fn eats_dots_steadily() {
        let mut g = playing(5);
        let mut ap = Autopilot::new(5);
        let start = g.maze.dots_left();
        for _ in 0..(120 * 10) {
            let input = ap.choose(&g);
            g.update(SIM_DT, input);
            if g.phase != Phase::Playing {
                break;
            }
        }
        assert!(
            start - g.maze.dots_left() >= 40,
            "only ate {}",
            start - g.maze.dots_left()
        );
    }

    /// Just past a junction's centre, on the lane, where a perpendicular
    /// request `d` would be taken at that junction.
    fn post_turn_hazard(g: &Game, d: Dir) -> bool {
        let m = &g.player.mover;
        let to_c = m.dist_to_center();
        m.moving
            && m.lane_error() == 0.0
            && d.is_horizontal() != m.dir.is_horizontal()
            && to_c > 0.0
            && 1.0 - to_c <= POST_TURN_WINDOW
            && g.maze.is_walkable(m.tile().step(d))
    }

    #[test]
    fn requests_are_always_walkable_from_the_decision_tile() {
        for (seed, level) in [(9, 1), (4, 5), (2, 12)] {
            let mut g = Game::new(GameOptions {
                seed,
                start_level: level,
                ..GameOptions::default()
            });
            g.debug_skip_ready();
            let mut ap = Autopilot::new(seed);
            let mut held = 0;
            for _ in 0..(120 * 20) {
                let input = ap.choose(&g);
                if let Some(d) = input {
                    let m = &g.player.mover;
                    // Never a turn Glim would take at the junction just passed.
                    assert!(!post_turn_hazard(&g, d), "{d:?} at {:?}", m.pos);
                    let n = if m.at_center() {
                        m.tile()
                    } else {
                        m.next_center_tile(&g.maze)
                    };
                    let walkable = g.maze.is_walkable(wrap_step(&g.maze, n, d));
                    let plan = ap.plan.expect("a plan");
                    if plan != d {
                        // Holding the heading through a post-turn window.
                        assert_eq!(d, m.dir);
                        assert!(in_post_turn_window(&g, plan));
                        held += 1;
                    } else if d != m.dir.opposite() {
                        assert!(walkable, "{d:?} from {n:?}");
                    }
                }
                g.update(SIM_DT, input);
                if g.is_over() {
                    break;
                }
            }
            // Holds are short and rare.
            assert!(held < 120 * 2, "held {held} ticks");
        }
    }

    #[test]
    fn holds_a_turn_meant_for_the_next_junction() {
        let g = playing(1);
        let maze = &g.maze;
        // A tile with a perpendicular exit, followed (heading left) by an
        // open tile: Glim just past its centre must not be asked to turn.
        let j = crate::actor::testutil::find_tile(maze, |t| {
            maze.is_walkable(t)
                && maze.is_walkable(t.step(Dir::Left))
                && maze.is_walkable(t.step(Dir::Right))
                && maze.is_walkable(t.step(Dir::Down))
        });
        let mut g = g;
        g.player
            .mover
            .teleport(j.center() - Vec2::new(0.05, 0.0), Dir::Left);
        g.player.mover.moving = true;
        assert_eq!(hold_post_turn(&g, Dir::Down), Dir::Left);
        assert_eq!(hold_post_turn(&g, Dir::Left), Dir::Left);
        assert_eq!(hold_post_turn(&g, Dir::Right), Dir::Right, "reversals pass");
        // Out of the window (or before the centre) the request passes.
        g.player.mover.teleport(
            j.center() - Vec2::new(POST_TURN_WINDOW + 0.05, 0.0),
            Dir::Left,
        );
        g.player.mover.moving = true;
        assert_eq!(hold_post_turn(&g, Dir::Down), Dir::Down);
        g.player
            .mover
            .teleport(j.center() + Vec2::new(0.05, 0.0), Dir::Left);
        g.player.mover.moving = true;
        assert_eq!(hold_post_turn(&g, Dir::Down), Dir::Down);
    }

    #[test]
    fn flees_a_shade_coming_head_on() {
        let mut g = playing(1);
        // Glim heads left along the start row; park Rook just ahead, facing it.
        let start = g.maze.player_start;
        let ahead = start.offset(-3, 0);
        assert!((ahead.x..start.x).all(|x| g.maze.is_walkable(Tile::new(x, start.y))));
        for _ in 0..3 {
            g.update(SIM_DT, None);
        }
        for s in &mut g.shades {
            s.mover.teleport(g.maze.nest_homes[0].center(), Dir::Up);
            s.state = ShadeState::InNest;
        }
        let rook = &mut g.shades[ShadeKind::Rook.index()];
        rook.state = ShadeState::Active;
        rook.mover.teleport(ahead.center(), Dir::Right);
        let mut ap = Autopilot::new(1);
        assert_eq!(ap.choose(&g), Some(Dir::Right));
    }

    #[test]
    fn deterministic() {
        let run = |seed| {
            let mut g = playing(seed);
            let mut ap = Autopilot::new(seed);
            let mut trace = Vec::new();
            for _ in 0..2000 {
                let input = ap.choose(&g);
                trace.push(input);
                g.update(SIM_DT, input);
            }
            (trace, g.score)
        };
        assert_eq!(run(3), run(3));
    }

    #[test]
    fn salt_varies() {
        let a = salt(1, Tile::new(3, 4));
        assert_ne!(a, salt(2, Tile::new(3, 4)));
        assert_ne!(a, salt(1, Tile::new(4, 3)));
    }
}
