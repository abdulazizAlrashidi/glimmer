//! A running game (real or attract-mode demo): the fixed-timestep loop, event
//! forwarding, the pause menu state and interlude playback.

use super::interlude::Interlude;
use super::overlay::PauseMenu;
use super::sound::SoundOut;
use crate::autopilot::Autopilot;
use crate::config::{MAX_FRAME_TIME, SIM_DT};
use crate::game::{Game, GameOptions, LoopHint, Phase};
use crate::geom::Dir;
use crate::render::Renderer;

/// Where game events go.
pub struct Fx<'a> {
    pub renderer: &'a mut Renderer,
    pub sound: &'a mut SoundOut,
    /// Forward events to audio (false for the silent demo).
    pub audible: bool,
}

pub struct Session {
    pub game: Game,
    /// Unsimulated wall-clock time (seconds).
    acc: f64,
    /// Drives the demo.
    autopilot: Option<Autopilot>,
    /// `Some` while paused (the pause menu's state).
    pub pause: Option<PauseMenu>,
    /// The interlude being shown while the game waits in `Phase::Interlude`.
    pub interlude: Option<Interlude>,
    /// Wall-clock seconds of play (demo time limit).
    pub clock: f32,
}

impl Session {
    pub fn new(opts: GameOptions) -> Session {
        let autopilot = opts.demo.then(|| Autopilot::new(opts.seed ^ 0xD3_40));
        Session {
            game: Game::new(opts),
            acc: 0.0,
            autopilot,
            pause: None,
            interlude: None,
            clock: 0.0,
        }
    }

    pub fn is_demo(&self) -> bool {
        self.game.opts.demo
    }

    pub fn paused(&self) -> bool {
        self.pause.is_some()
    }

    /// Fixed-step interpolation factor for rendering.
    pub fn alpha(&self) -> f32 {
        (self.acc / f64::from(SIM_DT)).clamp(0.0, 1.0) as f32
    }

    /// Advance by `dt` seconds of wall-clock time. `input` steers the player
    /// (ignored by the demo, whose autopilot drives); `skip` ends a running
    /// interlude. Returns the number of simulation ticks run.
    pub fn advance(&mut self, dt: f32, input: Option<Dir>, skip: bool, fx: &mut Fx) -> u32 {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.clock += dt;
        if let Phase::Interlude { variant } = self.game.phase {
            let scene = self
                .interlude
                .get_or_insert_with(|| Interlude::new(variant));
            scene.advance(dt);
            if skip || scene.finished() {
                self.interlude = None;
                self.game.end_interlude();
                self.acc = 0.0;
                self.flush_events(fx);
            }
            return 0;
        }
        self.interlude = None;

        self.acc += f64::from(dt).min(MAX_FRAME_TIME);
        let step = f64::from(SIM_DT);
        let mut ticks = 0;
        while self.acc >= step {
            self.tick(input, fx);
            self.acc -= step;
            ticks += 1;
            if let Phase::Interlude { variant } = self.game.phase {
                // Hand over to the interlude right away (no stale frame).
                self.acc = 0.0;
                self.interlude = Some(Interlude::new(variant));
                break;
            }
        }
        ticks
    }

    /// Run exactly one simulation step and forward its events.
    pub fn tick(&mut self, input: Option<Dir>, fx: &mut Fx) {
        let dir = match self.autopilot.as_mut() {
            Some(ap) => ap.choose(&self.game),
            None => input,
        };
        self.game.update(SIM_DT, dir);
        self.flush_events(fx);
    }

    fn flush_events(&mut self, fx: &mut Fx) {
        let events = self.game.take_events();
        for ev in &events {
            if fx.audible {
                fx.sound.event(ev);
            }
            fx.renderer.handle_event(ev, &self.game);
        }
    }

    /// Ambient loop that should be playing right now.
    pub fn ambience(&self, audible: bool) -> LoopHint {
        if !audible || self.paused() || self.interlude.is_some() {
            LoopHint::Silent
        } else {
            self.game.loop_hint()
        }
    }

    /// Draw the playfield (or the interlude). The caller drew the backdrop.
    pub fn draw(&self, renderer: &mut Renderer) {
        if let Some(scene) = &self.interlude {
            let l = renderer.layout;
            scene.draw(&l);
            return;
        }
        renderer.draw_game(&self.game, self.alpha());
        renderer.draw_hud(&self.game);
    }
}
