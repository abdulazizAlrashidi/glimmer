//! Application shell: screen state machine, menus, options, high scores,
//! name entry, pause, attract-mode demo, interludes, fixed-timestep loop, and
//! the `--shot` screenshot mode used for automated visual checks.
//!
//! # Screens
//! ```text
//! Splash -> Title <-> Options / HighScores / HowToPlay
//!             | ^
//!             | +-- Demo (attract mode, after ATTRACT_IDLE_TIME idle)
//!             v
//!          Playing <-> Pause <-> Options
//!             |  +---> Interlude (between some levels)
//!             v
//!     NameEntry (new high score) -> HighScores -> Title
//! ```
//! Each frame: sample input, handle global shortcuts (and auto-pause when the
//! window loses focus mid-game), advance the current screen (gameplay runs on
//! a fixed `SIM_DT` step with interpolation), keep music/ambience in sync,
//! then draw: backdrop -> screen -> overlays -> mute badge -> fade -> post
//! effects -> FPS.
//!
//! # Never losing a score
//! A finished game's score lives only in the session (GAME OVER hold) or the
//! entry screen until the player confirms a name. Closing the window (or
//! Cmd+Q) is intercepted with `prevent_quit`, so `App::shutdown` can still
//! record a qualifying score, under the initials typed so far or a default,
//! before the process exits.

mod entry;
mod howto;
mod interlude;
mod menu;
mod options;
mod overlay;
mod scores;
mod session;
mod shot;
mod sound;
mod title;
mod ui;

use crate::audio::Audio;
use crate::config::{DEMO_MAX_TIME, MAX_FRAME_TIME, READY_TIME_FIRST};
use crate::game::{GameOptions, LoopHint, Phase};
use crate::input::Input;
use crate::render::Renderer;
use crate::save::{self, HighScore, HighScores, Settings};
use crate::synth::{Sfx, Track};
use entry::{EntryAction, EntryState};
use howto::{HowToAction, HowToState};
use macroquad::input::{is_quit_requested, prevent_quit, show_mouse};
use macroquad::time::get_frame_time;
use macroquad::window::{next_frame, set_fullscreen};
use options::{Back, OptionsAction, OptionsState};
use overlay::{Fade, FpsCounter, PauseAction, PauseMenu, Toast};
use scores::ScoresState;
use session::{Fx, Session};
use sound::SoundOut;
use title::{SplashState, TitleAction, TitleState};

/// Crate version shown on the title screen and by `--version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every scene accepted by `--shot`.
pub const SHOT_SCENES: [&str; 24] = [
    "title",
    "play",
    "fright",
    "eat",
    "dying",
    "levelclear",
    "gameover",
    "pause",
    "options",
    "scores",
    "howto",
    "entry",
    "interlude0",
    "interlude1",
    "interlude2",
    "demo",
    "maze1",
    "maze2",
    "maze3",
    "splash",
    "howto2",
    "howto3",
    "howto4",
    "pauseconfirm",
];

/// Highest `--level` accepted.
const MAX_START_LEVEL: u32 = 999;
/// Highest `--shot-time` accepted (seconds).
const MAX_SHOT_TIME: f32 = 600.0;

/// Command-line options (parsed in `main.rs`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppArgs {
    /// `--shot <scene> <out.png>`: render one scene deterministically, save a
    /// PNG of the window and exit. Scenes: title, play, fright, eat, dying,
    /// levelclear, gameover, pause, options, scores, howto, entry, interlude0,
    /// interlude1, interlude2, demo, maze1, maze2, maze3 (plus splash, howto2,
    /// howto3 and howto4 for the remaining how-to pages, and pauseconfirm
    /// for the pause menu asking to confirm QUIT TO TITLE). Gameplay scenes
    /// honour `--level` and `--seed`.
    pub shot: Option<(String, String)>,
    /// `--shot-time S`: scene clock in seconds for `--shot` (splash, title,
    /// options, scores, how-to pages, entry and interlude scenes).
    pub shot_time: Option<f32>,
    /// `--level N`: start new games at level N (testing).
    pub start_level: Option<u32>,
    /// `--seed N`: deterministic RNG seed.
    pub seed: Option<u64>,
    /// `--windowed`: ignore the saved fullscreen setting.
    pub windowed: bool,
    /// `--mute`: start with master volume 0 (not saved).
    pub mute: bool,
}

/// What the command line asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Run(AppArgs),
    Help,
    Version,
}

/// Usage text for `--help` and argument errors.
pub fn usage() -> String {
    format!(
        "Glimmer {VERSION} - an original neon maze-chase arcade game (fully offline)\n\
         \n\
         USAGE:\n    glimmer [OPTIONS]\n\
         \n\
         OPTIONS:\n\
         \x20   --level <N>            start new games at level N (1-{MAX_START_LEVEL})\n\
         \x20   --seed <N>             use a fixed random seed\n\
         \x20   --windowed             ignore the saved fullscreen setting\n\
         \x20   --mute                 start muted (not saved)\n\
         \x20   --shot <SCENE> <OUT>   render SCENE, save it as the PNG file OUT and exit\n\
         \x20                          (gameplay scenes honour --level and --seed)\n\
         \x20   --shot-time <S>        scene clock in seconds for --shot (0-{MAX_SHOT_TIME})\n\
         \x20   -h, --help             print this help and exit\n\
         \x20   -V, --version          print the version and exit\n\
         \n\
         SCENES:\n    {}\n\
         \n\
         KEYS:\n    arrows/WASD move, Esc/P pause, Enter/Space select, M mute,\n    \
         F11 or Alt+Enter fullscreen (also Ctrl+Cmd+F on macOS), F3 debug overlay,\n    \
         Q quit (title)",
        SHOT_SCENES.join(", ")
    )
}

impl AppArgs {
    /// Parse `std::env::args()`-style arguments (first element = program
    /// name). No printing, no exiting: `main.rs` handles help and version.
    /// Errors include the usage text.
    pub fn parse_command(args: &[String]) -> Result<Command, String> {
        let err = |msg: String| Err(format!("error: {msg}\n\n{}", usage()));
        let mut out = AppArgs::default();
        let mut it = args.iter().skip(1);
        while let Some(raw) = it.next() {
            // Accept both `--flag value` and `--flag=value`.
            let (flag, inline) = match raw.split_once('=') {
                Some((f, v)) if f.starts_with("--") => (f, Some(v.to_owned())),
                _ => (raw.as_str(), None),
            };
            let mut value = |name: &str| -> Result<String, String> {
                match inline.clone().or_else(|| it.next().cloned()) {
                    Some(v) if !v.is_empty() => Ok(v),
                    _ => Err(format!("{name} needs a value")),
                }
            };
            match flag {
                "-h" | "--help" => return Ok(Command::Help),
                "-V" | "--version" => return Ok(Command::Version),
                "--windowed" => out.windowed = true,
                "--mute" => out.mute = true,
                "--level" => {
                    let v = match value("--level") {
                        Ok(v) => v,
                        Err(e) => return err(e),
                    };
                    match v.parse::<u32>() {
                        Ok(n) if (1..=MAX_START_LEVEL).contains(&n) => out.start_level = Some(n),
                        _ => {
                            return err(format!(
                                "--level expects a number from 1 to {MAX_START_LEVEL}, got '{v}'"
                            ))
                        }
                    }
                }
                "--seed" => {
                    let v = match value("--seed") {
                        Ok(v) => v,
                        Err(e) => return err(e),
                    };
                    match v.parse::<u64>() {
                        Ok(n) => out.seed = Some(n),
                        Err(_) => {
                            return err(format!("--seed expects a non-negative integer, got '{v}'"))
                        }
                    }
                }
                "--shot-time" => {
                    let v = match value("--shot-time") {
                        Ok(v) => v,
                        Err(e) => return err(e),
                    };
                    match v.parse::<f32>() {
                        Ok(t) if t.is_finite() && (0.0..=MAX_SHOT_TIME).contains(&t) => {
                            out.shot_time = Some(t)
                        }
                        _ => {
                            return err(format!(
                                "--shot-time expects seconds from 0 to {MAX_SHOT_TIME}, got '{v}'"
                            ))
                        }
                    }
                }
                "--shot" => {
                    if inline.is_some() {
                        return err("use --shot <SCENE> <OUT.png>".to_owned());
                    }
                    let (Some(scene), Some(path)) = (it.next(), it.next()) else {
                        return err("--shot needs a scene and an output path".to_owned());
                    };
                    if !SHOT_SCENES.contains(&scene.as_str()) {
                        return err(format!("unknown --shot scene '{scene}'"));
                    }
                    if path.is_empty() || path.starts_with("--") {
                        return err(format!("invalid --shot output path '{path}'"));
                    }
                    out.shot = Some((scene.clone(), path.clone()));
                }
                // Legacy macOS Finder launch argument (process serial number).
                f if f.starts_with("-psn_") => {}
                _ => return err(format!("unknown argument '{raw}'")),
            }
        }
        Ok(Command::Run(out))
    }
}

/// Run the game until the user quits.
pub async fn run(args: AppArgs) {
    if let Some((scene, out)) = args.shot.clone() {
        shot::run(args, &scene, &out).await;
        return;
    }
    prevent_quit();
    let settings = save::load_settings().clamped();
    let scores = save::load_scores();
    let mut app = App::new(args, settings, scores, SoundOut::disabled(), true);

    // Show a LOADING frame while the sounds are synthesised.
    for _ in 0..2 {
        app.renderer.begin_frame();
        app.renderer.draw_backdrop();
        let l = app.renderer.layout;
        title::draw_loading(&l);
        app.renderer.draw_post();
        next_frame().await;
    }
    let audio = Audio::load().await;
    app.sound = SoundOut::new(audio, app.args.mute);
    app.sound.apply(&app.settings);

    // `prevent_quit` turns the close button and Cmd+Q into a request, so the
    // loop can finish and `shutdown` saves before the window goes away.
    loop {
        if is_quit_requested() {
            app.quit = true;
        }
        if app.quit {
            break;
        }
        app.frame(get_frame_time(), true);
        next_frame().await;
    }
    app.shutdown();
}

/// The screen currently shown.
enum Screen {
    Splash(SplashState),
    /// The title screen (its state lives in `App::title`).
    Title,
    Options(OptionsState),
    HighScores(ScoresState),
    HowTo(Box<HowToState>),
    /// A real game (`App::session`), possibly paused or in an interlude.
    Playing,
    /// Attract-mode demo (`App::session` with the autopilot).
    Demo,
    NameEntry(EntryState),
}

/// Start levels the attract demo cycles through (one per maze).
const DEMO_LEVELS: [u32; 4] = [1, 3, 6, 10];
/// The demo skips most of its "GET SET!" hold.
const DEMO_READY_SKIP: f32 = 1.2;
/// The demo returns to the title this long after its game over.
const DEMO_GAME_OVER_HOLD: f32 = 2.0;
/// A finished game's GAME OVER hold can be skipped after this long.
const GAME_OVER_SKIP: f32 = 1.0;
/// Title music waits this long on the high-score table after a game, so the
/// end-of-game jingle is not drowned out.
const SCORES_MUSIC_DELAY: f32 = 1.6;
/// Resuming from pause during a new game's first "GET SET!" (whose intro
/// jingle the pause cut off) leaves at most this much of the hold, played in
/// silence, instead of seconds of quiet waiting for music that is gone.
const RESUME_READY_LEFT: f32 = 1.0;
/// Focus-loss events are ignored this long after the app switches
/// fullscreen: on macOS the switch moves the window, which miniquad reports
/// like a focus loss.
const FOCUS_GRACE: f32 = 1.0;
/// Name recorded for a qualifying score whose player never reached (or
/// typed nothing on) the entry screen, e.g. the window closed on GAME OVER.
const UNNAMED: &str = "???";

struct App {
    args: AppArgs,
    settings: Settings,
    scores: HighScores,
    renderer: Renderer,
    sound: SoundOut,
    input: Input,
    screen: Screen,
    title: TitleState,
    session: Option<Session>,
    /// Wall-clock seconds since start (UI animation), kept bounded by
    /// [`ui::tick_clock`].
    time: f32,
    /// Seconds left during which focus-loss events are ignored.
    focus_grace: f32,
    toast: Toast,
    fade: Fade,
    fps: FpsCounter,
    quit: bool,
    /// Read/write the settings and score files (false in screenshot mode).
    persist: bool,
    /// Screenshot mode: never advance the simulation on its own.
    freeze_sim: bool,
    /// The fullscreen value found in the settings file, and whether the
    /// player changed it this session (`--windowed` must not overwrite it).
    saved_fullscreen: bool,
    fullscreen_touched: bool,
    /// Fullscreen state last requested from the window system.
    fullscreen_now: bool,
    cursor_shown: bool,
    /// Name typed at the last high-score entry (pre-fills the next one).
    last_name: String,
    demo_count: u32,
    seed_counter: u64,
}

impl App {
    fn new(
        args: AppArgs,
        settings: Settings,
        scores: HighScores,
        sound: SoundOut,
        persist: bool,
    ) -> App {
        let saved_fullscreen = settings.fullscreen;
        let mut settings = settings.clamped();
        if args.windowed {
            settings.fullscreen = false;
        }
        let mut renderer = Renderer::new();
        renderer.shake_enabled = settings.screen_shake;
        renderer.scanlines = settings.scanlines;
        let fullscreen_now = settings.fullscreen;
        if fullscreen_now {
            set_fullscreen(true);
        }
        let mut fade = Fade::default();
        fade.start();
        App {
            args,
            settings,
            scores,
            renderer,
            sound,
            input: Input::new(),
            screen: Screen::Splash(SplashState::default()),
            title: TitleState::new(),
            session: None,
            time: 0.0,
            focus_grace: if fullscreen_now { FOCUS_GRACE } else { 0.0 },
            toast: Toast::default(),
            fade,
            fps: FpsCounter::default(),
            quit: false,
            persist,
            freeze_sim: false,
            saved_fullscreen,
            fullscreen_touched: false,
            fullscreen_now,
            cursor_shown: true,
            last_name: String::new(),
            demo_count: 0,
            seed_counter: 0,
        }
    }

    // ----- frame ----------------------------------------------------------------

    /// One full frame: input, logic, audio sync and drawing. `live_input`
    /// false (screenshot mode) runs with no input at all.
    fn frame(&mut self, raw_dt: f32, live_input: bool) {
        let dt = if raw_dt.is_finite() {
            raw_dt.clamp(0.0, MAX_FRAME_TIME as f32)
        } else {
            0.0
        };
        self.renderer.begin_frame();
        if live_input {
            self.input.poll(dt);
        } else {
            self.input.clear_frame();
        }
        self.step(dt);
        self.sync_cursor();
        self.draw();
    }

    /// Everything a frame does after sampling input and before drawing:
    /// global keys, auto-pause, the current screen and audio sync. Touches
    /// no window state, so tests drive it with synthetic [`Input::act`].
    fn step(&mut self, dt: f32) {
        ui::tick_clock(&mut self.time, dt);
        self.focus_grace = (self.focus_grace - dt).max(0.0);
        self.toast.update(dt);
        self.fade.update(dt);
        self.global_keys();
        if self.input.act.focus_lost && self.focus_grace <= 0.0 {
            self.auto_pause();
        }

        let frozen_world = match self.screen {
            Screen::Playing => self.session.as_ref().is_some_and(Session::paused),
            Screen::Options(ref o) => o.back == Back::Pause,
            _ => false,
        };
        self.renderer.update(if frozen_world { 0.0 } else { dt });

        self.update_screen(dt);
        self.sync_audio();
    }

    /// Shortcuts that work on every screen.
    fn global_keys(&mut self) {
        let act = self.input.act;
        if act.fullscreen {
            self.settings.fullscreen = !self.settings.fullscreen;
            self.fullscreen_touched = true;
            self.apply_settings();
            self.save_settings();
            self.toast.show(if self.settings.fullscreen {
                "FULLSCREEN"
            } else {
                "WINDOWED"
            });
        }
        if act.debug {
            self.renderer.debug_overlay = !self.renderer.debug_overlay;
            self.toast.show(if self.renderer.debug_overlay {
                "DEBUG VIEW ON"
            } else {
                "DEBUG VIEW OFF"
            });
        }
        // M types a letter during name entry.
        if act.mute && !matches!(self.screen, Screen::NameEntry(_)) {
            let muted = !self.sound.muted();
            self.sound.set_muted(muted, &self.settings);
            self.toast
                .show(if muted { "SOUND OFF" } else { "SOUND ON" });
        }
    }

    fn update_screen(&mut self, dt: f32) {
        let act = self.input.act;
        let l = self.renderer.layout;
        let mouse = ui::to_canvas(&l, self.input.mouse);
        match &mut self.screen {
            Screen::Splash(s) => {
                if s.update(dt, &act) {
                    self.go_title(false);
                }
            }
            Screen::Title => {
                let action = self.title.update(dt, &act, mouse);
                self.on_title_action(action);
                if matches!(self.screen, Screen::Title) && self.title.wants_demo() {
                    self.start_demo();
                }
            }
            Screen::Options(st) => {
                let action = st.update(dt, &act, mouse, &mut self.settings);
                self.on_options_action(action);
            }
            Screen::HighScores(st) => {
                if st.update(dt, &act) {
                    self.sound.play(Sfx::MenuBack);
                    self.go_title(false);
                }
            }
            Screen::HowTo(st) => match st.update(dt, &act) {
                HowToAction::None => {}
                HowToAction::Turned => self.sound.play(Sfx::MenuMove),
                HowToAction::Leave => {
                    self.sound.play(Sfx::MenuBack);
                    self.go_title(false);
                }
            },
            Screen::Playing => self.update_playing(dt),
            Screen::Demo => self.update_demo(dt),
            Screen::NameEntry(st) => match st.update(dt, &act, self.input.chars()) {
                EntryAction::None => {}
                EntryAction::Edited => self.sound.play(Sfx::Key),
                EntryAction::Confirmed => self.finish_entry(),
            },
        }
    }

    fn on_title_action(&mut self, action: TitleAction) {
        match action {
            TitleAction::None => {}
            TitleAction::Moved => self.sound.play(Sfx::MenuMove),
            TitleAction::Play => {
                self.sound.play(Sfx::MenuSelect);
                self.start_game();
            }
            TitleAction::Scores => {
                self.sound.play(Sfx::MenuSelect);
                self.switch(Screen::HighScores(ScoresState::new(
                    &self.scores,
                    None,
                    None,
                    false,
                )));
            }
            TitleAction::Options => {
                self.sound.play(Sfx::MenuSelect);
                self.switch(Screen::Options(OptionsState::new(Back::Title)));
            }
            TitleAction::HowTo => {
                self.sound.play(Sfx::MenuSelect);
                self.switch(Screen::HowTo(Box::new(HowToState::new(0))));
            }
            TitleAction::Quit => {
                self.sound.play(Sfx::MenuBack);
                self.quit = true;
            }
        }
    }

    fn on_options_action(&mut self, action: OptionsAction) {
        match action {
            OptionsAction::None => {}
            OptionsAction::Moved => self.sound.play(Sfx::MenuMove),
            OptionsAction::Changed => {
                self.fullscreen_touched |= self.settings.fullscreen != self.fullscreen_now;
                self.apply_settings();
                self.sound.play(Sfx::MenuMove);
            }
            OptionsAction::Armed => self.sound.play(Sfx::MenuBack),
            OptionsAction::ResetScores => {
                self.scores = HighScores::defaults();
                self.save_scores();
                self.sound.play(Sfx::MenuSelect);
            }
            OptionsAction::Leave => {
                let (dirty, back) = match &self.screen {
                    Screen::Options(st) => (st.dirty, st.back),
                    _ => (false, Back::Title),
                };
                if dirty {
                    self.save_settings();
                }
                self.sound.play(Sfx::MenuBack);
                match back {
                    Back::Title => self.go_title(false),
                    Back::Pause if self.session.is_some() => self.switch(Screen::Playing),
                    Back::Pause => self.go_title(false),
                }
            }
        }
    }

    fn update_playing(&mut self, dt: f32) {
        let act = self.input.act;
        let l = self.renderer.layout;
        let mouse = ui::to_canvas(&l, self.input.mouse);
        let Some(session) = self.session.as_mut() else {
            self.go_title(false);
            return;
        };

        // Pause menu.
        if let Some(pause) = session.pause.as_mut() {
            match pause.update(&act, mouse) {
                PauseAction::None => {}
                PauseAction::Moved | PauseAction::Disarmed => self.sound.play(Sfx::MenuMove),
                PauseAction::Armed => self.sound.play(Sfx::MenuBack),
                PauseAction::Resume => self.resume(),
                PauseAction::Restart => {
                    self.sound.play(Sfx::MenuSelect);
                    self.start_game();
                }
                PauseAction::Options => {
                    self.sound.play(Sfx::MenuSelect);
                    self.switch(Screen::Options(OptionsState::new(Back::Pause)));
                }
                PauseAction::QuitToTitle => {
                    self.sound.play(Sfx::MenuBack);
                    self.go_title(true);
                }
            }
            return;
        }

        let mut fx = Fx {
            renderer: &mut self.renderer,
            sound: &mut self.sound,
            audible: true,
        };

        // Interlude: Enter / Space / Esc skip it.
        if session.interlude.is_some() || matches!(session.game.phase, Phase::Interlude { .. }) {
            if !self.freeze_sim {
                session.advance(dt, None, act.confirm || act.escape, &mut fx);
            }
            return;
        }

        // A finished game's GAME OVER hold can be cut short.
        if let Phase::GameOver { t } = session.game.phase {
            if t >= GAME_OVER_SKIP && (act.confirm || act.escape) {
                self.end_game();
                return;
            }
        } else if act.pause {
            self.open_pause();
            return;
        }

        if !self.freeze_sim {
            let ticks = session.advance(dt, self.input.game_dir(), false, &mut fx);
            if ticks > 0 {
                self.input.consume_tap();
            }
        }
        if session.game.is_over() {
            self.end_game();
        }
    }

    fn update_demo(&mut self, dt: f32) {
        let act = self.input.act;
        let Some(session) = self.session.as_mut() else {
            self.go_title(true);
            return;
        };
        if act.any {
            self.go_title(true);
            return;
        }
        if !self.freeze_sim {
            if let Phase::Ready { t, .. } = session.game.phase {
                if t >= DEMO_READY_SKIP {
                    session.game.debug_skip_ready();
                }
            }
            let mut fx = Fx {
                renderer: &mut self.renderer,
                sound: &mut self.sound,
                audible: false,
            };
            session.advance(dt, None, false, &mut fx);
        }
        let over = session.game.is_over()
            || matches!(session.game.phase, Phase::GameOver { t } if t >= DEMO_GAME_OVER_HOLD)
            || session.clock >= DEMO_MAX_TIME;
        if over && !self.freeze_sim {
            self.go_title(true);
        }
    }

    // ----- pause ----------------------------------------------------------------------

    /// Whether the running game may be paused right now (a real game that is
    /// not already paused, not in an interlude and not over).
    fn can_pause(&self) -> bool {
        matches!(self.screen, Screen::Playing)
            && self.session.as_ref().is_some_and(|s| {
                !s.is_demo()
                    && !s.paused()
                    && s.interlude.is_none()
                    && !matches!(
                        s.game.phase,
                        Phase::GameOver { .. } | Phase::Interlude { .. }
                    )
            })
    }

    /// Open the pause menu: gameplay sound (loops, one-shots and jingles)
    /// stops at once; only the pause blip plays.
    fn open_pause(&mut self) {
        if !self.can_pause() {
            return;
        }
        if let Some(s) = self.session.as_mut() {
            s.pause = Some(PauseMenu::new());
        }
        self.sound.stop_effects();
        self.sound.ambience(LoopHint::Silent);
        self.sound.play(Sfx::PauseOn);
    }

    /// The window lost focus or was minimised: pause a game in progress so
    /// the player does not come back to lost lives.
    fn auto_pause(&mut self) {
        if self.can_pause() {
            self.open_pause();
            self.toast.show("AUTO-PAUSED");
        }
    }

    /// Close the pause menu. The pause cut off any jingle; during a new
    /// game's first "GET SET!" the intro jingle *was* the hold, so what is
    /// left of it is shortened to [`RESUME_READY_LEFT`] (silent) rather than
    /// leaving the player waiting for music that no longer plays.
    fn resume(&mut self) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        s.pause = None;
        if let Phase::Ready { t, total } = s.game.phase {
            if total == READY_TIME_FIRST && total - t > RESUME_READY_LEFT {
                s.game.phase = Phase::Ready {
                    t: total - RESUME_READY_LEFT,
                    total,
                };
            }
        }
        self.sound.play(Sfx::PauseOff);
    }

    // ----- transitions --------------------------------------------------------------

    fn switch(&mut self, screen: Screen) {
        self.screen = screen;
        self.input.reset();
        self.fade.start();
    }

    /// Back to the title. `replay` restarts its cast roll (after a game/demo).
    fn go_title(&mut self, replay: bool) {
        if let Some(s) = self.session.take() {
            if s.is_demo() {
                self.sound.ambience(LoopHint::Silent);
            } else {
                self.sound.stop_all();
            }
            self.renderer.reset();
        }
        if replay {
            self.title.replay();
        }
        self.title.idle = 0.0;
        self.switch(Screen::Title);
    }

    fn game_options(&mut self, demo: bool) -> GameOptions {
        let s = &self.settings;
        GameOptions {
            start_lives: if demo { 1 } else { u32::from(s.start_lives) },
            extra_life_at: if demo || s.extra_life_at == 0 {
                None
            } else {
                Some(s.extra_life_at)
            },
            seed: self.next_seed(),
            demo,
            start_level: self.args.start_level.unwrap_or(1).max(1),
            high_score: self.scores.best(),
        }
    }

    fn next_seed(&mut self) -> u64 {
        if let Some(seed) = self.args.seed {
            return seed;
        }
        self.seed_counter = self.seed_counter.wrapping_add(1);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x5EED_1234);
        now ^ self.seed_counter.wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    fn start_session(&mut self, opts: GameOptions) {
        let demo = opts.demo;
        if !demo {
            self.sound.stop_all();
        }
        self.renderer.reset();
        self.session = Some(Session::new(opts));
        self.switch(if demo { Screen::Demo } else { Screen::Playing });
    }

    fn start_game(&mut self) {
        let opts = self.game_options(false);
        self.start_session(opts);
    }

    fn start_demo(&mut self) {
        let mut opts = self.game_options(true);
        opts.start_level = DEMO_LEVELS[self.demo_count as usize % DEMO_LEVELS.len()];
        self.demo_count = self.demo_count.wrapping_add(1);
        self.start_session(opts);
    }

    /// The game finished: high-score entry or the table.
    fn end_game(&mut self) {
        let Some(s) = self.session.take() else {
            self.go_title(true);
            return;
        };
        let (score, level) = (s.game.score, s.game.level);
        self.sound.stop_all();
        self.renderer.reset();
        if self.scores.qualifies(score) {
            let rank = predicted_rank(&self.scores, score);
            let name = if self.last_name.is_empty() {
                DEFAULT_NAME
            } else {
                self.last_name.as_str()
            };
            let st = EntryState::new(name, score, level, rank);
            self.sound.play(Sfx::HighScore);
            self.switch(Screen::NameEntry(st));
        } else {
            self.title.replay();
            self.switch(Screen::HighScores(ScoresState::new(
                &self.scores,
                None,
                Some(score),
                true,
            )));
        }
    }

    fn finish_entry(&mut self) {
        let Screen::NameEntry(st) = &self.screen else {
            return;
        };
        let name = st.entry.name();
        let entry = HighScore::new(&name, st.score, st.level);
        self.last_name = name;
        let rank = self.scores.insert(entry);
        self.save_scores();
        self.sound.play(Sfx::MenuSelect);
        self.title.replay();
        self.switch(Screen::HighScores(ScoresState::new(
            &self.scores,
            rank,
            None,
            true,
        )));
    }

    /// A finished game's score that is not in the table yet: the one on the
    /// NEW HIGH SCORE screen (under the initials typed so far), or a real
    /// game sitting on its GAME OVER hold (under the last name used, or
    /// [`UNNAMED`]) when it qualifies.
    fn pending_score(&self) -> Option<HighScore> {
        match &self.screen {
            Screen::NameEntry(st) => Some(HighScore::new(&st.entry.name(), st.score, st.level)),
            Screen::Playing => {
                let s = self.session.as_ref()?;
                let over = matches!(s.game.phase, Phase::GameOver { .. });
                if s.is_demo() || !over || !self.scores.qualifies(s.game.score) {
                    return None;
                }
                let name = if self.last_name.is_empty() {
                    UNNAMED
                } else {
                    self.last_name.as_str()
                };
                Some(HighScore::new(name, s.game.score, s.game.level))
            }
            _ => None,
        }
    }

    /// Put a pending score (see [`App::pending_score`]) into the table and
    /// save it. Returns its rank. Called on quit; the screen is left as is.
    fn record_pending_score(&mut self) -> Option<usize> {
        let entry = self.pending_score()?;
        let rank = self.scores.insert(entry);
        self.save_scores();
        rank
    }

    fn save_scores(&mut self) {
        if self.persist && save::save_scores(&self.scores).is_err() {
            self.toast.show("COULD NOT SAVE HIGH SCORES");
        }
    }

    // ----- settings -------------------------------------------------------------------

    /// Apply the current settings to audio, renderer and window right away.
    fn apply_settings(&mut self) {
        self.sound.apply(&self.settings);
        self.renderer.shake_enabled = self.settings.screen_shake;
        self.renderer.scanlines = self.settings.scanlines;
        if self.settings.fullscreen != self.fullscreen_now {
            self.fullscreen_now = self.settings.fullscreen;
            self.focus_grace = FOCUS_GRACE;
            set_fullscreen(self.fullscreen_now);
        }
    }

    fn save_settings(&mut self) {
        if !self.persist {
            return;
        }
        let mut s = self.settings.clone();
        if self.args.windowed && !self.fullscreen_touched {
            // `--windowed` is a one-off override; keep the saved preference.
            s.fullscreen = self.saved_fullscreen;
        }
        if save::save_settings(&s).is_err() {
            self.toast.show("COULD NOT SAVE SETTINGS");
        }
    }

    /// Persist anything pending before the window closes: a qualifying
    /// score that has not been entered yet and unsaved option changes.
    fn shutdown(&mut self) {
        self.record_pending_score();
        if let Screen::Options(st) = &self.screen {
            if st.dirty {
                self.save_settings();
            }
        }
        self.sound.stop_all();
    }

    // ----- audio & cursor ---------------------------------------------------------------

    fn sync_audio(&mut self) {
        let (music, ambience) = match &self.screen {
            Screen::Splash(_) | Screen::NameEntry(_) => (None, LoopHint::Silent),
            Screen::Title | Screen::HowTo(_) | Screen::Demo => {
                (Some(Track::Title), LoopHint::Silent)
            }
            Screen::HighScores(st) => {
                let wait = st.from_game && st.t < SCORES_MUSIC_DELAY;
                (
                    if wait { None } else { Some(Track::Title) },
                    LoopHint::Silent,
                )
            }
            Screen::Options(st) => match st.back {
                Back::Title => (Some(Track::Title), LoopHint::Silent),
                Back::Pause => (None, LoopHint::Silent),
            },
            Screen::Playing => match &self.session {
                Some(s) if s.interlude.is_some() => (Some(Track::Interlude), LoopHint::Silent),
                Some(s) => (None, s.ambience(true)),
                None => (None, LoopHint::Silent),
            },
        };
        self.sound.music(music);
        self.sound.ambience(ambience);
    }

    /// Hide the mouse pointer during active play.
    fn sync_cursor(&mut self) {
        if self.freeze_sim {
            return;
        }
        let playing = matches!(self.screen, Screen::Playing | Screen::Demo)
            && self
                .session
                .as_ref()
                .is_some_and(|s| !s.paused() && s.interlude.is_none());
        let want = !playing;
        if want != self.cursor_shown {
            self.cursor_shown = want;
            show_mouse(want);
        }
    }

    // ----- drawing ----------------------------------------------------------------------

    fn draw(&mut self) {
        self.renderer.draw_backdrop();
        let l = self.renderer.layout;
        match &self.screen {
            Screen::Splash(s) => s.draw(&l),
            Screen::Title => self.title.draw(&l),
            Screen::Options(st) => {
                if st.back == Back::Pause {
                    if let Some(s) = &self.session {
                        s.draw(&mut self.renderer);
                        ui::dim_screen(&l, 0.84);
                    }
                }
                st.draw(&l, &self.settings, self.sound.muted());
            }
            Screen::HighScores(st) => st.draw(&l),
            Screen::HowTo(st) => st.draw(&l, &self.settings),
            Screen::Playing => {
                if let Some(s) = &self.session {
                    s.draw(&mut self.renderer);
                    if let Some(pause) = &s.pause {
                        overlay::draw_pause(&l, pause, self.time);
                    }
                }
            }
            Screen::Demo => {
                if let Some(s) = &self.session {
                    s.draw(&mut self.renderer);
                }
                overlay::draw_demo_banner(&l, self.time);
            }
            Screen::NameEntry(st) => st.draw(&l),
        }
        self.toast.draw(&l);
        if self.sound.muted() {
            overlay::draw_mute_badge(&l, self.time);
        }
        self.fade.draw(&l);
        self.renderer.draw_post();
        if self.settings.show_fps {
            self.fps.draw(&l);
        }
    }
}

/// Pre-filled initials on the first NEW HIGH SCORE screen of a session.
const DEFAULT_NAME: &str = "AAA";

/// 0-based rank `score` would take in `table` (stable: after equal scores).
fn predicted_rank(table: &HighScores, score: u32) -> usize {
    table
        .entries
        .iter()
        .position(|e| e.score < score)
        .unwrap_or(table.entries.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Actions;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("glimmer")
            .chain(list.iter().copied())
            .map(String::from)
            .collect()
    }

    fn run_args(list: &[&str]) -> AppArgs {
        match AppArgs::parse_command(&args(list)) {
            Ok(Command::Run(a)) => a,
            other => panic!("unexpected parse result for {list:?}: {other:?}"),
        }
    }

    #[test]
    fn parses_the_empty_command_line() {
        assert_eq!(run_args(&[]), AppArgs::default());
        // Even an empty argv (no program name) is fine.
        assert_eq!(
            AppArgs::parse_command(&[]),
            Ok(Command::Run(AppArgs::default()))
        );
    }

    #[test]
    fn parses_all_flags() {
        let a = run_args(&[
            "--level",
            "4",
            "--seed",
            "99",
            "--windowed",
            "--mute",
            "--shot",
            "pause",
            "out.png",
        ]);
        assert_eq!(a.start_level, Some(4));
        assert_eq!(a.seed, Some(99));
        assert!(a.windowed && a.mute);
        assert_eq!(a.shot, Some(("pause".to_owned(), "out.png".to_owned())));
        let b = run_args(&["--level=7", "--seed=3"]);
        assert_eq!((b.start_level, b.seed), (Some(7), Some(3)));
        assert!(run_args(&["-psn_0_12345"]).shot.is_none());
    }

    #[test]
    fn help_and_version() {
        assert_eq!(
            AppArgs::parse_command(&args(&["--help"])),
            Ok(Command::Help)
        );
        assert_eq!(
            AppArgs::parse_command(&args(&["--mute", "-h"])),
            Ok(Command::Help)
        );
        assert_eq!(
            AppArgs::parse_command(&args(&["--version"])),
            Ok(Command::Version)
        );
        assert!(usage().contains("--shot"));
        for scene in SHOT_SCENES {
            assert!(usage().contains(scene));
        }
    }

    #[test]
    fn rejects_bad_arguments_with_usage() {
        let bad: [&[&str]; 10] = [
            &["--bogus"],
            &["--level"],
            &["--level", "0"],
            &["--level", "abc"],
            &["--level", "100000"],
            &["--seed", "-1"],
            &["--shot", "title"],
            &["--shot", "nope", "x.png"],
            &["--shot", "title", "--mute"],
            &["play"],
        ];
        for list in bad {
            let e = AppArgs::parse_command(&args(list)).expect_err("should be rejected");
            assert!(e.contains("USAGE"), "{list:?}: {e}");
        }
    }

    #[test]
    fn rank_prediction_matches_insert() {
        let mut table = HighScores::defaults();
        for score in [1, 3_000, 3_001, 15_000, 29_999, 30_000, 99_999] {
            let predicted = predicted_rank(&table, score);
            let mut t = table.clone();
            let got = t.insert(HighScore::new("ABC", score, 1));
            if table.qualifies(score) {
                assert_eq!(got, Some(predicted), "score {score}");
            }
        }
        table.entries.clear();
        assert_eq!(predicted_rank(&table, 10), 0);
    }

    #[test]
    fn parses_shot_time() {
        let a = run_args(&["--shot", "interlude1", "x.png", "--shot-time", "2.5"]);
        assert_eq!(a.shot_time, Some(2.5));
        assert_eq!(run_args(&["--shot-time=0"]).shot_time, Some(0.0));
        for bad in ["-1", "nan", "inf", "abc", "100000"] {
            assert!(
                AppArgs::parse_command(&args(&["--shot-time", bad])).is_err(),
                "{bad}"
            );
        }
        assert!(usage().contains("--shot-time"));
    }

    // ----- flow tests (no window: synthetic input through `App::step`) -----

    const DT: f32 = 1.0 / 60.0;

    fn test_app() -> App {
        let mut a = App::new(
            AppArgs::default(),
            Settings::default(),
            HighScores::defaults(),
            SoundOut::disabled(),
            false,
        );
        a.screen = Screen::Title;
        a
    }

    fn step(a: &mut App, dt: f32, act: Actions) {
        a.input.act = act;
        a.step(dt);
    }

    fn enter() -> Actions {
        Actions {
            confirm: true,
            enter: true,
            any: true,
            ..Actions::default()
        }
    }

    fn esc() -> Actions {
        Actions {
            escape: true,
            back: true,
            pause: true,
            any: true,
            ..Actions::default()
        }
    }

    /// Start a game from the title and put it on its GAME OVER hold with
    /// `score` points.
    fn game_over(a: &mut App, score: u32, hold: f32) {
        step(a, DT, enter());
        assert!(matches!(a.screen, Screen::Playing));
        let s = a.session.as_mut().unwrap();
        s.game.score = score;
        s.game.phase = Phase::GameOver { t: hold };
    }

    fn paused(a: &App) -> bool {
        a.session.as_ref().is_some_and(Session::paused)
    }

    #[test]
    fn closing_during_name_entry_keeps_the_score() {
        let mut a = test_app();
        game_over(&mut a, 50_000, GAME_OVER_SKIP);
        step(&mut a, DT, enter());
        let Screen::NameEntry(st) = &mut a.screen else {
            panic!("expected name entry");
        };
        st.entry = entry::NameEntry::new("ZO");
        a.shutdown();
        assert_eq!(a.scores.best(), 50_000);
        assert_eq!(a.scores.entries[0].name, "ZOA");
        assert_eq!(a.scores.entries[0].level, 1);
    }

    #[test]
    fn closing_during_game_over_keeps_a_qualifying_score() {
        let mut a = test_app();
        game_over(&mut a, 60_000, 0.2);
        a.shutdown();
        assert_eq!(a.scores.best(), 60_000);
        assert_eq!(a.scores.entries[0].name, UNNAMED);

        // The last name used is the better guess when there is one.
        let mut a = test_app();
        a.last_name = "BOB".to_owned();
        game_over(&mut a, 60_000, 0.2);
        assert_eq!(a.record_pending_score(), Some(0));
        assert_eq!(a.scores.entries[0].name, "BOB");

        // Nothing is recorded for a score that does not qualify, a game
        // still running, or the demo.
        let mut a = test_app();
        game_over(&mut a, 10, 0.2);
        let before = a.scores.clone();
        a.shutdown();
        assert_eq!(a.scores, before);
        let mut a = test_app();
        step(&mut a, DT, enter());
        a.session.as_mut().unwrap().game.score = 90_000;
        assert_eq!(a.record_pending_score(), None);
        let mut a = test_app();
        a.start_demo();
        let s = a.session.as_mut().unwrap();
        s.game.score = 90_000;
        s.game.phase = Phase::GameOver { t: 0.2 };
        assert_eq!(a.record_pending_score(), None);
    }

    #[test]
    fn mashing_enter_past_game_over_does_not_confirm_the_name() {
        let mut a = test_app();
        game_over(&mut a, 50_000, GAME_OVER_SKIP);
        step(&mut a, DT, enter());
        assert!(matches!(a.screen, Screen::NameEntry(_)));
        for _ in 0..10 {
            step(&mut a, DT, enter());
            step(&mut a, DT, Actions::default());
        }
        assert!(
            matches!(a.screen, Screen::NameEntry(_)),
            "confirmed too early"
        );
        step(&mut a, entry::INPUT_GRACE, Actions::default());
        step(&mut a, DT, enter());
        assert!(matches!(a.screen, Screen::HighScores(_)));
        assert_eq!(a.scores.best(), 50_000);
    }

    #[test]
    fn losing_focus_pauses_a_running_game_only() {
        let lost = Actions {
            focus_lost: true,
            ..Actions::default()
        };
        let mut a = test_app();
        step(&mut a, DT, lost);
        assert!(matches!(a.screen, Screen::Title));
        step(&mut a, DT, enter());
        a.session.as_mut().unwrap().game.debug_skip_ready();
        step(&mut a, DT, Actions::default());
        assert!(!paused(&a));
        step(&mut a, DT, lost);
        assert!(paused(&a), "focus loss pauses");
        // Coming back does not unpause by itself; a second loss is harmless.
        step(&mut a, DT, lost);
        step(&mut a, DT, Actions::default());
        assert!(paused(&a));
        step(&mut a, DT, esc());
        assert!(!paused(&a));
        // Right after a fullscreen switch the window "moves": no pause.
        a.focus_grace = FOCUS_GRACE;
        step(&mut a, DT, lost);
        assert!(!paused(&a));
        // Never on GAME OVER or during the demo.
        a.session.as_mut().unwrap().game.phase = Phase::GameOver { t: 0.1 };
        a.focus_grace = 0.0;
        step(&mut a, DT, lost);
        assert!(!paused(&a));
        let mut a = test_app();
        a.start_demo();
        step(&mut a, DT, lost);
        assert!(!paused(&a));
    }

    #[test]
    fn restart_and_quit_ask_first() {
        let down = Actions {
            down: true,
            ..Actions::default()
        };
        let mut a = test_app();
        step(&mut a, DT, enter());
        a.session.as_mut().unwrap().game.score = 1234;
        step(&mut a, DT, esc());
        assert!(paused(&a));
        // RESTART: the first Enter only asks.
        step(&mut a, DT, down);
        step(&mut a, DT, enter());
        assert!(paused(&a));
        assert_eq!(a.session.as_ref().unwrap().game.score, 1234);
        // Moving away cancels: Enter now acts on OPTIONS instead.
        step(&mut a, DT, down);
        step(&mut a, DT, enter());
        assert!(matches!(a.screen, Screen::Options(_)));
        step(&mut a, DT, esc());
        assert!(matches!(a.screen, Screen::Playing) && paused(&a));
        let pause = a.session.as_mut().unwrap().pause.as_mut().unwrap();
        assert_eq!(pause.armed, None);
        // Enter twice on RESTART starts a fresh game.
        pause.menu.select(overlay::PAUSE_RESTART);
        step(&mut a, DT, enter());
        step(&mut a, DT, enter());
        assert!(!paused(&a));
        assert_eq!(a.session.as_ref().unwrap().game.score, 0);
        step(&mut a, DT, esc());
        // QUIT TO TITLE: Esc cancels the question without resuming.
        let pause = a.session.as_mut().unwrap().pause.as_mut().unwrap();
        pause.menu.select(overlay::PAUSE_QUIT);
        step(&mut a, DT, enter());
        step(&mut a, DT, esc());
        assert!(paused(&a));
        step(&mut a, DT, enter());
        step(&mut a, DT, enter());
        assert!(matches!(a.screen, Screen::Title));
        assert!(a.session.is_none());
    }

    #[test]
    fn resuming_the_first_get_set_skips_the_lost_jingle() {
        let mut a = test_app();
        step(&mut a, DT, enter());
        step(&mut a, 0.5, Actions::default());
        step(&mut a, DT, esc());
        assert!(paused(&a));
        step(&mut a, 30.0, Actions::default());
        step(&mut a, DT, esc());
        assert!(!paused(&a));
        match a.session.as_ref().unwrap().game.phase {
            Phase::Ready { t, total } => {
                assert_eq!(total, READY_TIME_FIRST);
                assert!((total - t - RESUME_READY_LEFT).abs() < 1e-4, "t {t}");
            }
            p => panic!("unexpected {p:?}"),
        }
        // A later pause near the end of the hold changes nothing.
        let s = a.session.as_mut().unwrap();
        s.game.phase = Phase::Ready {
            t: READY_TIME_FIRST - 0.3,
            total: READY_TIME_FIRST,
        };
        step(&mut a, DT, esc());
        step(&mut a, DT, esc());
        assert!(matches!(
            a.session.as_ref().unwrap().game.phase,
            Phase::Ready { t, .. } if (READY_TIME_FIRST - t) < 0.31
        ));
    }

    #[test]
    fn app_clock_stays_bounded() {
        let mut a = test_app();
        for _ in 0..100 {
            step(&mut a, 100.0, Actions::default());
        }
        assert!(a.time < ui::CLOCK_WRAP + ui::CLOCK_KEEP);
    }

    #[test]
    fn every_documented_scene_is_accepted() {
        for scene in SHOT_SCENES {
            let a = run_args(&["--shot", scene, "x.png"]);
            assert_eq!(a.shot.map(|s| s.0), Some(scene.to_owned()));
        }
    }
}
