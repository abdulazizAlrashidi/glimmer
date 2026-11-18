//! Runtime audio: loads every synthesised sound into macroquad's mixer and
//! maps game events / state to playback.
//!
//! Notes on macroquad 0.4.16 audio (quad-snd 0.2.8):
//! - `play_sound` may overlap instances of the same `Sound`.
//! - `stop_sound` / `set_sound_volume` affect *all* instances of a `Sound`.
//! - The mixer sums voices without limiting: keep peak levels modest.
//! - `load_sound_from_bytes` decodes WAV synchronously on native targets.
//! - Commands take effect at the start of the next output buffer (about
//!   23 ms) and a volume change is an instant step, so stopping a sound that
//!   is playing at full level is an audible click.
//!
//! The app calls `set_volumes` whenever settings change, passing each
//! `0..=10` setting divided by 10.
//!
//! # Behaviour
//! - [`Audio::load`] synthesises every sound on a small pool of worker
//!   threads (the calling thread takes part, so it also works where threads
//!   are unavailable), encodes each as an in-memory WAV and loads it. A sound
//!   that fails to synthesise or load is logged and simply stays silent.
//! - Each sound has a fixed mix gain so effects, ambience and music sit well
//!   together. The user's master / music / effects volumes scale on top
//!   through a perceptual curve: linear in decibels from -40 dB at 0.1 to
//!   0 dB at 1.0 (about 4.4 dB per settings step), and silent at exactly 0.
//! - Two loops can run at once: the gameplay *ambience*
//!   ([`Audio::set_ambience`]) and the *music* ([`Audio::set_music`]). A loop
//!   whose effective volume is zero is not played at all; it starts as soon
//!   as its volume is raised again.
//! - Nothing is ever cut at full level. Every stop is a fade timed by the
//!   clock (`macroquad::time::get_time`) and advanced on every call (the app
//!   calls `set_ambience` every frame, which keeps fades moving):
//!   - a *stop* (ambience going silent, player caught, level cleared, pause,
//!     [`Audio::stop_all`], muting) fades over 50 ms: it still reads as an
//!     instant cut, without the click;
//!   - one ambience loop replacing another cross-fades over 0.1 s, and a loop
//!     that is wanted again while it fades out fades back in;
//!   - music switched off or replaced fades out over 0.5 s;
//!   - re-triggering a long exclusive effect (jingles, death, flare) fades
//!     the running instance out and then starts the new one.
//! - [`Audio::stop_all`] and [`Audio::stop_effects`] never touch the UI blips
//!   ([`Sfx::is_ui`]), so a confirm blip played in the same frame as a screen
//!   change is still heard.
//! - Motes play the *glimmer run*: each mote eaten within 0.35 s of the
//!   previous one steps up a five-note pentatonic scale (wrapping after the
//!   top note); after a pause the run starts again from the bottom.
//!
//! The playback rules live in a backend-independent state machine with an
//! injected clock, so they are unit-tested without an audio device.

use crate::game::{GameEvent, LoopHint};
use crate::synth::{self, Sfx, Track};
use macroquad::audio::{
    load_sound_from_bytes, play_sound, set_sound_volume, stop_sound, PlaySoundParams, Sound,
};
use macroquad::time::get_time;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

const SFX_COUNT: usize = Sfx::ALL.len();
const TRACK_COUNT: usize = Track::ALL.len();
/// Mixer slots: every effect, then every loop (see [`slot`]).
const SLOTS: usize = SFX_COUNT + TRACK_COUNT;

/// Effective volumes at or below this are treated as silence.
const SILENT: f32 = 1e-4;

/// Sounds shorter than this are never handed to the decoder (quad-snd panics
/// on undecodable data, so only well-formed, non-trivial WAVs go in).
const MIN_SAMPLES: usize = 256;

/// Upper bound on synthesis worker threads (including the calling thread).
const MAX_WORKERS: usize = 8;

/// Length in seconds of a *stop*: a micro fade that still reads as an instant
/// cut but splits the drop to silence over two or three mixer buffers.
const STOP_FADE: f64 = 0.05;
/// One ambience loop replacing another, or a fading loop coming back.
const CROSS_FADE: f64 = 0.1;
/// Music switched off or replaced.
const MUSIC_FADE: f64 = 0.5;
/// While a fade runs, volume changes smaller than this are not sent (the
/// mixer may be called several times per frame).
const RAMP_EPS: f32 = 2e-3;
/// A one-shot is assumed to sound this much longer than its length (mixer
/// latency), so a stop never misses the end of it.
const END_MARGIN: f64 = 0.1;
/// Motes eaten at most this many seconds apart continue the glimmer run.
const RUN_GAP: f64 = 0.35;
/// Gain of the quietest audible volume setting (0.1), in decibels.
const VOLUME_FLOOR_DB: f32 = -40.0;

pub struct Audio {
    sounds: Sounds,
    mix: Mix,
}

impl Audio {
    /// Synthesise and load all sounds. Never panics: if anything fails the
    /// affected sounds are silently skipped.
    pub async fn load() -> Audio {
        let (sfx_pcm, track_pcm) = render_all();
        let mut secs = [0.0; SFX_COUNT];
        for (s, pcm) in secs.iter_mut().zip(&sfx_pcm) {
            *s = pcm
                .as_ref()
                .map_or(0.0, |p| p.len() as f64 / f64::from(synth::SAMPLE_RATE));
        }
        let mut sfx = Vec::with_capacity(SFX_COUNT);
        for (s, pcm) in Sfx::ALL.into_iter().zip(sfx_pcm) {
            sfx.push(load_pcm(&format!("{s:?}"), pcm).await);
        }
        let mut tracks = Vec::with_capacity(TRACK_COUNT);
        for (t, pcm) in Track::ALL.into_iter().zip(track_pcm) {
            tracks.push(load_pcm(&format!("{t:?}"), pcm).await);
        }
        Audio {
            sounds: Sounds { sfx, tracks },
            mix: Mix::new(secs),
        }
    }

    /// Linear volumes in `[0, 1]` (settings / 10), mapped through the
    /// perceptual curve: master scales everything; music applies to
    /// `Sfx::is_music` jingles and music tracks; sfx to everything else.
    pub fn set_volumes(&mut self, master: f32, music: f32, sfx: f32) {
        self.mix
            .set_volumes(&mut self.sounds, get_time(), master, music, sfx);
    }

    pub fn play(&mut self, sfx: Sfx) {
        self.mix.play(&mut self.sounds, get_time(), sfx);
    }

    /// React to a gameplay event (glimmer-run mote chirps, flare, death...).
    pub fn handle_event(&mut self, ev: &GameEvent) {
        self.mix.handle_event(&mut self.sounds, get_time(), ev);
    }

    /// Keep the gameplay ambience in sync with `Game::loop_hint()`; call every
    /// frame (it also advances every fade). Switching only restarts audio when
    /// the hint actually changes.
    pub fn set_ambience(&mut self, hint: LoopHint) {
        self.mix.set_ambience(&mut self.sounds, get_time(), hint);
    }

    /// Start (if not already playing) or stop a music track (Title/Interlude).
    /// The old track fades out over 0.5 s.
    pub fn set_music(&mut self, track: Option<Track>) {
        self.mix.set_music(&mut self.sounds, get_time(), track);
    }

    /// Stop every loop and one-shot except the UI blips (e.g. when a game
    /// starts or ends), each with a click-free 50 ms fade.
    pub fn stop_all(&mut self) {
        self.mix.stop_all(&mut self.sounds, get_time());
    }

    /// Silence gameplay audio for the pause menu: the ambience and every
    /// gameplay one-shot / jingle stop (with a click-free micro fade), while UI
    /// blips (menu, pause, key sounds), the high-score fanfare and the music
    /// keep playing. The ambience comes back on the first `set_ambience` call
    /// with an audible hint (the app passes `Silent` while paused).
    pub fn stop_effects(&mut self) {
        self.mix.stop_effects(&mut self.sounds, get_time());
    }
}

// ---------------------------------------------------------------------------
// Mixer backend
// ---------------------------------------------------------------------------

/// A sound the mixer can play.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SoundId {
    Sfx(Sfx),
    Track(Track),
}

/// The mixer operations the playback rules need: macroquad's mixer in the
/// game, a call recorder in the tests.
trait Backend {
    /// Start a new instance; returns `false` if the sound is unavailable.
    fn play(&mut self, id: SoundId, looped: bool, volume: f32) -> bool;
    /// Stop every instance of the sound.
    fn stop(&mut self, id: SoundId);
    /// Set the volume of every instance of the sound.
    fn set_volume(&mut self, id: SoundId, volume: f32);
}

/// The loaded macroquad sounds (`None` where loading failed).
struct Sounds {
    /// Indexed by [`sfx_index`].
    sfx: Vec<Option<Sound>>,
    /// Indexed by [`track_index`].
    tracks: Vec<Option<Sound>>,
}

impl Sounds {
    fn get(&self, id: SoundId) -> Option<&Sound> {
        let slot = match id {
            SoundId::Sfx(s) => self.sfx.get(sfx_index(s)),
            SoundId::Track(t) => self.tracks.get(track_index(t)),
        };
        slot.and_then(Option::as_ref)
    }
}

impl Backend for Sounds {
    fn play(&mut self, id: SoundId, looped: bool, volume: f32) -> bool {
        match self.get(id) {
            Some(sound) => {
                play_sound(sound, PlaySoundParams { looped, volume });
                true
            }
            None => false,
        }
    }

    fn stop(&mut self, id: SoundId) {
        if let Some(sound) = self.get(id) {
            stop_sound(sound);
        }
    }

    fn set_volume(&mut self, id: SoundId, volume: f32) {
        if let Some(sound) = self.get(id) {
            set_sound_volume(sound, volume);
        }
    }
}

// ---------------------------------------------------------------------------
// Playback rules
// ---------------------------------------------------------------------------

/// A linear volume ramp between two absolute mixer volumes, in clock time.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ramp {
    from: f32,
    to: f32,
    start: f64,
    secs: f64,
}

impl Ramp {
    fn new(from: f32, to: f32, start: f64, secs: f64) -> Ramp {
        Ramp {
            from,
            to,
            start,
            secs: secs.max(0.0),
        }
    }

    fn end(&self) -> f64 {
        self.start + self.secs
    }

    fn done(&self, now: f64) -> bool {
        now >= self.end()
    }

    /// Volume at clock time `now`.
    fn at(&self, now: f64) -> f32 {
        if self.done(now) {
            return self.to;
        }
        let u = ((now - self.start) / self.secs).clamp(0.0, 1.0) as f32;
        self.from + (self.to - self.from) * u
    }

    /// True for a fade that ends in silence (and a stop).
    fn is_out(&self) -> bool {
        self.to <= 0.0
    }
}

/// Mixer state of one sound.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Chan {
    Idle,
    /// Playing at its mix volume; a one-shot until about `ends` (clock time,
    /// infinite for loops).
    On {
        ends: f64,
    },
    /// Playing while its volume ramps. A ramp to zero ends in a stop, after
    /// which the sound starts afresh if `replay` (a re-trigger that waited
    /// for the fade).
    Ramping {
        ramp: Ramp,
        ends: f64,
        replay: bool,
    },
}

/// The glimmer run: consecutive motes step up [`Sfx::MOTES`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct GlimmerRun {
    step: usize,
    /// Clock time of the previous mote.
    last: Option<f64>,
}

impl GlimmerRun {
    /// The chirp for a mote eaten at `now`: one step up if the previous mote
    /// was at most [`RUN_GAP`] ago (wrapping after the top note), else the
    /// bottom of the run.
    fn next(&mut self, now: f64) -> Sfx {
        let continues = self.last.is_some_and(|t| now - t <= RUN_GAP);
        self.step = if continues {
            (self.step + 1) % Sfx::MOTES.len()
        } else {
            0
        };
        self.last = Some(now);
        Sfx::MOTES[self.step]
    }
}

/// Backend-independent playback state: volumes, which sounds are running,
/// which loops are wanted (ambience and music), fades and the glimmer run.
#[derive(Clone, Debug)]
struct Mix {
    /// Mixer gains of the user's volumes (after the perceptual curve).
    master: f32,
    music: f32,
    effects: f32,
    /// Ambience hint last seen by `set_ambience`.
    hint: LoopHint,
    /// Ambience loop that should be playing.
    ambience: Option<Track>,
    /// Music loop that should be playing.
    music_track: Option<Track>,
    /// Per [`slot`].
    chans: [Chan; SLOTS],
    /// Volume last sent to the backend, per [`slot`].
    vols: [f32; SLOTS],
    /// Length of each one-shot in seconds (by [`sfx_index`]).
    sfx_secs: [f64; SFX_COUNT],
    run: GlimmerRun,
    /// Latest clock reading (time never runs backwards here).
    now: f64,
}

impl Mix {
    fn new(sfx_secs: [f64; SFX_COUNT]) -> Mix {
        Mix {
            master: 1.0,
            music: 1.0,
            effects: 1.0,
            hint: LoopHint::Silent,
            ambience: None,
            music_track: None,
            chans: [Chan::Idle; SLOTS],
            vols: [0.0; SLOTS],
            sfx_secs,
            run: GlimmerRun::default(),
            now: 0.0,
        }
    }

    fn set_volumes(&mut self, b: &mut impl Backend, now: f64, master: f32, music: f32, sfx: f32) {
        self.advance(b, now);
        let gains = (
            perceptual_gain(master),
            perceptual_gain(music),
            perceptual_gain(sfx),
        );
        if gains == (self.master, self.music, self.effects) {
            return;
        }
        (self.master, self.music, self.effects) = gains;
        // Everything already playing follows the new levels; whatever became
        // inaudible fades out, and wanted loops that can be heard again start.
        for id in IDS {
            let i = slot(id);
            let full = self.full(id);
            match self.chans[i] {
                Chan::Idle => {
                    if let SoundId::Track(t) = id {
                        if self.wants(t) {
                            self.start(b, id);
                        }
                    }
                }
                Chan::On { .. } if full <= SILENT => self.fade_out(id, STOP_FADE),
                Chan::On { .. } => self.set_vol(b, i, id, full, 0.0),
                Chan::Ramping { ramp, .. } => {
                    if full <= SILENT {
                        self.fade_out(id, STOP_FADE);
                    } else if !ramp.is_out() || self.wanted(id) {
                        self.fade_in(id);
                    }
                }
            }
        }
    }

    fn play(&mut self, b: &mut impl Backend, now: f64, sfx: Sfx) {
        self.advance(b, now);
        self.trigger(b, sfx);
    }

    fn handle_event(&mut self, b: &mut impl Backend, now: f64, ev: &GameEvent) {
        self.advance(b, now);
        match cue_for(ev) {
            Cue::Nothing => {}
            Cue::Play(s) => self.trigger(b, s),
            Cue::Mote => {
                let s = self.run.next(self.now);
                self.trigger(b, s);
            }
            Cue::StopAmbience(then) => {
                // Silence the ambience; it restarts when the game's hint changes.
                if let Some(t) = self.ambience.take() {
                    self.release_loop(t, STOP_FADE);
                }
                if let Some(s) = then {
                    self.trigger(b, s);
                }
            }
        }
    }

    fn set_ambience(&mut self, b: &mut impl Backend, now: f64, hint: LoopHint) {
        self.advance(b, now);
        if hint == self.hint {
            return;
        }
        self.hint = hint;
        let want = ambience_track(hint);
        if want == self.ambience {
            return;
        }
        let old = std::mem::replace(&mut self.ambience, want);
        if let Some(t) = want {
            self.want_loop(b, t);
        }
        if let Some(t) = old {
            // Cross-fade between two ambiences; going silent is a stop.
            self.release_loop(
                t,
                if want.is_some() {
                    CROSS_FADE
                } else {
                    STOP_FADE
                },
            );
        }
    }

    fn set_music(&mut self, b: &mut impl Backend, now: f64, track: Option<Track>) {
        self.advance(b, now);
        if track == self.music_track {
            return; // already requested (it may be waiting for volume)
        }
        let old = std::mem::replace(&mut self.music_track, track);
        if let Some(t) = old {
            self.release_loop(t, MUSIC_FADE);
        }
        if let Some(t) = track {
            self.want_loop(b, t);
        }
    }

    fn stop_all(&mut self, b: &mut impl Backend, now: f64) {
        self.advance(b, now);
        self.ambience = None;
        self.music_track = None;
        self.hint = LoopHint::Silent;
        self.run = GlimmerRun::default();
        for id in IDS {
            if !matches!(id, SoundId::Sfx(s) if s.is_ui()) {
                self.fade_out(id, STOP_FADE);
            }
        }
    }

    fn stop_effects(&mut self, b: &mut impl Backend, now: f64) {
        self.advance(b, now);
        self.ambience = None;
        self.hint = LoopHint::Silent;
        self.run = GlimmerRun::default();
        for id in IDS {
            let gameplay = match id {
                SoundId::Sfx(s) => is_gameplay(s),
                SoundId::Track(t) => !t.is_music(),
            };
            if gameplay {
                self.fade_out(id, STOP_FADE);
            }
        }
    }

    // ----- internals -------------------------------------------------------

    /// Full mix volume of a sound at the current settings.
    fn full(&self, id: SoundId) -> f32 {
        let (music, gain) = match id {
            SoundId::Sfx(s) => (s.is_music(), sfx_gain(s)),
            SoundId::Track(t) => (t.is_music(), track_gain(t)),
        };
        mix_volume(
            self.master,
            if music { self.music } else { self.effects },
            gain,
        )
    }

    /// How long a fresh instance plays (loops: forever).
    fn duration(&self, id: SoundId) -> f64 {
        match id {
            SoundId::Sfx(s) => self.sfx_secs[sfx_index(s)].max(0.0) + END_MARGIN,
            SoundId::Track(_) => f64::INFINITY,
        }
    }

    /// True if the ambience or the music wants loop `t` to be playing.
    fn wants(&self, t: Track) -> bool {
        self.ambience == Some(t) || self.music_track == Some(t)
    }

    fn wanted(&self, id: SoundId) -> bool {
        matches!(id, SoundId::Track(t) if self.wants(t))
    }

    /// Advance the clock and every fade; stop sounds whose fade has ended.
    fn advance(&mut self, b: &mut impl Backend, now: f64) {
        if now > self.now {
            self.now = now;
        }
        let now = self.now;
        for id in IDS {
            let i = slot(id);
            match self.chans[i] {
                Chan::Idle => {}
                Chan::On { ends } => {
                    if ends <= now {
                        self.chans[i] = Chan::Idle; // a one-shot that has finished
                    }
                }
                Chan::Ramping { ramp, ends, replay } => {
                    if !ramp.done(now) {
                        self.set_vol(b, i, id, ramp.at(now), RAMP_EPS);
                    } else if !ramp.is_out() {
                        self.chans[i] = Chan::On { ends };
                        self.set_vol(b, i, id, ramp.to, 0.0);
                    } else {
                        b.stop(id);
                        self.chans[i] = Chan::Idle;
                        self.vols[i] = 0.0;
                        if replay {
                            self.start(b, id);
                        }
                    }
                }
            }
        }
    }

    /// Send a volume unless it is within `eps` of the last one sent.
    fn set_vol(&mut self, b: &mut impl Backend, i: usize, id: SoundId, vol: f32, eps: f32) {
        if (vol - self.vols[i]).abs() > eps {
            b.set_volume(id, vol);
            self.vols[i] = vol;
        }
    }

    /// Start a fresh instance at full volume, unless it would be inaudible.
    fn start(&mut self, b: &mut impl Backend, id: SoundId) {
        let vol = self.full(id);
        if vol > SILENT && b.play(id, matches!(id, SoundId::Track(_)), vol) {
            let i = slot(id);
            self.chans[i] = Chan::On {
                ends: self.now + self.duration(id),
            };
            self.vols[i] = vol;
        }
    }

    /// Play one-shot `s` now.
    fn trigger(&mut self, b: &mut impl Backend, s: Sfx) {
        let id = SoundId::Sfx(s);
        let i = slot(id);
        let vol = self.full(id);
        if vol <= SILENT {
            return;
        }
        match self.chans[i] {
            Chan::Idle => self.start(b, id),
            Chan::On { ends } if !is_exclusive(s) => {
                // Overlap another instance (all instances share one volume).
                if b.play(id, false, vol) {
                    self.chans[i] = Chan::On {
                        ends: ends.max(self.now + self.duration(id)),
                    };
                }
            }
            Chan::On { .. } => {
                // Restart: fade the running instance out, then play anew.
                self.fade_out(id, STOP_FADE);
                self.defer_replay(i);
            }
            // Mid-fade: play it as soon as the old instance is silent.
            Chan::Ramping { .. } => self.defer_replay(i),
        }
    }

    fn defer_replay(&mut self, i: usize) {
        if let Chan::Ramping { replay, .. } = &mut self.chans[i] {
            *replay = true;
        }
    }

    /// Make sure loop `t` plays: start it, or bring it back if it is fading.
    fn want_loop(&mut self, b: &mut impl Backend, t: Track) {
        let id = SoundId::Track(t);
        match self.chans[slot(id)] {
            Chan::Idle => self.start(b, id),
            Chan::On { .. } => {}
            Chan::Ramping { .. } => self.fade_in(id),
        }
    }

    /// Ramp a sound that is fading back up to its full volume (if audible).
    fn fade_in(&mut self, id: SoundId) {
        let i = slot(id);
        let full = self.full(id);
        if let Chan::Ramping { ramp, ends, .. } = self.chans[i] {
            if full > SILENT {
                let ramp = Ramp::new(ramp.at(self.now), full, self.now, CROSS_FADE);
                self.chans[i] = Chan::Ramping {
                    ramp,
                    ends,
                    replay: false,
                };
            }
        }
    }

    /// Fade out loop `t` over `secs` unless the ambience or the music still
    /// wants it.
    fn release_loop(&mut self, t: Track, secs: f64) {
        if !self.wants(t) {
            self.fade_out(SoundId::Track(t), secs);
        }
    }

    /// Fade a playing sound out over `secs` (or sooner, if it is already
    /// fading faster) and stop it; cancels any pending replay.
    fn fade_out(&mut self, id: SoundId, secs: f64) {
        let i = slot(id);
        let now = self.now;
        self.chans[i] = match self.chans[i] {
            Chan::Idle => Chan::Idle,
            Chan::On { ends } => Chan::Ramping {
                ramp: Ramp::new(self.vols[i], 0.0, now, secs),
                ends,
                replay: false,
            },
            Chan::Ramping { ramp, ends, .. } => {
                let ramp = if ramp.is_out() && ramp.end() <= now + secs {
                    ramp
                } else {
                    Ramp::new(ramp.at(now), 0.0, now, secs)
                };
                Chan::Ramping {
                    ramp,
                    ends,
                    replay: false,
                }
            }
        };
    }
}

/// What a game event asks of the audio system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cue {
    Nothing,
    Play(Sfx),
    /// The next chirp of the glimmer run.
    Mote,
    /// Stop the ambience loop, then optionally play an effect.
    StopAmbience(Option<Sfx>),
}

/// Map a gameplay event to its audio cue.
fn cue_for(ev: &GameEvent) -> Cue {
    match ev {
        GameEvent::ReadyStarted { first: true } => Cue::Play(Sfx::Intro),
        GameEvent::MoteEaten { .. } => Cue::Mote,
        GameEvent::FlareEaten { .. } => Cue::Play(Sfx::Flare),
        GameEvent::ShadeEaten { .. } => Cue::Play(Sfx::ShadeEaten),
        GameEvent::FullChain { .. } => Cue::Play(Sfx::FullChain),
        GameEvent::BonusSpawned { .. } => Cue::Play(Sfx::BonusSpawn),
        GameEvent::BonusEaten { .. } => Cue::Play(Sfx::BonusEaten),
        GameEvent::PlayerCaught { .. } => Cue::StopAmbience(None),
        GameEvent::DeathAnimStarted { .. } => Cue::Play(Sfx::Death),
        GameEvent::ExtraLife => Cue::Play(Sfx::ExtraLife),
        GameEvent::LevelCleared { .. } => Cue::StopAmbience(Some(Sfx::LevelClear)),
        GameEvent::GameOver { .. } => Cue::Play(Sfx::GameOver),
        // Everything else is conveyed by the ambience loop or is silent.
        _ => Cue::Nothing,
    }
}

/// The ambience loop for a loop hint.
fn ambience_track(hint: LoopHint) -> Option<Track> {
    match hint {
        LoopHint::Silent => None,
        LoopHint::Hum(0) => Some(Track::Hum0),
        LoopHint::Hum(1) => Some(Track::Hum1),
        LoopHint::Hum(2) => Some(Track::Hum2),
        LoopHint::Hum(_) => Some(Track::Hum3),
        LoopHint::Fright => Some(Track::Fright),
        LoopHint::Eyes => Some(Track::Eyes),
    }
}

/// Long effects that restart rather than stack when re-triggered.
fn is_exclusive(s: Sfx) -> bool {
    s.is_music() || matches!(s, Sfx::Death | Sfx::Flare)
}

/// Sounds of the game itself, which pausing silences: everything except the
/// UI blips and the high-score fanfare (played on the name-entry screen).
fn is_gameplay(s: Sfx) -> bool {
    !s.is_ui() && s != Sfx::HighScore
}

/// Fixed mix gain of each effect (on top of the user's volumes).
fn sfx_gain(s: Sfx) -> f32 {
    match s {
        Sfx::Mote0 | Sfx::Mote1 | Sfx::Mote2 | Sfx::Mote3 | Sfx::Mote4 => 0.75,
        Sfx::Flare | Sfx::ShadeEaten | Sfx::BonusEaten => 0.85,
        Sfx::FullChain | Sfx::ExtraLife => 0.9,
        Sfx::Death | Sfx::BonusSpawn => 0.8,
        Sfx::Intro | Sfx::LevelClear | Sfx::GameOver | Sfx::HighScore => 0.95,
        Sfx::MenuMove | Sfx::Key => 0.7,
        Sfx::MenuSelect | Sfx::MenuBack | Sfx::PauseOn | Sfx::PauseOff => 0.8,
    }
}

/// Fixed mix gain of each loop: ambience sits well under the effects.
fn track_gain(t: Track) -> f32 {
    match t {
        Track::Hum0 | Track::Hum1 | Track::Hum2 | Track::Hum3 => 0.45,
        Track::Fright => 0.5,
        Track::Eyes => 0.45,
        Track::Title | Track::Interlude => 1.0,
    }
}

/// Clamp a user volume to `[0, 1]` (NaN counts as 0).
fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Map a linear user volume (`0..=1`, i.e. settings / 10) to a mixer gain
/// that is linear in decibels: -40 dB at 0.1 up to 0 dB at 1.0, so every
/// settings step changes the level by the same audible amount (40/9, about
/// 4.4 dB). Exactly 0 at 0 (and for negative or NaN input).
fn perceptual_gain(v: f32) -> f32 {
    let v = unit(v);
    if v <= 0.0 {
        0.0
    } else if v >= 1.0 {
        1.0
    } else {
        let db = VOLUME_FLOOR_DB * (1.0 - v) / 0.9;
        10f32.powf(db / 20.0)
    }
}

/// Effective playback volume from the mixer gains.
fn mix_volume(master: f32, category: f32, gain: f32) -> f32 {
    (unit(master) * unit(category) * gain).clamp(0.0, 1.0)
}

fn sfx_index(s: Sfx) -> usize {
    s as usize
}

fn track_index(t: Track) -> usize {
    t as usize
}

/// Index of a sound in the mixer's per-sound tables: effects, then loops.
fn slot(id: SoundId) -> usize {
    match id {
        SoundId::Sfx(s) => sfx_index(s),
        SoundId::Track(t) => SFX_COUNT + track_index(t),
    }
}

/// Every sound, in [`slot`] order.
const IDS: [SoundId; SLOTS] = {
    let mut ids = [SoundId::Sfx(Sfx::ALL[0]); SLOTS];
    let mut i = 0;
    while i < SFX_COUNT {
        ids[i] = SoundId::Sfx(Sfx::ALL[i]);
        i += 1;
    }
    let mut j = 0;
    while j < TRACK_COUNT {
        ids[SFX_COUNT + j] = SoundId::Track(Track::ALL[j]);
        j += 1;
    }
    ids
};

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl SoundId {
    fn render(self) -> Vec<f32> {
        match self {
            SoundId::Sfx(s) => synth::render_sfx(s),
            SoundId::Track(t) => synth::render_track(t),
        }
    }
}

/// Every sound exactly once, the longest first so the pool finishes early.
fn jobs() -> Vec<SoundId> {
    const FIRST: [SoundId; 3] = [
        SoundId::Track(Track::Title),
        SoundId::Track(Track::Interlude),
        SoundId::Sfx(Sfx::Intro),
    ];
    let rest = Track::ALL
        .into_iter()
        .map(SoundId::Track)
        .chain(Sfx::ALL.into_iter().map(SoundId::Sfx))
        .filter(|j| !FIRST.contains(j));
    FIRST.into_iter().chain(rest).collect()
}

type Pcm = Option<Vec<f32>>;

/// Synthesise every sound in parallel. Returns `(sfx, tracks)` indexed by
/// [`sfx_index`] / [`track_index`]; `None` where synthesis failed.
fn render_all() -> (Vec<Pcm>, Vec<Pcm>) {
    let jobs = jobs();
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Pcm>> = Mutex::new(vec![None; jobs.len()]);
    let claim = || {
        let i = next.fetch_add(1, Ordering::Relaxed);
        jobs.get(i).map(|&job| (i, job))
    };
    let work = || {
        while let Some((i, job)) = claim() {
            let pcm = catch_unwind(AssertUnwindSafe(|| job.render())).ok();
            if pcm.is_none() {
                eprintln!("glimmer: audio: synthesis of {job:?} failed; it will be silent");
            }
            results.lock().unwrap_or_else(PoisonError::into_inner)[i] = pcm;
        }
    };
    let helpers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, MAX_WORKERS)
        - 1;
    std::thread::scope(|scope| {
        let work = &work;
        for _ in 0..helpers {
            let spawned = std::thread::Builder::new()
                .name("glimmer-synth".to_owned())
                .spawn_scoped(scope, work);
            if spawned.is_err() {
                break; // the calling thread below picks up the remaining jobs
            }
        }
        work();
    });
    let results = results.into_inner().unwrap_or_else(PoisonError::into_inner);
    let mut sfx = vec![None; SFX_COUNT];
    let mut tracks = vec![None; TRACK_COUNT];
    for (job, pcm) in jobs.into_iter().zip(results) {
        match job {
            SoundId::Sfx(s) => sfx[sfx_index(s)] = pcm,
            SoundId::Track(t) => tracks[track_index(t)] = pcm,
        }
    }
    (sfx, tracks)
}

/// True if `pcm` is safe to hand to the decoder.
fn is_loadable(pcm: &[f32]) -> bool {
    pcm.len() >= MIN_SAMPLES && pcm.iter().all(|x| x.is_finite())
}

/// Encode and load one synthesised sound; `None` (logged) on failure.
async fn load_pcm(name: &str, pcm: Pcm) -> Option<Sound> {
    let pcm = pcm?;
    if !is_loadable(&pcm) {
        eprintln!("glimmer: audio: {name} has unusable samples; skipping it");
        return None;
    }
    let wav = synth::encode_wav(&pcm, synth::SAMPLE_RATE);
    drop(pcm);
    match load_sound_from_bytes(&wav).await {
        Ok(sound) => Some(sound),
        Err(e) => {
            eprintln!("glimmer: audio: could not load {name}: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BonusKind, Mode};
    use crate::geom::Tile;
    use crate::shade::ShadeKind;
    use macroquad::math::Vec2;
    use std::collections::HashMap;

    /// A backend that records every mixer call and models each sound's
    /// current volume, to catch stops at an audible level.
    #[derive(Default)]
    struct Recorder {
        calls: Vec<Call>,
        /// Sounds that "failed to load".
        missing: Vec<SoundId>,
        /// `(current volume, loudest volume since it started)` per sound.
        levels: HashMap<SoundId, (f32, f32)>,
        /// Stops of a sound still above a third of its level: `(id, ratio)`.
        hard_stops: Vec<(SoundId, f32)>,
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Call {
        Play(SoundId, bool, f32),
        Stop(SoundId),
        Volume(SoundId, f32),
    }

    impl Recorder {
        fn take(&mut self) -> Vec<Call> {
            std::mem::take(&mut self.calls)
        }

        fn plays(&self) -> Vec<SoundId> {
            self.calls
                .iter()
                .filter_map(|c| match c {
                    Call::Play(id, ..) => Some(*id),
                    _ => None,
                })
                .collect()
        }
    }

    impl Backend for Recorder {
        fn play(&mut self, id: SoundId, looped: bool, volume: f32) -> bool {
            if self.missing.contains(&id) {
                return false;
            }
            self.calls.push(Call::Play(id, looped, volume));
            let level = self.levels.entry(id).or_insert((volume, volume));
            *level = (volume, level.1.max(volume));
            true
        }

        fn stop(&mut self, id: SoundId) {
            self.calls.push(Call::Stop(id));
            if let Some((now, loudest)) = self.levels.remove(&id) {
                if now > loudest / 3.0 + 1e-3 {
                    self.hard_stops.push((id, now / loudest));
                }
            }
        }

        fn set_volume(&mut self, id: SoundId, volume: f32) {
            self.calls.push(Call::Volume(id, volume));
            if let Some(level) = self.levels.get_mut(&id) {
                *level = (volume, level.1.max(volume));
            }
        }
    }

    const HUM0: SoundId = SoundId::Track(Track::Hum0);
    const HUM1: SoundId = SoundId::Track(Track::Hum1);
    const FRIGHT: SoundId = SoundId::Track(Track::Fright);
    const TITLE: SoundId = SoundId::Track(Track::Title);
    const INTERLUDE: SoundId = SoundId::Track(Track::Interlude);
    const INTRO: SoundId = SoundId::Sfx(Sfx::Intro);
    const FLARE: SoundId = SoundId::Sfx(Sfx::Flare);
    const DEATH: SoundId = SoundId::Sfx(Sfx::Death);
    const MENU_SELECT: SoundId = SoundId::Sfx(Sfx::MenuSelect);

    /// One frame at 60 fps.
    const FRAME: f64 = 1.0 / 60.0;
    /// Length of every one-shot in the tests.
    const SFX_SECS: f64 = 1.0;

    /// The mixer, a recorder and a clock, driven like the app drives `Audio`.
    struct Rig {
        mix: Mix,
        b: Recorder,
        t: f64,
    }

    impl Rig {
        fn new() -> Rig {
            Rig {
                mix: Mix::new([SFX_SECS; SFX_COUNT]),
                b: Recorder::default(),
                t: 5.0,
            }
        }

        fn play(&mut self, s: Sfx) {
            self.mix.play(&mut self.b, self.t, s);
        }

        fn event(&mut self, ev: &GameEvent) {
            self.mix.handle_event(&mut self.b, self.t, ev);
        }

        fn ambience(&mut self, hint: LoopHint) {
            self.mix.set_ambience(&mut self.b, self.t, hint);
        }

        fn music(&mut self, track: Option<Track>) {
            self.mix.set_music(&mut self.b, self.t, track);
        }

        fn volumes(&mut self, master: f32, music: f32, sfx: f32) {
            self.mix
                .set_volumes(&mut self.b, self.t, master, music, sfx);
        }

        fn stop_all(&mut self) {
            self.mix.stop_all(&mut self.b, self.t);
        }

        fn stop_effects(&mut self) {
            self.mix.stop_effects(&mut self.b, self.t);
        }

        /// Let `secs` pass frame by frame, the app re-sending the current
        /// ambience hint every frame (which is what advances fades).
        fn wait(&mut self, secs: f64) {
            let frames = (secs / FRAME).round() as usize;
            for _ in 0..frames {
                self.t += FRAME;
                let hint = self.mix.hint;
                self.ambience(hint);
            }
        }

        /// Frames until `id` is stopped (panics after `max` frames).
        fn frames_until_stop(&mut self, id: SoundId, max: usize) -> usize {
            for frame in 1..=max {
                self.wait(FRAME);
                if self.b.calls.contains(&Call::Stop(id)) {
                    return frame;
                }
            }
            panic!(
                "{id:?} was not stopped within {max} frames: {:?}",
                self.b.calls
            );
        }

        fn take(&mut self) -> Vec<Call> {
            self.b.take()
        }
    }

    fn volumes_of(calls: &[Call], id: SoundId) -> Vec<f32> {
        calls
            .iter()
            .filter_map(|c| match c {
                Call::Volume(i, v) if *i == id => Some(*v),
                _ => None,
            })
            .collect()
    }

    fn stops_of(calls: &[Call], id: SoundId) -> usize {
        calls.iter().filter(|c| **c == Call::Stop(id)).count()
    }

    fn strictly_falling(v: &[f32]) -> bool {
        v.windows(2).all(|w| w[1] < w[0])
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-5 * b.abs().max(1.0)
    }

    fn db(gain: f32) -> f32 {
        20.0 * gain.log10()
    }

    #[test]
    fn indices_follow_the_all_tables() {
        for (i, s) in Sfx::ALL.into_iter().enumerate() {
            assert_eq!(sfx_index(s), i);
        }
        for (i, t) in Track::ALL.into_iter().enumerate() {
            assert_eq!(track_index(t), i);
        }
        for (i, id) in IDS.into_iter().enumerate() {
            assert_eq!(slot(id), i, "{id:?}");
        }
    }

    #[test]
    fn events_map_to_cues() {
        let p = Vec2::ZERO;
        let cases = [
            (
                GameEvent::ReadyStarted { first: true },
                Cue::Play(Sfx::Intro),
            ),
            (GameEvent::ReadyStarted { first: false }, Cue::Nothing),
            (
                GameEvent::MoteEaten {
                    tile: Tile::new(1, 1),
                },
                Cue::Mote,
            ),
            (
                GameEvent::FlareEaten {
                    tile: Tile::new(1, 3),
                },
                Cue::Play(Sfx::Flare),
            ),
            (
                GameEvent::ShadeEaten {
                    kind: ShadeKind::Rook,
                    points: 200,
                    pos: p,
                },
                Cue::Play(Sfx::ShadeEaten),
            ),
            (
                GameEvent::FullChain {
                    points: 2000,
                    pos: p,
                },
                Cue::Play(Sfx::FullChain),
            ),
            (
                GameEvent::BonusSpawned {
                    kind: BonusKind::Shard,
                    pos: p,
                },
                Cue::Play(Sfx::BonusSpawn),
            ),
            (
                GameEvent::BonusEaten {
                    kind: BonusKind::Prism,
                    points: 300,
                    pos: p,
                },
                Cue::Play(Sfx::BonusEaten),
            ),
            (GameEvent::PlayerCaught { pos: p }, Cue::StopAmbience(None)),
            (
                GameEvent::DeathAnimStarted { pos: p },
                Cue::Play(Sfx::Death),
            ),
            (GameEvent::ExtraLife, Cue::Play(Sfx::ExtraLife)),
            (
                GameEvent::LevelCleared { level: 1 },
                Cue::StopAmbience(Some(Sfx::LevelClear)),
            ),
            (
                GameEvent::GameOver {
                    score: 10,
                    level: 1,
                },
                Cue::Play(Sfx::GameOver),
            ),
            (GameEvent::PlayStarted, Cue::Nothing),
            (GameEvent::BonusExpired, Cue::Nothing),
            (GameEvent::FrightStarted, Cue::Nothing),
            (GameEvent::FrightEnded, Cue::Nothing),
            (GameEvent::ModeChanged(Mode::Chase), Cue::Nothing),
            (GameEvent::FuryStarted { stage: 1 }, Cue::Nothing),
            (GameEvent::LevelStarted { level: 2 }, Cue::Nothing),
            (GameEvent::InterludeStarted { variant: 0 }, Cue::Nothing),
        ];
        for (ev, cue) in cases {
            assert_eq!(cue_for(&ev), cue, "{ev:?}");
        }
    }

    #[test]
    fn hints_map_to_ambience_tracks() {
        assert_eq!(ambience_track(LoopHint::Silent), None);
        assert_eq!(ambience_track(LoopHint::Hum(0)), Some(Track::Hum0));
        assert_eq!(ambience_track(LoopHint::Hum(1)), Some(Track::Hum1));
        assert_eq!(ambience_track(LoopHint::Hum(2)), Some(Track::Hum2));
        assert_eq!(ambience_track(LoopHint::Hum(3)), Some(Track::Hum3));
        assert_eq!(ambience_track(LoopHint::Hum(200)), Some(Track::Hum3));
        assert_eq!(ambience_track(LoopHint::Fright), Some(Track::Fright));
        assert_eq!(ambience_track(LoopHint::Eyes), Some(Track::Eyes));
        for hint in [LoopHint::Hum(0), LoopHint::Fright, LoopHint::Eyes] {
            assert!(ambience_track(hint).is_some_and(|t| !t.is_music()));
        }
    }

    #[test]
    fn volumes_are_clamped_and_zero_is_silent() {
        assert_eq!(unit(f32::NAN), 0.0);
        assert_eq!(unit(-1.0), 0.0);
        assert_eq!(unit(3.0), 1.0);
        assert_eq!(mix_volume(1.0, 1.0, 0.8), 0.8);
        assert_eq!(mix_volume(0.5, 0.5, 1.0), 0.25);
        assert!(mix_volume(0.0, 1.0, 1.0) <= SILENT);
        assert!(mix_volume(1.0, 0.0, 1.0) <= SILENT);
        assert!(mix_volume(f32::INFINITY, 1.0, 1.0) <= SILENT);
    }

    #[test]
    fn volume_curve_is_linear_in_decibels() {
        assert_eq!(perceptual_gain(0.0), 0.0);
        assert_eq!(perceptual_gain(-0.5), 0.0);
        assert_eq!(perceptual_gain(f32::NAN), 0.0);
        assert_eq!(perceptual_gain(1.0), 1.0);
        assert_eq!(perceptual_gain(7.0), 1.0);
        assert!(
            (db(perceptual_gain(0.1)) + 40.0).abs() < 0.01,
            "-40 dB at 0.1"
        );
        assert!(
            (db(perceptual_gain(0.5)) + 22.22).abs() < 0.01,
            "-22.2 dB at 0.5"
        );
        // Every settings step is the same number of decibels.
        let gains: Vec<f32> = (0..=10).map(|k| perceptual_gain(k as f32 / 10.0)).collect();
        for k in 1..10 {
            let step = db(gains[k + 1]) - db(gains[k]);
            assert!((step - 40.0 / 9.0).abs() < 0.01, "step {k}: {step} dB");
        }
        assert!(
            gains.windows(2).all(|w| w[1] > w[0]),
            "monotonic: {gains:?}"
        );
    }

    #[test]
    fn set_volumes_applies_the_perceptual_curve() {
        let mut r = Rig::new();
        r.volumes(0.5, 1.0, 0.5);
        r.take();
        r.play(Sfx::Mote0);
        let half = perceptual_gain(0.5);
        let calls = r.take();
        assert!(
            matches!(calls[..], [Call::Play(SoundId::Sfx(Sfx::Mote0), false, v)]
            if approx(v, 0.75 * half * half)),
            "{calls:?}"
        );
        r.play(Sfx::Intro);
        assert!(matches!(r.take()[..], [Call::Play(INTRO, false, v)] if approx(v, 0.95 * half)));
        // Setting 1 of 10 on both master and effects is -80 dB: inaudible.
        r.volumes(0.1, 1.0, 0.1);
        r.play(Sfx::Flare);
        assert!(r.b.plays().is_empty());
    }

    #[test]
    fn mix_gains_leave_headroom() {
        for s in Sfx::ALL {
            assert!((0.1..=1.0).contains(&sfx_gain(s)), "{s:?}");
        }
        for t in Track::ALL {
            assert!((0.1..=1.0).contains(&track_gain(t)), "{t:?}");
        }
        assert!(track_gain(Track::Hum0) < sfx_gain(Sfx::Mote0));
    }

    #[test]
    fn effects_play_once_at_their_mix_volume() {
        let mut r = Rig::new();
        r.play(Sfx::MenuMove);
        assert_eq!(
            r.take(),
            vec![Call::Play(SoundId::Sfx(Sfx::MenuMove), false, 0.7)]
        );
        r.play(Sfx::MenuMove);
        assert_eq!(
            r.take(),
            vec![Call::Play(SoundId::Sfx(Sfx::MenuMove), false, 0.7)],
            "stacks"
        );
    }

    #[test]
    fn zero_volume_skips_playback() {
        let mut r = Rig::new();
        r.volumes(1.0, 1.0, 0.0);
        r.take();
        r.play(Sfx::Mote0);
        assert!(r.take().is_empty(), "effects muted");
        r.play(Sfx::Intro);
        assert_eq!(r.b.plays(), vec![INTRO], "jingles follow music volume");
        r.stop_all();
        r.wait(0.2);
        r.volumes(0.0, 1.0, 1.0);
        r.take();
        r.play(Sfx::Intro);
        r.play(Sfx::Flare);
        r.ambience(LoopHint::Hum(0));
        r.music(Some(Track::Title));
        assert!(r.b.plays().is_empty(), "master muted");
    }

    #[test]
    fn exclusive_effects_restart_after_a_micro_fade() {
        let mut r = Rig::new();
        r.play(Sfx::Death);
        assert_eq!(r.take(), vec![Call::Play(DEATH, false, 0.8)]);
        r.wait(0.2);
        assert!(r.take().is_empty());
        // Re-triggered while playing: no cut, no overlap; it fades, then restarts.
        r.play(Sfx::Death);
        assert!(r.take().is_empty());
        let frames = r.frames_until_stop(DEATH, 10);
        assert!((3..=4).contains(&frames), "{frames} frames");
        let calls = r.take();
        let n = calls.len();
        assert_eq!(
            calls[n - 2..],
            [Call::Stop(DEATH), Call::Play(DEATH, false, 0.8)]
        );
        let fade = volumes_of(&calls, DEATH);
        assert!(!fade.is_empty() && strictly_falling(&fade), "{fade:?}");
        // Once it has finished, it simply plays again.
        r.wait(SFX_SECS + END_MARGIN + 0.1);
        r.take();
        r.play(Sfx::Death);
        assert_eq!(r.take(), vec![Call::Play(DEATH, false, 0.8)]);
        assert!(!is_exclusive(Sfx::Mote0) && !is_exclusive(Sfx::MenuMove));
        assert!(r.b.hard_stops.is_empty(), "{:?}", r.b.hard_stops);
    }

    #[test]
    fn glimmer_run_steps_up_wraps_and_resets() {
        let mut run = GlimmerRun::default();
        let notes: Vec<Sfx> = (0..7)
            .map(|k| run.next(1.0 + 0.12 * f64::from(k)))
            .collect();
        let m = Sfx::MOTES;
        assert_eq!(notes, vec![m[0], m[1], m[2], m[3], m[4], m[0], m[1]]);
        // A pause restarts the run from the bottom.
        assert_eq!(run.next(1.72 + RUN_GAP + 0.01), m[0]);
        // Exactly at the gap it still continues.
        let mut run = GlimmerRun::default();
        assert_eq!(run.next(0.0), m[0]);
        assert_eq!(run.next(0.25), m[1]);
        assert_eq!(run.next(0.25 + RUN_GAP), m[2]);
        assert_eq!(run.next(0.25 + 2.0 * RUN_GAP + 0.05), m[0]);
    }

    #[test]
    fn motes_play_the_glimmer_run() {
        let mut r = Rig::new();
        let ev = GameEvent::MoteEaten {
            tile: Tile::new(2, 1),
        };
        let eat = |r: &mut Rig| {
            r.event(&ev);
            r.wait(0.13);
        };
        for _ in 0..6 {
            eat(&mut r);
        }
        r.wait(0.5); // Glim stops eating for a while
        eat(&mut r);
        eat(&mut r);
        let m = Sfx::MOTES.map(SoundId::Sfx);
        assert_eq!(
            r.b.plays(),
            vec![m[0], m[1], m[2], m[3], m[4], m[0], m[0], m[1]]
        );
        // A new game starts the run from the bottom, even right away.
        r.stop_all();
        r.wait(STOP_FADE + FRAME);
        r.take();
        r.event(&ev);
        assert_eq!(r.b.plays(), vec![m[0]]);
    }

    #[test]
    fn ambience_only_restarts_when_the_hint_changes() {
        let mut r = Rig::new();
        r.ambience(LoopHint::Hum(0));
        assert_eq!(r.take(), vec![Call::Play(HUM0, true, 0.45)]);
        r.wait(1.0);
        assert!(r.take().is_empty());
    }

    #[test]
    fn going_silent_is_a_micro_fade_not_a_cut() {
        let mut r = Rig::new();
        r.ambience(LoopHint::Hum(0));
        r.wait(0.5);
        r.take();
        r.ambience(LoopHint::Silent);
        assert!(r.take().is_empty(), "nothing is cut on the spot");
        let frames = r.frames_until_stop(HUM0, 10);
        assert!((3..=4).contains(&frames), "{frames} frames");
        let fade = volumes_of(&r.take(), HUM0);
        assert!(fade.len() >= 2 && strictly_falling(&fade), "{fade:?}");
        assert!(fade.iter().all(|&v| v < 0.45));
        assert!(r.b.hard_stops.is_empty(), "{:?}", r.b.hard_stops);
        r.wait(1.0);
        assert!(r.take().is_empty(), "nothing left to do");
    }

    #[test]
    fn fades_follow_the_clock_not_the_call_count() {
        let mut r = Rig::new();
        r.ambience(LoopHint::Hum(0));
        r.ambience(LoopHint::Silent);
        r.take();
        for _ in 0..100 {
            r.ambience(LoopHint::Silent);
        }
        assert!(r.take().is_empty(), "no time has passed");
        // Two calls in one frame send one volume change between them.
        r.t += FRAME;
        r.ambience(LoopHint::Silent);
        r.t += 1e-4;
        r.ambience(LoopHint::Silent);
        assert_eq!(volumes_of(&r.take(), HUM0).len(), 1);
        // A long stall finishes the fade at once.
        r.t += 1.0;
        r.ambience(LoopHint::Silent);
        assert_eq!(r.take(), vec![Call::Stop(HUM0)]);
        // The clock never runs backwards.
        r.ambience(LoopHint::Hum(0));
        r.t -= 10.0;
        r.ambience(LoopHint::Silent);
        r.take();
        r.t += 10.0 + FRAME;
        r.ambience(LoopHint::Silent);
        assert!(!volumes_of(&r.take(), HUM0).is_empty());
    }

    #[test]
    fn replaced_ambience_cross_fades() {
        let mut r = Rig::new();
        r.ambience(LoopHint::Hum(0));
        r.take();
        r.ambience(LoopHint::Hum(1));
        assert_eq!(r.take(), vec![Call::Play(HUM1, true, 0.45)]);
        let frames = r.frames_until_stop(HUM0, 20);
        let expected = (CROSS_FADE / FRAME).round() as usize;
        assert!(
            (expected..=expected + 1).contains(&frames),
            "{frames} frames"
        );
        let fade = volumes_of(&r.take(), HUM0);
        assert!(
            fade.len() >= 4 && strictly_falling(&fade),
            "fade is monotonic: {fade:?}"
        );
        r.wait(0.5);
        assert!(r.take().is_empty(), "nothing left to do");
    }

    #[test]
    fn a_fading_loop_that_is_wanted_again_fades_back_in() {
        let mut r = Rig::new();
        r.ambience(LoopHint::Hum(0));
        r.ambience(LoopHint::Fright);
        r.wait(2.0 * FRAME);
        r.take();
        r.ambience(LoopHint::Hum(0));
        r.wait(0.5);
        let calls = r.take();
        assert!(
            !calls.iter().any(|c| matches!(c, Call::Play(HUM0, ..))),
            "no second instance"
        );
        assert!(!calls.contains(&Call::Stop(HUM0)));
        let back = volumes_of(&calls, HUM0);
        assert!(
            back.windows(2).all(|w| w[1] > w[0]),
            "fades back in: {back:?}"
        );
        assert_eq!(back.last(), Some(&0.45), "back to full volume");
        assert_eq!(stops_of(&calls, FRIGHT), 1);
        assert!(r.b.hard_stops.is_empty(), "{:?}", r.b.hard_stops);
    }

    #[test]
    fn caught_and_level_clear_fade_the_ambience() {
        let mut r = Rig::new();
        r.ambience(LoopHint::Hum(0));
        r.wait(0.3);
        r.take();
        r.event(&GameEvent::PlayerCaught { pos: Vec2::ZERO });
        assert!(r.take().is_empty(), "no hard stop");
        r.wait(0.2);
        let calls = r.take();
        assert_eq!(stops_of(&calls, HUM0), 1);
        assert!(strictly_falling(&volumes_of(&calls, HUM0)));
        r.ambience(LoopHint::Hum(0));
        assert!(r.take().is_empty(), "stays silent until the hint changes");
        r.ambience(LoopHint::Silent);
        r.ambience(LoopHint::Hum(0));
        assert_eq!(r.take(), vec![Call::Play(HUM0, true, 0.45)]);
        r.wait(0.3);
        r.event(&GameEvent::LevelCleared { level: 1 });
        let level_clear = SoundId::Sfx(Sfx::LevelClear);
        assert_eq!(
            r.take(),
            vec![Call::Play(level_clear, false, 0.95)],
            "fanfare at once"
        );
        r.wait(0.2);
        let calls = r.take();
        assert_eq!(stops_of(&calls, HUM0), 1);
        assert_eq!(stops_of(&calls, level_clear), 0);
        assert!(r.b.hard_stops.is_empty(), "{:?}", r.b.hard_stops);
    }

    #[test]
    fn music_starts_once_and_fades_out_over_half_a_second() {
        let mut r = Rig::new();
        r.music(Some(Track::Title));
        r.music(Some(Track::Title));
        assert_eq!(r.take(), vec![Call::Play(TITLE, true, 1.0)]);
        r.wait(1.0);
        r.music(Some(Track::Interlude));
        assert_eq!(
            r.take(),
            vec![Call::Play(INTERLUDE, true, 1.0)],
            "the new track starts"
        );
        let frames = r.frames_until_stop(TITLE, 60);
        let expected = (MUSIC_FADE / FRAME).round() as usize;
        assert!(
            (expected..=expected + 1).contains(&frames),
            "{frames} frames"
        );
        let fade = volumes_of(&r.take(), TITLE);
        assert!(fade.len() >= 20 && strictly_falling(&fade), "{fade:?}");
        assert!(
            fade[fade.len() / 2] > 0.3,
            "half way it is still clearly audible"
        );
        r.wait(2.0);
        r.music(None);
        assert!(r.take().is_empty());
        let frames = r.frames_until_stop(INTERLUDE, 60);
        assert!(
            (expected..=expected + 1).contains(&frames),
            "{frames} frames"
        );
        assert!(r.b.hard_stops.is_empty(), "{:?}", r.b.hard_stops);
    }

    #[test]
    fn interlude_fade_starts_after_the_full_tune() {
        // The scene (8.2 s or more) hears the whole 8.0 s loop; the music is
        // only released when the scene ends, then fades for 0.5 s.
        let mut r = Rig::new();
        r.music(Some(Track::Interlude));
        r.wait(8.2);
        let calls = r.take();
        assert!(
            volumes_of(&calls, INTERLUDE).is_empty(),
            "untouched while the scene runs"
        );
        r.music(None);
        r.wait(0.4);
        assert_eq!(stops_of(&r.take(), INTERLUDE), 0, "still fading");
        r.wait(0.2);
        assert_eq!(stops_of(&r.take(), INTERLUDE), 1);
    }

    #[test]
    fn muted_music_starts_when_unmuted_and_fades_when_muted() {
        let mut r = Rig::new();
        r.volumes(1.0, 0.0, 1.0);
        r.music(Some(Track::Title));
        assert!(r.b.plays().is_empty());
        r.volumes(1.0, 0.5, 1.0);
        assert_eq!(r.b.plays(), vec![TITLE]);
        assert!(r
            .take()
            .contains(&Call::Play(TITLE, true, perceptual_gain(0.5))));
        r.volumes(1.0, 0.3, 1.0);
        assert_eq!(r.take(), vec![Call::Volume(TITLE, perceptual_gain(0.3))]);
        r.volumes(1.0, 0.0, 1.0);
        assert!(r.take().is_empty(), "muting fades instead of cutting");
        let frames = r.frames_until_stop(TITLE, 10);
        assert!((3..=4).contains(&frames), "{frames} frames");
        // Unchanged volumes are a no-op.
        r.take();
        r.volumes(1.0, 0.0, 1.0);
        assert!(r.take().is_empty());
        // Unmuting restarts the wanted music.
        r.volumes(1.0, 1.0, 1.0);
        assert_eq!(r.take(), vec![Call::Play(TITLE, true, 1.0)]);
        assert!(r.b.hard_stops.is_empty(), "{:?}", r.b.hard_stops);
    }

    #[test]
    fn mute_fades_effects_and_ambience_and_unmute_restarts_loops() {
        let mut r = Rig::new();
        r.ambience(LoopHint::Hum(0));
        r.play(Sfx::Flare);
        r.wait(0.1);
        r.take();
        r.volumes(0.0, 1.0, 1.0);
        assert!(r.take().is_empty());
        r.wait(0.1);
        let calls = r.take();
        assert_eq!(stops_of(&calls, HUM0), 1);
        assert_eq!(stops_of(&calls, FLARE), 1);
        r.volumes(1.0, 1.0, 1.0);
        assert_eq!(
            r.take(),
            vec![Call::Play(HUM0, true, 0.45)],
            "only the loop comes back"
        );
        assert!(r.b.hard_stops.is_empty(), "{:?}", r.b.hard_stops);
    }

    #[test]
    fn stop_all_spares_ui_blips_and_fades_the_rest() {
        let mut r = Rig::new();
        r.music(Some(Track::Title));
        r.ambience(LoopHint::Fright);
        r.play(Sfx::Intro);
        r.play(Sfx::Flare);
        r.wait(0.2);
        r.take();
        // PLAY on the title: the confirm blip, then the new game's stop_all.
        r.play(Sfx::MenuSelect);
        r.stop_all();
        assert_eq!(
            r.take(),
            vec![Call::Play(MENU_SELECT, false, 0.8)],
            "nothing cut at once"
        );
        r.wait(0.2);
        let calls = r.take();
        for id in [TITLE, FRIGHT, INTRO, FLARE] {
            assert_eq!(stops_of(&calls, id), 1, "{id:?}: {calls:?}");
            assert!(strictly_falling(&volumes_of(&calls, id)), "{id:?} fades");
        }
        assert_eq!(
            stops_of(&calls, MENU_SELECT),
            0,
            "the confirm blip is heard"
        );
        // Sounds that were not playing are left alone.
        assert_eq!(
            calls.iter().filter(|c| matches!(c, Call::Stop(_))).count(),
            4,
            "{calls:?}"
        );
        assert!(r.b.hard_stops.is_empty(), "{:?}", r.b.hard_stops);
        // Everything can start again.
        r.ambience(LoopHint::Fright);
        r.music(Some(Track::Title));
        assert_eq!(r.b.plays(), vec![FRIGHT, TITLE]);
    }

    #[test]
    fn stop_all_spares_every_ui_blip_but_not_jingles() {
        let mut r = Rig::new();
        for s in Sfx::ALL {
            r.play(s);
        }
        r.stop_all();
        r.wait(0.2);
        let calls = r.take();
        for s in Sfx::ALL {
            let stopped = stops_of(&calls, SoundId::Sfx(s)) == 1;
            assert_eq!(stopped, !s.is_ui(), "{s:?}");
        }
    }

    #[test]
    fn a_new_game_jingle_waits_for_the_old_one_to_fade() {
        let mut r = Rig::new();
        // Pause > RESTART during the first GET SET: the intro is still playing.
        r.play(Sfx::Intro);
        r.wait(0.5);
        r.take();
        r.stop_all();
        r.event(&GameEvent::ReadyStarted { first: true });
        assert!(r.take().is_empty(), "no cut and no overlap");
        r.wait(0.1);
        let calls = r.take();
        let stop = calls
            .iter()
            .position(|c| *c == Call::Stop(INTRO))
            .expect("old intro stops");
        assert_eq!(
            calls[stop + 1..],
            [Call::Play(INTRO, false, 0.95)],
            "then the new one plays"
        );
        // With nothing playing, the jingle starts at once after stop_all.
        r.wait(SFX_SECS + 0.5);
        r.take();
        r.stop_all();
        r.event(&GameEvent::ReadyStarted { first: true });
        assert_eq!(r.take(), vec![Call::Play(INTRO, false, 0.95)]);
        assert!(r.b.hard_stops.is_empty(), "{:?}", r.b.hard_stops);
    }

    #[test]
    fn stop_effects_silences_gameplay_but_not_ui_or_music() {
        let mut r = Rig::new();
        r.music(Some(Track::Title));
        r.ambience(LoopHint::Hum(0));
        r.event(&GameEvent::ReadyStarted { first: true });
        r.event(&GameEvent::FlareEaten {
            tile: Tile::new(1, 3),
        });
        r.event(&GameEvent::MoteEaten {
            tile: Tile::new(2, 3),
        });
        r.play(Sfx::HighScore);
        r.play(Sfx::MenuMove);
        r.wait(0.2);
        r.take();
        // Opening the pause menu.
        r.play(Sfx::PauseOn);
        r.stop_effects();
        r.ambience(LoopHint::Silent);
        assert_eq!(
            r.take(),
            vec![Call::Play(SoundId::Sfx(Sfx::PauseOn), false, 0.8)]
        );
        r.wait(0.2);
        let calls = r.take();
        let mote = SoundId::Sfx(Sfx::Mote0);
        for id in [HUM0, INTRO, FLARE, mote] {
            assert_eq!(stops_of(&calls, id), 1, "{id:?} stops");
            assert!(strictly_falling(&volumes_of(&calls, id)), "{id:?} fades");
        }
        for s in [Sfx::PauseOn, Sfx::MenuMove, Sfx::HighScore] {
            assert_eq!(stops_of(&calls, SoundId::Sfx(s)), 0, "{s:?} keeps playing");
        }
        assert_eq!(stops_of(&calls, TITLE), 0, "music is not gameplay");
        assert!(r.b.hard_stops.is_empty(), "{:?}", r.b.hard_stops);
        // The pause menu stays quiet; resuming brings the ambience back.
        r.wait(2.0);
        assert!(r.take().is_empty());
        r.play(Sfx::PauseOff);
        r.ambience(LoopHint::Hum(0));
        assert_eq!(r.b.plays(), vec![SoundId::Sfx(Sfx::PauseOff), HUM0]);
    }

    #[test]
    fn stop_effects_also_ends_a_cross_fade_quickly() {
        let mut r = Rig::new();
        r.ambience(LoopHint::Hum(0));
        r.ambience(LoopHint::Fright);
        r.take();
        r.stop_effects();
        let frames = r.frames_until_stop(HUM0, 10);
        assert!(frames <= 4, "{frames}");
        r.wait(0.1);
        let calls = r.take();
        assert_eq!(stops_of(&calls, FRIGHT), 1);
        assert!(r.b.hard_stops.is_empty(), "{:?}", r.b.hard_stops);
    }

    #[test]
    fn finished_one_shots_are_not_stopped_again() {
        let mut r = Rig::new();
        r.play(Sfx::Flare);
        r.wait(SFX_SECS + END_MARGIN + 0.05);
        r.take();
        r.stop_all();
        r.wait(0.2);
        assert!(r.take().is_empty());
    }

    /// Plays a long scripted session (menus, a game with deaths, a level
    /// clear, pauses, an interlude, mute, quitting) and checks that the mixer
    /// never stopped a sound while it was louder than a third of its level.
    #[test]
    fn nothing_is_ever_cut_at_full_level() {
        let mut r = Rig::new();
        let p = Vec2::ZERO;
        let mote = GameEvent::MoteEaten {
            tile: Tile::new(1, 1),
        };
        r.music(Some(Track::Title));
        r.wait(3.0);
        for round in 0..3 {
            r.play(Sfx::MenuSelect);
            r.stop_all();
            r.event(&GameEvent::ReadyStarted { first: true });
            r.wait(0.5 + 0.3 * f64::from(round));
            // Pause during GET SET, then resume.
            r.play(Sfx::PauseOn);
            r.stop_effects();
            r.ambience(LoopHint::Silent);
            r.wait(1.0);
            r.play(Sfx::PauseOff);
            r.wait(1.0);
            r.ambience(LoopHint::Hum(0));
            for k in 0..30 {
                r.event(&mote);
                r.wait(0.12 + 0.05 * f64::from(k % 3));
            }
            r.event(&GameEvent::FlareEaten {
                tile: Tile::new(1, 3),
            });
            r.ambience(LoopHint::Fright);
            r.wait(0.4);
            r.event(&GameEvent::ShadeEaten {
                kind: ShadeKind::Vex,
                points: 200,
                pos: p,
            });
            r.ambience(LoopHint::Eyes);
            r.wait(0.3);
            r.event(&GameEvent::FlareEaten {
                tile: Tile::new(3, 3),
            });
            r.ambience(LoopHint::Fright);
            r.wait(0.5);
            r.ambience(LoopHint::Hum(1));
            r.wait(0.7);
            r.event(&GameEvent::PlayerCaught { pos: p });
            r.wait(1.0);
            r.ambience(LoopHint::Silent);
            r.event(&GameEvent::DeathAnimStarted { pos: p });
            r.wait(0.4);
            r.volumes(0.0, 1.0, 1.0); // mute mid-death
            r.wait(0.3);
            r.volumes(0.8, 0.7, 0.8);
            r.wait(1.0);
            r.ambience(LoopHint::Hum(2));
            r.wait(2.0);
            r.event(&GameEvent::LevelCleared { level: 1 });
            r.ambience(LoopHint::Silent);
            r.wait(1.0);
            r.music(Some(Track::Interlude));
            r.wait(8.4);
            r.music(None);
            r.wait(0.2);
            r.ambience(LoopHint::Hum(3));
            r.wait(1.5);
            // Pause > QUIT TO TITLE.
            r.play(Sfx::PauseOn);
            r.stop_effects();
            r.ambience(LoopHint::Silent);
            r.wait(0.5);
            r.play(Sfx::MenuBack);
            r.stop_all();
            r.music(Some(Track::Title));
            r.wait(2.0);
        }
        assert!(r.b.hard_stops.is_empty(), "{:?}", r.b.hard_stops);
        assert!(
            r.b.calls
                .iter()
                .filter(|c| matches!(c, Call::Stop(_)))
                .count()
                > 20
        );
    }

    #[test]
    fn missing_sounds_are_harmless() {
        let mut r = Rig::new();
        r.b.missing.push(HUM0);
        r.ambience(LoopHint::Hum(0));
        r.volumes(0.5, 0.5, 0.5);
        r.ambience(LoopHint::Silent);
        r.wait(0.5);
        r.stop_all();
        r.wait(0.5);
        let touched = |c: &Call| matches!(c, Call::Stop(HUM0) | Call::Volume(HUM0, _));
        assert!(!r.b.calls.iter().any(touched));
    }

    #[test]
    fn every_sound_is_one_job() {
        let jobs = jobs();
        assert_eq!(jobs.len(), SFX_COUNT + TRACK_COUNT);
        assert_eq!(jobs[0], TITLE);
        for s in Sfx::ALL {
            assert_eq!(jobs.iter().filter(|&&j| j == SoundId::Sfx(s)).count(), 1);
        }
        for t in Track::ALL {
            assert_eq!(jobs.iter().filter(|&&j| j == SoundId::Track(t)).count(), 1);
        }
    }

    #[test]
    fn parallel_render_produces_every_sound() {
        let (sfx, tracks) = render_all();
        assert_eq!(sfx.len(), SFX_COUNT);
        assert_eq!(tracks.len(), TRACK_COUNT);
        for (s, pcm) in Sfx::ALL.into_iter().zip(&sfx) {
            let pcm = pcm.as_deref().unwrap_or_default();
            assert!(is_loadable(pcm), "{s:?}");
            assert_eq!(
                pcm,
                synth::render_sfx(s).as_slice(),
                "{s:?} differs from a serial render"
            );
        }
        for (t, pcm) in Track::ALL.into_iter().zip(&tracks) {
            assert!(is_loadable(pcm.as_deref().unwrap_or_default()), "{t:?}");
        }
    }

    #[test]
    fn loadability_check() {
        assert!(!is_loadable(&[]));
        assert!(!is_loadable(&[0.0; 100]));
        assert!(is_loadable(&[0.0; 1000]));
        let mut bad = vec![0.0; 1000];
        bad[10] = f32::NAN;
        assert!(!is_loadable(&bad));
    }
}
