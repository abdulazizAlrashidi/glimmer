//! The player character: Glim, a small firefly of light.
//!
//! Public fields are read by the renderer.
//!
//! # Controls model
//! - A requested direction is buffered in `queued` for `INPUT_BUFFER_TIME`
//!   seconds after the key was last seen (held keys refresh it every tick).
//! - Requesting the opposite direction reverses immediately, even mid-tile.
//! - At a tile centre Glim turns into the queued direction if it is open,
//!   otherwise keeps going straight, otherwise stops.
//! - Cornering: a perpendicular turn requested within `CORNER_WINDOW` tiles
//!   before a junction's centre (pre-turn) or within `POST_TURN_WINDOW` tiles
//!   after it (post-turn) is taken at that junction. The coordinate that is
//!   now off its lane then converges back at the same speed while Glim
//!   travels in the new direction (a diagonal cut, never a snap).
//! - While cornering, convergence and travel always act on *different* axes.
//!   If Glim reaches a centre on his travel axis before the convergence is
//!   done (after a corner-then-reverse), a perpendicular request whose exit
//!   is open is honoured there: the leftover error simply becomes distance
//!   along the new direction, so no tick ever moves him farther than his
//!   speed allows on either axis.

use crate::actor::{lane_of, Mover};
use crate::config::{CORNER_WINDOW, INPUT_BUFFER_TIME, POST_TURN_WINDOW};
use crate::geom::{Dir, Tile};
use crate::maze::Maze;

#[derive(Clone, Debug)]
pub struct Player {
    pub mover: Mover,
    /// Buffered direction request (see `config::INPUT_BUFFER_TIME`).
    pub queued: Option<Dir>,
    /// Remaining eat-stall time (player does not move while > 0).
    pub stall: f32,
    /// Animation clock in seconds; advances only while actually moving.
    pub anim_t: f32,
    /// Whether the renderer should draw the player at all.
    pub visible: bool,
    /// Seconds since `queued` was last requested.
    queue_age: f32,
}

impl Player {
    /// A player standing on the centre of `start`, facing left.
    pub fn new(start: Tile) -> Player {
        Player {
            mover: Mover::new(start.center(), Dir::Left),
            queued: None,
            stall: 0.0,
            anim_t: 0.0,
            visible: true,
            queue_age: 0.0,
        }
    }

    /// Back to the start tile (new life / new level).
    pub fn reset(&mut self, start: Tile) {
        *self = Player::new(start);
    }

    /// Feed this tick's input: `Some(d)` (re)queues `d`; `None` ages the
    /// queued request and forgets it after `INPUT_BUFFER_TIME`.
    pub fn handle_input(&mut self, input: Option<Dir>, dt: f32) {
        match input {
            Some(d) => {
                self.queued = Some(d);
                self.queue_age = 0.0;
            }
            None => {
                if self.queued.is_some() {
                    self.queue_age += dt.max(0.0);
                    if self.queue_age > INPUT_BUFFER_TIME {
                        self.queued = None;
                        self.queue_age = 0.0;
                    }
                }
            }
        }
    }

    /// True while the player is off its lane after a cornering turn.
    #[cfg(test)]
    pub fn is_cornering(&self) -> bool {
        self.mover.lane_error() != 0.0
    }

    /// Advance the player by `dt` seconds at `speed` tiles/second, honouring
    /// the eat stall, reversal, cornering and centre turning rules. Returns the
    /// distance travelled (along the lane, or onto it while cornering).
    pub fn step(&mut self, maze: &Maze, dt: f32, speed: f32) -> f32 {
        let mut time = dt.max(0.0);
        if self.stall > 0.0 {
            let s = self.stall.min(time);
            self.stall -= s;
            time -= s;
            if self.stall < 1e-6 {
                self.stall = 0.0;
            }
        }
        if time.is_nan() || time <= 0.0 || !speed.is_finite() || speed <= 0.0 {
            return 0.0;
        }
        let dist = speed * time;
        let queued = self.queued;
        let m = &mut self.mover;

        // Immediate reversal (also restarts a player stopped at a wall).
        if queued == Some(m.dir.opposite()) {
            m.dir = m.dir.opposite();
        }
        if let Some(q) = queued {
            try_corner(m, maze, q);
        }

        let moved = travel(m, maze, dist, queued);
        if moved > 0.0 {
            self.anim_t += time * (moved / dist).min(1.0);
        }
        moved
    }
}

/// Cornering, decided at the start of a tick while on the lane: turn into the
/// perpendicular `q` early (within `CORNER_WINDOW` before a junction's centre)
/// or late (within `POST_TURN_WINDOW` after the centre just passed) when that
/// junction's `q` exit is open. Travel continues along the new direction and
/// the old along-axis coordinate becomes the off-lane error to converge.
fn try_corner(m: &mut Mover, maze: &Maze, q: Dir) {
    if q.is_horizontal() == m.dir.is_horizontal() || m.lane_error() != 0.0 {
        return;
    }
    let to_c = m.dist_to_center();
    let junction = if to_c > 0.0 && to_c <= CORNER_WINDOW {
        m.next_center_tile(maze)
    } else if to_c > 0.0 && 1.0 - to_c <= POST_TURN_WINDOW {
        // Just past a centre: still inside that junction's tile.
        m.tile()
    } else {
        return;
    };
    if maze.is_walkable(junction.step(q)) {
        m.dir = q;
    }
}

/// Move `dist` along the current direction. On the lane this is plain grid
/// movement ([`Mover::advance`]); while cornering, the travel axis advances
/// while the cross axis converges, each by at most `dist`.
fn travel(m: &mut Mover, maze: &Maze, dist: f32, queued: Option<Dir>) -> f32 {
    let mut left = dist;
    let mut moved = 0.0;
    // Each pass either finishes, converges the lane, or reaches a centre.
    for _ in 0..8 {
        if left <= 0.0 {
            break;
        }
        let err = m.lane_error();
        if err == 0.0 {
            moved += m.advance(maze, left, |t, d| choose(maze, t, d, queued));
            break;
        }
        if m.at_center() {
            // A centre on the travel axis while still converging (cornering
            // followed by a reversal). The junction is the tile we are in.
            let t = m.tile();
            if let Some(q) = queued.filter(|q| q.is_horizontal() != m.dir.is_horizontal()) {
                if maze.is_walkable(t.step(q)) {
                    // Honour the turn: the leftover error becomes distance
                    // along `q` and the new cross axis is exactly on its lane.
                    m.dir = q;
                    continue;
                }
            }
            if !maze.is_walkable(t.step(m.dir)) {
                // Blocked ahead: only slide onto the lane, then decide again.
                let s = left.min(err.abs());
                converge(m, s, err);
                m.moving = true;
                left -= s;
                moved += s;
                continue;
            }
        }
        // Diagonal cut: both axes move by `s`, stopping at the lane, at the
        // next centre on the travel axis, or when the distance runs out.
        // (|err| < 0.5, so from a centre the next one is never reached.)
        let to_c = m.dist_to_center();
        let s = left.min(err.abs());
        let s = if to_c > 0.0 && s >= to_c {
            m.snap_to_center_ahead(maze);
            to_c
        } else {
            m.shift(maze, s);
            s
        };
        converge(m, s, err);
        m.moving = true;
        left -= s;
        moved += s;
    }
    moved
}

/// Move the cross-axis coordinate `s` toward its lane (`err` = signed offset
/// to the lane), landing exactly on it when `s` covers the whole error.
fn converge(m: &mut Mover, s: f32, err: f32) {
    let cross = if m.dir.is_horizontal() {
        &mut m.pos.y
    } else {
        &mut m.pos.x
    };
    if s >= err.abs() {
        *cross = lane_of(*cross);
    } else {
        *cross += s * err.signum();
    }
}

/// Direction to leave the centre of `t` in, travelling `dir`.
fn choose(maze: &Maze, t: Tile, dir: Dir, queued: Option<Dir>) -> Option<Dir> {
    if let Some(q) = queued {
        if maze.is_walkable(t.step(q)) {
            return Some(q);
        }
    }
    maze.is_walkable(t.step(dir)).then_some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::testutil::{crossroads, find_tile};
    use crate::config::{BASE_SPEED, SIM_DT};
    use macroquad::math::Vec2;

    const EPS: f32 = 1e-4;
    const SPEED: f32 = 0.8 * BASE_SPEED;
    const STEP: f32 = SPEED * SIM_DT;

    fn setup() -> (Maze, Player) {
        let m = Maze::builtin(0);
        let p = Player::new(m.player_start);
        (m, p)
    }

    fn tick(p: &mut Player, m: &Maze, input: Option<Dir>) -> f32 {
        p.mover.begin_tick();
        p.handle_input(input, SIM_DT);
        p.step(m, SIM_DT, SPEED)
    }

    /// The first tile left of the start (on the start row, before any wall)
    /// that satisfies `pred`.
    fn left_of_start(m: &Maze, pred: impl Fn(Tile) -> bool) -> Tile {
        let mut t = m.player_start.step(Dir::Left);
        while m.is_walkable(t) {
            if pred(t) {
                return t;
            }
            t = t.step(Dir::Left);
        }
        panic!("no such tile left of the start");
    }

    fn cx(t: Tile) -> f32 {
        t.x as f32 + 0.5
    }

    fn cy(t: Tile) -> f32 {
        t.y as f32 + 0.5
    }

    #[test]
    fn starts_moving_left_at_exact_speed() {
        let (m, mut p) = setup();
        assert_eq!(p.mover.dir, Dir::Left);
        assert!(!p.mover.moving);
        for _ in 0..10 {
            let moved = tick(&mut p, &m, None);
            assert!((moved - STEP).abs() < EPS);
            assert!(((p.mover.prev_pos - p.mover.pos).length() - STEP).abs() < EPS);
            assert_eq!(p.mover.pos.y, m.player_start.center().y);
        }
        assert!(p.mover.moving);
        assert!(p.mover.pos.x < m.player_start.center().x);
        assert!(p.anim_t > 0.0);
    }

    #[test]
    fn reversal_is_immediate_mid_tile() {
        let (m, mut p) = setup();
        for _ in 0..5 {
            tick(&mut p, &m, None);
        }
        let x = p.mover.pos.x;
        assert!(!p.mover.at_center());
        tick(&mut p, &m, Some(Dir::Right));
        assert_eq!(p.mover.dir, Dir::Right);
        assert!((p.mover.pos.x - (x + STEP)).abs() < EPS);
    }

    #[test]
    fn stops_at_walls_and_restarts_on_request() {
        let (m, mut p) = setup();
        let end = left_of_start(&m, |t| m.is_wall(t.step(Dir::Left)));
        let open: Vec<Dir> = [Dir::Up, Dir::Down]
            .into_iter()
            .filter(|&d| m.is_walkable(end.step(d)))
            .collect();
        assert!(!open.is_empty(), "the row end is a corner");
        for _ in 0..((m.player_start.x - end.x) as f32 / STEP) as usize + 20 {
            tick(&mut p, &m, None);
        }
        assert_eq!(p.mover.pos, end.center());
        assert!(!p.mover.moving);
        let anim = p.anim_t;
        tick(&mut p, &m, None);
        assert_eq!(p.anim_t, anim, "anim clock must not run while stopped");
        // A closed side keeps Glim stuck; an open one restarts him.
        for d in [Dir::Up, Dir::Down, Dir::Left] {
            if !open.contains(&d) {
                tick(&mut p, &m, Some(d));
                assert!(!p.mover.moving, "{d:?} is a wall");
                assert_eq!(p.mover.pos, end.center());
            }
        }
        tick(&mut p, &m, Some(open[0]));
        assert!(p.mover.moving);
        assert_eq!(p.mover.dir, open[0]);
        assert_eq!(p.mover.pos.x, cx(end));
    }

    #[test]
    fn turns_into_queued_direction_at_centre() {
        let (m, mut p) = setup();
        // Hold Up from the start: Glim turns at the first opening upward.
        let j = left_of_start(&m, |t| m.is_walkable(t.step(Dir::Up)));
        let mut turned_at = None;
        let ticks = ((m.player_start.x - j.x) as f32 / STEP) as usize + 12;
        for _ in 0..ticks {
            tick(&mut p, &m, Some(Dir::Up));
            if p.mover.dir == Dir::Up && turned_at.is_none() {
                turned_at = Some(p.mover.prev_pos);
            }
        }
        let at = turned_at.expect("never turned up");
        assert_eq!(p.mover.pos.x, cx(j));
        assert!(p.mover.pos.y < cy(j));
        // The turn was taken within the cornering window of the junction.
        assert!(
            (cx(j)..=cx(j) + CORNER_WINDOW + STEP).contains(&at.x),
            "{at:?}"
        );
    }

    #[test]
    fn cornering_cuts_diagonally_and_converges() {
        let (m, mut p) = setup();
        let j = left_of_start(&m, |t| m.is_walkable(t.step(Dir::Down)));
        // Move left until within the window before the junction.
        while p.mover.pos.x - cx(j) > CORNER_WINDOW - 0.05 {
            tick(&mut p, &m, None);
        }
        let before = p.mover.pos;
        assert!(before.x > cx(j));
        tick(&mut p, &m, Some(Dir::Down));
        assert_eq!(p.mover.dir, Dir::Down);
        assert!(p.is_cornering());
        // Both coordinates advanced by one step: a diagonal cut.
        assert!((p.mover.pos.y - (before.y + STEP)).abs() < EPS);
        assert!((p.mover.pos.x - (before.x - STEP)).abs() < EPS);
        let mut prev = p.mover.pos;
        for _ in 0..7 {
            tick(&mut p, &m, None);
            let d = p.mover.pos - prev;
            assert!((d.y - STEP).abs() < EPS, "primary axis keeps full speed");
            assert!(
                (-STEP - EPS..=0.0).contains(&d.x),
                "cross axis converges, never jumps"
            );
            prev = p.mover.pos;
        }
        assert!(!p.is_cornering());
        assert_eq!(p.mover.pos.x, cx(j));
        assert!(m.is_walkable(p.mover.tile()));
    }

    #[test]
    fn cornering_is_rejected_when_the_side_is_closed() {
        let (m, mut p) = setup();
        let j = left_of_start(&m, |t| m.is_wall(t.step(Dir::Up)));
        while p.mover.pos.x - cx(j) > CORNER_WINDOW - 0.05 {
            tick(&mut p, &m, None);
        }
        // Up is a wall at the junction: the queued Up waits, Glim keeps going.
        tick(&mut p, &m, Some(Dir::Up));
        assert_eq!(p.mover.dir, Dir::Left);
        assert!(!p.is_cornering());
    }

    #[test]
    fn reversing_while_cornering_never_turns_off_lane() {
        let (m, mut p) = setup();
        let j = left_of_start(&m, |t| {
            m.is_walkable(t.step(Dir::Down)) && m.is_wall(t.step(Dir::Up))
        });
        while p.mover.pos.x - cx(j) > CORNER_WINDOW - 0.05 {
            tick(&mut p, &m, None);
        }
        tick(&mut p, &m, Some(Dir::Down));
        tick(&mut p, &m, Some(Dir::Up));
        for _ in 0..40 {
            tick(&mut p, &m, Some(Dir::Up));
            assert!(m.is_walkable(p.mover.tile()));
            assert!(p.mover.pos.y <= cy(j) + EPS);
        }
        // Back on the junction (Up is a wall there), lane restored exactly.
        assert_eq!(p.mover.pos, j.center());
    }

    /// A crossroads with a straight run of open tiles to its right, so Glim
    /// can approach it heading left.
    fn approachable_crossroads(m: &Maze) -> Tile {
        find_tile(m, |t| {
            crossroads(m, t) && (1..=3).all(|k| m.is_walkable(t.offset(k, 0)))
        })
    }

    /// Corner Down into a crossroads from `offset` tiles before its centre,
    /// reverse Up for as many ticks as were spent going down (arriving back on
    /// the junction's centre while still converging), then hold `turn`.
    /// Returns the trace of positions.
    fn corner_reverse_turn(m: &Maze, j: Tile, offset: f32, down: usize, turn: Dir) -> Vec<Vec2> {
        let mut p = Player::new(j);
        p.mover
            .teleport(Vec2::new(cx(j) + offset, cy(j)), Dir::Left);
        let mut trace = vec![p.mover.pos];
        let inputs = std::iter::repeat_n(Dir::Down, down)
            .chain(std::iter::repeat_n(Dir::Up, down))
            .chain(std::iter::repeat_n(turn, 10));
        for input in inputs {
            tick(&mut p, m, Some(input));
            trace.push(p.mover.pos);
        }
        assert_eq!(p.mover.dir, turn, "offset {offset} down {down}");
        trace
    }

    #[test]
    fn corner_reverse_then_turn_is_honoured_without_a_double_step() {
        let m = Maze::builtin(0);
        let j = approachable_crossroads(&m);
        for turn in [Dir::Left, Dir::Right] {
            for k in 1..=30 {
                let offset = CORNER_WINDOW * k as f32 / 31.0;
                for down in 1..=3 {
                    let trace = corner_reverse_turn(&m, j, offset, down, turn);
                    let ctx = format!("{turn:?} offset {offset} down {down}");
                    for w in trace.windows(2) {
                        let d = w[1] - w[0];
                        // Never more than one step on either axis per tick.
                        assert!(d.x.abs() <= STEP + EPS, "{ctx}: x hop {d:?}");
                        assert!(d.y.abs() <= STEP + EPS, "{ctx}: y hop {d:?}");
                    }
                    // The turn happened at the junction: Glim never left the
                    // junction's row after coming back up, and ends up travelling
                    // along it at full speed.
                    let back = 1 + 2 * down;
                    for pos in &trace[back..] {
                        assert_eq!(pos.y, cy(j), "{ctx}: left the row at {pos:?}");
                    }
                    let n = trace.len();
                    let last = trace[n - 1] - trace[n - 2];
                    assert!(
                        (last.x - STEP * turn.vec().x).abs() < EPS,
                        "{ctx}: {last:?}"
                    );
                    // Never moved against the new heading after the turn.
                    for w in trace[back..].windows(2) {
                        assert!((w[1].x - w[0].x) * turn.vec().x >= -EPS, "{ctx}: stutter");
                    }
                }
            }
        }
    }

    #[test]
    fn late_turn_just_past_a_junction_is_honoured() {
        let (m, mut p) = setup();
        let j = left_of_start(&m, |t| m.is_walkable(t.step(Dir::Down)));
        // Glide left just past the junction's centre, then press Down.
        while p.mover.pos.x > cx(j) {
            tick(&mut p, &m, None);
        }
        let before = p.mover.pos;
        assert!(cx(j) - before.x <= POST_TURN_WINDOW);
        tick(&mut p, &m, Some(Dir::Down));
        assert_eq!(p.mover.dir, Dir::Down, "turned at the junction just passed");
        assert_eq!(p.mover.tile(), j);
        // It mirrors a pre-turn: both axes move one step, back toward the lane.
        assert!((p.mover.pos.y - (before.y + STEP)).abs() < EPS);
        assert!(p.mover.pos.x > before.x && p.mover.pos.x <= cx(j));
        for _ in 0..6 {
            tick(&mut p, &m, None);
            assert!(m.is_walkable(p.mover.tile()));
        }
        assert_eq!(p.mover.pos.x, cx(j));
        assert!(!p.is_cornering());
    }

    #[test]
    fn post_turn_window_covers_its_whole_range() {
        let m = Maze::builtin(0);
        let j = left_of_start(&m, |t| m.is_walkable(t.step(Dir::Down)));
        for k in 1..=15 {
            let past = (POST_TURN_WINDOW - 1e-3) * k as f32 / 15.0;
            let mut p = Player::new(j);
            p.mover.teleport(Vec2::new(cx(j) - past, cy(j)), Dir::Left);
            tick(&mut p, &m, Some(Dir::Down));
            assert_eq!(p.mover.dir, Dir::Down, "{past} past the centre");
            assert!(
                p.mover.pos.x >= cx(j) - past,
                "converges back toward the lane"
            );
        }
        // Beyond the window the request waits for the next opening.
        let mut p = Player::new(j);
        p.mover
            .teleport(Vec2::new(cx(j) - POST_TURN_WINDOW - 0.05, cy(j)), Dir::Left);
        tick(&mut p, &m, Some(Dir::Down));
        assert_eq!(p.mover.dir, Dir::Left);
        assert!(!p.is_cornering());
    }

    #[test]
    fn post_turn_is_rejected_when_the_side_is_closed() {
        let m = Maze::builtin(0);
        let j = left_of_start(&m, |t| m.is_wall(t.step(Dir::Up)));
        let mut p = Player::new(j);
        p.mover.teleport(Vec2::new(cx(j) - 0.05, cy(j)), Dir::Left);
        tick(&mut p, &m, Some(Dir::Up));
        assert_eq!(p.mover.dir, Dir::Left);
        assert!(!p.is_cornering());
    }

    #[test]
    fn input_buffer_expires() {
        let (_m, mut p) = setup();
        p.handle_input(Some(Dir::Up), SIM_DT);
        let mut t = 0.0;
        while t < INPUT_BUFFER_TIME - 0.02 {
            p.handle_input(None, SIM_DT);
            t += SIM_DT;
        }
        assert_eq!(p.queued, Some(Dir::Up));
        for _ in 0..10 {
            p.handle_input(None, SIM_DT);
        }
        assert_eq!(p.queued, None);
    }

    #[test]
    fn stall_pauses_movement_exactly() {
        let (m, mut p) = setup();
        p.stall = 2.5 * SIM_DT;
        assert_eq!(tick(&mut p, &m, None), 0.0);
        assert_eq!(tick(&mut p, &m, None), 0.0);
        let moved = tick(&mut p, &m, None);
        assert!((moved - 0.5 * STEP).abs() < EPS);
        assert_eq!(p.stall, 0.0);
    }

    #[test]
    fn reset_restores_start() {
        let (m, mut p) = setup();
        for _ in 0..30 {
            tick(&mut p, &m, Some(Dir::Right));
        }
        p.visible = false;
        p.reset(m.player_start);
        assert_eq!(p.mover.pos, m.player_start.center());
        assert_eq!(p.mover.prev_pos, p.mover.pos);
        assert!(p.visible && p.queued.is_none() && p.anim_t == 0.0);
    }
}
