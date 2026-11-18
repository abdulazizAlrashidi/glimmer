//! The grid mover shared by the player and the shades, plus the tunnel-aware
//! navigation helpers (neighbour stepping and breadth-first distance fields)
//! used by the shade AI and the autopilot.
//!
//! The public fields below are also read by the renderer.
//!
//! Invariants (see `geom` for coordinate conventions):
//! - `pos` is in maze space and always lies on a lane: if `dir` is horizontal,
//!   `pos.y` is a tile-centre y (`k + 0.5`); if vertical, `pos.x` is a tile-centre x.
//!   (Exception: while the player is cornering, the off-lane coordinate converges
//!   back to the lane at the same speed; scripted nest motion may also be off-lane.)
//! - `prev_pos` is `pos` at the start of the current simulation tick; the
//!   renderer interpolates between them.
//! - `x` stays within `[-1, w + 1)` thanks to `Maze::wrap_x`.
//!
//! # Grid movement model
//! [`Mover::advance`] moves an actor an exact distance along its lane. Whenever
//! the actor reaches (or starts on) a tile centre with distance left to travel,
//! a caller-supplied decision function picks the next direction (or stops the
//! actor there); leftover distance is carried through the centre so speed is
//! exact. An actor that lands *exactly* on a centre with no distance left makes
//! its decision at the start of the next advance, so every centre is decided
//! exactly once.
//!
//! # Tunnel topology
//! On tunnel rows the columns `-1 ..= w` form a ring of `w + 2` tiles: the
//! virtual tile `-1` is adjacent to the virtual tile `w`. [`wrap_step`] and
//! [`DistField`] use exactly this topology, so path lengths through a tunnel
//! match the distance an actor really travels.

use crate::geom::{Dir, Tile};
use crate::maze::Maze;
use macroquad::math::Vec2;
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq)]
pub struct Mover {
    pub pos: Vec2,
    pub prev_pos: Vec2,
    /// Facing / travel direction (kept while stopped, for rendering).
    pub dir: Dir,
    /// False while blocked by a wall (player) or frozen.
    pub moving: bool,
}

impl Mover {
    pub fn new(pos: Vec2, dir: Dir) -> Mover {
        Mover {
            pos,
            prev_pos: pos,
            dir,
            moving: false,
        }
    }

    /// Call once at the start of every simulation tick, before any movement.
    pub fn begin_tick(&mut self) {
        self.prev_pos = self.pos;
    }

    /// The tile containing `pos`.
    pub fn tile(&self) -> Tile {
        Tile::from_pos(self.pos)
    }

    /// Interpolated position for rendering (`alpha` in `[0, 1]`). Jumps longer
    /// than 1.5 tiles (tunnel wrap, teleport/reset) are not interpolated.
    pub fn render_pos(&self, alpha: f32) -> Vec2 {
        if self.prev_pos.distance_squared(self.pos) > 1.5 * 1.5 {
            self.pos
        } else {
            self.prev_pos.lerp(self.pos, alpha.clamp(0.0, 1.0))
        }
    }

    /// Place the mover on a point (no interpolation from the old position).
    pub fn teleport(&mut self, pos: Vec2, dir: Dir) {
        self.pos = pos;
        self.prev_pos = pos;
        self.dir = dir;
        self.moving = false;
    }

    /// The coordinate along the current direction of travel.
    pub fn along(&self) -> f32 {
        if self.dir.is_horizontal() {
            self.pos.x
        } else {
            self.pos.y
        }
    }

    /// Signed offset that would bring the cross-axis coordinate back onto its
    /// lane (`0.0` when the mover is exactly on its lane).
    pub fn lane_error(&self) -> f32 {
        let c = if self.dir.is_horizontal() {
            self.pos.y
        } else {
            self.pos.x
        };
        lane_of(c) - c
    }

    /// True when the along-axis coordinate sits exactly on a tile centre.
    pub fn at_center(&self) -> bool {
        let c = self.along();
        c == lane_of(c)
    }

    /// Distance along `dir` to the next tile centre ahead (`0.0` when exactly
    /// on a centre).
    pub fn dist_to_center(&self) -> f32 {
        let c = self.along();
        (center_ahead(c, positive(self.dir)) - c).abs()
    }

    /// The tile whose centre the mover reaches next travelling along `dir`
    /// (the current tile when exactly on its centre). Tunnel-wrapped.
    pub fn next_center_tile(&self, maze: &Maze) -> Tile {
        let c = center_ahead(self.along(), positive(self.dir));
        let mut p = self.pos;
        if self.dir.is_horizontal() {
            p.x = maze.wrap_x(c);
        } else {
            p.y = c;
        }
        Tile::from_pos(p)
    }

    /// Move the mover `dist` tiles along its lane, consulting `decide` at every
    /// tile centre reached with distance left over (see the module docs).
    ///
    /// `decide(tile, dir)` returns the direction to leave the centre of `tile`
    /// in, or `None` to stop there. A direction whose neighbouring tile is not
    /// walkable is treated as `None`, so a mover driven by this function never
    /// enters a wall, door or nest tile. Returns the distance actually travelled.
    pub fn advance<F>(&mut self, maze: &Maze, dist: f32, mut decide: F) -> f32
    where
        F: FnMut(Tile, Dir) -> Option<Dir>,
    {
        if !dist.is_finite() || dist <= 0.0 {
            return 0.0;
        }
        let mut left = dist;
        let mut moved = 0.0;
        // Each iteration either finishes the move or reaches a tile centre, so
        // the bound only matters for absurdly large steps.
        for _ in 0..64 {
            if self.at_center() {
                let t = self.tile();
                match decide(t, self.dir) {
                    Some(d) if maze.is_walkable(t.step(d)) => {
                        self.dir = d;
                        self.moving = true;
                    }
                    _ => {
                        self.moving = false;
                        break;
                    }
                }
                let step = left.min(1.0);
                self.shift(maze, step);
                left -= step;
                moved += step;
                if left <= 0.0 {
                    break;
                }
                continue;
            }
            let to_c = self.dist_to_center();
            self.moving = true;
            if left < to_c {
                self.shift(maze, left);
                moved += left;
                break;
            }
            self.snap_to_center_ahead(maze);
            left -= to_c;
            moved += to_c;
            if left <= 0.0 {
                // Landed exactly on a centre: decide at the next advance.
                break;
            }
        }
        moved
    }

    /// Scripted straight-line motion toward `target` (used for nest motion).
    /// Moves at most `dist`, updates `dir` to the dominant direction of travel
    /// and returns the distance travelled; arrival is exact (no overshoot).
    pub fn move_toward(&mut self, target: Vec2, dist: f32) -> f32 {
        let delta = target - self.pos;
        let len = delta.length();
        if dist.is_nan() || dist <= 0.0 || len <= 0.0 {
            return 0.0;
        }
        if delta.x.abs() >= delta.y.abs() {
            self.dir = if delta.x < 0.0 { Dir::Left } else { Dir::Right };
        } else {
            self.dir = if delta.y < 0.0 { Dir::Up } else { Dir::Down };
        }
        self.moving = true;
        if dist >= len {
            self.pos = target;
            len
        } else {
            self.pos += delta * (dist / len);
            dist
        }
    }

    /// Move along `dir` without crossing a centre (caller guarantees it).
    pub(crate) fn shift(&mut self, maze: &Maze, step: f32) {
        self.pos += self.dir.vec() * step;
        self.pos.x = maze.wrap_x(self.pos.x);
    }

    /// Jump exactly onto the next tile centre ahead along `dir` (tunnel-wrapped).
    pub(crate) fn snap_to_center_ahead(&mut self, maze: &Maze) {
        let c = center_ahead(self.along(), positive(self.dir));
        if self.dir.is_horizontal() {
            self.pos.x = maze.wrap_x(c);
        } else {
            self.pos.y = c;
        }
    }
}

/// The lane (tile-centre coordinate) containing coordinate `c`.
pub fn lane_of(c: f32) -> f32 {
    c.floor() + 0.5
}

fn positive(d: Dir) -> bool {
    matches!(d, Dir::Right | Dir::Down)
}

/// The tile-centre coordinate at or ahead of `c` in the given direction.
fn center_ahead(c: f32, positive: bool) -> f32 {
    let mid = lane_of(c);
    if positive {
        if c <= mid {
            mid
        } else {
            mid + 1.0
        }
    } else if c >= mid {
        mid
    } else {
        mid - 1.0
    }
}

/// The tile reached from `t` by one step in `d`, following the tunnel ring
/// (virtual columns `-1` and `w` are adjacent on tunnel rows). Off tunnel rows
/// this is plain `t.step(d)`.
pub fn wrap_step(maze: &Maze, t: Tile, d: Dir) -> Tile {
    let n = t.step(d);
    if n.x < -1 {
        Tile::new(n.x + maze.w + 2, n.y)
    } else if n.x > maze.w {
        Tile::new(n.x - (maze.w + 2), n.y)
    } else {
        n
    }
}

/// Shortest distance between two maze-space points, accounting for the
/// horizontal tunnel wrap (the ring of `w + 2` columns).
pub fn wrapped_distance(maze: &Maze, a: Vec2, b: Vec2) -> f32 {
    let span = (maze.w + 2) as f32;
    let mut dx = (a.x - b.x).abs();
    if dx > span * 0.5 {
        dx = span - dx;
    }
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

/// Number of cells in the extended navigation grid (`(w + 2) * h`, columns
/// `-1 ..= w`).
pub fn nav_len(maze: &Maze) -> usize {
    ((maze.w + 2).max(0) * maze.h.max(0)) as usize
}

/// Index of `t` in the extended navigation grid, or `None` outside it.
pub fn nav_index(maze: &Maze, t: Tile) -> Option<usize> {
    if t.x < -1 || t.x > maze.w || t.y < 0 || t.y >= maze.h {
        None
    } else {
        Some((t.y * (maze.w + 2) + t.x + 1) as usize)
    }
}

/// The tile at extended-grid index `i` (inverse of [`nav_index`]).
pub fn nav_tile(maze: &Maze, i: usize) -> Tile {
    let stride = maze.w + 2;
    let i = i as i32;
    Tile::new(i % stride - 1, i / stride)
}

/// Breadth-first step distances from one source tile to every walkable tile
/// (tunnel virtual tiles included), following the real movement topology.
#[derive(Clone, Debug, PartialEq)]
pub struct DistField {
    dist: Vec<u16>,
}

impl DistField {
    /// Marker for "unreachable".
    pub const UNREACHABLE: u16 = u16::MAX;

    /// Compute the field for `maze` from `source` (which need not itself be
    /// walkable; it is always distance 0).
    pub fn new(maze: &Maze, source: Tile) -> DistField {
        let mut dist = vec![Self::UNREACHABLE; nav_len(maze)];
        let mut queue = VecDeque::new();
        if let Some(i) = nav_index(maze, source) {
            dist[i] = 0;
            queue.push_back(source);
        }
        while let Some(t) = queue.pop_front() {
            let Some(ti) = nav_index(maze, t) else {
                continue;
            };
            let next = dist[ti].saturating_add(1);
            for d in Dir::ALL {
                let n = wrap_step(maze, t, d);
                if !maze.is_walkable(n) {
                    continue;
                }
                if let Some(ni) = nav_index(maze, n) {
                    if dist[ni] == Self::UNREACHABLE {
                        dist[ni] = next;
                        queue.push_back(n);
                    }
                }
            }
        }
        DistField { dist }
    }

    /// Step distance from the source to `t`, if reachable.
    pub fn get(&self, maze: &Maze, t: Tile) -> Option<u16> {
        nav_index(maze, t)
            .and_then(|i| self.dist.get(i).copied())
            .filter(|&d| d != Self::UNREACHABLE)
    }
}

/// Layout-independent lookups for unit tests: every test derives the tiles
/// it needs from the maze instead of hard-coding one layout's coordinates.
#[cfg(test)]
pub(crate) mod testutil {
    use crate::geom::{Dir, Tile};
    use crate::maze::Maze;

    /// The longest horizontal run of walkable on-board tiles on a non-tunnel
    /// row, as `(y, x0, x1)`; the tiles beyond both ends are walls.
    pub fn longest_corridor(m: &Maze) -> (i32, i32, i32) {
        let mut best = (0, 0, -1);
        for y in (0..m.h).filter(|&y| !m.is_tunnel_row(y)) {
            let mut x = 0;
            while x < m.w {
                if !m.is_walkable(Tile::new(x, y)) {
                    x += 1;
                    continue;
                }
                let x0 = x;
                while m.is_walkable(Tile::new(x + 1, y)) {
                    x += 1;
                }
                if x - x0 > best.2 - best.1 {
                    best = (y, x0, x);
                }
                x += 1;
            }
        }
        assert!(best.2 > best.1 + 4, "no long corridor");
        best
    }

    /// The first on-board tile in row-major order satisfying `pred`.
    pub fn find_tile(m: &Maze, pred: impl Fn(Tile) -> bool) -> Tile {
        (0..m.h)
            .flat_map(|y| (0..m.w).map(move |x| Tile::new(x, y)))
            .find(|&t| pred(t))
            .expect("no tile matches")
    }

    /// A walkable tile all four of whose neighbours are walkable.
    pub fn crossroads(m: &Maze, t: Tile) -> bool {
        m.is_walkable(t) && Dir::ALL.iter().all(|&d| m.is_walkable(t.step(d)))
    }

    /// On tunnel row `y`: the innermost x of the open run touching the left
    /// edge and of the one touching the right edge (they cross on a row that
    /// is open all the way across).
    pub fn tunnel_mouths(m: &Maze, y: i32) -> (i32, i32) {
        assert!(m.is_tunnel_row(y));
        let mut left = 0;
        while left + 1 < m.w && m.is_walkable(Tile::new(left + 1, y)) {
            left += 1;
        }
        let mut right = m.w - 1;
        while right > 0 && m.is_walkable(Tile::new(right - 1, y)) {
            right -= 1;
        }
        (left, right)
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;
    use crate::config::SIM_DT;

    const EPS: f32 = 1e-4;

    fn maze() -> Maze {
        Maze::builtin(0)
    }

    /// Keep going straight; stop at walls.
    fn straight(m: &Maze) -> impl FnMut(Tile, Dir) -> Option<Dir> + '_ {
        move |t, d| m.is_walkable(t.step(d)).then_some(d)
    }

    #[test]
    fn exact_distance_per_tick_and_lane_kept() {
        let m = maze();
        let (y, x0, x1) = longest_corridor(&m);
        let mut a = Mover::new(Tile::new(x0, y).center(), Dir::Right);
        let step = 7.2 * SIM_DT;
        let mut total = 0.0;
        let ticks = ((x1 - x0) as f32 / step) as usize + 20;
        for _ in 0..ticks {
            a.begin_tick();
            let moved = a.advance(&m, step, straight(&m));
            total += moved;
            let d = a.pos - a.prev_pos;
            if a.moving {
                assert!(
                    (d.length() - step).abs() < EPS,
                    "moved {} expected {}",
                    d.length(),
                    step
                );
                assert!((moved - step).abs() < EPS);
            }
            assert_eq!(a.pos.y, y as f32 + 0.5, "drifted off lane");
        }
        // Stopped exactly on the centre of the last open tile before the wall.
        assert_eq!(a.pos, Tile::new(x1, y).center());
        assert!(!a.moving);
        assert!((total - (x1 - x0) as f32).abs() < 1e-3);
    }

    #[test]
    fn centre_decisions_are_made_once_and_carry_leftover() {
        let m = maze();
        let t = find_tile(&m, |t| m.is_walkable(t) && m.is_walkable(t.step(Dir::Down)));
        let mut a = Mover::new(Vec2::new(t.x as f32 + 0.4, t.y as f32 + 0.5), Dir::Right);
        let mut calls = Vec::new();
        // 0.1 to the centre of `t`, then 0.15 beyond it, downward.
        let moved = a.advance(&m, 0.25, |t, _| {
            calls.push(t);
            Some(Dir::Down)
        });
        assert!((moved - 0.25).abs() < EPS);
        assert_eq!(calls, vec![t]);
        assert_eq!(a.pos.x, t.x as f32 + 0.5);
        assert!((a.pos.y - (t.y as f32 + 0.65)).abs() < EPS);
        assert_eq!(a.dir, Dir::Down);
    }

    #[test]
    fn exact_landing_defers_the_decision() {
        let m = maze();
        let (y, x0, _) = longest_corridor(&m);
        let mut a = Mover::new(Vec2::new(x0 as f32 + 0.25, y as f32 + 0.5), Dir::Right);
        let mut calls = 0;
        a.advance(&m, 0.25, |_, d| {
            calls += 1;
            Some(d)
        });
        assert_eq!(calls, 0);
        assert!(a.at_center());
        a.advance(&m, 0.25, |_, d| {
            calls += 1;
            Some(d)
        });
        assert_eq!(calls, 1);
        assert!((a.pos.x - (x0 as f32 + 0.75)).abs() < EPS);
    }

    #[test]
    fn walls_are_never_entered_even_if_requested() {
        let m = maze();
        let t = find_tile(&m, |t| m.is_walkable(t) && m.is_wall(t.step(Dir::Up)));
        let mut a = Mover::new(t.center(), Dir::Up);
        let moved = a.advance(&m, 0.5, |_, _| Some(Dir::Up));
        assert_eq!(moved, 0.0);
        assert!(!a.moving);
        assert_eq!(a.pos, t.center());
    }

    #[test]
    fn huge_steps_are_safe() {
        let m = maze();
        let (y, x0, x1) = longest_corridor(&m);
        let mut a = Mover::new(Tile::new(x0, y).center(), Dir::Right);
        a.advance(&m, 1000.0, straight(&m));
        assert_eq!(a.pos, Tile::new(x1, y).center());
        assert_eq!(a.advance(&m, f32::NAN, straight(&m)), 0.0);
        assert_eq!(a.advance(&m, -1.0, straight(&m)), 0.0);
    }

    fn tunnel_trip(m: &Maze, y: i32, dir: Dir) {
        let (left, right) = tunnel_mouths(m, y);
        let start = if dir == Dir::Left {
            Tile::new(left.min(2), y)
        } else {
            Tile::new(right.max(m.w - 3), y)
        };
        let mut a = Mover::new(start.center(), dir);
        let step = 0.09;
        let mut wrapped = false;
        for _ in 0..200 {
            a.begin_tick();
            a.advance(m, step, |_, d| Some(d));
            assert!((-1.0..m.w as f32 + 1.0).contains(&a.pos.x));
            assert_eq!(a.pos.y, y as f32 + 0.5);
            assert!(m.is_walkable(a.tile()), "left the tunnel at {:?}", a.pos);
            if (a.pos.x - a.prev_pos.x).abs() > 1.5 {
                wrapped = true;
            }
        }
        assert!(wrapped, "never wrapped going {dir:?} on row {y}");
    }

    #[test]
    fn tunnel_wraps_both_ways() {
        for i in 0..Maze::builtin_count() {
            let m = Maze::builtin(i);
            for &y in &m.tunnel_rows {
                tunnel_trip(&m, y, Dir::Left);
                tunnel_trip(&m, y, Dir::Right);
            }
        }
    }

    #[test]
    fn tunnel_ring_has_w_plus_2_tiles() {
        let m = maze();
        let y = m.tunnel_rows[0];
        let (left, right) = tunnel_mouths(&m, y);
        assert!(
            left < right,
            "maze 0's tunnel rows are closed in the middle"
        );
        let goal = Tile::new(right, y);
        let mut a = Mover::new(Tile::new(left, y).center(), Dir::Left);
        let mut total = 0.0;
        for _ in 0..1000 {
            total += a.advance(&m, 0.1, |t, d| (t != goal).then_some(d));
        }
        // Going left from the left mouth to the right one crosses the ring of
        // w+2 columns.
        let expected = (left - goal.x).rem_euclid(m.w + 2) as f32;
        assert!(expected < m.w as f32 / 2.0, "the short way round");
        assert!((total - expected).abs() < 1e-3, "{total} vs {expected}");
        assert_eq!(a.pos, goal.center());
    }

    #[test]
    fn helpers() {
        let m = maze();
        let y = m.tunnel_rows[0];
        assert_eq!(
            wrap_step(&m, Tile::new(-1, y), Dir::Left),
            Tile::new(m.w, y)
        );
        assert_eq!(
            wrap_step(&m, Tile::new(m.w, y), Dir::Right),
            Tile::new(-1, y)
        );
        assert_eq!(wrap_step(&m, Tile::new(3, 3), Dir::Up), Tile::new(3, 2));
        let a = Vec2::new(-0.9, 2.0);
        let b = Vec2::new(m.w as f32 + 0.9, 2.0);
        assert!((wrapped_distance(&m, a, b) - 0.2).abs() < EPS);
        for i in [0, 5, nav_len(&m) - 1] {
            assert_eq!(nav_index(&m, nav_tile(&m, i)), Some(i));
        }
        assert_eq!(nav_index(&m, Tile::new(-2, 0)), None);
        let mut mv = Mover::new(Vec2::new(3.5, 2.2), Dir::Right);
        assert!((mv.lane_error() - 0.3).abs() < EPS);
        assert!(mv.at_center());
        mv.dir = Dir::Down;
        assert!((mv.dist_to_center() - 0.3).abs() < EPS);
        assert_eq!(mv.next_center_tile(&m), Tile::new(3, 2));
        mv.dir = Dir::Up;
        assert!((mv.dist_to_center() - 0.7).abs() < EPS);
        assert_eq!(mv.next_center_tile(&m), Tile::new(3, 1));
    }

    #[test]
    fn move_toward_is_exact() {
        let mut a = Mover::new(Vec2::new(1.0, 1.0), Dir::Left);
        assert!((a.move_toward(Vec2::new(1.0, 3.0), 0.5) - 0.5).abs() < EPS);
        assert_eq!(a.dir, Dir::Down);
        assert!((a.move_toward(Vec2::new(1.0, 3.0), 5.0) - 1.5).abs() < EPS);
        assert_eq!(a.pos, Vec2::new(1.0, 3.0));
        assert_eq!(a.move_toward(Vec2::new(1.0, 3.0), 5.0), 0.0);
    }

    #[test]
    fn dist_field_follows_the_maze() {
        let m = maze();
        let f = DistField::new(&m, m.nest_exit);
        assert_eq!(f.get(&m, m.nest_exit), Some(0));
        let mut reachable = 0;
        for t in m.walkable_tiles() {
            let d = f.get(&m, t).expect("every walkable tile is reachable");
            reachable += 1;
            if d > 0 {
                // Some neighbour is exactly one step closer.
                assert!(Dir::ALL
                    .iter()
                    .any(|&dir| f.get(&m, wrap_step(&m, t, dir)) == Some(d - 1)));
            }
        }
        assert!(reachable > 200);
        assert_eq!(f.get(&m, m.door), None);
        assert_eq!(f.get(&m, Tile::new(0, 0)), None);
        let y = m.tunnel_rows[0];
        assert!(f.get(&m, Tile::new(-1, y)).is_some());
    }
}
