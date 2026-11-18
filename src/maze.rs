//! Maze data model: parsing the ASCII maze definitions, tile queries used by
//! the simulation, dot bookkeeping, and (in tests) structural validation.
//!
//! # ASCII legend (see [`crate::mazes`])
//! | char | meaning |
//! |------|---------|
//! | `#`  | wall |
//! | `.`  | mote (small dot, 10 pts) |
//! | `o`  | flare (power dot, 50 pts, frightens shades) |
//! | ` `  | open floor, no dot |
//! | `t`  | open floor where shades move slowly (tunnel approaches) |
//! | `P`  | player start (open floor) |
//! | `B`  | bonus relic spawn tile (open floor) |
//! | `-`  | nest door (only crossed by scripted shade motion) |
//! | `n`  | nest interior (only occupied by scripted shade motion) |
//!
//! # Nest geometry (derived from the single door tile `D = (dx, dy)`)
//! - `nest_exit = (dx, dy - 1)`: the open tile above the door. Shades appear
//!   here after leaving the nest, and returning eyes aim for it. (Rook starts
//!   in its own lair instead, see `shade::rook_lair`.)
//! - `nest_homes = [(dx, dy + 2), (dx - 2, dy + 2), (dx + 2, dy + 2)]`: the
//!   idle positions of Vex, Flint and Moss (centre, left, right).
//!
//! # Tunnels
//! A row whose first and last tiles are both open is a tunnel row. On such a
//! row the virtual tiles `x = -1` and `x = w` are open (and slow), and actor
//! positions wrap: `x < -1` becomes `x + (w + 2)` and `x >= w + 1` becomes
//! `x - (w + 2)` (see [`Maze::wrap_x`]). Rows above/below a tunnel row are
//! walls outside the board, so actors in the tunnel can only continue
//! straight.

use crate::geom::Tile;

#[cfg(test)]
use crate::config::{MAZE_H, MAZE_W};
#[cfg(test)]
use crate::geom::Dir;
#[cfg(test)]
use std::collections::VecDeque;

/// Content of one tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cell {
    Wall,
    Open,
    Mote,
    Flare,
    Door,
    Nest,
}

impl Cell {
    /// Floor an actor can travel on in normal (grid) movement.
    pub const fn is_walkable(self) -> bool {
        matches!(self, Cell::Open | Cell::Mote | Cell::Flare)
    }

    pub const fn is_dot(self) -> bool {
        matches!(self, Cell::Mote | Cell::Flare)
    }
}

/// A static maze definition (name + ASCII rows).
#[derive(Clone, Copy, Debug)]
pub struct MazeDef {
    /// Display name, shown e.g. on the "GET SET" banner.
    pub name: &'static str,
    pub rows: &'static [&'static str],
}

/// A playable maze instance. Dots are consumed as they are eaten and can be
/// restored with [`Maze::reset_dots`].
#[derive(Clone, Debug)]
pub struct Maze {
    pub name: &'static str,
    /// Index into [`crate::mazes::ALL`] (0 for ad-hoc parsed mazes).
    pub index: usize,
    pub w: i32,
    pub h: i32,
    cells: Vec<Cell>,
    initial: Vec<Cell>,
    slow: Vec<bool>,
    pub player_start: Tile,
    pub bonus_tile: Tile,
    pub door: Tile,
    pub nest_exit: Tile,
    /// Idle positions inside the nest: centre, left, right.
    pub nest_homes: [Tile; 3],
    pub tunnel_rows: Vec<i32>,
    /// Number of motes + flares at the start of the level.
    pub total_dots: u32,
    dots_left: u32,
}

impl Maze {
    /// Parse an ASCII definition. Returns a descriptive error for malformed
    /// input (wrong dimensions, unknown characters, missing markers, bad nest).
    pub fn parse(name: &'static str, rows: &[&str], index: usize) -> Result<Maze, String> {
        let h = rows.len() as i32;
        if h == 0 {
            return Err(format!("{name}: no rows"));
        }
        let w = rows[0].chars().count() as i32;
        if w < 5 || h < 5 {
            return Err(format!("{name}: maze too small ({w}x{h})"));
        }
        let mut cells = Vec::with_capacity((w * h) as usize);
        let mut slow = Vec::with_capacity((w * h) as usize);
        let mut player = None;
        let mut bonus = None;
        let mut doors = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            let n = row.chars().count() as i32;
            if n != w {
                return Err(format!("{name}: row {y} has {n} columns, expected {w}"));
            }
            for (x, ch) in row.chars().enumerate() {
                let t = Tile::new(x as i32, y as i32);
                let (cell, is_slow) = match ch {
                    '#' => (Cell::Wall, false),
                    '.' => (Cell::Mote, false),
                    'o' => (Cell::Flare, false),
                    ' ' => (Cell::Open, false),
                    't' => (Cell::Open, true),
                    'P' => {
                        if player.replace(t).is_some() {
                            return Err(format!("{name}: more than one 'P'"));
                        }
                        (Cell::Open, false)
                    }
                    'B' => {
                        if bonus.replace(t).is_some() {
                            return Err(format!("{name}: more than one 'B'"));
                        }
                        (Cell::Open, false)
                    }
                    '-' => {
                        doors.push(t);
                        (Cell::Door, false)
                    }
                    'n' => (Cell::Nest, false),
                    other => {
                        return Err(format!("{name}: unknown character {other:?} at ({x},{y})"))
                    }
                };
                cells.push(cell);
                slow.push(is_slow);
            }
        }
        let player_start = player.ok_or_else(|| format!("{name}: missing 'P'"))?;
        let bonus_tile = bonus.ok_or_else(|| format!("{name}: missing 'B'"))?;
        if doors.len() != 1 {
            return Err(format!(
                "{name}: expected exactly one door '-', found {}",
                doors.len()
            ));
        }
        let door = doors[0];
        let nest_exit = door.offset(0, -1);
        let nest_homes = [door.offset(0, 2), door.offset(-2, 2), door.offset(2, 2)];

        let idx = |t: Tile| (t.y * w + t.x) as usize;
        let inb = |t: Tile| t.x >= 0 && t.y >= 0 && t.x < w && t.y < h;
        if !inb(nest_exit) || !cells[idx(nest_exit)].is_walkable() {
            return Err(format!("{name}: tile above the door must be open floor"));
        }
        // Nest interior: 5 wide x 3 tall directly below the door.
        for dy in 1..=3 {
            for dx in -2..=2 {
                let t = door.offset(dx, dy);
                if !inb(t) || cells[idx(t)] != Cell::Nest {
                    return Err(format!(
                        "{name}: nest interior must be 5x3 'n' below the door (bad tile {t:?})"
                    ));
                }
            }
        }

        let tunnel_rows: Vec<i32> = (0..h)
            .filter(|&y| {
                cells[(y * w) as usize].is_walkable()
                    && cells[(y * w + w - 1) as usize].is_walkable()
            })
            .collect();
        let total_dots = cells.iter().filter(|c| c.is_dot()).count() as u32;

        Ok(Maze {
            name,
            index,
            w,
            h,
            initial: cells.clone(),
            cells,
            slow,
            player_start,
            bonus_tile,
            door,
            nest_exit,
            nest_homes,
            tunnel_rows,
            total_dots,
            dots_left: total_dots,
        })
    }

    /// Load built-in maze `index` (taken modulo the number of built-ins).
    pub fn builtin(index: usize) -> Maze {
        let i = index % crate::mazes::ALL.len();
        let def = &crate::mazes::ALL[i];
        Maze::parse(def.name, def.rows, i)
            .unwrap_or_else(|e| panic!("built-in maze {i} is invalid: {e}"))
    }

    /// Number of built-in mazes.
    pub fn builtin_count() -> usize {
        crate::mazes::ALL.len()
    }

    pub fn in_bounds(&self, t: Tile) -> bool {
        t.x >= 0 && t.y >= 0 && t.x < self.w && t.y < self.h
    }

    fn idx(&self, t: Tile) -> usize {
        (t.y * self.w + t.x) as usize
    }

    pub fn is_tunnel_row(&self, y: i32) -> bool {
        self.tunnel_rows.contains(&y)
    }

    /// Tile content. Outside the board: open on tunnel rows, wall elsewhere.
    pub fn cell(&self, t: Tile) -> Cell {
        if self.in_bounds(t) {
            self.cells[self.idx(t)]
        } else if t.y >= 0 && t.y < self.h && self.is_tunnel_row(t.y) {
            Cell::Open
        } else {
            Cell::Wall
        }
    }

    /// Can an actor in normal grid movement occupy this tile?
    /// (Door and nest tiles are only crossed by scripted shade motion.)
    pub fn is_walkable(&self, t: Tile) -> bool {
        self.cell(t).is_walkable()
    }

    /// Is this a solid wall (for rendering)?
    pub fn is_wall(&self, t: Tile) -> bool {
        self.cell(t) == Cell::Wall
    }

    /// Do shades move at tunnel speed here? (Off-board tunnel tiles are slow.)
    pub fn is_slow(&self, t: Tile) -> bool {
        if self.in_bounds(t) {
            self.slow[self.idx(t)]
        } else {
            self.cell(t) == Cell::Open
        }
    }

    /// Wrap an actor's x position through the tunnel (see module docs).
    pub fn wrap_x(&self, x: f32) -> f32 {
        let span = (self.w + 2) as f32;
        if x < -1.0 {
            x + span
        } else if x >= self.w as f32 + 1.0 {
            x - span
        } else {
            x
        }
    }

    /// The dot on `t`, if any.
    pub fn dot_at(&self, t: Tile) -> Option<Cell> {
        if self.in_bounds(t) {
            let c = self.cells[self.idx(t)];
            c.is_dot().then_some(c)
        } else {
            None
        }
    }

    /// Remove and return the dot on `t`, if any.
    pub fn take_dot(&mut self, t: Tile) -> Option<Cell> {
        let c = self.dot_at(t)?;
        let i = self.idx(t);
        self.cells[i] = Cell::Open;
        self.dots_left -= 1;
        Some(c)
    }

    pub fn dots_left(&self) -> u32 {
        self.dots_left
    }

    pub fn dots_eaten(&self) -> u32 {
        self.total_dots - self.dots_left
    }

    /// Restore every dot (start of a level).
    pub fn reset_dots(&mut self) {
        self.cells.clone_from(&self.initial);
        self.dots_left = self.total_dots;
    }

    /// All remaining dots with their tiles (row-major order).
    pub fn dots(&self) -> impl Iterator<Item = (Tile, Cell)> + '_ {
        self.cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_dot())
            .map(move |(i, &c)| {
                let i = i as i32;
                (Tile::new(i % self.w, i / self.w), c)
            })
    }
}

// Only the tests use these (here and in `mazes`, `actor` and `shade`), so they
// stay out of the game build.
#[cfg(test)]
impl Maze {
    /// Directions in which the neighbour of `t` is walkable, in `Dir::ALL` order.
    pub(crate) fn exits(&self, t: Tile) -> impl Iterator<Item = Dir> + '_ {
        Dir::ALL
            .into_iter()
            .filter(move |&d| self.is_walkable(t.step(d)))
    }

    /// Wrap a tile's x into the board on tunnel rows (identity elsewhere).
    pub(crate) fn wrap_tile(&self, t: Tile) -> Tile {
        if self.is_tunnel_row(t.y) {
            Tile::new(t.x.rem_euclid(self.w), t.y)
        } else {
            t
        }
    }

    /// Every walkable on-board tile.
    pub(crate) fn walkable_tiles(&self) -> Vec<Tile> {
        (0..self.h)
            .flat_map(|y| (0..self.w).map(move |x| Tile::new(x, y)))
            .filter(|&t| self.is_walkable(t))
            .collect()
    }

    /// Structural checks every shipped maze must pass. Returns all problems.
    pub(crate) fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        if self.w != MAZE_W || self.h != MAZE_H {
            errs.push(format!(
                "size {}x{} != {}x{}",
                self.w, self.h, MAZE_W, MAZE_H
            ));
        }
        // Mirror symmetry (gameplay fairness + look).
        for y in 0..self.h {
            for x in 0..self.w / 2 {
                let a = self.initial[self.idx(Tile::new(x, y))];
                let b = self.initial[self.idx(Tile::new(self.w - 1 - x, y))];
                if a != b
                    || self.slow[self.idx(Tile::new(x, y))]
                        != self.slow[self.idx(Tile::new(self.w - 1 - x, y))]
                {
                    errs.push(format!("not mirror-symmetric at row {y}, col {x}"));
                }
            }
        }
        if self.door.x != self.w / 2 {
            errs.push("door is not on the centre column".into());
        }
        if self.player_start.x != self.w / 2 {
            errs.push("player start is not on the centre column".into());
        }
        if !self.is_walkable(self.bonus_tile) || self.dot_at(self.bonus_tile).is_some() {
            errs.push("bonus tile must be dot-free open floor".into());
        }
        if self.tunnel_rows.is_empty() {
            errs.push("maze has no tunnel".into());
        }
        // Outer border must be wall except tunnel openings.
        for x in 0..self.w {
            for y in [0, self.h - 1] {
                if !self.is_wall(Tile::new(x, y)) {
                    errs.push(format!("border open at ({x},{y})"));
                }
            }
        }
        for y in 0..self.h {
            for x in [0, self.w - 1] {
                let t = Tile::new(x, y);
                if !self.is_wall(t) && !self.is_tunnel_row(y) {
                    errs.push(format!("border open at ({x},{y})"));
                }
            }
        }
        for &y in &self.tunnel_rows {
            for dy in [-1, 1] {
                if !self.is_wall(Tile::new(0, y + dy))
                    || !self.is_wall(Tile::new(self.w - 1, y + dy))
                {
                    errs.push(format!(
                        "tunnel row {y} must be walled above and below at the edges"
                    ));
                }
            }
        }
        let flares = self.initial.iter().filter(|&&c| c == Cell::Flare).count();
        if flares != 4 {
            errs.push(format!("expected 4 flares, found {flares}"));
        }
        if !(180..=320).contains(&self.total_dots) {
            errs.push(format!("dot count {} outside 180..=320", self.total_dots));
        }
        // No dead ends, no 2x2 open areas.
        for y in 0..self.h {
            for x in 0..self.w {
                let t = Tile::new(x, y);
                if !self.is_walkable(t) {
                    continue;
                }
                let n = self.exits(t).count();
                if n < 2 {
                    errs.push(format!("dead end at ({x},{y})"));
                }
                let square = [t, t.offset(1, 0), t.offset(0, 1), t.offset(1, 1)];
                if x + 1 < self.w && y + 1 < self.h && square.iter().all(|&s| self.is_walkable(s)) {
                    errs.push(format!("2x2 open area at ({x},{y})"));
                }
            }
        }
        // Everything reachable from the player start (through tunnels).
        let all = self.walkable_tiles();
        let mut seen = vec![false; (self.w * self.h) as usize];
        let mut q = VecDeque::from([self.player_start]);
        seen[self.idx(self.player_start)] = true;
        let mut count = 1;
        while let Some(t) = q.pop_front() {
            for d in Dir::ALL {
                let n = self.wrap_tile(t.step(d));
                if self.in_bounds(n) && self.is_walkable(n) && !seen[self.idx(n)] {
                    seen[self.idx(n)] = true;
                    count += 1;
                    q.push_back(n);
                }
            }
        }
        if count != all.len() {
            errs.push(format!(
                "{} walkable tiles unreachable from the player start",
                all.len() - count
            ));
        }
        for t in [self.player_start, self.nest_exit, self.bonus_tile] {
            if !self.is_walkable(t) {
                errs.push(format!("marker tile {t:?} is not walkable"));
            }
        }
        errs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_maze_is_valid() {
        assert!(Maze::builtin_count() >= 1);
        for i in 0..Maze::builtin_count() {
            let m = Maze::builtin(i);
            let errs = m.validate();
            assert!(
                errs.is_empty(),
                "maze {i} ({}) invalid:\n{}",
                m.name,
                errs.join("\n")
            );
        }
    }

    #[test]
    fn dots_are_consumed_and_restored() {
        let mut m = Maze::builtin(0);
        let total = m.total_dots;
        let (t, c) = m.dots().next().unwrap();
        assert_eq!(m.take_dot(t), Some(c));
        assert_eq!(m.take_dot(t), None);
        assert_eq!(m.dots_left(), total - 1);
        assert_eq!(m.dots_eaten(), 1);
        m.reset_dots();
        assert_eq!(m.dots_left(), total);
        assert_eq!(m.dot_at(t), Some(c));
    }

    #[test]
    fn tunnel_wraps_and_off_board_rules() {
        let m = Maze::builtin(0);
        let y = m.tunnel_rows[0];
        assert!(m.is_walkable(Tile::new(-1, y)));
        assert!(m.is_walkable(Tile::new(m.w, y)));
        assert!(m.is_slow(Tile::new(-1, y)));
        assert!(!m.is_walkable(Tile::new(-1, y - 1)));
        assert!(!m.is_walkable(Tile::new(3, -1)));
        assert_eq!(m.wrap_tile(Tile::new(-1, y)), Tile::new(m.w - 1, y));
        assert!((m.wrap_x(-1.01) - (m.w as f32 + 0.99)).abs() < 1e-4);
        assert!((m.wrap_x(m.w as f32 + 1.0) - (-1.0)).abs() < 1e-4);
        assert_eq!(m.wrap_x(5.5), 5.5);
    }

    #[test]
    fn nest_geometry() {
        let m = Maze::builtin(0);
        assert_eq!(m.cell(m.door), Cell::Door);
        assert!(m.is_walkable(m.nest_exit));
        for h in m.nest_homes {
            assert_eq!(m.cell(h), Cell::Nest);
        }
        assert!(!m.is_walkable(m.door));
    }

    #[test]
    fn parse_errors_are_reported() {
        assert!(Maze::parse("x", &[], 0).is_err());
        assert!(Maze::parse("x", &["#####", "#P.#", "#####"], 0).is_err());
        assert!(Maze::parse("x", &["#####", "#?..#", "#####", "#####", "#####"], 0).is_err());
    }
}
