//! `--shot <scene> <out.png>`: build one scene deterministically, render it,
//! save the window as a PNG and return.
//!
//! Scenes run with a fixed seed (`--seed` overrides), no audio, default
//! settings and the default high-score table; nothing is read from or written
//! to the user's data directory. Gameplay scenes are produced by simulating
//! the real game with the autopilot plus the `Game::debug_*` hooks, then the
//! simulation is frozen while a few frames are drawn so visual effects settle.
//! Gameplay scenes start at `--level` (default 1); the maze and interlude
//! scenes use the first level at or after it that shows that maze or
//! interlude. `--mute` shows the mute badge; `--shot-time` sets the clock of
//! the menu-style and interlude scenes.

use super::entry::EntryState;
use super::howto::HowToState;
use super::interlude::Interlude;
use super::options::{Back, OptionsState};
use super::overlay::{Fade, PauseMenu, PAUSE_QUIT};
use super::scores::ScoresState;
use super::session::{Fx, Session};
use super::sound::SoundOut;
use super::title::{SplashState, TitleState};
use super::{App, AppArgs, Screen};
use crate::autopilot::Autopilot;
use crate::config::{
    interlude_after, maze_for_level, CAUGHT_TIME, DEATH_TIME, LEVEL_CLEAR_HOLD, SIM_DT,
};
use crate::game::{GameOptions, Phase};
use crate::save::{HighScore, HighScores, Settings};
use crate::shade::ShadeState;
use macroquad::texture::get_screen_data;
use macroquad::window::next_frame;
use std::panic::AssertUnwindSafe;
use std::path::Path;

/// Default seed for every shot.
const SHOT_SEED: u64 = 20_261_117;
/// Frame time used while drawing (deterministic animation).
const SHOT_DT: f32 = 1.0 / 60.0;
/// Frames drawn before the scene is built (window settles at its size).
const WARMUP_FRAMES: u32 = 4;
/// Frames drawn before the capture (animations settle).
const SETTLE_FRAMES: u32 = 20;
/// Seconds of autopilot play before most gameplay scenes.
const PLAY_SECONDS: f32 = 6.0;

/// Render `scene` into `out` (a `.png` path) and return.
pub async fn run(args: AppArgs, scene: &str, out: &str) {
    if let Err(e) = check_output(out) {
        fail(scene, &e);
    }
    let mut app = App::new(
        AppArgs {
            windowed: true,
            ..args
        },
        Settings::default(),
        HighScores::defaults(),
        SoundOut::disabled(),
        false,
    );
    // No audio either way; the flag only decides whether the badge shows.
    let mute = app.args.mute;
    app.sound.set_muted(mute, &app.settings);
    // The window opens at its configured 594 x 792 (logical) size. It is not
    // resized here: `request_new_screen_size` applies the DPI scale a second
    // time on macOS.
    app.freeze_sim = true;
    // Shake would offset the frame by a random amount; keep shots stable.
    app.renderer.shake_enabled = false;

    for _ in 0..WARMUP_FRAMES {
        app.renderer.begin_frame();
        app.renderer.draw_backdrop();
        next_frame().await;
    }
    if let Err(e) = setup(&mut app, scene) {
        fail(scene, &e);
    }
    app.fade = Fade::default();
    app.input.reset();

    for i in 0..SETTLE_FRAMES {
        app.frame(SHOT_DT, false);
        if i + 1 == SETTLE_FRAMES {
            match save_png(out) {
                Ok(()) => println!("glimmer: saved scene '{scene}' to {out}"),
                Err(e) => fail(scene, &e),
            }
        }
        next_frame().await;
    }
}

fn fail(scene: &str, msg: &str) -> ! {
    eprintln!("glimmer: --shot {scene}: {msg}");
    std::process::exit(1);
}

fn check_output(out: &str) -> Result<(), String> {
    let path = Path::new(out);
    let is_png = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("png"));
    if !is_png {
        return Err(format!("output file '{out}' must end in .png"));
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create '{}': {e}", dir.display()))?;
    }
    Ok(())
}

fn save_png(out: &str) -> Result<(), String> {
    let mut image = get_screen_data();
    if image.width == 0 || image.height == 0 {
        return Err("the window has no pixels to capture".to_owned());
    }
    // The window is opaque, but translucent overlays leave partial alpha in
    // the framebuffer; store what is actually on screen.
    for px in image.bytes.as_chunks_mut::<4>().0 {
        px[3] = u8::MAX;
    }
    // `export_png` panics on I/O errors; contain that and report it instead.
    std::panic::catch_unwind(AssertUnwindSafe(|| image.export_png(out)))
        .map_err(|_| format!("could not write '{out}'"))
}

/// First level at or after `from` for which `pick` holds (searching a
/// generous window so every cyclic schedule is found).
fn level_from(from: u32, pick: impl Fn(u32) -> bool) -> Option<u32> {
    (from.max(1)..from.max(1).saturating_add(400)).find(|&l| pick(l))
}

/// Build the requested scene.
fn setup(app: &mut App, scene: &str) -> Result<(), String> {
    let seed = app.args.seed.unwrap_or(SHOT_SEED);
    let level = app.args.start_level.unwrap_or(1).max(1);
    let clock = |default: f32| app.args.shot_time.unwrap_or(default);
    let mut pilot = Autopilot::new(seed);
    match scene {
        "splash" => app.screen = Screen::Splash(SplashState { t: clock(1.3) }),
        "title" => {
            app.title = TitleState::new();
            app.title.t = clock(6.0);
            app.screen = Screen::Title;
        }
        "options" => {
            let mut st = OptionsState::new(Back::Title);
            st.t = clock(1.0);
            app.screen = Screen::Options(st);
        }
        "scores" => {
            let rank = app.scores.insert(HighScore::new("GLI", 25_000, 5));
            let mut st = ScoresState::new(&app.scores, rank, None, false);
            st.t = clock(2.0);
            app.screen = Screen::HighScores(st);
        }
        "howto" | "howto2" | "howto3" | "howto4" => {
            let page = match scene {
                "howto2" => 1,
                "howto3" => 2,
                "howto4" => 3,
                _ => 0,
            };
            let mut st = HowToState::new(page);
            st.t = clock(1.0);
            st.page_t = st.t;
            app.screen = Screen::HowTo(Box::new(st));
        }
        "entry" => {
            let mut st = EntryState::new("GLI", 48_620, 7, 2);
            st.entry.cursor = 2;
            st.t = clock(1.0);
            app.screen = Screen::NameEntry(st);
        }
        "play" | "pause" | "pauseconfirm" => {
            play(app, &mut pilot, level, seed);
            if scene != "play" {
                let mut pause = PauseMenu::new();
                if scene == "pauseconfirm" {
                    pause.menu.select(PAUSE_QUIT);
                    pause.armed = Some(PAUSE_QUIT);
                }
                if let Some(s) = app.session.as_mut() {
                    s.pause = Some(pause);
                }
            }
        }
        "maze1" | "maze2" | "maze3" => {
            let index = match scene {
                "maze1" => 1,
                "maze2" => 2,
                _ => 3,
            };
            let level = level_from(level, |l| maze_for_level(l) == index)
                .ok_or("no level uses that maze")?;
            play(app, &mut pilot, level, seed);
        }
        "fright" => {
            play_for(app, &mut pilot, level, seed, 4.0);
            with_game(app, |g| g.debug_start_fright());
            simulate(app, &mut pilot, 1.0);
            expect(app, "a fright", |p| {
                matches!(p, Phase::Playing | Phase::EatPause { .. })
            })?;
        }
        "eat" => {
            play_for(app, &mut pilot, level, seed, 4.0);
            with_game(app, |g| g.debug_start_fright());
            simulate(app, &mut pilot, 0.4);
            for _ in 0..4 {
                if phase(app).is_some_and(|p| matches!(p, Phase::EatPause { .. })) {
                    break;
                }
                with_game(app, |g| {
                    let ppos = g.player.mover.pos;
                    let pdir = g.player.mover.dir;
                    if let Some(s) = g
                        .shades
                        .iter_mut()
                        .find(|s| s.state == ShadeState::Active && s.frightened)
                    {
                        s.mover.pos = ppos;
                        s.mover.prev_pos = ppos;
                        s.mover.dir = pdir;
                    }
                });
                simulate(app, &mut pilot, SIM_DT);
            }
            simulate(app, &mut pilot, 0.2);
            expect(app, "an eat pause", |p| matches!(p, Phase::EatPause { .. }))?;
        }
        "dying" => {
            play_for(app, &mut pilot, level, seed, 4.0);
            with_game(app, |g| g.debug_kill_player());
            simulate(app, &mut pilot, CAUGHT_TIME + DEATH_TIME * 0.55);
            expect(app, "the death animation", |p| {
                matches!(p, Phase::Dying { .. })
            })?;
        }
        "levelclear" => {
            play_for(app, &mut pilot, level, seed, 3.0);
            with_game(app, |g| g.debug_clear_dots(0));
            simulate(app, &mut pilot, SIM_DT + LEVEL_CLEAR_HOLD + 0.45);
            expect(app, "a level clear", |p| {
                matches!(p, Phase::LevelClear { .. })
            })?;
        }
        "gameover" => {
            start(app, level, seed, 1, false);
            with_game(app, |g| g.debug_skip_ready());
            simulate(app, &mut pilot, 3.0);
            with_game(app, |g| g.debug_kill_player());
            simulate(app, &mut pilot, CAUGHT_TIME + DEATH_TIME + 0.8);
            expect(app, "game over", |p| matches!(p, Phase::GameOver { .. }))?;
        }
        "interlude0" | "interlude1" | "interlude2" => {
            let variant: u8 = match scene {
                "interlude0" => 0,
                "interlude1" => 1,
                _ => 2,
            };
            let level = level_from(level, |l| interlude_after(l) == Some(variant))
                .ok_or("no level leads to that interlude")?;
            start(app, level, seed, 3, false);
            with_game(app, |g| {
                g.debug_skip_ready();
                g.debug_clear_dots(0);
            });
            // Level clear hold + flash, then the game waits in `Interlude`.
            for _ in 0..(8.0 / SIM_DT) as u32 {
                if phase(app).is_some_and(|p| matches!(p, Phase::Interlude { .. })) {
                    break;
                }
                simulate(app, &mut pilot, SIM_DT);
            }
            expect(app, "an interlude", |p| {
                matches!(p, Phase::Interlude { .. })
            })?;
            let mid: f32 = match variant {
                0 => 4.3,
                1 => 6.05,
                _ => 4.75,
            };
            let t = app
                .args
                .shot_time
                .unwrap_or(mid)
                .min(Interlude::duration(variant));
            if let Some(s) = app.session.as_mut() {
                let mut scene = Interlude::new(variant);
                scene.t = t;
                s.interlude = Some(scene);
            }
        }
        "demo" => {
            start(app, level, seed, 1, true);
            with_game(app, |g| g.debug_skip_ready());
            simulate(app, &mut pilot, PLAY_SECONDS);
        }
        _ => return Err(format!("unknown scene '{scene}'")),
    }
    Ok(())
}

/// Start a game and replace the current screen with it.
fn start(app: &mut App, level: u32, seed: u64, lives: u32, demo: bool) {
    let opts = GameOptions {
        start_lives: lives,
        extra_life_at: if demo {
            None
        } else {
            Some(crate::save::Settings::default().extra_life_at)
        },
        seed,
        demo,
        start_level: level,
        high_score: app.scores.best(),
    };
    app.renderer.reset();
    app.session = Some(Session::new(opts));
    app.screen = if demo { Screen::Demo } else { Screen::Playing };
}

/// Start at `level`, skip "GET SET!" and let the autopilot play a while.
fn play(app: &mut App, pilot: &mut Autopilot, level: u32, seed: u64) {
    play_for(app, pilot, level, seed, PLAY_SECONDS);
}

fn play_for(app: &mut App, pilot: &mut Autopilot, level: u32, seed: u64, seconds: f32) {
    start(app, level, seed, 3, false);
    with_game(app, |g| g.debug_skip_ready());
    simulate(app, pilot, seconds);
}

fn with_game(app: &mut App, f: impl FnOnce(&mut crate::game::Game)) {
    if let Some(s) = app.session.as_mut() {
        f(&mut s.game);
    }
}

fn phase(app: &App) -> Option<Phase> {
    app.session.as_ref().map(|s| s.game.phase)
}

fn expect(app: &App, what: &str, ok: impl Fn(Phase) -> bool) -> Result<(), String> {
    match phase(app) {
        Some(p) if ok(p) => Ok(()),
        Some(p) => Err(format!("could not set up {what} (game is in {p:?})")),
        None => Err(format!("could not set up {what} (no game)")),
    }
}

/// Run the simulation for `seconds` with the autopilot steering, advancing
/// the renderer's visual clock alongside so effects age naturally.
fn simulate(app: &mut App, pilot: &mut Autopilot, seconds: f32) {
    let ticks = (seconds / SIM_DT).round().max(1.0) as u32;
    let App {
        session,
        renderer,
        sound,
        ..
    } = app;
    let Some(s) = session.as_mut() else {
        return;
    };
    let mut fx = Fx {
        renderer,
        sound,
        audible: false,
    };
    for _ in 0..ticks {
        let dir = pilot.choose(&s.game);
        s.tick(dir, &mut fx);
        fx.renderer.update(SIM_DT);
    }
}
