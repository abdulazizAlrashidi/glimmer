//! The four pursuers ("the Gloom"): their identities, states and AI.
//!
//! Public items are used by the renderer, the audio system and the app.
//!
//! # Targeting
//! At each tile centre a roaming shade takes the non-reversing walkable exit
//! whose next tile is closest (squared Euclidean) to its target tile:
//! - Rook (the Hunter): targets the player's tile.
//! - Vex (the Ambusher): targets `VEX_LOOKAHEAD` tiles ahead of the player.
//! - Flint (the Flanker): takes the tile `FLINT_PIVOT` ahead of the player and
//!   doubles the vector from Rook to it (pincer with Rook).
//! - Moss (the Drifter): chases like Rook while farther than
//!   `MOSS_SHY_RADIUS`; otherwise retreats to its scatter corner.
//! - In Scatter mode each shade targets its `SCATTER_TARGETS` corner (Rook
//!   keeps hunting during its fury).
//! - Frightened: a uniformly random non-reversing exit (deterministic `Rng`).
//! - Eyes: rank exits by true path length to `maze.nest_exit` (a breadth-first
//!   [`DistField`]) rather than straight-line distance, so they always find
//!   their way home in any valid maze; then a scripted descent.
//!
//! Ties keep momentum: when several exits score the same, a shade prefers
//! going straight on, then turning to its own right, then to its own left
//! (relative to its heading, in screen space with +y down). The rule is
//! deterministic and heading-relative, so ties resolve differently depending
//! on where a shade comes from rather than always favouring one compass
//! direction.
//!
//! # Opening formation
//! Rook starts every life roaming from its lair: the walkable tile nearest the
//! top-right corner (see [`rook_lair`]), heading along its first open exit in
//! the order Left, Down, Right, Up. Vex, Flint and Moss start inside the nest
//! (centre, left, right) and are released through the door by the game.
//!
//! # The door
//! Shades never enter door or nest tiles in normal movement; only the scripted
//! states (`InNest`, `Leaving`, `Entering`) move through the nest, in straight
//! lines along the door column. One shade uses the door at a time: returning
//! eyes wait above the door while [`ShadeWorld::door_free`] is false, and a
//! revived shade waits in the nest ([`Shade::revived`]) until the game
//! releases it.

use crate::actor::{wrap_step, DistField, Mover};
use crate::config::{
    LevelSpec, Mode, BASE_SPEED, EYES_SPEED, FLINT_PIVOT, MOSS_SHY_RADIUS, NEST_BOB, NEST_SPEED,
    SCATTER_TARGETS, VEX_LOOKAHEAD,
};
use crate::geom::{Dir, Tile};
use crate::maze::Maze;
use crate::rng::Rng;
use macroquad::math::Vec2;

/// Speed (fraction of base) of eyes descending through the door into the nest.
pub const ENTER_SPEED: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShadeKind {
    Rook,
    Vex,
    Flint,
    Moss,
}

impl ShadeKind {
    pub const ALL: [ShadeKind; 4] = [
        ShadeKind::Rook,
        ShadeKind::Vex,
        ShadeKind::Flint,
        ShadeKind::Moss,
    ];

    pub const fn index(self) -> usize {
        match self {
            ShadeKind::Rook => 0,
            ShadeKind::Vex => 1,
            ShadeKind::Flint => 2,
            ShadeKind::Moss => 3,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            ShadeKind::Rook => "ROOK",
            ShadeKind::Vex => "VEX",
            ShadeKind::Flint => "FLINT",
            ShadeKind::Moss => "MOSS",
        }
    }

    /// Epithet shown on the title-screen cast roll.
    pub const fn title(self) -> &'static str {
        match self {
            ShadeKind::Rook => "THE HUNTER",
            ShadeKind::Vex => "THE AMBUSHER",
            ShadeKind::Flint => "THE FLANKER",
            ShadeKind::Moss => "THE DRIFTER",
        }
    }
}

/// Lifecycle state of a shade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShadeState {
    /// Bobbing up and down at its nest home, waiting to be released (a
    /// revived shade waits at the nest centre with [`Shade::revived`] set).
    InNest,
    /// Scripted: slide to the door column, then rise through the door to
    /// `nest_exit`; then becomes `Active` heading Left.
    Leaving,
    /// Roaming the maze (scatter / chase / frightened).
    Active,
    /// Eaten: only the eyes remain, racing back to `nest_exit` (and hovering
    /// there while another shade is using the door).
    Eyes,
    /// Scripted: eyes descend through the door to the nest centre, then the
    /// shade revives and waits there (`InNest`, `revived`) for the game to
    /// release it.
    Entering,
}

/// How the renderer should draw a shade right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShadeLook {
    Normal,
    /// `flash` = draw the alternate (white) frightened colours this frame.
    Frightened {
        flash: bool,
    },
    Eyes,
}

#[derive(Clone, Debug)]
pub struct Shade {
    pub kind: ShadeKind,
    pub mover: Mover,
    pub state: ShadeState,
    /// Frightened by a flare (meaningful in InNest / Leaving / Active).
    pub frightened: bool,
    /// Current target tile (exposed for the debug overlay).
    pub target: Tile,
    /// Animation clock in seconds (always advancing while the game runs).
    pub anim_t: f32,
    pub visible: bool,
    /// Set by the game on mode changes / fright start; consumed by the AI,
    /// which then reverses direction as soon as possible.
    pub reverse_pending: bool,
    /// Personal dot counter used for nest release.
    pub dot_counter: u32,
    /// Revived after being eaten and waiting in the nest for the door. The
    /// game releases such shades first, as soon as the door is clear,
    /// regardless of dot counters. Cleared when the shade starts leaving.
    pub revived: bool,
}

/// What the shade AI needs to know about the player and Rook to pick targets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetInfo {
    pub player_tile: Tile,
    pub player_dir: Dir,
    pub rook_tile: Tile,
}

/// Read-only world state for one shade movement step.
#[derive(Clone, Copy, Debug)]
pub struct ShadeWorld<'a> {
    pub maze: &'a Maze,
    pub spec: &'a LevelSpec,
    /// Rook's fury stage (0..=2).
    pub fury: u8,
    /// Path distances to `maze.nest_exit` (guides returning eyes).
    pub home_field: &'a DistField,
    /// No *other* shade is in the door column (`Leaving` or `Entering`).
    /// Returning eyes only start their descent while this holds.
    pub door_free: bool,
}

/// A state change that happened during [`Shade::step`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadeTransition {
    None,
    /// Finished climbing out of the nest (`Leaving` -> `Active`).
    Emerged,
    /// Eyes reached the nest exit (`Eyes` -> `Entering`).
    ReachedNest,
    /// Eyes reached the nest centre and the shade revived (`Entering` ->
    /// `InNest` with `revived` set, waiting for the door).
    Revived,
}

impl Shade {
    /// A shade placed at `pos` in `state`, facing `dir`.
    pub fn new(kind: ShadeKind, pos: Vec2, dir: Dir, state: ShadeState) -> Shade {
        Shade {
            kind,
            mover: Mover::new(pos, dir),
            state,
            frightened: false,
            target: Tile::default(),
            anim_t: 0.0,
            visible: true,
            reverse_pending: false,
            dot_counter: 0,
            revived: false,
        }
    }

    /// The shade in its starting placement for `maze`: Rook roaming from its
    /// lair (see [`rook_lair`]), the others bobbing at their nest homes.
    pub fn spawn(kind: ShadeKind, maze: &Maze) -> Shade {
        let mut s = Shade::new(kind, Vec2::ZERO, Dir::Left, ShadeState::InNest);
        s.reset(maze);
        s
    }

    /// Back to the starting placement (new life / level). Keeps `dot_counter`
    /// and the animation clock.
    pub fn reset(&mut self, maze: &Maze) {
        let (pos, dir, state) = match self.kind {
            ShadeKind::Rook => {
                let lair = rook_lair(maze);
                (lair.center(), lair_heading(maze, lair), ShadeState::Active)
            }
            ShadeKind::Vex => (maze.nest_homes[0].center(), Dir::Down, ShadeState::InNest),
            ShadeKind::Flint => (maze.nest_homes[1].center(), Dir::Up, ShadeState::InNest),
            ShadeKind::Moss => (maze.nest_homes[2].center(), Dir::Up, ShadeState::InNest),
        };
        self.mover.teleport(pos, dir);
        self.state = state;
        self.frightened = false;
        self.target = SCATTER_TARGETS[self.kind.index()];
        self.visible = true;
        self.reverse_pending = false;
        self.revived = false;
    }

    /// Only the eyes remain (returning home or descending into the nest).
    pub fn is_eyes(&self) -> bool {
        matches!(self.state, ShadeState::Eyes | ShadeState::Entering)
    }

    /// Begin the scripted exit from the nest (the caller makes sure the door
    /// is clear).
    pub fn start_leaving(&mut self) {
        if self.state == ShadeState::InNest {
            self.state = ShadeState::Leaving;
            self.revived = false;
        }
    }

    /// A flare was eaten: become frightened (unless only eyes remain) and, if
    /// roaming, reverse as soon as possible.
    pub fn frighten(&mut self) {
        if !self.is_eyes() {
            self.frightened = true;
        }
        if self.state == ShadeState::Active {
            self.reverse_pending = true;
        }
    }

    /// Eaten by the player: only the eyes remain.
    pub fn get_eaten(&mut self) {
        self.state = ShadeState::Eyes;
        self.frightened = false;
        self.reverse_pending = false;
    }

    /// The tile this shade aims for when roaming un-frightened in `mode`.
    pub fn chase_target(&self, mode: Mode, fury: u8, info: &TargetInfo) -> Tile {
        let scatter = SCATTER_TARGETS[self.kind.index()];
        let chasing = mode == Mode::Chase || (self.kind == ShadeKind::Rook && fury > 0);
        if !chasing {
            return scatter;
        }
        let p = info.player_tile;
        match self.kind {
            ShadeKind::Rook => p,
            ShadeKind::Vex => p.ahead(info.player_dir, VEX_LOOKAHEAD),
            ShadeKind::Flint => {
                let pivot = p.ahead(info.player_dir, FLINT_PIVOT);
                let r = info.rook_tile;
                Tile::new(2 * pivot.x - r.x, 2 * pivot.y - r.y)
            }
            ShadeKind::Moss => {
                let d2 = self.mover.tile().dist2(p) as f32;
                if d2 > MOSS_SHY_RADIUS * MOSS_SHY_RADIUS {
                    p
                } else {
                    scatter
                }
            }
        }
    }

    /// Refresh `target` for the debug overlay and the AI.
    pub fn update_target(&mut self, maze: &Maze, mode: Mode, fury: u8, info: &TargetInfo) {
        self.target = if self.is_eyes() {
            maze.nest_exit
        } else {
            self.chase_target(mode, fury, info)
        };
    }

    /// Current speed as a fraction of `BASE_SPEED`.
    pub fn speed_fraction(&self, world: &ShadeWorld) -> f32 {
        let spec = world.spec;
        match self.state {
            ShadeState::InNest | ShadeState::Leaving => NEST_SPEED,
            ShadeState::Entering => ENTER_SPEED,
            ShadeState::Eyes => EYES_SPEED,
            ShadeState::Active => {
                if world.maze.is_slow(self.mover.tile()) {
                    spec.shade_tunnel_speed
                } else if self.frightened {
                    spec.shade_fright_speed
                } else if self.kind == ShadeKind::Rook && world.fury >= 2 {
                    spec.fury2_speed
                } else if self.kind == ShadeKind::Rook && world.fury == 1 {
                    spec.fury1_speed
                } else {
                    spec.shade_speed
                }
            }
        }
    }

    /// Move for `dt` seconds according to the current state. `target` must be
    /// up to date (see [`Shade::update_target`]); `rng` drives frightened
    /// choices.
    pub fn step(&mut self, world: &ShadeWorld, dt: f32, rng: &mut Rng) -> ShadeTransition {
        let dist = self.speed_fraction(world) * BASE_SPEED * dt.max(0.0);
        let maze = world.maze;
        match self.state {
            ShadeState::InNest => {
                self.bob(maze, dist);
                ShadeTransition::None
            }
            ShadeState::Leaving => {
                if self.climb_out(maze, dist) {
                    self.state = ShadeState::Active;
                    self.mover.dir = Dir::Left;
                    ShadeTransition::Emerged
                } else {
                    ShadeTransition::None
                }
            }
            ShadeState::Active => {
                if self.reverse_pending {
                    self.reverse_pending = false;
                    self.mover.dir = self.mover.dir.opposite();
                }
                let target = self.target;
                let frightened = self.frightened;
                self.mover.advance(maze, dist, |t, d| {
                    if frightened {
                        choose_random(maze, t, d, rng)
                    } else {
                        choose_toward(maze, t, d, target)
                    }
                });
                ShadeTransition::None
            }
            ShadeState::Eyes => {
                let exit = maze.nest_exit;
                if self.mover.pos != exit.center() {
                    let field = world.home_field;
                    self.mover.advance(maze, dist, |t, d| {
                        if t == exit {
                            None
                        } else {
                            choose_home(maze, field, t, d)
                        }
                    });
                }
                if self.mover.pos != exit.center() {
                    ShadeTransition::None
                } else if world.door_free {
                    self.state = ShadeState::Entering;
                    self.mover.dir = Dir::Down;
                    self.mover.moving = true;
                    ShadeTransition::ReachedNest
                } else {
                    // Hover above the door until the shade using it is through.
                    self.mover.moving = false;
                    ShadeTransition::None
                }
            }
            ShadeState::Entering => {
                let home = maze.nest_homes[0].center();
                self.mover.move_toward(home, dist);
                if self.mover.pos == home {
                    self.frightened = false;
                    self.state = ShadeState::InNest;
                    self.revived = true;
                    self.mover.dir = Dir::Up;
                    ShadeTransition::Revived
                } else {
                    ShadeTransition::None
                }
            }
        }
    }

    /// Idle bob around the nest row while waiting.
    fn bob(&mut self, maze: &Maze, dist: f32) {
        let cy = maze.nest_homes[0].center().y;
        let (lo, hi) = (cy - NEST_BOB, cy + NEST_BOB);
        let m = &mut self.mover;
        if m.dir.is_horizontal() {
            m.dir = Dir::Up;
        }
        m.moving = true;
        let mut left = dist;
        for _ in 0..8 {
            if left <= 0.0 {
                break;
            }
            if m.dir == Dir::Up {
                let room = (m.pos.y - lo).max(0.0);
                if left <= room {
                    m.pos.y -= left;
                    break;
                }
                m.pos.y = lo;
                left -= room;
                m.dir = Dir::Down;
            } else {
                let room = (hi - m.pos.y).max(0.0);
                if left <= room {
                    m.pos.y += left;
                    break;
                }
                m.pos.y = hi;
                left -= room;
                m.dir = Dir::Up;
            }
        }
    }

    /// Scripted exit: to the door column, then straight up to the nest exit.
    /// Returns true on arrival.
    fn climb_out(&mut self, maze: &Maze, dist: f32) -> bool {
        let exit = maze.nest_exit.center();
        let m = &mut self.mover;
        let mut left = dist;
        if m.pos.x != exit.x {
            left -= m.move_toward(Vec2::new(exit.x, m.pos.y), left);
        }
        if left > 0.0 && m.pos.x == exit.x {
            m.move_toward(exit, left);
        }
        m.pos == exit
    }
}

/// Rook's lair: the walkable on-board tile nearest the top-right corner tile
/// `(w - 2, 1)`; ties go to the earlier tile in row-major order. Falls back
/// to the nest exit for a maze without walkable tiles (never a valid maze).
pub fn rook_lair(maze: &Maze) -> Tile {
    let corner = Tile::new(maze.w - 2, 1);
    (0..maze.h)
        .flat_map(|y| (0..maze.w).map(move |x| Tile::new(x, y)))
        .filter(|&t| maze.is_walkable(t))
        .min_by_key(|&t| (t.dist2(corner), t.y, t.x))
        .unwrap_or(maze.nest_exit)
}

/// Rook's starting heading at `lair`: its first open exit in the order Left,
/// Down, Right, Up.
pub fn lair_heading(maze: &Maze, lair: Tile) -> Dir {
    [Dir::Left, Dir::Down, Dir::Right, Dir::Up]
        .into_iter()
        .find(|&d| maze.is_walkable(lair.step(d)))
        .unwrap_or(Dir::Left)
}

/// The direction 90 degrees clockwise on screen (+y down) from `d`: a shade
/// heading `d` turns to its own right by taking it.
const fn right_of(d: Dir) -> Dir {
    match d {
        Dir::Up => Dir::Right,
        Dir::Right => Dir::Down,
        Dir::Down => Dir::Left,
        Dir::Left => Dir::Up,
    }
}

/// Non-reversing headings from `dir` in momentum order: straight on, the
/// shade's right turn, then its left turn.
pub const fn momentum_order(dir: Dir) -> [Dir; 3] {
    [dir, right_of(dir), right_of(dir).opposite()]
}

/// Walkable exits of `t` other than the reverse of `dir`, in momentum order.
fn forward_exits(maze: &Maze, t: Tile, dir: Dir) -> impl Iterator<Item = Dir> + '_ {
    momentum_order(dir)
        .into_iter()
        .filter(move |&d| maze.is_walkable(t.step(d)))
}

/// Dead end (never in a valid maze): turn back if possible.
fn fallback(maze: &Maze, t: Tile, dir: Dir) -> Option<Dir> {
    let rev = dir.opposite();
    maze.is_walkable(t.step(rev)).then_some(rev)
}

/// Target-seeking choice: the non-reversing exit whose next tile is closest
/// (squared Euclidean) to `target`; ties keep momentum (straight on, then the
/// shade's right turn, then its left turn). Off-board tunnel tiles just
/// continue straight.
pub fn choose_toward(maze: &Maze, t: Tile, dir: Dir, target: Tile) -> Option<Dir> {
    if !maze.in_bounds(t) {
        return Some(dir);
    }
    let mut best: Option<(i64, Dir)> = None;
    for d in forward_exits(maze, t, dir) {
        let score = t.step(d).dist2(target);
        if best.is_none_or(|(b, _)| score < b) {
            best = Some((score, d));
        }
    }
    best.map(|(_, d)| d).or_else(|| fallback(maze, t, dir))
}

/// Frightened choice: a uniformly random non-reversing exit.
pub fn choose_random(maze: &Maze, t: Tile, dir: Dir, rng: &mut Rng) -> Option<Dir> {
    if !maze.in_bounds(t) {
        return Some(dir);
    }
    let mut options = [Dir::Up; 4];
    let mut n = 0;
    for d in forward_exits(maze, t, dir) {
        options[n] = d;
        n += 1;
    }
    if n == 0 {
        return fallback(maze, t, dir);
    }
    Some(options[rng.below(n as u32) as usize])
}

/// Eyes: the non-reversing exit with the shortest path home (ties keep
/// momentum, as in [`choose_toward`]).
fn choose_home(maze: &Maze, field: &DistField, t: Tile, dir: Dir) -> Option<Dir> {
    if !maze.in_bounds(t) {
        return Some(dir);
    }
    let mut best: Option<(u32, Dir)> = None;
    for d in forward_exits(maze, t, dir) {
        let score = field
            .get(maze, wrap_step(maze, t, d))
            .map_or(u32::MAX, u32::from);
        if best.is_none_or(|(b, _)| score < b) {
            best = Some((score, d));
        }
    }
    match best {
        Some((u32::MAX, _)) | None => choose_toward(maze, t, dir, maze.nest_exit),
        Some((_, d)) => Some(d),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::testutil::{crossroads, find_tile};
    use crate::config::{level_spec, SIM_DT};

    struct Fixture {
        maze: Maze,
        spec: LevelSpec,
        field: DistField,
    }

    impl Fixture {
        fn new() -> Fixture {
            let maze = Maze::builtin(0);
            let field = DistField::new(&maze, maze.nest_exit);
            Fixture {
                maze,
                spec: level_spec(1),
                field,
            }
        }

        fn world(&self) -> ShadeWorld<'_> {
            ShadeWorld {
                maze: &self.maze,
                spec: &self.spec,
                fury: 0,
                home_field: &self.field,
                door_free: true,
            }
        }
    }

    fn info(p: Tile, d: Dir, rook: Tile) -> TargetInfo {
        TargetInfo {
            player_tile: p,
            player_dir: d,
            rook_tile: rook,
        }
    }

    #[test]
    fn spawn_positions() {
        let f = Fixture::new();
        let m = &f.maze;
        let rook = Shade::spawn(ShadeKind::Rook, m);
        assert_eq!(rook.state, ShadeState::Active);
        assert_eq!(rook.mover.pos, rook_lair(m).center());
        assert_eq!(rook.mover.dir, lair_heading(m, rook_lair(m)));
        assert_ne!(
            rook.mover.pos,
            m.nest_exit.center(),
            "Rook no longer waits at the door"
        );
        for (k, home) in [ShadeKind::Vex, ShadeKind::Flint, ShadeKind::Moss]
            .into_iter()
            .zip(m.nest_homes)
        {
            let s = Shade::spawn(k, m);
            assert_eq!(s.state, ShadeState::InNest);
            assert_eq!(s.mover.pos, home.center());
            assert!(!s.revived);
        }
    }

    #[test]
    fn rook_lair_is_the_open_tile_nearest_the_top_right_corner() {
        for i in 0..Maze::builtin_count() {
            let m = Maze::builtin(i);
            let lair = rook_lair(&m);
            let corner = Tile::new(m.w - 2, 1);
            assert!(m.in_bounds(lair) && m.is_walkable(lair), "maze {i}");
            let d = lair.dist2(corner);
            for t in m.walkable_tiles() {
                assert!(
                    t.dist2(corner) >= d,
                    "maze {i}: {t:?} is nearer than {lair:?}"
                );
            }
            // In the top-right quarter, well away from the nest.
            assert!(lair.x > m.w / 2 && lair.y < m.h / 4, "maze {i}: {lair:?}");
            let heading = lair_heading(&m, lair);
            assert!(m.is_walkable(lair.step(heading)), "maze {i}");
            // The first open exit in the order Left, Down, Right, Up.
            for d in [Dir::Left, Dir::Down, Dir::Right, Dir::Up] {
                if d == heading {
                    break;
                }
                assert!(!m.is_walkable(lair.step(d)), "maze {i}: {d:?} was open");
            }
            // Deterministic, and where Rook respawns after a reset.
            assert_eq!(rook_lair(&m), lair);
            let mut rook = Shade::spawn(ShadeKind::Rook, &m);
            rook.mover.teleport(m.nest_exit.center(), Dir::Up);
            rook.reset(&m);
            assert_eq!(rook.mover.pos, lair.center());
            assert_eq!(rook.mover.dir, heading);
        }
        // Maze 0: the lair is in the top-right flower bed.
        let m = Maze::builtin(0);
        assert_eq!(rook_lair(&m), Tile::new(23, 2));
        assert_eq!(lair_heading(&m, Tile::new(23, 2)), Dir::Left);
    }

    #[test]
    fn chase_targets_follow_personalities() {
        let f = Fixture::new();
        let m = &f.maze;
        let p = Tile::new(10, 20);
        let i = info(p, Dir::Right, Tile::new(4, 18));
        let rook = Shade::spawn(ShadeKind::Rook, m);
        let vex = Shade::spawn(ShadeKind::Vex, m);
        let flint = Shade::spawn(ShadeKind::Flint, m);
        let mut moss = Shade::spawn(ShadeKind::Moss, m);
        assert_eq!(rook.chase_target(Mode::Chase, 0, &i), p);
        assert_eq!(vex.chase_target(Mode::Chase, 0, &i), Tile::new(13, 20));
        // pivot = (11, 20); rook (4, 18) -> target = pivot + (pivot - rook) = (18, 22)
        assert_eq!(flint.chase_target(Mode::Chase, 0, &i), Tile::new(18, 22));
        // Vex looks ahead along the player's heading.
        let up = info(p, Dir::Up, Tile::new(4, 18));
        assert_eq!(vex.chase_target(Mode::Chase, 0, &up), Tile::new(10, 17));
        // Moss chases from farther than 6.5 tiles, retreats when nearer.
        moss.mover.teleport(p.offset(6, 3).center(), Dir::Left); // 6.71 tiles
        assert_eq!(moss.chase_target(Mode::Chase, 0, &i), p);
        moss.mover.teleport(p.offset(5, 4).center(), Dir::Left); // 6.40 tiles
        assert_eq!(moss.chase_target(Mode::Chase, 0, &i), SCATTER_TARGETS[3]);
        moss.mover.teleport(p.offset(-1, 0).center(), Dir::Left);
        assert_eq!(moss.chase_target(Mode::Chase, 0, &i), SCATTER_TARGETS[3]);
        // Scatter: corners, except a furious Rook.
        for s in [&rook, &vex, &flint, &moss] {
            assert_eq!(
                s.chase_target(Mode::Scatter, 0, &i),
                SCATTER_TARGETS[s.kind.index()]
            );
        }
        assert_eq!(rook.chase_target(Mode::Scatter, 1, &i), p);
        assert_eq!(vex.chase_target(Mode::Scatter, 2, &i), SCATTER_TARGETS[1]);
    }

    #[test]
    fn momentum_order_is_straight_right_left() {
        assert_eq!(momentum_order(Dir::Up), [Dir::Up, Dir::Right, Dir::Left]);
        assert_eq!(momentum_order(Dir::Right), [Dir::Right, Dir::Down, Dir::Up]);
        assert_eq!(
            momentum_order(Dir::Down),
            [Dir::Down, Dir::Left, Dir::Right]
        );
        assert_eq!(momentum_order(Dir::Left), [Dir::Left, Dir::Up, Dir::Down]);
        for d in Dir::ALL {
            assert!(!momentum_order(d).contains(&d.opposite()));
        }
    }

    #[test]
    fn target_choice_never_reverses_and_breaks_ties_by_momentum() {
        let m = Maze::builtin(0);
        let t = find_tile(&m, |t| crossroads(&m, t));
        for dir in Dir::ALL {
            // Target straight behind: never reverse; the two side exits tie
            // and the shade's right turn wins.
            let behind = t.ahead(dir.opposite(), 6);
            assert_eq!(
                choose_toward(&m, t, dir, behind),
                Some(momentum_order(dir)[1]),
                "heading {dir:?}"
            );
            // Target on the junction itself: all three exits tie, so the
            // shade keeps going straight.
            assert_eq!(choose_toward(&m, t, dir, t), Some(dir), "heading {dir:?}");
            // A clear winner is taken whatever the momentum.
            let left = momentum_order(dir)[2];
            assert_eq!(
                choose_toward(&m, t, dir, t.ahead(left, 5)),
                Some(left),
                "heading {dir:?}"
            );
        }
        // Off-board tunnel tiles continue straight.
        let y = m.tunnel_rows[0];
        assert_eq!(
            choose_toward(&m, Tile::new(-1, y), Dir::Left, Tile::new(20, 0)),
            Some(Dir::Left)
        );
    }

    #[test]
    fn frightened_choices_are_random_but_legal() {
        let m = Maze::builtin(0);
        let t = find_tile(&m, |t| crossroads(&m, t));
        let mut rng = Rng::new(7);
        let mut seen = [0u32; 4];
        for _ in 0..300 {
            let d = choose_random(&m, t, Dir::Left, &mut rng).unwrap();
            assert_ne!(d, Dir::Right);
            seen[d as usize] += 1;
        }
        // All three forward exits are used, roughly evenly.
        assert_eq!(seen.iter().filter(|&&n| n > 0).count(), 3);
        assert!(seen.iter().all(|&n| n == 0 || n > 60), "{seen:?}");
        // Same seed, same choices.
        let pick = |seed| {
            let mut r = Rng::new(seed);
            (0..20)
                .map(|_| choose_random(&m, t, Dir::Up, &mut r))
                .collect::<Vec<_>>()
        };
        assert_eq!(pick(11), pick(11));
    }

    #[test]
    fn active_shades_stay_on_walkable_lanes() {
        let f = Fixture::new();
        let w = f.world();
        let mut rng = Rng::new(3);
        let mut s = Shade::spawn(ShadeKind::Rook, &f.maze);
        let player = f.maze.player_start;
        for i in 0..20_000 {
            s.mover.begin_tick();
            s.frightened = (i / 2000) % 2 == 1;
            let rook = s.mover.tile();
            s.update_target(&f.maze, Mode::Chase, 0, &info(player, Dir::Left, rook));
            let before = s.mover.dir;
            s.step(&w, SIM_DT, &mut rng);
            let p = s.mover.pos;
            assert!(f.maze.is_walkable(s.mover.tile()), "on wall at {p:?}");
            let lane_ok = if s.mover.dir.is_horizontal() {
                p.y.fract() == 0.5
            } else {
                p.x.fract() == 0.5 || p.x.fract() == -0.5
            };
            assert!(lane_ok, "off lane at {p:?}");
            // Never reverses on its own.
            if s.mover.moving {
                assert_ne!(s.mover.dir, before.opposite(), "reversed at {p:?}");
            }
        }
    }

    #[test]
    fn rook_reaches_the_player_tile_when_chasing() {
        for i in 0..Maze::builtin_count() {
            let maze = Maze::builtin(i);
            let field = DistField::new(&maze, maze.nest_exit);
            let spec = level_spec(1);
            let w = ShadeWorld {
                maze: &maze,
                spec: &spec,
                fury: 0,
                home_field: &field,
                door_free: true,
            };
            let mut rng = Rng::new(1);
            let mut s = Shade::spawn(ShadeKind::Rook, &maze);
            let player = maze.player_start;
            let mut reached = false;
            for _ in 0..3000 {
                s.update_target(
                    &maze,
                    Mode::Chase,
                    0,
                    &info(player, Dir::Left, s.mover.tile()),
                );
                s.step(&w, SIM_DT, &mut rng);
                if s.mover.tile() == player {
                    reached = true;
                    break;
                }
            }
            assert!(reached, "maze {i}");
        }
    }

    #[test]
    fn reverse_pending_reverses_once() {
        let f = Fixture::new();
        let w = f.world();
        let mut rng = Rng::new(1);
        let mut s = Shade::spawn(ShadeKind::Rook, &f.maze);
        for _ in 0..30 {
            s.step(&w, SIM_DT, &mut rng);
        }
        let d = s.mover.dir;
        s.reverse_pending = true;
        s.step(&w, SIM_DT, &mut rng);
        assert_eq!(s.mover.dir, d.opposite());
        assert!(!s.reverse_pending);
    }

    #[test]
    fn nest_bob_stays_in_bounds() {
        let f = Fixture::new();
        let w = f.world();
        let mut rng = Rng::new(1);
        let mut s = Shade::spawn(ShadeKind::Flint, &f.maze);
        let home = f.maze.nest_homes[1].center();
        let (mut min, mut max) = (f32::MAX, f32::MIN);
        for _ in 0..600 {
            s.step(&w, SIM_DT, &mut rng);
            assert_eq!(s.mover.pos.x, home.x);
            min = min.min(s.mover.pos.y);
            max = max.max(s.mover.pos.y);
        }
        assert!((min - (home.y - NEST_BOB)).abs() < 0.05);
        assert!((max - (home.y + NEST_BOB)).abs() < 0.05);
        assert!(min >= home.y - NEST_BOB - 1e-4 && max <= home.y + NEST_BOB + 1e-4);
    }

    #[test]
    fn leaving_climbs_out_through_the_door() {
        let f = Fixture::new();
        let w = f.world();
        let mut rng = Rng::new(1);
        for kind in [ShadeKind::Vex, ShadeKind::Flint, ShadeKind::Moss] {
            let mut s = Shade::spawn(kind, &f.maze);
            for _ in 0..37 {
                s.step(&w, SIM_DT, &mut rng); // bob a little first
            }
            s.start_leaving();
            assert_eq!(s.state, ShadeState::Leaving);
            let door_x = f.maze.door.x as f32 + 0.5;
            let mut emerged = false;
            for _ in 0..400 {
                let before = s.mover.pos;
                if s.step(&w, SIM_DT, &mut rng) == ShadeTransition::Emerged {
                    emerged = true;
                    break;
                }
                // Vertical motion happens only on the door column.
                if s.mover.pos.y != before.y {
                    assert_eq!(s.mover.pos.x, door_x);
                }
                let step = (s.mover.pos - before).length();
                assert!(step <= NEST_SPEED * BASE_SPEED * SIM_DT + 1e-4);
            }
            assert!(emerged, "{kind:?} never left");
            assert_eq!(s.state, ShadeState::Active);
            assert_eq!(s.mover.pos, f.maze.nest_exit.center());
            assert_eq!(s.mover.dir, Dir::Left);
        }
    }

    #[test]
    fn eyes_return_home_enter_and_revive() {
        let f = Fixture::new();
        let m = &f.maze;
        let w = f.world();
        let mut rng = Rng::new(1);
        // Start the eyes far away, heading in awkward directions: near every
        // corner of the board, in the tunnel, and right next to the exit.
        let nearest = |c: Tile| {
            m.walkable_tiles()
                .into_iter()
                .min_by_key(|t| t.dist2(c))
                .unwrap()
        };
        let starts = [
            (nearest(Tile::new(0, m.h - 1)), Dir::Left),
            (nearest(Tile::new(m.w - 1, 0)), Dir::Up),
            (nearest(Tile::new(0, 0)), Dir::Right),
            (nearest(Tile::new(m.w - 1, m.h - 1)), Dir::Down),
            (Tile::new(-1, m.tunnel_rows[0]), Dir::Left),
            (m.nest_exit.step(Dir::Right), Dir::Right),
        ];
        for (start, dir) in starts {
            let mut s = Shade::spawn(ShadeKind::Vex, m);
            s.state = ShadeState::Active;
            s.frightened = true;
            s.mover.teleport(start.center(), dir);
            s.get_eaten();
            assert!(s.is_eyes() && !s.frightened);
            let mut log = Vec::new();
            for _ in 0..2000 {
                s.update_target(
                    m,
                    Mode::Chase,
                    0,
                    &info(m.player_start, Dir::Left, m.player_start),
                );
                let tr = s.step(&w, SIM_DT, &mut rng);
                if tr != ShadeTransition::None {
                    log.push(tr);
                }
                if s.state == ShadeState::Eyes {
                    assert!(m.is_walkable(s.mover.tile()));
                    assert_eq!(s.target, m.nest_exit);
                }
                if tr == ShadeTransition::Revived {
                    break;
                }
            }
            assert_eq!(
                log,
                vec![ShadeTransition::ReachedNest, ShadeTransition::Revived],
                "from {start:?}"
            );
            // Revived: waiting at the nest centre for the game to release it.
            assert_eq!(s.state, ShadeState::InNest);
            assert!(s.revived && !s.frightened && !s.is_eyes());
            assert_eq!(s.mover.pos, m.nest_homes[0].center());
            for _ in 0..60 {
                assert_eq!(s.step(&w, SIM_DT, &mut rng), ShadeTransition::None);
                assert_eq!(s.state, ShadeState::InNest, "waits until released");
            }
            s.start_leaving();
            assert_eq!(s.state, ShadeState::Leaving);
            assert!(!s.revived);
        }
    }

    #[test]
    fn eyes_wait_above_the_door_while_it_is_in_use() {
        let f = Fixture::new();
        let m = &f.maze;
        let mut w = f.world();
        w.door_free = false;
        let mut rng = Rng::new(1);
        let mut s = Shade::spawn(ShadeKind::Flint, m);
        s.state = ShadeState::Active;
        s.mover
            .teleport(m.nest_exit.step(Dir::Left).center(), Dir::Right);
        s.get_eaten();
        for _ in 0..240 {
            assert_eq!(s.step(&w, SIM_DT, &mut rng), ShadeTransition::None);
        }
        assert_eq!(s.state, ShadeState::Eyes, "hovering above the door");
        assert_eq!(s.mover.pos, m.nest_exit.center());
        assert!(!s.mover.moving);
        w.door_free = true;
        assert_eq!(s.step(&w, SIM_DT, &mut rng), ShadeTransition::ReachedNest);
        assert_eq!(s.state, ShadeState::Entering);
    }

    #[test]
    fn speeds_follow_the_level_spec() {
        let f = Fixture::new();
        let mut w = f.world();
        let mut s = Shade::spawn(ShadeKind::Rook, &f.maze);
        assert_eq!(s.speed_fraction(&w), f.spec.shade_speed);
        w.fury = 1;
        assert_eq!(s.speed_fraction(&w), f.spec.fury1_speed);
        w.fury = 2;
        assert_eq!(s.speed_fraction(&w), f.spec.fury2_speed);
        s.frightened = true;
        assert_eq!(s.speed_fraction(&w), f.spec.shade_fright_speed);
        let slow = find_tile(&f.maze, |t| f.maze.is_walkable(t) && f.maze.is_slow(t));
        s.mover.teleport(slow.center(), Dir::Left);
        assert_eq!(s.speed_fraction(&w), f.spec.shade_tunnel_speed);
        let y = f.maze.tunnel_rows[0];
        s.mover.teleport(Tile::new(-1, y).center(), Dir::Left);
        assert_eq!(s.speed_fraction(&w), f.spec.shade_tunnel_speed);
        s.get_eaten();
        assert_eq!(s.speed_fraction(&w), EYES_SPEED);
        let v = Shade::spawn(ShadeKind::Vex, &f.maze);
        assert_eq!(v.speed_fraction(&w), NEST_SPEED);
    }

    #[test]
    fn frighten_rules() {
        let f = Fixture::new();
        let mut rook = Shade::spawn(ShadeKind::Rook, &f.maze);
        let mut vex = Shade::spawn(ShadeKind::Vex, &f.maze);
        rook.frighten();
        vex.frighten();
        assert!(rook.frightened && rook.reverse_pending);
        assert!(vex.frightened && !vex.reverse_pending);
        rook.get_eaten();
        rook.frighten();
        assert!(!rook.frightened);
        assert_ne!(rook.state, ShadeState::Active);
    }
}
