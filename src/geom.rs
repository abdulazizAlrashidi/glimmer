//! Grid geometry shared by the simulation and the renderer.
//!
//! Coordinate conventions (used everywhere in the crate):
//! - Maze space is measured in tiles. Tile `(x, y)` covers `x..x + 1`
//!   horizontally and `y..y + 1` vertically, and its centre is
//!   `(x + 0.5, y + 0.5)`.
//! - `+x` is right, `+y` is **down** (screen convention).
//! - Actor positions are `Vec2` in maze space and always sit on a "lane":
//!   when moving horizontally `pos.y` is exactly a tile-centre y, and vice versa.

use macroquad::math::Vec2;

/// A cardinal direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    Up,
    Left,
    Down,
    Right,
}

impl Dir {
    /// All four directions in a fixed canonical order (Up, Left, Down, Right).
    /// The shade AI breaks ties by momentum instead (see
    /// `shade::momentum_order`).
    pub const ALL: [Dir; 4] = [Dir::Up, Dir::Left, Dir::Down, Dir::Right];

    /// Integer grid delta `(dx, dy)`.
    pub const fn delta(self) -> (i32, i32) {
        match self {
            Dir::Up => (0, -1),
            Dir::Left => (-1, 0),
            Dir::Down => (0, 1),
            Dir::Right => (1, 0),
        }
    }

    /// Unit vector in maze space.
    pub fn vec(self) -> Vec2 {
        let (dx, dy) = self.delta();
        Vec2::new(dx as f32, dy as f32)
    }

    pub const fn opposite(self) -> Dir {
        match self {
            Dir::Up => Dir::Down,
            Dir::Left => Dir::Right,
            Dir::Down => Dir::Up,
            Dir::Right => Dir::Left,
        }
    }

    pub const fn is_horizontal(self) -> bool {
        matches!(self, Dir::Left | Dir::Right)
    }
}

/// Integer tile coordinate. May lie outside the maze (tunnel virtual tiles,
/// off-board AI targets).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
}

impl Tile {
    pub const fn new(x: i32, y: i32) -> Tile {
        Tile { x, y }
    }

    /// The neighbouring tile in direction `d`.
    pub const fn step(self, d: Dir) -> Tile {
        let (dx, dy) = d.delta();
        Tile {
            x: self.x + dx,
            y: self.y + dy,
        }
    }

    /// The tile `n` steps away in direction `d`.
    pub const fn ahead(self, d: Dir, n: i32) -> Tile {
        let (dx, dy) = d.delta();
        Tile {
            x: self.x + dx * n,
            y: self.y + dy * n,
        }
    }

    pub const fn offset(self, dx: i32, dy: i32) -> Tile {
        Tile {
            x: self.x + dx,
            y: self.y + dy,
        }
    }

    /// Centre of the tile in maze space.
    pub fn center(self) -> Vec2 {
        Vec2::new(self.x as f32 + 0.5, self.y as f32 + 0.5)
    }

    /// The tile containing a maze-space point.
    pub fn from_pos(p: Vec2) -> Tile {
        Tile {
            x: p.x.floor() as i32,
            y: p.y.floor() as i32,
        }
    }

    /// Squared Euclidean distance between tile indices (exact, no floats).
    pub fn dist2(self, o: Tile) -> i64 {
        let dx = (self.x - o.x) as i64;
        let dy = (self.y - o.y) as i64;
        dx * dx + dy * dy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite_is_involution() {
        for d in Dir::ALL {
            assert_eq!(d.opposite().opposite(), d);
            assert_ne!(d.opposite(), d);
        }
    }

    #[test]
    fn tile_center_roundtrip() {
        let t = Tile::new(-1, 7);
        assert_eq!(Tile::from_pos(t.center()), t);
        assert_eq!(t.step(Dir::Right), Tile::new(0, 7));
        assert_eq!(t.ahead(Dir::Down, 3), Tile::new(-1, 10));
        assert_eq!(Tile::new(0, 0).dist2(Tile::new(3, 4)), 25);
    }
}
