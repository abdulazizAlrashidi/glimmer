//! Wall outline geometry, computed once per maze. Pure data with no drawing,
//! so it is easy to test.
//!
//! Walls are drawn as single neon strokes along the lattice of wall-tile
//! centres. A segment joins the centres of two orthogonally adjacent wall
//! tiles when at least one of the tiles on either side of that segment is not
//! a wall (off the board counts as wall, except on tunnel rows). That gives
//! one stroke for 1-tile-thick walls and an inset outline for thick blocks.
//! Degree-2 vertices whose two segments are perpendicular become quarter arcs
//! of radius `CORNER_RADIUS`; isolated wall tiles become dots. Door tiles are
//! handled separately. All coordinates are in maze space (tiles).
//!
//! [`build_outline`] returns those strokes plus the extra data the renderer
//! needs for the full look:
//! - [`Cap`]s: rounded stroke ends at degree-1 lattice vertices;
//! - a [`FillCell`] / [`Sliver`] decomposition of the wall *interior* (the area
//!   on the wall side of the strokes) for a faint "glass" fill. The outer wall
//!   mass (wall reachable from the board margin without crossing a stroke) is
//!   left unfilled, so the board is framed by its double border alone;
//! - a second outline along the board border, offset by [`BORDER_GAP`] toward
//!   the wall side, so the outer wall reads as a double neon tube. It follows
//!   every stroke of the border-connected walls that has a wall side, turning
//!   through corners and through the wall quadrant of junctions with quarter
//!   arcs. Wherever it cannot go on (tunnel mouths, or a wall turning thin, so
//!   neither side is wall) it closes onto the main stroke with a semicircular
//!   "hairpin" arc: the double line never dangles or runs into a stroke.

use crate::geom::{Dir, Tile};
use crate::maze::{Cell, Maze};
use macroquad::math::Vec2;
use std::collections::VecDeque;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Corner rounding radius in tiles.
pub const CORNER_RADIUS: f32 = 0.5;

/// Distance (tiles) between the two lines of the double border.
pub const BORDER_GAP: f32 = 0.26;

/// Segments shorter than this (tiles) are dropped.
const MIN_LEN: f32 = 1e-4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    pub center: Vec2,
    pub radius: f32,
    /// Start angle in radians (screen space, +y down) and sweep (radians).
    pub start: f32,
    pub sweep: f32,
}

#[cfg(test)]
impl Arc {
    /// Point on the arc at parameter `t` in `[0, 1]`.
    pub fn point(&self, t: f32) -> Vec2 {
        let a = self.start + self.sweep * t;
        self.center + Vec2::new(a.cos(), a.sin()) * self.radius
    }

    /// The two end points (start, end).
    pub fn ends(&self) -> (Vec2, Vec2) {
        (self.point(0.0), self.point(1.0))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WallGeometry {
    /// Straight strokes (already shortened where they meet an arc).
    pub segments: Vec<(Vec2, Vec2)>,
    pub arcs: Vec<Arc>,
    /// Isolated wall tiles drawn as round dots (centres).
    pub dots: Vec<Vec2>,
    /// Door bars: (left end, right end) at the door tile's centre line.
    pub doors: Vec<(Vec2, Vec2)>,
}

/// A rounded stroke end at a degree-1 lattice vertex.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cap {
    /// The stroke's end point (a wall-tile centre).
    pub center: Vec2,
    /// Outward unit direction (away from the stroke).
    pub dir: Vec2,
}

/// One cell of the wall interior: the unit square between four mutually
/// adjacent wall-tile centres. Corners flagged in `rounded` are replaced by a
/// quarter circle of `CORNER_RADIUS` centred on the cell centre (the matching
/// stroke arc), so the fill never pokes outside a rounded outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FillCell {
    pub center: Vec2,
    /// Corner flags, bit `i` for corner `i` in the order
    /// top-left, top-right, bottom-right, bottom-left.
    pub rounded: u8,
}

/// The small wall-side area between a concave corner's arc and the sharp
/// lattice corner it replaces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sliver {
    /// The sharp lattice corner (a wall-tile centre).
    pub corner: Vec2,
    /// Centre of the stroke arc (radius `CORNER_RADIUS`).
    pub arc_center: Vec2,
}

/// Everything the renderer draws for one maze's walls.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MazeOutline {
    /// The main strokes.
    pub walls: WallGeometry,
    /// Rounded ends of the main strokes (excluding ends closed by a hairpin).
    pub caps: Vec<Cap>,
    /// Main-stroke ends closed by a border hairpin: no visible cap, but the
    /// soft glow around the stroke is rounded off there too.
    pub closed_caps: Vec<Cap>,
    /// Secondary line of the outer border (segments + arcs incl. hairpins).
    pub border: WallGeometry,
    /// Wall interior decomposition for a translucent fill.
    pub fills: Vec<FillCell>,
    pub slivers: Vec<Sliver>,
}

/// Full outline data for `maze` (see the module docs).
pub fn build_outline(maze: &Maze) -> MazeOutline {
    Grid::from_maze(maze).outline()
}

// ---------------------------------------------------------------------------
// Lattice
// ---------------------------------------------------------------------------

/// Wall occupancy of a maze, with the board-edge rules of [`Maze::is_wall`].
struct Grid {
    w: i32,
    h: i32,
    walls: Vec<bool>,
    tunnel: Vec<bool>,
    doors: Vec<Tile>,
}

/// Unit vector of a direction as `(dx, dy)` floats.
fn unit(d: Dir) -> Vec2 {
    d.vec()
}

/// Lattice point (wall-tile centre) of a tile.
fn vertex_pos(x: i32, y: i32) -> Vec2 {
    Vec2::new(x as f32 + 0.5, y as f32 + 0.5)
}

/// Normalise an angle into `[0, TAU)`.
fn norm_angle(a: f32) -> f32 {
    let r = a.rem_euclid(TAU);
    if r >= TAU - 1e-6 {
        0.0
    } else {
        r
    }
}

/// Positive quarter arc around `center` joining directions `a` and `b`
/// (perpendicular unit vectors, seen from the centre).
fn quarter_arc(center: Vec2, radius: f32, a: Vec2, b: Vec2) -> Arc {
    let from = if a.perp_dot(b) > 0.0 { a } else { b };
    Arc {
        center,
        radius,
        start: norm_angle(from.y.atan2(from.x)),
        sweep: FRAC_PI_2,
    }
}

impl Grid {
    fn from_maze(maze: &Maze) -> Grid {
        let mut walls = Vec::with_capacity((maze.w * maze.h).max(0) as usize);
        let mut doors = Vec::new();
        for y in 0..maze.h {
            for x in 0..maze.w {
                let t = Tile::new(x, y);
                let c = maze.cell(t);
                walls.push(c == Cell::Wall);
                if c == Cell::Door {
                    doors.push(t);
                }
            }
        }
        let tunnel = (0..maze.h).map(|y| maze.is_tunnel_row(y)).collect();
        Grid {
            w: maze.w,
            h: maze.h,
            walls,
            tunnel,
            doors,
        }
    }

    fn in_board(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }

    /// Wall test including the out-of-board rule.
    fn is_wall(&self, x: i32, y: i32) -> bool {
        if self.in_board(x, y) {
            self.walls[(y * self.w + x) as usize]
        } else {
            !(y >= 0 && y < self.h && self.tunnel[y as usize])
        }
    }

    /// Lattice vertices are the in-board wall tiles.
    fn is_vertex(&self, x: i32, y: i32) -> bool {
        self.in_board(x, y) && self.walls[(y * self.w + x) as usize]
    }

    /// Is there a stroke from vertex `(x, y)` toward `d`?
    fn edge(&self, x: i32, y: i32, d: Dir) -> bool {
        let (dx, dy) = d.delta();
        let (bx, by) = (x + dx, y + dy);
        if !self.is_vertex(x, y) || !self.is_vertex(bx, by) {
            return false;
        }
        let (px, py) = (dy.abs(), dx.abs());
        [-1, 1].iter().any(|&s| {
            !self.is_wall(x + s * px, y + s * py) || !self.is_wall(bx + s * px, by + s * py)
        })
    }

    /// Which directions have strokes, in `Dir::ALL` order.
    fn dirs(&self, x: i32, y: i32) -> [bool; 4] {
        Dir::ALL.map(|d| self.edge(x, y, d))
    }

    /// The two stroke directions of a degree-2 perpendicular vertex.
    fn corner_dirs(&self, x: i32, y: i32) -> Option<(Dir, Dir)> {
        let ds = self.dirs(x, y);
        let mut it = Dir::ALL
            .iter()
            .zip(ds)
            .filter(|(_, on)| *on)
            .map(|(d, _)| *d);
        let (a, b) = (it.next()?, it.next()?);
        if it.next().is_some() || a.is_horizontal() == b.is_horizontal() {
            return None;
        }
        Some((a, b))
    }

    fn degree(&self, x: i32, y: i32) -> usize {
        self.dirs(x, y).iter().filter(|&&on| on).count()
    }

    /// Wall side of the unit edge from `(x, y)` toward `d` (`Right`/`Down`):
    /// the perpendicular unit offset whose two flank tiles are both walls, if
    /// exactly one side qualifies.
    fn edge_side(&self, x: i32, y: i32, d: Dir) -> Option<(i32, i32)> {
        let (dx, dy) = d.delta();
        let (px, py) = (dy.abs(), dx.abs());
        let side = |s: i32| {
            self.is_wall(x + s * px, y + s * py) && self.is_wall(x + dx + s * px, y + dy + s * py)
        };
        match (side(-1), side(1)) {
            (true, false) => Some((-px, -py)),
            (false, true) => Some((px, py)),
            _ => None,
        }
    }

    /// Lattice vertices connected (through strokes) to the board edge.
    fn border_component(&self) -> Vec<bool> {
        let mut seen = vec![false; (self.w * self.h).max(0) as usize];
        let mut queue = VecDeque::new();
        for y in 0..self.h {
            for x in 0..self.w {
                let on_edge = x == 0 || y == 0 || x == self.w - 1 || y == self.h - 1;
                if on_edge && self.is_vertex(x, y) {
                    seen[(y * self.w + x) as usize] = true;
                    queue.push_back((x, y));
                }
            }
        }
        while let Some((x, y)) = queue.pop_front() {
            for d in Dir::ALL {
                if self.edge(x, y, d) {
                    let (dx, dy) = d.delta();
                    let i = ((y + dy) * self.w + x + dx) as usize;
                    if !seen[i] {
                        seen[i] = true;
                        queue.push_back((x + dx, y + dy));
                    }
                }
            }
        }
        seen
    }

    fn outline(&self) -> MazeOutline {
        let r = CORNER_RADIUS;
        let g = BORDER_GAP;
        let border = self.border_component();
        let in_border = |x: i32, y: i32| border[(y * self.w + x) as usize];
        let mut out = MazeOutline::default();

        // Straight runs (horizontal, then vertical), merged through straight
        // vertices and junctions, trimmed by `r` at corners.
        for d in [Dir::Right, Dir::Down] {
            let (dx, dy) = d.delta();
            let back = d.opposite();
            for y in 0..self.h {
                for x in 0..self.w {
                    if !self.edge(x, y, d) || self.edge(x, y, back) {
                        continue;
                    }
                    let (mut ex, mut ey) = (x, y);
                    while self.edge(ex, ey, d) {
                        ex += dx;
                        ey += dy;
                    }
                    let dir = unit(d);
                    let trim_a = if self.corner_dirs(x, y).is_some() {
                        r
                    } else {
                        0.0
                    };
                    let trim_b = if self.corner_dirs(ex, ey).is_some() {
                        r
                    } else {
                        0.0
                    };
                    let a = vertex_pos(x, y) + dir * trim_a;
                    let b = vertex_pos(ex, ey) - dir * trim_b;
                    if a.distance(b) > MIN_LEN {
                        out.walls.segments.push((a, b));
                    }
                }
            }
        }

        // The double border: runs of border edges sharing one wall side.
        for d in [Dir::Right, Dir::Down] {
            let (dx, dy) = d.delta();
            for y in 0..self.h {
                for x in 0..self.w {
                    if !self.is_vertex(x, y) || !in_border(x, y) {
                        continue;
                    }
                    let Some(s) = self.side_from(x, y, d) else {
                        continue;
                    };
                    // Only start at the first edge of a run.
                    if self.side_from(x, y, d.opposite()) == Some(s) {
                        continue;
                    }
                    let (mut ex, mut ey) = (x + dx, y + dy);
                    while self.side_from(ex, ey, d) == Some(s) {
                        ex += dx;
                        ey += dy;
                    }
                    let a = self.border_end(&mut out.border, x, y, d, s);
                    let b = self.border_end(&mut out.border, ex, ey, d.opposite(), s);
                    if a.distance(b) > MIN_LEN {
                        let off = Vec2::new(s.0 as f32, s.1 as f32) * g;
                        out.border.segments.push((a + off, b + off));
                    }
                }
            }
        }

        for y in 0..self.h {
            for x in 0..self.w {
                if !self.is_vertex(x, y) {
                    continue;
                }
                let v = vertex_pos(x, y);
                // Corners: quarter arcs (+ fill details, + border offset arc).
                if let Some((da, db)) = self.corner_dirs(x, y) {
                    let (ua, ub) = (unit(da), unit(db));
                    let c = v + (ua + ub) * r;
                    out.walls.arcs.push(quarter_arc(c, r, -ua, -ub));
                    let (ox, oy) = (da.delta().0 + db.delta().0, da.delta().1 + db.delta().1);
                    let convex = self.is_wall(x + ox, y + oy);
                    if !convex {
                        out.slivers.push(Sliver {
                            corner: v,
                            arc_center: c,
                        });
                    }
                }
                // Border arcs: at corners, and in the wall-side quadrants of
                // junctions (where two border runs meet at a right angle).
                if in_border(x, y) {
                    for (da, db) in QUADRANTS {
                        if let Some(rr) = self.border_join(x, y, da, db) {
                            let (ua, ub) = (unit(da), unit(db));
                            let c = v + (ua + ub) * r;
                            out.border.arcs.push(quarter_arc(c, rr, -ua, -ub));
                        }
                    }
                }
                match self.degree(x, y) {
                    0 => {
                        let lonely = Dir::ALL.iter().all(|&d| {
                            let (dx, dy) = d.delta();
                            !self.is_vertex(x + dx, y + dy)
                        });
                        if lonely {
                            out.walls.dots.push(v);
                        }
                    }
                    1 => {
                        let Some(d) = Dir::ALL.into_iter().find(|&d| self.edge(x, y, d)) else {
                            continue;
                        };
                        // A border run ending here is closed by a hairpin
                        // (see `border_end`) instead of a cap.
                        let cap = Cap {
                            center: v,
                            dir: -unit(d),
                        };
                        if !in_border(x, y) || self.side_from(x, y, d).is_none() {
                            out.caps.push(cap);
                        } else {
                            out.closed_caps.push(cap);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Interior fill cells: unit squares whose four corners are walls,
        // except those of the outer wall mass (open to the board margin).
        let outside = self.outside_cells();
        for y in 0..self.h - 1 {
            for x in 0..self.w - 1 {
                if !self.is_fill_cell(x, y) || outside[self.cell_index(x, y)] {
                    continue;
                }
                let corners = [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)];
                let center = Vec2::new(x as f32 + 1.0, y as f32 + 1.0);
                let mut rounded = 0u8;
                for (i, &(cx, cy)) in corners.iter().enumerate() {
                    if let Some((da, db)) = self.corner_dirs(cx, cy) {
                        let c = vertex_pos(cx, cy) + (unit(da) + unit(db)) * r;
                        if c.distance_squared(center) < 1e-6 {
                            rounded |= 1 << i;
                        }
                    }
                }
                out.fills.push(FillCell { center, rounded });
            }
        }
        out.slivers.retain(|sl| !self.sliver_outside(sl, &outside));

        out.walls.doors = self
            .doors
            .iter()
            .map(|t| {
                let y = t.y as f32 + 0.5;
                (Vec2::new(t.x as f32, y), Vec2::new(t.x as f32 + 1.0, y))
            })
            .collect();
        out
    }

    /// Index of the fill cell whose top-left corner is vertex `(x, y)`.
    fn cell_index(&self, x: i32, y: i32) -> usize {
        (y * (self.w - 1) + x) as usize
    }

    /// Is the unit square between vertices `(x, y)` and `(x + 1, y + 1)`
    /// wall interior (all four corners are wall tiles)?
    fn is_fill_cell(&self, x: i32, y: i32) -> bool {
        [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)]
            .iter()
            .all(|&(cx, cy)| self.is_vertex(cx, cy))
    }

    /// Fill cells of the outer wall mass: reachable from the edge of the
    /// lattice (the board margin) without crossing a stroke. They lie beyond
    /// the outermost strokes, outside the playfield, and stay unfilled so the
    /// board is framed by its double neon border alone.
    fn outside_cells(&self) -> Vec<bool> {
        let (cw, ch) = (self.w - 1, self.h - 1);
        let mut outside = vec![false; (cw * ch).max(0) as usize];
        let mut queue = VecDeque::new();
        for y in 0..ch {
            for x in 0..cw {
                let on_edge = x == 0 || y == 0 || x == cw - 1 || y == ch - 1;
                if on_edge && self.is_fill_cell(x, y) {
                    outside[self.cell_index(x, y)] = true;
                    queue.push_back((x, y));
                }
            }
        }
        while let Some((x, y)) = queue.pop_front() {
            // Neighbour cell and whether the shared side is a stroke.
            let sides = [
                ((x, y - 1), self.edge(x, y, Dir::Right)),
                ((x, y + 1), self.edge(x, y + 1, Dir::Right)),
                ((x - 1, y), self.edge(x, y, Dir::Down)),
                ((x + 1, y), self.edge(x + 1, y, Dir::Down)),
            ];
            for ((nx, ny), stroke) in sides {
                if stroke || nx < 0 || ny < 0 || nx >= cw || ny >= ch {
                    continue;
                }
                let i = self.cell_index(nx, ny);
                if !outside[i] && self.is_fill_cell(nx, ny) {
                    outside[i] = true;
                    queue.push_back((nx, ny));
                }
            }
        }
        outside
    }

    /// Does the sliver at a concave corner belong to the outer wall mass
    /// (one of the corner's wall-side cells is outside, or off the lattice)?
    fn sliver_outside(&self, sl: &Sliver, outside: &[bool]) -> bool {
        let (vx, vy) = ((sl.corner.x - 0.5) as i32, (sl.corner.y - 0.5) as i32);
        let open = sl.arc_center - sl.corner;
        [(-1, -1), (1, -1), (1, 1), (-1, 1)]
            .into_iter()
            .filter(|&(sx, sy)| !(sx as f32 * open.x > 0.0 && sy as f32 * open.y > 0.0))
            .any(|(sx, sy)| {
                let (cx, cy) = (vx + sx.min(0), vy + sy.min(0));
                if cx < 0 || cy < 0 || cx >= self.w - 1 || cy >= self.h - 1 {
                    return true;
                }
                outside[self.cell_index(cx, cy)]
            })
    }

    /// Wall side of the stroke from vertex `(x, y)` toward `d`, if that
    /// stroke exists and has one (see [`Grid::edge_side`]).
    fn side_from(&self, x: i32, y: i32, d: Dir) -> Option<(i32, i32)> {
        if !self.edge(x, y, d) {
            return None;
        }
        if matches!(d, Dir::Right | Dir::Down) {
            self.edge_side(x, y, d)
        } else {
            let (dx, dy) = d.delta();
            self.edge_side(x + dx, y + dy, d.opposite())
        }
    }

    /// Radius of the border arc joining the border lines of the strokes from
    /// `(x, y)` toward the perpendicular directions `da` and `db`, if they join.
    ///
    /// They join around the wall-side quadrant between them when each stroke's
    /// wall side points toward the other stroke (a convex corner, or the wall
    /// quadrant of a junction), and around the open quadrant of a concave
    /// corner when both wall sides point away from it.
    fn border_join(&self, x: i32, y: i32, da: Dir, db: Dir) -> Option<f32> {
        let (sa, sb) = (self.side_from(x, y, da)?, self.side_from(x, y, db)?);
        let toward = |s: (i32, i32), d: Dir| s == d.delta();
        let away = |s: (i32, i32), d: Dir| s == d.opposite().delta();
        if toward(sa, db) && toward(sb, da) {
            Some(CORNER_RADIUS - BORDER_GAP)
        } else if self.degree(x, y) == 2 && away(sa, db) && away(sb, da) {
            Some(CORNER_RADIUS + BORDER_GAP)
        } else {
            None
        }
    }

    /// Close one end of a border run and return the main-stroke point the
    /// border line is offset from there.
    ///
    /// The run leaves vertex `(x, y)` toward `d` with wall side `s`. If a
    /// border arc continues it (see [`Grid::border_join`]) the run is trimmed
    /// to the arc. Otherwise the double border ends here: a semicircular
    /// hairpin of diameter [`BORDER_GAP`] folds the border line back onto the
    /// main stroke, at the stroke's own end (degree-1 vertices, e.g. tunnel
    /// mouths) or where the stroke enters its corner arc (where the wall
    /// turns into a thin wall with no wall side to follow).
    fn border_end(&self, g: &mut WallGeometry, x: i32, y: i32, d: Dir, s: (i32, i32)) -> Vec2 {
        let u = unit(d);
        let v = vertex_pos(x, y);
        let joined = Dir::ALL
            .into_iter()
            .filter(|db| db.is_horizontal() != d.is_horizontal())
            .any(|db| self.border_join(x, y, d, db).is_some());
        let corner = self.corner_dirs(x, y).is_some();
        let p = v + u * if joined || corner { CORNER_RADIUS } else { 0.0 };
        if !joined {
            let s = Vec2::new(s.0 as f32, s.1 as f32);
            let out_dir = -u;
            let from = if s.perp_dot(out_dir) > 0.0 { s } else { -s };
            g.arcs.push(Arc {
                center: p + s * (BORDER_GAP * 0.5),
                radius: BORDER_GAP * 0.5,
                start: norm_angle(from.y.atan2(from.x)),
                sweep: PI,
            });
        }
        p
    }
}

/// Pairs of perpendicular directions bounding each quadrant around a vertex.
const QUADRANTS: [(Dir, Dir); 4] = [
    (Dir::Up, Dir::Right),
    (Dir::Right, Dir::Down),
    (Dir::Down, Dir::Left),
    (Dir::Left, Dir::Up),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a grid from ASCII rows: `#` wall, `-` door, anything else open.
    /// Tunnel rows follow the maze rule (first and last tile open).
    fn grid(rows: &[&str]) -> Grid {
        let h = rows.len() as i32;
        let w = rows[0].len() as i32;
        let mut walls = Vec::new();
        let mut doors = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            assert_eq!(row.len() as i32, w);
            for (x, ch) in row.chars().enumerate() {
                walls.push(ch == '#');
                if ch == '-' {
                    doors.push(Tile::new(x as i32, y as i32));
                }
            }
        }
        let tunnel = (0..h)
            .map(|y| !walls[(y * w) as usize] && !walls[(y * w + w - 1) as usize])
            .collect();
        Grid {
            w,
            h,
            walls,
            tunnel,
            doors,
        }
    }

    fn approx(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-4
    }

    fn assert_clean(g: &WallGeometry) {
        for &(a, b) in &g.segments {
            assert!(a.distance(b) > MIN_LEN, "zero-length segment {a:?}");
            assert!(
                (a.x - b.x).abs() < 1e-6 || (a.y - b.y).abs() < 1e-6,
                "diagonal segment"
            );
            for v in [a.x, a.y, b.x, b.y] {
                assert!(v.is_finite());
            }
        }
    }

    /// Every arc end must meet exactly one other primitive end (closed,
    /// tangent-continuous outline around corners).
    fn assert_arcs_connected(g: &WallGeometry) {
        let mut ends: Vec<Vec2> = Vec::new();
        for &(a, b) in &g.segments {
            ends.push(a);
            ends.push(b);
        }
        for arc in &g.arcs {
            let (a, b) = arc.ends();
            ends.push(a);
            ends.push(b);
        }
        for arc in &g.arcs {
            let (a, b) = arc.ends();
            for p in [a, b] {
                let n = ends.iter().filter(|&&e| approx(e, p)).count();
                assert_eq!(n, 2, "arc end {p:?} meets {} other ends", n - 1);
            }
        }
    }

    #[test]
    fn isolated_tile_is_a_dot() {
        let o = grid(&[".....", ".....", "..#..", ".....", "....."]).outline();
        assert_eq!(o.walls.dots, vec![Vec2::new(2.5, 2.5)]);
        assert!(o.walls.segments.is_empty() && o.walls.arcs.is_empty() && o.caps.is_empty());
    }

    #[test]
    fn bar_is_one_segment_with_two_caps() {
        let o = grid(&[".....", ".###.", "....."]).outline();
        assert_eq!(
            o.walls.segments,
            vec![(Vec2::new(1.5, 1.5), Vec2::new(3.5, 1.5))]
        );
        assert!(o.walls.arcs.is_empty());
        assert_eq!(o.caps.len(), 2);
        assert!(o.caps.iter().any(|c| approx(c.dir, Vec2::new(-1.0, 0.0))));
        assert!(o.caps.iter().any(|c| approx(c.dir, Vec2::new(1.0, 0.0))));
        assert!(o.fills.is_empty());
    }

    #[test]
    fn two_by_two_block_is_a_circle() {
        let o = grid(&["....", ".##.", ".##.", "...."]).outline();
        assert!(o.walls.segments.is_empty(), "{:?}", o.walls.segments);
        assert_eq!(o.walls.arcs.len(), 4);
        for a in &o.walls.arcs {
            assert!(approx(a.center, Vec2::new(2.0, 2.0)));
            assert_eq!(a.radius, CORNER_RADIUS);
            assert!((a.sweep - FRAC_PI_2).abs() < 1e-6);
        }
        assert_arcs_connected(&o.walls);
        assert_eq!(
            o.fills,
            vec![FillCell {
                center: Vec2::new(2.0, 2.0),
                rounded: 0b1111
            }]
        );
        assert!(o.caps.is_empty());
    }

    #[test]
    fn thick_block_is_a_rounded_rectangle() {
        let o = grid(&[".....", ".###.", ".###.", ".###.", "....."]).outline();
        assert_eq!(o.walls.segments.len(), 4);
        for &(a, b) in &o.walls.segments {
            assert!((a.distance(b) - 1.0).abs() < 1e-5);
        }
        assert_eq!(o.walls.arcs.len(), 4);
        assert_arcs_connected(&o.walls);
        assert_eq!(o.fills.len(), 4);
        assert!(o.fills.iter().all(|f| f.rounded.count_ones() == 1));
        assert!(o.slivers.is_empty());
    }

    #[test]
    fn l_shape_has_one_arc_and_two_caps() {
        let o = grid(&[".....", ".##..", ".#...", "....."]).outline();
        assert_eq!(o.walls.arcs.len(), 1);
        assert_eq!(o.walls.segments.len(), 2);
        for &(a, b) in &o.walls.segments {
            assert!((a.distance(b) - 0.5).abs() < 1e-5);
        }
        assert_eq!(o.caps.len(), 2);
        assert_arcs_connected(&o.walls);
    }

    #[test]
    fn junctions_go_straight_through() {
        // T: the bar is one run, the stem ends on the bar (no arcs).
        let t = grid(&[".....", ".###.", "..#..", "....."]).outline();
        assert!(t.walls.arcs.is_empty());
        assert_eq!(t.walls.segments.len(), 2);
        assert_eq!(t.caps.len(), 3);
        // Plus: two crossing runs.
        let p = grid(&[".....", "..#..", ".###.", "..#..", "....."]).outline();
        assert!(p.walls.arcs.is_empty());
        assert_eq!(p.walls.segments.len(), 2);
        assert_eq!(p.caps.len(), 4);
    }

    #[test]
    fn concave_corner_gets_a_sliver() {
        // A 3x3 block with a missing corner: the inner corner is concave.
        let o = grid(&["......", ".###..", ".###..", ".##...", "......"]).outline();
        assert_eq!(o.slivers.len(), 1);
        let s = o.slivers[0];
        assert!(approx(s.corner, Vec2::new(2.5, 2.5)));
        assert_arcs_connected(&o.walls);
    }

    #[test]
    fn doors_are_bars_across_the_tile() {
        let o = grid(&["#######", "#.....#", "##.-.##", "#.....#", "#######"]).outline();
        assert_eq!(
            o.walls.doors,
            vec![(Vec2::new(3.0, 2.5), Vec2::new(4.0, 2.5))]
        );
    }

    /// End points of every primitive of `g` (segments, then arcs).
    fn primitive_ends(g: &WallGeometry) -> Vec<Vec2> {
        let mut ends = Vec::new();
        for &(a, b) in &g.segments {
            ends.push(a);
            ends.push(b);
        }
        for arc in &g.arcs {
            let (a, b) = arc.ends();
            ends.push(a);
            ends.push(b);
        }
        ends
    }

    /// Is `p` strictly inside one of the straight main strokes?
    fn inside_main_segment(g: &WallGeometry, p: Vec2) -> bool {
        g.segments.iter().any(|&(a, b)| {
            let (lo, hi) = (a.min(b), a.max(b));
            if (a.x - b.x).abs() < 1e-6 {
                (p.x - a.x).abs() < 1e-4 && p.y > lo.y + 1e-3 && p.y < hi.y - 1e-3
            } else {
                (p.y - a.y).abs() < 1e-4 && p.x > lo.x + 1e-3 && p.x < hi.x - 1e-3
            }
        })
    }

    /// The double border is one closed line per wall stretch: every border
    /// primitive end continues into another border primitive or folds onto
    /// the end of a main stroke; it never dangles or runs into a stroke.
    fn assert_border_closed(o: &MazeOutline, what: &str) {
        let border_ends = primitive_ends(&o.border);
        let main_ends = primitive_ends(&o.walls);
        for (k, &p) in border_ends.iter().enumerate() {
            let partners = border_ends
                .iter()
                .enumerate()
                .filter(|&(j, &e)| j != k && approx(e, p))
                .count();
            let on_main = main_ends.iter().any(|&e| approx(e, p));
            assert!(
                partners + usize::from(on_main) == 1,
                "{what}: border end {p:?} meets {partners} border ends (main end: {on_main})"
            );
            assert!(
                !inside_main_segment(&o.walls, p),
                "{what}: border end {p:?} runs into a main stroke"
            );
        }
        // A border line folding onto a main stroke end replaces its cap.
        for c in &o.caps {
            assert!(
                !border_ends.iter().any(|&e| approx(e, c.center)),
                "{what}: capped stroke end {:?} also closes a border line",
                c.center
            );
        }
        for c in &o.closed_caps {
            assert!(
                border_ends.iter().any(|&e| approx(e, c.center)),
                "{what}: closed stroke end {:?} has no hairpin",
                c.center
            );
        }
    }

    #[test]
    fn every_maze_outline_is_clean() {
        for i in 0..Maze::builtin_count() {
            let m = Maze::builtin(i);
            let what = format!("maze {i} ({})", m.name);
            let o = build_outline(&m);
            let g = &o.walls;
            assert!(
                g.segments.len() > 40,
                "{what}: {} segments",
                g.segments.len()
            );
            assert!(g.arcs.len() > 40, "{what}: {} arcs", g.arcs.len());
            assert_eq!(g.doors.len(), 1, "{what}");
            assert_clean(g);
            assert_clean(&o.border);
            assert_arcs_connected(g);
            assert_border_closed(&o, &what);
            assert!(!o.border.segments.is_empty() && !o.border.arcs.is_empty());
            // The border line runs on the wall side, never in a corridor.
            for &(a, b) in &o.border.segments {
                let t = Tile::from_pos((a + b) * 0.5);
                assert!(
                    m.is_wall(t),
                    "{what}: border segment {a:?}-{b:?} off the wall"
                );
            }
            // Everything stays inside the board.
            for p in primitive_ends(g)
                .into_iter()
                .chain(primitive_ends(&o.border))
            {
                assert!(
                    p.x >= 0.0 && p.y >= 0.0 && p.x <= m.w as f32 && p.y <= m.h as f32,
                    "{what}: {p:?} outside the board"
                );
            }
        }
    }

    #[test]
    fn glass_fill_stays_inside_the_playfield() {
        for i in 0..Maze::builtin_count() {
            let m = Maze::builtin(i);
            let gr = Grid::from_maze(&m);
            let o = gr.outline();
            let filled = |x: i32, y: i32| {
                o.fills
                    .iter()
                    .any(|f| approx(f.center, Vec2::new(x as f32 + 1.0, y as f32 + 1.0)))
            };
            assert!(!o.fills.is_empty(), "maze {i}: inner blocks get glass");
            for f in &o.fills {
                let (x, y) = (f.center.x as i32 - 1, f.center.y as i32 - 1);
                // Never on the board margin...
                assert!(
                    x > 0 && y > 0 && x < gr.w - 2 && y < gr.h - 2,
                    "maze {i}: {f:?}"
                );
                // ...and closed: every side is a stroke or more glass.
                let sides = [
                    ((x, y - 1), gr.edge(x, y, Dir::Right)),
                    ((x, y + 1), gr.edge(x, y + 1, Dir::Right)),
                    ((x - 1, y), gr.edge(x, y, Dir::Down)),
                    ((x + 1, y), gr.edge(x + 1, y, Dir::Down)),
                ];
                for ((nx, ny), stroke) in sides {
                    assert!(
                        stroke || filled(nx, ny),
                        "maze {i}: glass leaks out of {f:?}"
                    );
                }
            }
        }
        // A thick block hanging off the outer wall is outer wall mass, a
        // free-standing one is glass.
        let o = grid(&[
            "#########", //
            "#.......#", //
            "#.###...#", //
            "#.###.###", //
            "#.###.###", //
            "#.....###", //
            "#########",
        ])
        .outline();
        assert_eq!(o.fills.len(), 4, "{:?}", o.fills);
        assert!(o.fills.iter().all(|f| f.center.x < 4.5));
        assert!(o.slivers.is_empty(), "{:?}", o.slivers);
    }

    #[test]
    fn border_closes_where_the_outer_wall_turns_thin() {
        // The outer wall comes down the left edge, then turns into a
        // one-tile-thick ledge over a tunnel: the border hairpins onto the
        // main stroke where it enters the corner arc instead of stopping.
        let o = grid(&[
            "#######", //
            "#.....#", //
            "###.###", //
            ".......", //
            "###.###", //
            "#.....#", //
            "#######",
        ])
        .outline();
        assert_border_closed(&o, "ledge");
        let hairpins: Vec<&Arc> = o
            .border
            .arcs
            .iter()
            .filter(|a| (a.sweep - PI).abs() < 1e-6)
            .collect();
        assert_eq!(hairpins.len(), 4, "{hairpins:?}");
        for h in &hairpins {
            assert!(
                approx(h.center, Vec2::new(0.37, 2.0))
                    || approx(h.center, Vec2::new(0.37, 5.0))
                    || approx(h.center, Vec2::new(6.63, 2.0))
                    || approx(h.center, Vec2::new(6.63, 5.0)),
                "{h:?}"
            );
        }
    }

    #[test]
    fn border_turns_through_the_wall_quadrant_of_a_junction() {
        // A thick corner block whose lower edge continues as a thin ledge to
        // the right (a T-junction at (2, 3)) over a tunnel.
        let o = grid(&[
            "#########", //
            "###.....#", //
            "###.###.#", //
            "######..#", //
            ".........", //
            "#########",
        ])
        .outline();
        assert_border_closed(&o, "junction");
        // The column-2 border line turns into the row-3 border line through a
        // convex arc in the block's quadrant of the junction.
        let want = Arc {
            center: Vec2::new(2.0, 3.0),
            radius: CORNER_RADIUS - BORDER_GAP,
            start: 0.0,
            sweep: FRAC_PI_2,
        };
        assert!(
            o.border.arcs.iter().any(|a| same_arc(a, &want)),
            "{:?}",
            o.border.arcs
        );
        assert!(o
            .border
            .segments
            .iter()
            .any(|&s| same_seg(s, (Vec2::new(2.24, 1.0), Vec2::new(2.24, 3.0)))));
        assert!(o
            .border
            .segments
            .iter()
            .any(|&s| same_seg(s, (Vec2::new(0.5, 3.24), Vec2::new(2.0, 3.24)))));
    }

    #[test]
    fn arcs_only_at_perpendicular_degree_two_vertices() {
        for i in 0..Maze::builtin_count() {
            arcs_only_at_corners(&Maze::builtin(i));
        }
    }

    fn arcs_only_at_corners(m: &Maze) {
        let gr = Grid::from_maze(m);
        let g = build_outline(m).walls;
        let mut corners = 0;
        for y in 0..gr.h {
            for x in 0..gr.w {
                if gr.corner_dirs(x, y).is_some() {
                    corners += 1;
                }
            }
        }
        assert_eq!(g.arcs.len(), corners);
        for arc in &g.arcs {
            assert_eq!(arc.radius, CORNER_RADIUS);
            assert!((arc.sweep - FRAC_PI_2).abs() < 1e-6);
            // The sharp corner sits diagonally from the arc centre, on a wall
            // tile centre with exactly two perpendicular strokes.
            let mid = arc.point(0.5);
            let v = arc.center + (mid - arc.center).normalize() * CORNER_RADIUS * 2f32.sqrt();
            let t = Tile::from_pos(v);
            assert!(approx(v, t.center()), "corner {v:?} is not a tile centre");
            assert!(gr.corner_dirs(t.x, t.y).is_some());
        }
    }

    fn mirror_seg(w: f32, (a, b): (Vec2, Vec2)) -> (Vec2, Vec2) {
        (Vec2::new(w - a.x, a.y), Vec2::new(w - b.x, b.y))
    }

    fn same_seg(p: (Vec2, Vec2), q: (Vec2, Vec2)) -> bool {
        (approx(p.0, q.0) && approx(p.1, q.1)) || (approx(p.0, q.1) && approx(p.1, q.0))
    }

    fn mirror_arc(w: f32, a: &Arc) -> Arc {
        Arc {
            center: Vec2::new(w - a.center.x, a.center.y),
            radius: a.radius,
            start: norm_angle(PI - a.start - a.sweep),
            sweep: a.sweep,
        }
    }

    fn same_arc(p: &Arc, q: &Arc) -> bool {
        let da = (p.start - q.start).rem_euclid(TAU);
        approx(p.center, q.center)
            && (p.radius - q.radius).abs() < 1e-5
            && (p.sweep - q.sweep).abs() < 1e-5
            && (da < 1e-4 || da > TAU - 1e-4)
    }

    fn assert_mirror_symmetric(g: &WallGeometry, w: f32) {
        for &s in &g.segments {
            let m = mirror_seg(w, s);
            assert!(
                g.segments.iter().any(|&q| same_seg(m, q)),
                "no mirror for {s:?}"
            );
        }
        for a in &g.arcs {
            let m = mirror_arc(w, a);
            assert!(
                g.arcs.iter().any(|q| same_arc(&m, q)),
                "no mirror for {a:?}"
            );
        }
        for &d in &g.dots {
            assert!(g.dots.iter().any(|&q| approx(q, Vec2::new(w - d.x, d.y))));
        }
    }

    #[test]
    fn symmetric_mazes_give_symmetric_output() {
        for i in 0..Maze::builtin_count() {
            let m = Maze::builtin(i);
            let o = build_outline(&m);
            assert_mirror_symmetric(&o.walls, m.w as f32);
            assert_mirror_symmetric(&o.border, m.w as f32);
            for c in &o.caps {
                let mc = Vec2::new(m.w as f32 - c.center.x, c.center.y);
                assert!(o.caps.iter().any(|q| approx(q.center, mc)));
            }
        }
        let small = grid(&[".......", ".#...#.", ".##.##.", "...#...", "......."]).outline();
        assert_mirror_symmetric(&small.walls, 7.0);
    }

    #[test]
    fn border_hairpins_close_tunnel_mouths() {
        for i in 0..Maze::builtin_count() {
            let m = Maze::builtin(i);
            let o = build_outline(&m);
            let hairpins: Vec<&Arc> = o
                .border
                .arcs
                .iter()
                .filter(|a| (a.sweep - PI).abs() < 1e-6)
                .collect();
            for a in &hairpins {
                assert!((a.radius - BORDER_GAP * 0.5).abs() < 1e-6);
            }
            // The outer wall above and below each tunnel mouth (both sides)
            // folds its double line closed next to the mouth.
            for &row in &m.tunnel_rows {
                for x in [0.0, m.w as f32] {
                    for above in [true, false] {
                        let found = hairpins.iter().any(|a| {
                            let dy = a.center.y - (row as f32 + 0.5);
                            (a.center.x - x).abs() < 1.5
                                && if above {
                                    dy < 0.0 && dy > -1.6
                                } else {
                                    dy > 0.0 && dy < 1.6
                                }
                        });
                        assert!(
                            found,
                            "maze {i}: tunnel row {row} at x={x} has no hairpin {}",
                            if above { "above" } else { "below" }
                        );
                    }
                }
            }
        }
    }
}
