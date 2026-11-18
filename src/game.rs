//! The gameplay simulation: rules, timers, scoring, collisions and events.
//!
//! Everything `pub` here is used by the renderer, the audio system and the
//! app.
//!
//! The game is advanced only through [`Game::update`] with a fixed step
//! (`config::SIM_DT`). It never touches the window, audio or the clock, so it is
//! fully deterministic for a given seed and input sequence and runs headless
//! in tests.
//!
//! ## Phase flow
//! ```text
//! new() -> Ready -> Playing -+-> EatPause -> Playing
//!            ^               +-> Caught -> Dying -+-> Ready (lives left)
//!            |               |                    +-> GameOver
//!            |               +-> LevelClear -+-> Ready (next level)
//!            +-------------------------------+-> Interlude -(end_interlude)-> Ready
//! ```
//! Demo games (`opts.demo`) never enter `Interlude` and end in `GameOver` after
//! the first death.
//!
//! ## Order of one `Playing` tick
//! 1. scatter/chase schedule (paused during a fright), fright countdown;
//! 2. player input + movement, then eating the dot under the player;
//! 3. relic timer / pickup, nest release (dot counters, idle timer);
//! 4. shade targets + movement;
//! 5. player/shade collisions.
//!
//! ## Opening formation
//! Every life starts with Rook already roaming from its lair near the
//! top-right corner (`shade::rook_lair`) and Vex, Flint and Moss waiting in
//! the nest.
//!
//! ## Nest release and the door
//! Only one shade uses the door at a time. While a shade is `Leaving` or
//! `Entering`, nobody else is released and returning eyes hover above the
//! door; eyes waiting there go in before the next regular release.
//!
//! A shade revived after being eaten waits at the nest centre
//! (`Shade::revived`) and is released first, as soon as the door is clear,
//! ignoring every dot counter.
//!
//! Otherwise the waiting (non-revived) shade with the lowest index is "next".
//! Normally each dot eaten advances its personal counter and it leaves once
//! the counter reaches `spec.release_dots`. After a life is lost a global dot
//! counter takes over (`GLOBAL_RELEASE_DOTS`) until Moss has left. If no dot
//! is eaten for `spec.idle_release` seconds the next shade is released anyway.

use crate::actor::{wrapped_distance, DistField};
use crate::config::{
    interlude_after, level_spec, maze_for_level, BonusKind, LevelSpec, Mode, BASE_SPEED,
    BONUS_TIME_MAX, BONUS_TIME_MIN, BONUS_TRIGGER_FRACTIONS, CAUGHT_TIME, COLLIDE_DIST, DEATH_TIME,
    EAT_PAUSE_TIME, FLARE_POINTS, FLARE_STALL, FRIGHT_FLASH_PERIOD, FULL_CHAIN_BONUS,
    GAME_OVER_TIME, GLOBAL_RELEASE_DOTS, LEVEL_CLEAR_FLASH, LEVEL_CLEAR_HOLD, MOTE_POINTS,
    MOTE_STALL, POPUP_TIME, READY_TIME, READY_TIME_FIRST, SHADE_CHAIN_POINTS,
};
use crate::geom::{Dir, Tile};
use crate::maze::{Cell, Maze};
use crate::player::Player;
use crate::rng::Rng;
use crate::shade::{Shade, ShadeKind, ShadeLook, ShadeState, ShadeWorld, TargetInfo};
use macroquad::math::Vec2;

/// Largest step `update` will simulate at once (seconds); longer steps are
/// clamped so a stalled caller cannot tunnel actors through each other.
pub const MAX_STEP: f32 = 0.05;

/// Parameters fixed for a whole game.
#[derive(Clone, Debug)]
pub struct GameOptions {
    /// Lives at the start, including the one in play (1..=9).
    pub start_lives: u32,
    /// Score at which one extra life is awarded (`None` = never).
    pub extra_life_at: Option<u32>,
    pub seed: u64,
    /// Attract-mode demo: autopilot input, no extra lives/interludes, 1 life.
    pub demo: bool,
    /// First level (1-based).
    pub start_level: u32,
    /// Best score so far (for the HUD; updated live as the score passes it).
    pub high_score: u32,
}

impl Default for GameOptions {
    fn default() -> Self {
        GameOptions {
            start_lives: 3,
            extra_life_at: Some(15_000),
            seed: 1,
            demo: false,
            start_level: 1,
            high_score: 0,
        }
    }
}

/// Current phase. Every `t` is the time elapsed **in** that phase (counts up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// "GET SET!" hold. `total` is its full length (READY_TIME_FIRST or READY_TIME).
    Ready {
        t: f32,
        total: f32,
    },
    Playing,
    /// Freeze after eating shade `shade` (index) worth `points`.
    EatPause {
        t: f32,
        shade: usize,
        points: u32,
    },
    /// Freeze after the player was caught.
    Caught {
        t: f32,
    },
    /// Player death animation (shades hidden).
    Dying {
        t: f32,
    },
    /// Level complete: hold, then shades hidden and walls flash.
    LevelClear {
        t: f32,
    },
    /// Waiting for the app to play interlude `variant` and call `end_interlude`.
    Interlude {
        variant: u8,
    },
    GameOver {
        t: f32,
    },
}

/// Something noteworthy that happened during `update`; drained by the app and
/// forwarded to audio and renderer (particles, shake).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GameEvent {
    /// A Ready phase began. `first` = very first Ready of a new game (play intro).
    ReadyStarted {
        first: bool,
    },
    /// Ready ended; actors start moving.
    PlayStarted,
    MoteEaten {
        tile: Tile,
    },
    FlareEaten {
        tile: Tile,
    },
    ShadeEaten {
        kind: ShadeKind,
        points: u32,
        pos: Vec2,
    },
    /// All four shades eaten during one fright (FULL_CHAIN_BONUS awarded).
    FullChain {
        points: u32,
        pos: Vec2,
    },
    BonusSpawned {
        kind: BonusKind,
        pos: Vec2,
    },
    BonusEaten {
        kind: BonusKind,
        points: u32,
        pos: Vec2,
    },
    BonusExpired,
    FrightStarted,
    FrightEnded,
    ModeChanged(Mode),
    /// Rook entered fury stage 1 or 2.
    FuryStarted {
        stage: u8,
    },
    PlayerCaught {
        pos: Vec2,
    },
    DeathAnimStarted {
        pos: Vec2,
    },
    ExtraLife,
    LevelCleared {
        level: u32,
    },
    LevelStarted {
        level: u32,
    },
    InterludeStarted {
        variant: u8,
    },
    GameOver {
        score: u32,
        level: u32,
    },
}

/// Which ambient loop the audio system should be playing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoopHint {
    Silent,
    /// Background hum; stage 0..=3 rises as the level empties.
    Hum(u8),
    Fright,
    Eyes,
}

/// A floating score label in maze space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Popup {
    pub pos: Vec2,
    pub points: u32,
    /// Seconds since creation (removed after `POPUP_TIME`).
    pub age: f32,
    /// Emphasised style (shade chain / full chain).
    pub big: bool,
}

/// The relic currently on the board.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveBonus {
    pub kind: BonusKind,
    pub pos: Vec2,
    pub age: f32,
    pub duration: f32,
}

#[derive(Clone, Debug)]
pub struct Game {
    pub opts: GameOptions,
    pub maze: Maze,
    /// Current level, 1-based.
    pub level: u32,
    pub spec: LevelSpec,
    pub score: u32,
    /// max(opts.high_score, score).
    pub high_score: u32,
    /// Reserve lives (not counting the one in play).
    pub lives: u32,
    pub player: Player,
    /// Indexed by `ShadeKind::index()`.
    pub shades: [Shade; 4],
    pub phase: Phase,
    /// Current scatter/chase mode.
    pub mode: Mode,
    /// Remaining fright time (> 0 while a fright is active).
    pub fright_timer: f32,
    /// Shades eaten during the current fright.
    pub chain: u32,
    pub bonus: Option<ActiveBonus>,
    /// Relics collected, most recent last (HUD shows the last BONUS_HISTORY_LEN).
    pub bonus_history: Vec<BonusKind>,
    pub popups: Vec<Popup>,
    /// Pending events (drain with `take_events`).
    pub events: Vec<GameEvent>,
    /// Total simulated seconds since `new` (drives ambient animation).
    pub time: f32,
    /// Seconds spent in `Playing` on the current level.
    pub level_time: f32,
    /// Rook fury stage 0..=2.
    pub fury: u8,
    pub extra_life_awarded: bool,
    pub rng: Rng,
    /// Index into `spec.schedule` of the current scatter/chase entry.
    mode_index: usize,
    /// Seconds spent in the current schedule entry.
    mode_timer: f32,
    /// Seconds since the player last ate a dot (idle nest release).
    idle_timer: f32,
    /// After a lost life, the global dot counter releases shades.
    global_release: bool,
    global_dots: u32,
    /// How many `BONUS_TRIGGER_FRACTIONS` have fired this level.
    bonus_triggers: usize,
    /// Path distances to the nest exit (returning eyes).
    home_field: DistField,
}

impl Game {
    /// Start a new game at `opts.start_level` in the first Ready phase.
    /// Emits `LevelStarted` and `ReadyStarted { first: true }`.
    pub fn new(opts: GameOptions) -> Game {
        let level = opts.start_level.max(1);
        let spec = level_spec(level);
        let maze = Maze::builtin(maze_for_level(level));
        let lives = if opts.demo {
            0
        } else {
            opts.start_lives.clamp(1, 9) - 1
        };
        let player = Player::new(maze.player_start);
        let shades = ShadeKind::ALL.map(|k| Shade::spawn(k, &maze));
        let home_field = DistField::new(&maze, maze.nest_exit);
        let mode = first_mode(&spec);
        let mut game = Game {
            high_score: opts.high_score,
            rng: Rng::new(opts.seed),
            opts,
            maze,
            level,
            spec,
            score: 0,
            lives,
            player,
            shades,
            phase: Phase::Ready {
                t: 0.0,
                total: READY_TIME_FIRST,
            },
            mode,
            fright_timer: 0.0,
            chain: 0,
            bonus: None,
            bonus_history: Vec::new(),
            popups: Vec::with_capacity(8),
            events: Vec::with_capacity(16),
            time: 0.0,
            level_time: 0.0,
            fury: 0,
            extra_life_awarded: false,
            mode_index: 0,
            mode_timer: 0.0,
            idle_timer: 0.0,
            global_release: false,
            global_dots: 0,
            bonus_triggers: 0,
            home_field,
        };
        game.events.push(GameEvent::LevelStarted { level });
        game.events.push(GameEvent::ReadyStarted { first: true });
        game
    }

    /// Advance the simulation by one fixed step `dt` (normally `SIM_DT`).
    /// `input` is the direction currently requested by the player (latest
    /// pressed-and-held arrow, or a fresh tap), `None` when nothing is pressed.
    pub fn update(&mut self, dt: f32, input: Option<Dir>) {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, MAX_STEP)
        } else {
            0.0
        };
        self.player.mover.begin_tick();
        for s in &mut self.shades {
            s.mover.begin_tick();
            s.anim_t += dt;
        }
        self.time += dt;
        self.age_popups(dt);

        match self.phase {
            Phase::Ready { t, total } => {
                self.player.handle_input(input, dt);
                let t = t + dt;
                if t >= total {
                    self.start_playing();
                } else {
                    self.phase = Phase::Ready { t, total };
                }
            }
            Phase::Playing => self.tick_playing(dt, input),
            Phase::EatPause { t, shade, points } => {
                self.player.handle_input(input, dt);
                self.move_returning_shades(dt, shade);
                let t = t + dt;
                if t >= EAT_PAUSE_TIME {
                    self.phase = Phase::Playing;
                    self.player.visible = true;
                    if let Some(s) = self.shades.get_mut(shade) {
                        s.visible = true;
                    }
                } else {
                    self.phase = Phase::EatPause { t, shade, points };
                }
            }
            Phase::Caught { t } => {
                let t = t + dt;
                if t >= CAUGHT_TIME {
                    self.phase = Phase::Dying { t: 0.0 };
                    self.bonus = None;
                    for s in &mut self.shades {
                        s.visible = false;
                    }
                    self.events.push(GameEvent::DeathAnimStarted {
                        pos: self.player.mover.pos,
                    });
                } else {
                    self.phase = Phase::Caught { t };
                }
            }
            Phase::Dying { t } => {
                let t = t + dt;
                if t >= DEATH_TIME {
                    self.finish_death();
                } else {
                    self.phase = Phase::Dying { t };
                }
            }
            Phase::LevelClear { t } => {
                let t = t + dt;
                if t >= LEVEL_CLEAR_HOLD {
                    for s in &mut self.shades {
                        s.visible = false;
                    }
                }
                if t >= LEVEL_CLEAR_HOLD + LEVEL_CLEAR_FLASH {
                    let interlude = if self.opts.demo {
                        None
                    } else {
                        interlude_after(self.level)
                    };
                    match interlude {
                        Some(variant) => {
                            self.phase = Phase::Interlude { variant };
                            self.events.push(GameEvent::InterludeStarted { variant });
                        }
                        None => self.next_level(),
                    }
                } else {
                    self.phase = Phase::LevelClear { t };
                }
            }
            Phase::Interlude { .. } => {}
            Phase::GameOver { t } => {
                self.phase = Phase::GameOver { t: t + dt };
            }
        }
    }

    /// Drain pending events.
    pub fn take_events(&mut self) -> Vec<GameEvent> {
        std::mem::take(&mut self.events)
    }

    /// How shade `i` should be drawn this frame (frightened flashing included).
    pub fn shade_look(&self, i: usize) -> ShadeLook {
        let Some(s) = self.shades.get(i) else {
            return ShadeLook::Normal;
        };
        if s.is_eyes() {
            ShadeLook::Eyes
        } else if s.frightened {
            ShadeLook::Frightened {
                flash: self.fright_flash(),
            }
        } else {
            ShadeLook::Normal
        }
    }

    /// Which ambient loop should play (Silent outside `Playing`).
    pub fn loop_hint(&self) -> LoopHint {
        if !matches!(self.phase, Phase::Playing | Phase::EatPause { .. }) {
            return LoopHint::Silent;
        }
        if self.shades.iter().any(Shade::is_eyes) {
            LoopHint::Eyes
        } else if self.fright_active() {
            LoopHint::Fright
        } else {
            let p = self.dots_progress();
            let stage = if p < 0.40 {
                0
            } else if p < 0.65 {
                1
            } else if p < 0.85 {
                2
            } else {
                3
            };
            LoopHint::Hum(stage)
        }
    }

    /// True while a fright is active.
    pub fn fright_active(&self) -> bool {
        self.fright_timer > 0.0
    }

    /// Fraction of this level's dots eaten, 0..=1.
    pub fn dots_progress(&self) -> f32 {
        if self.maze.total_dots == 0 {
            1.0
        } else {
            self.maze.dots_eaten() as f32 / self.maze.total_dots as f32
        }
    }

    /// True once the GameOver phase has lasted `GAME_OVER_TIME`.
    pub fn is_over(&self) -> bool {
        matches!(self.phase, Phase::GameOver { t } if t >= GAME_OVER_TIME)
    }

    /// Called by the app when the interlude scene finished or was skipped:
    /// advances to the next level's Ready phase. No-op in other phases.
    pub fn end_interlude(&mut self) {
        if matches!(self.phase, Phase::Interlude { .. }) {
            self.next_level();
        }
    }

    // ----- debug / test hooks (used by tests and the app's --shot mode) -----

    /// Jump from Ready straight to Playing.
    pub fn debug_skip_ready(&mut self) {
        if matches!(self.phase, Phase::Ready { .. }) {
            self.start_playing();
        }
    }

    /// Start a fright exactly as if a flare had been eaten (no dot removed).
    pub fn debug_start_fright(&mut self) {
        self.start_fright();
    }

    /// Remove dots until only `leave` remain (keeps the ones nearest the player
    /// start last), without awarding points.
    pub fn debug_clear_dots(&mut self, leave: u32) {
        let start = self.maze.player_start;
        let mut dots: Vec<Tile> = self.maze.dots().map(|(t, _)| t).collect();
        let keep = leave as usize;
        if dots.len() <= keep {
            return;
        }
        // Farthest first; the stable sort keeps row-major order among equals.
        dots.sort_by_key(|t| std::cmp::Reverse(t.dist2(start)));
        let remove = dots.len() - keep;
        for &t in &dots[..remove] {
            self.maze.take_dot(t);
        }
        // Relic thresholds already passed are consumed without spawning.
        let eaten = self.maze.dots_eaten();
        self.bonus_triggers = (0..BONUS_TRIGGER_FRACTIONS.len())
            .take_while(|&i| eaten >= self.bonus_threshold(i))
            .count();
        self.update_fury();
    }

    /// Make the nearest non-frightened shade catch the player immediately.
    pub fn debug_kill_player(&mut self) {
        if !matches!(
            self.phase,
            Phase::Ready { .. } | Phase::Playing | Phase::EatPause { .. }
        ) {
            return;
        }
        let ppos = self.player.mover.pos;
        let nearest = |pred: &dyn Fn(&Shade) -> bool| {
            self.shades
                .iter()
                .enumerate()
                .filter(|(_, s)| pred(s))
                .map(|(i, s)| (i, wrapped_distance(&self.maze, s.mover.pos, ppos)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        };
        let i = nearest(&|s| !s.is_eyes() && !s.frightened)
            .or_else(|| nearest(&|s| !s.is_eyes()))
            .unwrap_or(0);
        for s in &mut self.shades {
            s.visible = true;
        }
        let s = &mut self.shades[i];
        s.state = ShadeState::Active;
        s.frightened = false;
        s.reverse_pending = false;
        s.mover.pos = ppos;
        s.mover.prev_pos = ppos;
        self.player.visible = true;
        self.catch_player();
    }

    // ----- internals ---------------------------------------------------------

    fn start_playing(&mut self) {
        self.phase = Phase::Playing;
        self.events.push(GameEvent::PlayStarted);
    }

    fn tick_playing(&mut self, dt: f32, input: Option<Dir>) {
        self.level_time += dt;
        self.update_mode(dt);
        self.update_fright(dt);

        // Player.
        self.player.handle_input(input, dt);
        let frac = if self.fright_active() {
            self.spec.player_fright_speed
        } else {
            self.spec.player_speed
        };
        self.player.step(&self.maze, dt, frac * BASE_SPEED);

        // Eating.
        self.idle_timer += dt;
        let tile = self.player.mover.tile();
        if let Some(cell) = self.maze.take_dot(tile) {
            self.on_dot_eaten(tile, cell);
        }
        if self.maze.dots_left() == 0 {
            self.start_level_clear();
            return;
        }

        self.update_bonus(dt);
        self.update_release();
        self.move_shades(dt);
        self.check_collisions();
    }

    /// Scatter/chase schedule (paused while a fright is active).
    fn update_mode(&mut self, dt: f32) {
        if self.fright_active() {
            return;
        }
        let schedule = self.spec.schedule;
        let Some(&(_, duration)) = schedule.get(self.mode_index) else {
            return;
        };
        self.mode_timer += dt;
        if self.mode_timer >= duration && self.mode_index + 1 < schedule.len() {
            self.mode_timer -= duration;
            self.mode_index += 1;
            self.mode = schedule[self.mode_index].0;
            for s in &mut self.shades {
                if s.state == ShadeState::Active {
                    s.reverse_pending = true;
                }
            }
            self.events.push(GameEvent::ModeChanged(self.mode));
        }
    }

    fn update_fright(&mut self, dt: f32) {
        if self.fright_timer <= 0.0 {
            return;
        }
        self.fright_timer -= dt;
        if self.fright_timer <= 0.0 {
            self.end_fright();
            self.events.push(GameEvent::FrightEnded);
        }
    }

    fn end_fright(&mut self) {
        self.fright_timer = 0.0;
        self.chain = 0;
        for s in &mut self.shades {
            s.frightened = false;
        }
    }

    fn start_fright(&mut self) {
        self.chain = 0;
        let time = self.spec.fright_time;
        if time > 0.0 {
            self.fright_timer = time;
        }
        for s in &mut self.shades {
            if time > 0.0 {
                s.frighten();
            } else if s.state == ShadeState::Active {
                s.reverse_pending = true;
            }
        }
        if time > 0.0 {
            self.events.push(GameEvent::FrightStarted);
        }
    }

    fn on_dot_eaten(&mut self, tile: Tile, cell: Cell) {
        if cell == Cell::Flare {
            self.add_score(FLARE_POINTS);
            self.player.stall = FLARE_STALL;
            self.events.push(GameEvent::FlareEaten { tile });
            self.start_fright();
        } else {
            self.add_score(MOTE_POINTS);
            self.player.stall = MOTE_STALL;
            self.events.push(GameEvent::MoteEaten { tile });
        }
        self.idle_timer = 0.0;
        if self.global_release {
            self.global_dots += 1;
        } else if let Some(i) = self.next_waiting() {
            self.shades[i].dot_counter += 1;
        }
        self.check_bonus_trigger();
        self.update_fury();
    }

    fn bonus_threshold(&self, i: usize) -> u32 {
        let total = self.maze.total_dots as f32;
        ((BONUS_TRIGGER_FRACTIONS[i] * total).ceil() as u32).max(1)
    }

    fn check_bonus_trigger(&mut self) {
        let eaten = self.maze.dots_eaten();
        let mut fired = false;
        while self.bonus_triggers < BONUS_TRIGGER_FRACTIONS.len()
            && eaten >= self.bonus_threshold(self.bonus_triggers)
        {
            self.bonus_triggers += 1;
            fired = true;
        }
        if fired {
            self.spawn_bonus();
        }
    }

    fn spawn_bonus(&mut self) {
        let kind = self.spec.bonus;
        let pos = self.maze.bonus_tile.center();
        let duration = self.rng.range_f32(BONUS_TIME_MIN, BONUS_TIME_MAX);
        self.bonus = Some(ActiveBonus {
            kind,
            pos,
            age: 0.0,
            duration,
        });
        self.events.push(GameEvent::BonusSpawned { kind, pos });
    }

    fn update_bonus(&mut self, dt: f32) {
        let Some(mut b) = self.bonus else {
            return;
        };
        b.age += dt;
        if wrapped_distance(&self.maze, self.player.mover.pos, b.pos) < COLLIDE_DIST {
            let points = b.kind.points();
            self.bonus = None;
            self.add_score(points);
            self.popups.push(Popup {
                pos: b.pos,
                points,
                age: 0.0,
                big: false,
            });
            self.bonus_history.push(b.kind);
            self.events.push(GameEvent::BonusEaten {
                kind: b.kind,
                points,
                pos: b.pos,
            });
        } else if b.age >= b.duration {
            self.bonus = None;
            self.events.push(GameEvent::BonusExpired);
        } else {
            self.bonus = Some(b);
        }
    }

    fn update_fury(&mut self) {
        let left = self.maze.dots_left();
        let stage = if left <= self.spec.fury2_dots {
            2
        } else if left <= self.spec.fury1_dots {
            1
        } else {
            0
        };
        while self.fury < stage {
            self.fury += 1;
            self.events
                .push(GameEvent::FuryStarted { stage: self.fury });
        }
    }

    /// The waiting shade with the lowest index (revived shades excepted:
    /// they leave as soon as the door is clear and need no counter).
    fn next_waiting(&self) -> Option<usize> {
        self.shades
            .iter()
            .position(|s| s.state == ShadeState::InNest && !s.revived)
    }

    /// True while some shade is climbing out or descending through the door.
    fn door_in_use(&self) -> bool {
        self.shades.iter().any(in_door)
    }

    fn update_release(&mut self) {
        if self.global_release && self.shades[ShadeKind::Moss.index()].state != ShadeState::InNest {
            self.global_release = false;
        }
        if self.door_in_use() {
            return; // one at a time through the door
        }
        // Revived shades first, regardless of any counter.
        if let Some(s) = self
            .shades
            .iter_mut()
            .find(|s| s.state == ShadeState::InNest && s.revived)
        {
            s.start_leaving();
            return;
        }
        // Eyes waiting above the door go in before the next regular release.
        let exit = self.maze.nest_exit.center();
        if self
            .shades
            .iter()
            .any(|s| s.state == ShadeState::Eyes && s.mover.pos == exit)
        {
            return;
        }
        let Some(i) = self.next_waiting() else {
            return;
        };
        let k = self.shades[i].kind.index();
        let due = if self.global_release {
            k == 0 || self.global_dots >= GLOBAL_RELEASE_DOTS[k - 1]
        } else {
            self.shades[i].dot_counter >= self.spec.release_dots[k]
        };
        if due {
            self.shades[i].start_leaving();
        } else if self.idle_timer >= self.spec.idle_release {
            self.shades[i].start_leaving();
            self.idle_timer = 0.0;
        }
    }

    fn target_info(&self) -> TargetInfo {
        TargetInfo {
            player_tile: self.player.mover.tile(),
            player_dir: self.player.mover.dir,
            rook_tile: self.shades[ShadeKind::Rook.index()].mover.tile(),
        }
    }

    fn move_shades(&mut self, dt: f32) {
        self.step_shades(dt, |_, _| true);
    }

    /// During an eat pause only eyes keep moving (not the one just eaten).
    fn move_returning_shades(&mut self, dt: f32, eaten: usize) {
        self.step_shades(dt, |i, s| i != eaten && s.is_eyes());
    }

    /// Retarget and move every shade selected by `pick`, in index order. The
    /// door state is re-read before each shade moves, so two shades never
    /// enter the door column in the same tick.
    fn step_shades(&mut self, dt: f32, pick: impl Fn(usize, &Shade) -> bool) {
        let info = self.target_info();
        for i in 0..self.shades.len() {
            if !pick(i, &self.shades[i]) {
                continue;
            }
            let door_free = self
                .shades
                .iter()
                .enumerate()
                .all(|(j, s)| j == i || !in_door(s));
            let world = ShadeWorld {
                maze: &self.maze,
                spec: &self.spec,
                fury: self.fury,
                home_field: &self.home_field,
                door_free,
            };
            let s = &mut self.shades[i];
            s.update_target(world.maze, self.mode, self.fury, &info);
            s.step(&world, dt, &mut self.rng);
        }
    }

    fn check_collisions(&mut self) {
        let ppos = self.player.mover.pos;
        let hit = self.shades.iter().position(|s| {
            s.state == ShadeState::Active
                && wrapped_distance(&self.maze, s.mover.pos, ppos) < COLLIDE_DIST
        });
        match hit {
            Some(i) if self.shades[i].frightened => self.eat_shade(i),
            Some(_) => self.catch_player(),
            None => {}
        }
    }

    fn eat_shade(&mut self, i: usize) {
        let points = SHADE_CHAIN_POINTS[(self.chain as usize).min(SHADE_CHAIN_POINTS.len() - 1)];
        self.chain += 1;
        self.add_score(points);
        let s = &mut self.shades[i];
        let pos = s.mover.pos;
        let kind = s.kind;
        s.get_eaten();
        s.visible = false;
        self.player.visible = false;
        self.freeze_movers();
        self.popups.push(Popup {
            pos,
            points,
            age: 0.0,
            big: true,
        });
        self.events
            .push(GameEvent::ShadeEaten { kind, points, pos });
        if self.chain == ShadeKind::ALL.len() as u32 {
            self.add_score(FULL_CHAIN_BONUS);
            let bonus_pos = Vec2::new(pos.x, (pos.y - 1.0).max(0.5));
            self.popups.push(Popup {
                pos: bonus_pos,
                points: FULL_CHAIN_BONUS,
                age: 0.0,
                big: true,
            });
            self.events.push(GameEvent::FullChain {
                points: FULL_CHAIN_BONUS,
                pos,
            });
        }
        self.phase = Phase::EatPause {
            t: 0.0,
            shade: i,
            points,
        };
    }

    fn catch_player(&mut self) {
        self.freeze_movers();
        self.phase = Phase::Caught { t: 0.0 };
        self.events.push(GameEvent::PlayerCaught {
            pos: self.player.mover.pos,
        });
    }

    fn freeze_movers(&mut self) {
        self.player.mover.moving = false;
        for s in &mut self.shades {
            s.mover.moving = false;
        }
    }

    fn finish_death(&mut self) {
        if self.opts.demo || self.lives == 0 {
            self.phase = Phase::GameOver { t: 0.0 };
            self.player.visible = false;
            for s in &mut self.shades {
                s.visible = false;
            }
            self.events.push(GameEvent::GameOver {
                score: self.score,
                level: self.level,
            });
            return;
        }
        self.lives -= 1;
        self.reset_actors();
        self.global_release = true;
        self.global_dots = 0;
        self.phase = Phase::Ready {
            t: 0.0,
            total: READY_TIME,
        };
        self.events.push(GameEvent::ReadyStarted { first: false });
    }

    /// Actors back to their starting places, fright/relic/schedule reset.
    fn reset_actors(&mut self) {
        self.player.reset(self.maze.player_start);
        for s in &mut self.shades {
            s.reset(&self.maze);
        }
        self.fright_timer = 0.0;
        self.chain = 0;
        self.mode_index = 0;
        self.mode_timer = 0.0;
        self.mode = first_mode(&self.spec);
        self.idle_timer = 0.0;
        self.bonus = None;
    }

    fn start_level_clear(&mut self) {
        self.end_fright();
        self.bonus = None;
        self.freeze_movers();
        self.player.visible = true;
        self.phase = Phase::LevelClear { t: 0.0 };
        self.events
            .push(GameEvent::LevelCleared { level: self.level });
    }

    fn next_level(&mut self) {
        self.level = self.level.saturating_add(1);
        self.spec = level_spec(self.level);
        let index = maze_for_level(self.level) % Maze::builtin_count().max(1);
        if index == self.maze.index {
            self.maze.reset_dots();
        } else {
            self.maze = Maze::builtin(index);
            self.home_field = DistField::new(&self.maze, self.maze.nest_exit);
        }
        self.reset_actors();
        for s in &mut self.shades {
            s.dot_counter = 0;
        }
        self.global_release = false;
        self.global_dots = 0;
        self.bonus_triggers = 0;
        self.fury = 0;
        self.popups.clear();
        self.level_time = 0.0;
        self.phase = Phase::Ready {
            t: 0.0,
            total: READY_TIME,
        };
        self.events
            .push(GameEvent::LevelStarted { level: self.level });
        self.events.push(GameEvent::ReadyStarted { first: false });
    }

    fn add_score(&mut self, points: u32) {
        self.score = self.score.saturating_add(points);
        self.high_score = self.high_score.max(self.score);
        if !self.opts.demo && !self.extra_life_awarded {
            if let Some(at) = self.opts.extra_life_at {
                if self.score >= at {
                    self.extra_life_awarded = true;
                    self.lives += 1;
                    self.events.push(GameEvent::ExtraLife);
                }
            }
        }
    }

    fn age_popups(&mut self, dt: f32) {
        for p in &mut self.popups {
            p.age += dt;
        }
        self.popups.retain(|p| p.age < POPUP_TIME);
    }

    /// Frightened shades show their alternate colours during the final
    /// `fright_flashes` periods, white first.
    fn fright_flash(&self) -> bool {
        let window = self.spec.fright_flashes as f32 * FRIGHT_FLASH_PERIOD;
        if self.fright_timer <= 0.0 || self.fright_timer > window {
            return false;
        }
        let elapsed = window - self.fright_timer;
        ((elapsed / (FRIGHT_FLASH_PERIOD * 0.5)) as u32).is_multiple_of(2)
    }
}

/// Is this shade in the door column (climbing out or descending)?
fn in_door(s: &Shade) -> bool {
    matches!(s.state, ShadeState::Leaving | ShadeState::Entering)
}

fn first_mode(spec: &LevelSpec) -> Mode {
    spec.schedule.first().map_or(Mode::Chase, |e| e.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::testutil::{longest_corridor, tunnel_mouths};
    use crate::autopilot::Autopilot;
    use crate::config::SIM_DT;
    use crate::shade::rook_lair;

    static SCATTER_ONLY: [(Mode, f32); 1] = [(Mode::Scatter, f32::INFINITY)];

    fn playing_with(opts: GameOptions) -> Game {
        let mut g = Game::new(opts);
        g.debug_skip_ready();
        g.take_events();
        g
    }

    fn playing() -> Game {
        playing_with(GameOptions::default())
    }

    /// No mode changes and no shade on the board: Rook joins the others in
    /// the nest and nobody is released (tests put shades out explicitly).
    fn calm(g: &mut Game) {
        g.spec.schedule = &SCATTER_ONLY;
        g.spec.idle_release = f32::INFINITY;
        g.spec.release_dots = [u32::MAX; 4];
        let home = g.maze.nest_homes[0].center();
        let rook = &mut g.shades[ShadeKind::Rook.index()];
        rook.state = ShadeState::InNest;
        rook.mover.teleport(home, Dir::Up);
    }

    /// Freeze Glim in the off-board tunnel tile of the lowest tunnel row, on
    /// the left: as far from Rook's top-right lair and scatter corner as the
    /// maze allows, and on walkable ground so eaten shades can find home.
    fn park_player(g: &mut Game) {
        let y = *g.maze.tunnel_rows.last().expect("a tunnel row");
        g.player
            .mover
            .teleport(Tile::new(-1, y).center(), Dir::Left);
        g.player.stall = 1.0e6;
    }

    /// Place shade `i` roaming in the middle of the longest corridor, a
    /// quarter tile past a centre heading right: it can reverse there without
    /// reaching a junction within a few ticks.
    fn roam_mid_corridor(g: &mut Game, i: usize) {
        let (y, x0, _) = longest_corridor(&g.maze);
        let s = &mut g.shades[i];
        s.state = ShadeState::Active;
        s.frightened = false;
        s.reverse_pending = false;
        s.mover
            .teleport(Vec2::new(x0 as f32 + 2.75, y as f32 + 0.5), Dir::Right);
    }

    /// The current scatter/chase schedule position: (entry index, seconds
    /// spent in it).
    fn mode_clock(g: &Game) -> (usize, f32) {
        (g.mode_index, g.mode_timer)
    }

    /// Shades in the door column (climbing out or descending).
    fn in_door_count(g: &Game) -> usize {
        g.shades.iter().filter(|s| in_door(s)).count()
    }

    fn ticks(g: &mut Game, n: usize) -> Vec<GameEvent> {
        let mut out = Vec::new();
        for _ in 0..n {
            g.update(SIM_DT, None);
            out.extend(g.take_events());
        }
        out
    }

    fn secs(s: f32) -> usize {
        (s / SIM_DT).ceil() as usize
    }

    /// Tick until `pred` holds (at most `max_s` seconds); returns the events.
    fn run_until(g: &mut Game, max_s: f32, pred: impl Fn(&Game) -> bool) -> Vec<GameEvent> {
        let mut out = Vec::new();
        for _ in 0..secs(max_s) {
            if pred(g) {
                return out;
            }
            g.update(SIM_DT, None);
            out.extend(g.take_events());
        }
        assert!(
            pred(g),
            "condition not reached within {max_s}s (phase {:?})",
            g.phase
        );
        out
    }

    /// Eat `n` motes far from the player through the normal scoring path
    /// (without disturbing a parked player's stall).
    fn eat_far_motes(g: &mut Game, n: u32) {
        let stall = g.player.stall;
        let start = g.maze.player_start;
        let mut motes: Vec<Tile> = g
            .maze
            .dots()
            .filter(|&(_, c)| c == Cell::Mote)
            .map(|(t, _)| t)
            .collect();
        motes.sort_by_key(|t| std::cmp::Reverse(t.dist2(start)));
        for &t in motes.iter().take(n as usize) {
            let c = g.maze.take_dot(t).expect("dot present");
            g.on_dot_eaten(t, c);
        }
        g.player.stall = g.player.stall.max(stall);
    }

    fn feed_shade(g: &mut Game, i: usize) {
        let p = g.player.mover.pos;
        let s = &mut g.shades[i];
        s.state = ShadeState::Active;
        s.mover.teleport(p, Dir::Left);
    }

    #[test]
    fn new_game_starts_in_ready() {
        let mut g = Game::new(GameOptions::default());
        assert_eq!(
            g.phase,
            Phase::Ready {
                t: 0.0,
                total: READY_TIME_FIRST
            }
        );
        assert_eq!(g.level, 1);
        assert_eq!(g.lives, 2);
        assert_eq!(g.score, 0);
        assert_eq!(g.mode, Mode::Scatter);
        assert_eq!(
            g.take_events(),
            vec![
                GameEvent::LevelStarted { level: 1 },
                GameEvent::ReadyStarted { first: true }
            ]
        );
        assert!(g.take_events().is_empty());
        let demo = Game::new(GameOptions {
            demo: true,
            start_level: 0,
            ..GameOptions::default()
        });
        assert_eq!(demo.lives, 0);
        assert_eq!(demo.level, 1);
        let hs = Game::new(GameOptions {
            high_score: 777,
            start_lives: 50,
            ..GameOptions::default()
        });
        assert_eq!(hs.high_score, 777);
        assert_eq!(hs.lives, 8);
    }

    #[test]
    fn ready_holds_then_plays() {
        let mut g = Game::new(GameOptions::default());
        g.take_events();
        let start = g.player.mover.pos;
        let rook = g.shades[0].mover.pos;
        let ev = ticks(&mut g, secs(READY_TIME_FIRST) - 2);
        assert!(ev.is_empty());
        assert!(matches!(g.phase, Phase::Ready { .. }));
        assert_eq!(g.player.mover.pos, start);
        assert_eq!(g.shades[0].mover.pos, rook);
        assert_eq!(g.loop_hint(), LoopHint::Silent);
        let ev = ticks(&mut g, 3);
        assert_eq!(ev, vec![GameEvent::PlayStarted]);
        assert_eq!(g.phase, Phase::Playing);
        assert_eq!(g.loop_hint(), LoopHint::Hum(0));
        ticks(&mut g, 2);
        assert!(
            g.player.mover.pos.x < start.x,
            "Glim starts moving left by itself"
        );
        assert!(g.level_time > 0.0 && g.time > READY_TIME_FIRST);
    }

    #[test]
    fn eating_a_mote_scores_and_stalls() {
        let mut g = playing();
        let total = g.maze.dots_left();
        let first = g.maze.player_start.step(Dir::Left);
        assert_eq!(g.maze.dot_at(first), Some(Cell::Mote));
        let ev = run_until(&mut g, 1.0, |g| g.score > 0);
        assert_eq!(g.score, MOTE_POINTS);
        assert_eq!(g.maze.dots_left(), total - 1);
        assert!(ev.contains(&GameEvent::MoteEaten { tile: first }));
        assert_eq!(g.player.stall, MOTE_STALL);
        let x = g.player.mover.pos.x;
        ticks(&mut g, 1);
        assert_eq!(g.player.mover.pos.x, x, "stalled after eating");
        ticks(&mut g, 2);
        assert!(g.player.mover.pos.x < x);
        assert_eq!(g.high_score, g.score);
    }

    #[test]
    fn flare_frightens_and_reverses_shades() {
        let mut g = playing();
        // A flare and an open neighbour to approach it from.
        let (flare, from, dir) = g
            .maze
            .dots()
            .filter(|&(_, c)| c == Cell::Flare)
            .find_map(|(f, _)| {
                Dir::ALL
                    .into_iter()
                    .map(|d| (f.step(d.opposite()), d))
                    .find(|&(n, _)| g.maze.is_walkable(n))
                    .map(|(n, d)| (f, n, d))
            })
            .expect("a reachable flare");
        // Glim one tick away from the flare's tile, Rook roaming mid-corridor.
        g.maze.take_dot(from);
        g.player
            .mover
            .teleport(from.center() + dir.vec() * 0.45, dir);
        roam_mid_corridor(&mut g, 0);
        let score = g.score;
        let ev = ticks(&mut g, 1);
        assert!(g.fright_active());
        assert_eq!(g.score, score + FLARE_POINTS);
        assert!(ev.contains(&GameEvent::FlareEaten { tile: flare }));
        assert!(ev.contains(&GameEvent::FrightStarted));
        assert_eq!(g.player.stall, FLARE_STALL);
        assert!((g.fright_timer - g.spec.fright_time).abs() < 1e-6);
        assert!(g.shades.iter().all(|s| s.frightened));
        assert_eq!(g.shades[0].mover.dir, Dir::Left, "active shades reverse");
        assert_eq!(g.loop_hint(), LoopHint::Fright);
        assert_eq!(g.shade_look(0), ShadeLook::Frightened { flash: false });
        // Expiry.
        let ev = run_until(&mut g, 10.0, |g| !g.fright_active());
        assert!(ev.contains(&GameEvent::FrightEnded));
        assert!(g.shades.iter().all(|s| !s.frightened));
        assert_eq!(g.chain, 0);
    }

    #[test]
    fn chain_scoring_and_full_chain_bonus() {
        let mut g = playing();
        calm(&mut g);
        park_player(&mut g);
        g.debug_start_fright();
        assert_eq!(g.take_events(), vec![GameEvent::FrightStarted]);
        let mut expected = 0;
        for (n, i) in [2, 0, 3, 1].into_iter().enumerate() {
            feed_shade(&mut g, i);
            let ev = ticks(&mut g, 1);
            let points = SHADE_CHAIN_POINTS[n];
            expected += points;
            let kind = g.shades[i].kind;
            assert!(
                ev.iter().any(|e| matches!(e, GameEvent::ShadeEaten { kind: k, points: p, .. } if *k == kind && *p == points)),
                "{ev:?}"
            );
            assert_eq!(
                g.phase,
                Phase::EatPause {
                    t: 0.0,
                    shade: i,
                    points
                }
            );
            assert!(!g.player.visible && !g.shades[i].visible);
            assert_eq!(g.shades[i].state, ShadeState::Eyes);
            assert_eq!(g.shade_look(i), ShadeLook::Eyes);
            assert!(g.popups.iter().any(|p| p.big && p.points == points));
            let full = ev.iter().any(|e| matches!(e, GameEvent::FullChain { .. }));
            assert_eq!(full, n == 3);
            if full {
                expected += FULL_CHAIN_BONUS;
                assert!(g.popups.iter().any(|p| p.points == FULL_CHAIN_BONUS));
            }
            assert_eq!(g.score, expected);
            // The pause freezes the fright timer and everyone but the eyes.
            let fright = g.fright_timer;
            ticks(&mut g, secs(EAT_PAUSE_TIME) + 1);
            assert_eq!(g.phase, Phase::Playing);
            assert!(g.player.visible && g.shades[i].visible);
            assert_eq!(g.fright_timer, fright);
        }
        assert_eq!(g.chain, 4);
        assert_eq!(g.score, 250 + 500 + 1000 + 2000 + 3000);
    }

    #[test]
    fn eat_pause_freezes_all_but_eyes() {
        let mut g = playing();
        calm(&mut g);
        park_player(&mut g);
        g.debug_start_fright();
        feed_shade(&mut g, 1);
        ticks(&mut g, 1);
        ticks(&mut g, secs(EAT_PAUSE_TIME) + 1);
        // Vex's eyes are now heading home; eat Rook while they travel.
        assert_eq!(g.shades[1].state, ShadeState::Eyes);
        feed_shade(&mut g, 0);
        ticks(&mut g, 1);
        assert!(matches!(g.phase, Phase::EatPause { shade: 0, .. }));
        let rook = g.shades[0].mover.pos;
        let flint = g.shades[2].mover.pos;
        let vex = g.shades[1].mover.pos;
        let glim = g.player.mover.pos;
        ticks(&mut g, 20);
        assert_eq!(g.shades[0].mover.pos, rook, "the eaten shade waits");
        assert_eq!(g.shades[2].mover.pos, flint);
        assert_eq!(g.player.mover.pos, glim);
        assert_ne!(g.shades[1].mover.pos, vex, "other eyes keep moving");
        assert_eq!(g.loop_hint(), LoopHint::Eyes);
    }

    #[test]
    fn eyes_return_home_and_revive() {
        let mut g = playing();
        calm(&mut g);
        park_player(&mut g);
        g.debug_start_fright();
        feed_shade(&mut g, 0);
        ticks(&mut g, 1);
        let mut states = vec![g.shades[0].state];
        let mut waited = 0;
        for _ in 0..secs(30.0) {
            g.update(SIM_DT, None);
            g.take_events();
            let s = &g.shades[0];
            if s.state == ShadeState::Eyes {
                assert!(g.maze.is_walkable(s.mover.tile()));
                assert_eq!(g.shade_look(0), ShadeLook::Eyes);
            }
            if s.state == ShadeState::InNest {
                assert!(s.revived, "back in the nest only once revived");
                assert_eq!(g.shade_look(0), ShadeLook::Normal);
                waited += 1;
            }
            if states.last() != Some(&s.state) {
                states.push(s.state);
                if s.state == ShadeState::Leaving {
                    assert!(!s.frightened, "revived shades are not frightened");
                    assert!(!s.revived);
                }
            }
            if s.state == ShadeState::Active {
                break;
            }
        }
        assert_eq!(
            states,
            vec![
                ShadeState::Eyes,
                ShadeState::Entering,
                ShadeState::InNest,
                ShadeState::Leaving,
                ShadeState::Active
            ]
        );
        // The door was clear and dot counters do not apply: out at once,
        // although `calm` keeps every personal counter out of reach.
        assert_eq!(waited, 1, "released on the next tick");
        assert_eq!(g.shades[0].mover.pos, g.maze.nest_exit.center());
        assert_eq!(g.shade_look(0), ShadeLook::Normal);
    }

    #[test]
    fn mode_schedule_switches_reverse_and_pause_in_fright() {
        let mut g = playing();
        park_player(&mut g);
        g.spec.idle_release = f32::INFINITY;
        g.spec.release_dots = [u32::MAX; 4];
        let first = g.spec.schedule[0].1;
        let mut switched = false;
        let mut placed = false;
        for _ in 0..secs(first + 0.5) {
            if !placed && mode_clock(&g).1 >= first - 3.0 * SIM_DT {
                // Just before the switch: Rook mid-corridor heading right.
                roam_mid_corridor(&mut g, 0);
                placed = true;
            }
            g.update(SIM_DT, None);
            let ev = g.take_events();
            if ev.contains(&GameEvent::ModeChanged(Mode::Chase)) {
                assert!(placed, "switched early");
                assert!(
                    (g.level_time - first).abs() <= SIM_DT * 1.5,
                    "switched at {}",
                    g.level_time
                );
                assert_eq!(g.mode, Mode::Chase);
                assert_eq!(
                    g.shades[0].mover.dir,
                    Dir::Left,
                    "shades reverse on a switch"
                );
                switched = true;
                break;
            }
            if placed {
                assert_eq!(g.shades[0].mover.dir, Dir::Right, "no reversal before");
            }
        }
        assert!(switched);
        assert_eq!(mode_clock(&g).0, 1);
        // The schedule clock stops during a fright.
        let clock = mode_clock(&g);
        g.debug_start_fright();
        ticks(&mut g, secs(1.0));
        assert_eq!(mode_clock(&g), clock);
    }

    #[test]
    fn nest_release_by_dots_and_idle_timer() {
        let mut g = playing();
        park_player(&mut g);
        g.spec.schedule = &SCATTER_ONLY;
        // Level 1: Vex needs no dots, Flint 24, Moss 48.
        assert_eq!(g.spec.release_dots, [0, 0, 24, 48]);
        let flint = g.spec.release_dots[2];
        ticks(&mut g, 1);
        assert_eq!(g.shades[1].state, ShadeState::Leaving);
        assert_eq!(g.shades[2].state, ShadeState::InNest);
        run_until(&mut g, 3.0, |g| g.shades[1].state == ShadeState::Active);
        eat_far_motes(&mut g, flint - 1);
        assert_eq!(g.shades[2].dot_counter, flint - 1);
        ticks(&mut g, 1);
        assert_eq!(g.shades[2].state, ShadeState::InNest);
        eat_far_motes(&mut g, 1);
        ticks(&mut g, 1);
        assert_eq!(g.shades[2].state, ShadeState::Leaving);
        assert_eq!(g.shades[3].state, ShadeState::InNest);
        // Moss's counter only starts once it is next in line.
        assert_eq!(g.shades[3].dot_counter, 0);
        // Idle release: nothing eaten for `idle_release` seconds.
        run_until(&mut g, 3.0, |g| g.shades[2].state == ShadeState::Active);
        let idle = g.idle_timer;
        let wait = g.spec.idle_release - idle;
        ticks(&mut g, secs(wait) - 2);
        assert_eq!(g.shades[3].state, ShadeState::InNest);
        ticks(&mut g, 3);
        assert_eq!(g.shades[3].state, ShadeState::Leaving);
        assert!(g.idle_timer < 0.1);
    }

    #[test]
    fn only_one_shade_climbs_out_at_a_time() {
        let mut g = playing_with(GameOptions {
            start_level: 5,
            ..GameOptions::default()
        });
        park_player(&mut g);
        g.spec.schedule = &SCATTER_ONLY;
        for _ in 0..secs(6.0) {
            g.update(SIM_DT, None);
            assert!(in_door_count(&g) <= 1);
        }
        assert!(g.shades.iter().all(|s| s.state == ShadeState::Active));
    }

    /// Run until `done` holds, checking the door rule every tick. Returns
    /// the order in which shades came out of the door.
    fn door_trace(g: &mut Game, max_s: f32, done: impl Fn(&Game) -> bool) -> Vec<ShadeKind> {
        let mut out = Vec::new();
        for _ in 0..secs(max_s) {
            let before: Vec<ShadeState> = g.shades.iter().map(|s| s.state).collect();
            g.update(SIM_DT, None);
            g.take_events();
            assert!(
                matches!(g.phase, Phase::Playing | Phase::EatPause { .. }),
                "{:?}",
                g.phase
            );
            assert!(
                in_door_count(g) <= 1,
                "two shades in the door: {:?}",
                g.shades
            );
            for (s, b) in g.shades.iter().zip(before) {
                if b == ShadeState::Leaving && s.state == ShadeState::Active {
                    out.push(s.kind);
                }
            }
            if done(g) {
                return out;
            }
        }
        panic!("not done in {max_s}s: {:?}", g.shades);
    }

    #[test]
    fn revived_shades_use_the_door_alone_and_first() {
        let mut g = playing();
        park_player(&mut g);
        g.spec.schedule = &SCATTER_ONLY;
        g.spec.idle_release = f32::INFINITY;
        // Flint is due right away, Moss never by dots.
        g.spec.release_dots = [0, 0, 0, u32::MAX];
        // Vex's eyes have just reached the nest exit (as after being eaten).
        let exit = g.maze.nest_exit.center();
        g.shades[1].get_eaten();
        g.shades[1].mover.teleport(exit, Dir::Left);
        let order = door_trace(&mut g, 6.0, |g| {
            g.shades[1..3].iter().all(|s| s.state == ShadeState::Active)
        });
        // The eyes go in first, the revived Vex comes out before Flint.
        assert_eq!(order, [ShadeKind::Vex, ShadeKind::Flint]);
        assert_eq!(g.shades[3].state, ShadeState::InNest);
    }

    #[test]
    fn returning_eyes_wait_while_a_shade_climbs_out() {
        let mut g = playing();
        park_player(&mut g);
        g.spec.schedule = &SCATTER_ONLY;
        g.spec.idle_release = f32::INFINITY;
        g.spec.release_dots = [0, 0, u32::MAX, u32::MAX];
        // Vex starts climbing out on the first tick.
        ticks(&mut g, 1);
        assert_eq!(g.shades[1].state, ShadeState::Leaving);
        // Rook's eyes arrive above the door meanwhile.
        let exit = g.maze.nest_exit.center();
        g.shades[0].get_eaten();
        g.shades[0].mover.teleport(exit, Dir::Left);
        let mut hovered = 0;
        for _ in 0..secs(4.0) {
            g.update(SIM_DT, None);
            assert!(in_door_count(&g) <= 1, "{:?}", g.shades);
            let rook = &g.shades[0];
            if rook.state == ShadeState::Eyes {
                assert_eq!(rook.mover.pos, exit, "eyes hover above the door");
                // Vex is still climbing (or came out this very tick).
                let vex = &g.shades[1];
                assert!(vex.state == ShadeState::Leaving || vex.mover.pos == exit);
                hovered += 1;
            }
            if rook.state == ShadeState::Active {
                break;
            }
        }
        assert!(hovered > secs(0.3), "eyes waited {hovered} ticks");
        assert_eq!(g.shades[0].state, ShadeState::Active, "Rook back in play");
        assert_eq!(g.shades[1].state, ShadeState::Active);
        // Flint and Moss never left: the revived Rook did not need dots.
        assert_eq!(g.shades[2].state, ShadeState::InNest);
    }

    #[test]
    fn eaten_shades_and_releases_never_share_the_door() {
        // Eat each shade in turn while the others are still being released,
        // on a level whose personal counters would hold a revived Flint back.
        for victim in 0..4 {
            let mut g = playing();
            park_player(&mut g);
            g.spec.schedule = &SCATTER_ONLY;
            g.debug_start_fright();
            g.take_events();
            feed_shade(&mut g, victim);
            ticks(&mut g, 1);
            assert_eq!(g.shades[victim].state, ShadeState::Eyes);
            g.spec.idle_release = 0.5;
            let order = door_trace(&mut g, 10.0, |g| {
                g.shades.iter().all(|s| s.state == ShadeState::Active)
            });
            assert!(
                order.contains(&g.shades[victim].kind),
                "victim {victim} came back"
            );
        }
    }

    #[test]
    fn global_release_after_a_death() {
        let mut g = playing();
        park_player(&mut g);
        g.spec.schedule = &SCATTER_ONLY;
        g.spec.idle_release = f32::INFINITY;
        g.debug_kill_player();
        run_until(&mut g, 5.0, |g| matches!(g.phase, Phase::Ready { .. }));
        g.debug_skip_ready();
        g.take_events();
        park_player(&mut g);
        assert!(g.global_release);
        assert_eq!(GLOBAL_RELEASE_DOTS, [6, 14, 26]);
        for (k, need) in [1, 2, 3].into_iter().zip(GLOBAL_RELEASE_DOTS) {
            let eaten = g.global_dots;
            eat_far_motes(&mut g, need - eaten - 1);
            ticks(&mut g, 1);
            assert_eq!(
                g.shades[k].state,
                ShadeState::InNest,
                "shade {k} left early"
            );
            eat_far_motes(&mut g, 1);
            ticks(&mut g, 1);
            assert_eq!(
                g.shades[k].state,
                ShadeState::Leaving,
                "shade {k} not released"
            );
            run_until(&mut g, 3.0, |g| g.shades[k].state == ShadeState::Active);
        }
        ticks(&mut g, 1);
        assert!(
            !g.global_release,
            "personal counters resume once Moss has left"
        );
    }

    #[test]
    fn collision_death_lives_and_game_over() {
        let mut g = playing();
        calm(&mut g);
        park_player(&mut g);
        eat_far_motes(&mut g, 5);
        let dots = g.maze.dots_left();
        // A real collision with a roaming shade.
        feed_shade(&mut g, 0);
        let ev = ticks(&mut g, 1);
        assert!(ev
            .iter()
            .any(|e| matches!(e, GameEvent::PlayerCaught { .. })));
        assert_eq!(g.phase, Phase::Caught { t: 0.0 });
        let rook = g.shades[0].mover.pos;
        let ev = ticks(&mut g, secs(CAUGHT_TIME) - 1);
        assert!(ev.is_empty());
        assert_eq!(g.shades[0].mover.pos, rook, "frozen while caught");
        let ev = run_until(&mut g, 1.0, |g| matches!(g.phase, Phase::Dying { .. }));
        assert!(ev
            .iter()
            .any(|e| matches!(e, GameEvent::DeathAnimStarted { .. })));
        assert!(g.shades.iter().all(|s| !s.visible));
        assert_eq!(g.loop_hint(), LoopHint::Silent);
        let ev = run_until(&mut g, DEATH_TIME + 0.1, |g| {
            matches!(g.phase, Phase::Ready { .. })
        });
        assert_eq!(ev, vec![GameEvent::ReadyStarted { first: false }]);
        assert_eq!(
            g.phase,
            Phase::Ready {
                t: 0.0,
                total: READY_TIME
            }
        );
        assert_eq!(g.lives, 1);
        assert_eq!(g.maze.dots_left(), dots, "dots are kept after a death");
        assert_eq!(g.player.mover.pos, g.maze.player_start.center());
        assert_eq!(g.shades[0].mover.pos, rook_lair(&g.maze).center());
        assert_eq!(
            g.shades[0].state,
            ShadeState::Active,
            "Rook respawns at its lair"
        );
        assert!(g.shades[1..].iter().all(|s| s.state == ShadeState::InNest));
        assert!(g.shades.iter().all(|s| s.visible) && g.player.visible);
        // Two more deaths end the game.
        for lives_after in [Some(0), None] {
            g.debug_kill_player();
            let ev = run_until(&mut g, 5.0, |g| {
                matches!(g.phase, Phase::Ready { .. } | Phase::GameOver { .. })
            });
            match lives_after {
                Some(l) => assert_eq!(g.lives, l),
                None => {
                    assert!(ev.contains(&GameEvent::GameOver {
                        score: g.score,
                        level: 1
                    }));
                    assert!(!g.is_over());
                    ticks(&mut g, secs(GAME_OVER_TIME));
                    assert!(g.is_over());
                }
            }
        }
        // Hooks are inert once the game is over.
        g.debug_kill_player();
        g.debug_skip_ready();
        assert!(matches!(g.phase, Phase::GameOver { .. }));
    }

    #[test]
    fn demo_ends_after_the_first_death() {
        let opts = GameOptions {
            demo: true,
            extra_life_at: Some(1),
            ..GameOptions::default()
        };
        let mut g = playing_with(opts);
        eat_far_motes(&mut g, 3);
        assert_eq!(g.lives, 0, "no extra lives in demo games");
        g.debug_kill_player();
        let ev = run_until(&mut g, 5.0, |g| matches!(g.phase, Phase::GameOver { .. }));
        assert!(ev.iter().any(|e| matches!(e, GameEvent::GameOver { .. })));
    }

    #[test]
    fn extra_life_exactly_once() {
        let mut g = playing_with(GameOptions {
            extra_life_at: Some(100),
            ..GameOptions::default()
        });
        eat_far_motes(&mut g, 9);
        assert_eq!(g.lives, 2);
        g.take_events();
        eat_far_motes(&mut g, 1);
        assert_eq!(g.lives, 3);
        assert!(g.extra_life_awarded);
        assert_eq!(
            g.take_events()
                .iter()
                .filter(|e| **e == GameEvent::ExtraLife)
                .count(),
            1
        );
        eat_far_motes(&mut g, 50);
        assert_eq!(g.lives, 3);
        assert!(!g.take_events().contains(&GameEvent::ExtraLife));
    }

    #[test]
    fn relics_spawn_expire_and_are_eaten() {
        let mut g = playing();
        calm(&mut g);
        park_player(&mut g);
        let first = (BONUS_TRIGGER_FRACTIONS[0] * g.maze.total_dots as f32).ceil() as u32;
        eat_far_motes(&mut g, first - 1);
        assert!(g.bonus.is_none());
        g.take_events();
        eat_far_motes(&mut g, 1);
        let b = g.bonus.expect("relic spawned");
        assert_eq!(b.kind, g.spec.bonus);
        assert_eq!(b.pos, g.maze.bonus_tile.center());
        assert!((BONUS_TIME_MIN..BONUS_TIME_MAX).contains(&b.duration));
        assert!(g.take_events().contains(&GameEvent::BonusSpawned {
            kind: b.kind,
            pos: b.pos
        }));
        let ev = run_until(&mut g, BONUS_TIME_MAX + 0.1, |g| g.bonus.is_none());
        assert!(ev.contains(&GameEvent::BonusExpired));
        assert!((g.level_time - b.duration).abs() < 0.05);
        // Second threshold, then eat it.
        let second = (BONUS_TRIGGER_FRACTIONS[1] * g.maze.total_dots as f32).ceil() as u32;
        let missing = second - g.maze.dots_eaten();
        eat_far_motes(&mut g, missing);
        assert!(g.bonus.is_some());
        g.take_events();
        let score = g.score;
        g.player
            .mover
            .teleport(g.maze.bonus_tile.offset(-1, 0).center(), Dir::Right);
        g.player.stall = 0.0;
        let ev = run_until(&mut g, 1.0, |g| g.bonus.is_none());
        let points = g.spec.bonus.points();
        assert!(ev.contains(&GameEvent::BonusEaten {
            kind: g.spec.bonus,
            points,
            pos: b.pos
        }));
        assert_eq!(g.score, score + points);
        assert_eq!(g.bonus_history, vec![g.spec.bonus]);
        assert!(g.popups.iter().any(|p| p.points == points && !p.big));
        // No third relic this level.
        eat_far_motes(&mut g, 20);
        assert!(g.bonus.is_none());
    }

    #[test]
    fn popups_expire() {
        let mut g = playing();
        g.popups.push(Popup {
            pos: Vec2::ZERO,
            points: 10,
            age: 0.0,
            big: false,
        });
        ticks(&mut g, secs(POPUP_TIME) - 2);
        assert_eq!(g.popups.len(), 1);
        ticks(&mut g, 3);
        assert!(g.popups.is_empty());
    }

    #[test]
    fn fury_stages_fire_once_each() {
        let mut g = playing();
        calm(&mut g);
        park_player(&mut g);
        g.debug_clear_dots(g.spec.fury1_dots);
        assert_eq!(g.fury, 1);
        assert_eq!(g.take_events(), vec![GameEvent::FuryStarted { stage: 1 }]);
        // A furious Rook hunts the player even in scatter mode.
        ticks(&mut g, 1);
        assert_eq!(g.mode, Mode::Scatter);
        assert_eq!(g.shades[0].target, g.player.mover.tile());
        g.debug_clear_dots(g.spec.fury2_dots);
        assert_eq!(g.fury, 2);
        assert_eq!(g.take_events(), vec![GameEvent::FuryStarted { stage: 2 }]);
        g.debug_clear_dots(3);
        assert!(g.take_events().is_empty());
        // No relic spawns retroactively for thresholds skipped by the hook.
        assert!(g.bonus.is_none());
    }

    #[test]
    fn debug_clear_dots_keeps_the_nearest() {
        let mut g = playing();
        g.debug_clear_dots(4);
        assert_eq!(g.maze.dots_left(), 4);
        let start = g.maze.player_start;
        let max_kept = g
            .maze
            .dots()
            .map(|(t, _)| t.dist2(start))
            .max()
            .unwrap_or(0);
        assert!(max_kept <= 4, "kept dots are next to the start");
        assert_eq!(g.score, 0);
        g.debug_clear_dots(10);
        assert_eq!(g.maze.dots_left(), 4);
    }

    #[test]
    fn level_clear_leads_to_the_next_level() {
        let mut g = playing();
        calm(&mut g);
        park_player(&mut g);
        g.debug_start_fright();
        g.spawn_bonus();
        g.debug_clear_dots(0);
        let ev = ticks(&mut g, 1);
        assert!(ev.contains(&GameEvent::LevelCleared { level: 1 }));
        assert_eq!(g.phase, Phase::LevelClear { t: 0.0 });
        assert!(!g.fright_active() && g.bonus.is_none());
        assert_eq!(g.loop_hint(), LoopHint::Silent);
        ticks(&mut g, secs(LEVEL_CLEAR_HOLD) - 2);
        assert!(g.shades.iter().all(|s| s.visible));
        ticks(&mut g, 3);
        assert!(g.shades.iter().all(|s| !s.visible));
        assert!(g.player.visible);
        let ev = run_until(&mut g, LEVEL_CLEAR_FLASH + 0.1, |g| {
            matches!(g.phase, Phase::Ready { .. })
        });
        assert_eq!(
            ev,
            vec![
                GameEvent::LevelStarted { level: 2 },
                GameEvent::ReadyStarted { first: false }
            ]
        );
        assert_eq!(g.level, 2);
        assert_eq!(g.spec, level_spec(2));
        assert_eq!(
            g.phase,
            Phase::Ready {
                t: 0.0,
                total: READY_TIME
            }
        );
        assert_eq!(g.maze.dots_left(), g.maze.total_dots);
        assert_eq!(g.fury, 0);
        assert_eq!(g.level_time, 0.0);
        assert_eq!(g.mode, Mode::Scatter);
        assert!(g.shades.iter().all(|s| s.visible && s.dot_counter == 0));
        assert_eq!(g.lives, 2);
    }

    #[test]
    fn interludes_follow_selected_levels() {
        let mut g = playing_with(GameOptions {
            start_level: 3,
            ..GameOptions::default()
        });
        calm(&mut g);
        park_player(&mut g);
        g.debug_clear_dots(0);
        let ev = run_until(&mut g, 5.0, |g| matches!(g.phase, Phase::Interlude { .. }));
        assert_eq!(g.phase, Phase::Interlude { variant: 0 });
        assert!(ev.contains(&GameEvent::InterludeStarted { variant: 0 }));
        ticks(&mut g, 100);
        assert_eq!(g.phase, Phase::Interlude { variant: 0 });
        g.end_interlude();
        assert_eq!(g.level, 4);
        assert_eq!(
            g.phase,
            Phase::Ready {
                t: 0.0,
                total: READY_TIME
            }
        );
        assert!(g
            .take_events()
            .contains(&GameEvent::LevelStarted { level: 4 }));
        assert_eq!(g.maze.index, maze_for_level(4));
        g.end_interlude();
        assert_eq!(g.level, 4, "no-op outside the interlude");
        // Demo games skip interludes.
        let mut d = playing_with(GameOptions {
            start_level: 3,
            demo: true,
            ..GameOptions::default()
        });
        calm(&mut d);
        park_player(&mut d);
        d.debug_clear_dots(0);
        run_until(&mut d, 5.0, |g| g.level == 4);
        assert!(matches!(d.phase, Phase::Ready { .. }));
    }

    #[test]
    fn hum_rises_with_progress() {
        let mut g = playing();
        calm(&mut g);
        park_player(&mut g);
        let total = g.maze.total_dots as f32;
        for (frac, stage) in [(0.3, 0), (0.5, 1), (0.7, 2), (0.9, 3)] {
            let leave = (total * (1.0 - frac)) as u32;
            g.debug_clear_dots(leave);
            assert_eq!(g.loop_hint(), LoopHint::Hum(stage), "at {frac}");
        }
    }

    #[test]
    fn frightened_shades_flash_at_the_end() {
        let mut g = playing();
        g.debug_start_fright();
        let window = g.spec.fright_flashes as f32 * FRIGHT_FLASH_PERIOD;
        let half = FRIGHT_FLASH_PERIOD * 0.5;
        let look = |g: &mut Game, left: f32| {
            g.fright_timer = left;
            g.shade_look(0)
        };
        assert_eq!(
            look(&mut g, window + 0.1),
            ShadeLook::Frightened { flash: false }
        );
        assert_eq!(
            look(&mut g, window - 0.01),
            ShadeLook::Frightened { flash: true }
        );
        assert_eq!(
            look(&mut g, window - half - 0.01),
            ShadeLook::Frightened { flash: false }
        );
        assert_eq!(
            look(&mut g, window - 2.0 * half - 0.01),
            ShadeLook::Frightened { flash: true }
        );
        assert_eq!(look(&mut g, 0.01), ShadeLook::Frightened { flash: false });
        assert_eq!(g.shade_look(99), ShadeLook::Normal);
    }

    #[test]
    fn tunnel_wraps_both_ways_in_game() {
        for dir in [Dir::Left, Dir::Right] {
            let mut g = playing();
            calm(&mut g);
            let y = g.maze.tunnel_rows[0];
            let (left, right) = tunnel_mouths(&g.maze, y);
            let x = if dir == Dir::Left {
                left.min(3)
            } else {
                right.max(g.maze.w - 4)
            };
            g.player.mover.teleport(Tile::new(x, y).center(), dir);
            let mut wrapped = false;
            for _ in 0..secs(2.0) {
                g.update(SIM_DT, Some(dir));
                let m = &g.player.mover;
                assert!(g.maze.is_walkable(m.tile()));
                assert_eq!(m.pos.y, y as f32 + 0.5);
                wrapped |= (m.pos.x - m.prev_pos.x).abs() > 1.5;
            }
            assert!(wrapped, "no wrap going {dir:?}");
        }
    }

    #[test]
    fn bad_time_steps_are_harmless() {
        let mut g = playing();
        for dt in [f32::NAN, -1.0, 0.0, f32::INFINITY, 10.0] {
            g.update(dt, Some(Dir::Up));
            assert!(g.player.mover.pos.is_finite());
            assert!(g.time.is_finite());
        }
    }

    #[test]
    fn shades_never_reverse_unprompted_or_leave_the_lanes() {
        for (seed, level) in [(1, 1), (7, 5), (3, 21)] {
            let mut g = playing_with(GameOptions {
                seed,
                start_level: level,
                ..GameOptions::default()
            });
            let mut ap = Autopilot::new(seed);
            let mut prev: Vec<(ShadeState, Dir)> =
                g.shades.iter().map(|s| (s.state, s.mover.dir)).collect();
            for _ in 0..secs(60.0) {
                let input = ap.choose(&g);
                let was_playing = g.phase == Phase::Playing;
                g.update(SIM_DT, input);
                let ev = g.take_events();
                let prompted = ev
                    .iter()
                    .any(|e| matches!(e, GameEvent::ModeChanged(_) | GameEvent::FrightStarted));
                for (s, &(pstate, pdir)) in g.shades.iter().zip(&prev) {
                    if matches!(s.state, ShadeState::Active | ShadeState::Eyes) {
                        let t = s.mover.tile();
                        assert!(g.maze.is_walkable(t), "{:?} on {t:?}", s.kind);
                        let p = s.mover.pos;
                        let lane = if s.mover.dir.is_horizontal() {
                            p.y
                        } else {
                            p.x
                        };
                        assert_eq!(lane, lane.floor() + 0.5, "{:?} off lane at {p:?}", s.kind);
                    }
                    if s.state == pstate
                        && s.state == ShadeState::Active
                        && s.mover.dir == pdir.opposite()
                    {
                        assert!(
                            prompted || !was_playing,
                            "{:?} reversed without a mode change or flare at {:?} (phase {:?}, t {})",
                            s.kind,
                            s.mover.pos,
                            g.phase,
                            g.time
                        );
                    }
                }
                prev = g.shades.iter().map(|s| (s.state, s.mover.dir)).collect();
                match g.phase {
                    Phase::GameOver { .. } => break,
                    Phase::Interlude { .. } => g.end_interlude(),
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn deterministic_for_same_seed_and_inputs() {
        let run = |seed: u64| {
            let mut g = Game::new(GameOptions {
                seed,
                ..GameOptions::default()
            });
            let mut inputs = Rng::new(99);
            let mut input = None;
            for i in 0..secs(40.0) {
                if i % 30 == 0 {
                    input = match inputs.below(5) {
                        0 => None,
                        k => Some(Dir::ALL[k as usize - 1]),
                    };
                }
                g.update(SIM_DT, input);
                if g.is_over() {
                    break;
                }
            }
            format!("{g:?}")
        };
        assert_eq!(run(5), run(5));
        assert_ne!(run(5), run(6));
    }
}
