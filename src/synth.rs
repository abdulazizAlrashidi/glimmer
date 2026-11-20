//! Procedural sound synthesis (pure, no audio device needed).
//!
//! Every sound and music loop in Glimmer is synthesised here at startup
//! from oscillators, envelopes, noise and simple filters, then encoded as a
//! 16-bit mono WAV in memory. All melodies are original compositions.
//!
//! The module is layered bottom-up:
//!
//! 1. DSP toolkit: a fast polynomial sine, triangle, PolyBLEP saw, soft
//!    square, FM, deterministic xorshift noise, ADSR and exponential
//!    envelopes, one-pole and state-variable filters, a short modulated
//!    feedback-delay "shimmer" reverb and a soft clipper.
//! 2. Voices: tiny per-note synthesisers (FM bell, pluck, pad, bass, lead,
//!    kick, snare, hat, shaker, plus closure-driven one-off voices).
//! 3. Canvas: a mono mix with a dry bus, a reverb-send bus and a
//!    side-chained "pump" bus. On a *looping* canvas, notes that run past the
//!    end wrap around to the start and every stateful effect is pre-warmed
//!    with the loop's own tail, so the rendered loop is exactly periodic and
//!    the seam can't click.
//! 4. Sound designs: one function per [`Sfx`] and [`Track`].
//!
//! The palette is "neon fireflies at night": sine/FM bells, soft plucks,
//! filtered pads and light noise sparkles. Every output is peak-normalised
//! (one-shots <= 0.5, loops <= 0.35) because the mixer sums voices without a
//! limiter. Synthesis is fully deterministic.

use std::f32::consts::{PI, TAU};

/// Output sample rate. macroquad's mixer runs at 44.1 kHz, so no resampling.
pub const SAMPLE_RATE: u32 = 44_100;

/// Sample rate as a float, for DSP maths.
const SR: f32 = SAMPLE_RATE as f32;

/// Highest peak level a one-shot is ever normalised to.
const SFX_PEAK_LIMIT: f32 = 0.5;
/// Highest peak level a loop is ever normalised to.
const LOOP_PEAK_LIMIT: f32 = 0.35;

/// One-shot effects and jingles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sfx {
    /// The "glimmer run": five soft glassy chirps rising through a major
    /// pentatonic scale. Motes eaten in quick succession step up the run
    /// (see [`Sfx::MOTES`]); the audio module picks the step.
    Mote0,
    Mote1,
    Mote2,
    Mote3,
    Mote4,
    Flare,
    ShadeEaten,
    FullChain,
    Death,
    ExtraLife,
    BonusSpawn,
    BonusEaten,
    LevelClear,
    /// New-game jingle; must be at most `config::READY_TIME_FIRST` seconds.
    Intro,
    GameOver,
    HighScore,
    MenuMove,
    MenuSelect,
    MenuBack,
    PauseOn,
    PauseOff,
    /// Short tick for the name-entry letter changes.
    Key,
}

impl Sfx {
    pub const ALL: [Sfx; 22] = [
        Sfx::Mote0,
        Sfx::Mote1,
        Sfx::Mote2,
        Sfx::Mote3,
        Sfx::Mote4,
        Sfx::Flare,
        Sfx::ShadeEaten,
        Sfx::FullChain,
        Sfx::Death,
        Sfx::ExtraLife,
        Sfx::BonusSpawn,
        Sfx::BonusEaten,
        Sfx::LevelClear,
        Sfx::Intro,
        Sfx::GameOver,
        Sfx::HighScore,
        Sfx::MenuMove,
        Sfx::MenuSelect,
        Sfx::MenuBack,
        Sfx::PauseOn,
        Sfx::PauseOff,
        Sfx::Key,
    ];

    /// The glimmer-run chirps, lowest first.
    pub const MOTES: [Sfx; 5] = [Sfx::Mote0, Sfx::Mote1, Sfx::Mote2, Sfx::Mote3, Sfx::Mote4];

    /// Jingles that obey the music volume rather than the effects volume.
    pub const fn is_music(self) -> bool {
        matches!(
            self,
            Sfx::Intro | Sfx::LevelClear | Sfx::GameOver | Sfx::HighScore
        )
    }

    /// Menu / pause / name-entry feedback blips. They are never cut by
    /// `Audio::stop_all` or `Audio::stop_effects`, so a confirm blip played in
    /// the same frame as a screen change is still heard.
    pub const fn is_ui(self) -> bool {
        matches!(
            self,
            Sfx::MenuMove
                | Sfx::MenuSelect
                | Sfx::MenuBack
                | Sfx::PauseOn
                | Sfx::PauseOff
                | Sfx::Key
        )
    }
}

/// Looping tracks (each one's end joins its start without a click).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Track {
    /// Gameplay ambience, stage 0..=3 (rising urgency as the level empties).
    Hum0,
    Hum1,
    Hum2,
    Hum3,
    /// While shades are frightened.
    Fright,
    /// While eyes are returning to the nest.
    Eyes,
    /// Title-screen music (synthwave loop, ~16-32 s).
    Title,
    /// Interlude music: exactly 8.0 s (four bars at 120 BPM), two complete
    /// phrases ending on the tonic, so a scene (8.2 s or longer) always hears
    /// the whole tune before the music fades out.
    Interlude,
}

impl Track {
    pub const ALL: [Track; 8] = [
        Track::Hum0,
        Track::Hum1,
        Track::Hum2,
        Track::Hum3,
        Track::Fright,
        Track::Eyes,
        Track::Title,
        Track::Interlude,
    ];

    pub const fn is_music(self) -> bool {
        matches!(self, Track::Title | Track::Interlude)
    }
}

/// Render a one-shot effect as mono samples in `[-1, 1]` at `SAMPLE_RATE`.
pub fn render_sfx(sfx: Sfx) -> Vec<f32> {
    match sfx {
        Sfx::Mote0 => glimmer(0),
        Sfx::Mote1 => glimmer(1),
        Sfx::Mote2 => glimmer(2),
        Sfx::Mote3 => glimmer(3),
        Sfx::Mote4 => glimmer(4),
        Sfx::Flare => flare(),
        Sfx::ShadeEaten => shade_eaten(),
        Sfx::FullChain => full_chain(),
        Sfx::Death => death(),
        Sfx::ExtraLife => extra_life(),
        Sfx::BonusSpawn => bonus_spawn(),
        Sfx::BonusEaten => bonus_eaten(),
        Sfx::LevelClear => level_clear(),
        Sfx::Intro => intro(),
        Sfx::GameOver => game_over(),
        Sfx::HighScore => high_score(),
        Sfx::MenuMove => ui_blip(&[(0.0, 86.0)], 0.05, 0.012, 0.22),
        Sfx::MenuSelect => ui_blip(&[(0.0, 81.0), (0.045, 88.0)], 0.14, 0.03, 0.28),
        Sfx::MenuBack => ui_blip(&[(0.0, 86.0), (0.045, 79.0)], 0.12, 0.028, 0.25),
        Sfx::PauseOn => ui_blip(&[(0.0, 84.0), (0.04, 79.0), (0.08, 72.0)], 0.15, 0.03, 0.27),
        Sfx::PauseOff => ui_blip(&[(0.0, 72.0), (0.04, 79.0), (0.08, 84.0)], 0.15, 0.03, 0.27),
        Sfx::Key => ui_blip(&[(0.0, 91.0)], 0.035, 0.007, 0.18),
    }
}

/// Render a loop as mono samples in `[-1, 1]` whose end joins its start
/// without a click.
pub fn render_track(track: Track) -> Vec<f32> {
    match track {
        Track::Hum0 => hum(0),
        Track::Hum1 => hum(1),
        Track::Hum2 => hum(2),
        Track::Hum3 => hum(3),
        Track::Fright => fright(),
        Track::Eyes => eyes(),
        Track::Title => title(),
        Track::Interlude => interlude(),
    }
}

/// Encode mono `[-1, 1]` float samples as a 16-bit PCM mono RIFF/WAVE file.
///
/// Samples are clamped to `[-1, 1]`; non-finite samples become silence. The
/// output always has a valid 44-byte canonical header (little endian).
pub fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    const HEADER_LEN: usize = 44;
    const BYTES_PER_SAMPLE: usize = 2;
    // The RIFF size field is 32 bits: never emit more data than it can hold.
    let max_samples = (u32::MAX as usize - (HEADER_LEN - 8)) / BYTES_PER_SAMPLE;
    let samples = &samples[..samples.len().min(max_samples)];
    let data_len = (samples.len() * BYTES_PER_SAMPLE) as u32;

    let mut out = Vec::with_capacity(HEADER_LEN + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(data_len + (HEADER_LEN as u32 - 8)).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(
        &sample_rate
            .saturating_mul(BYTES_PER_SAMPLE as u32)
            .to_le_bytes(),
    );
    out.extend_from_slice(&(BYTES_PER_SAMPLE as u16).to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let v = if s.is_finite() {
            s.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        out.extend_from_slice(&((v * 32767.0).round() as i16).to_le_bytes());
    }
    out
}

/// Frequency in Hz of a MIDI note number (69 = A4 = 440 Hz). Fractional
/// note numbers give microtonal pitches.
pub fn midi_hz(note: f32) -> f32 {
    440.0 * ((note - 69.0) / 12.0).exp2()
}

// ===========================================================================
// DSP toolkit
// ===========================================================================

/// Whole number of samples in `secs` seconds (rounded; negative/NaN -> 0).
fn samples(secs: f32) -> usize {
    if secs.is_finite() && secs > 0.0 {
        (secs * SR).round() as usize
    } else {
        0
    }
}

/// Exponential (musically linear) interpolation between two positive values.
#[inline]
fn exp_lerp(a: f32, b: f32, t: f32) -> f32 {
    a * (b / a).powf(t)
}

/// Cubic smoothstep of `t` clamped to `[0, 1]`.
#[inline]
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Linear attack ramp: 0 at `t = 0`, reaching 1 after `len` samples.
#[inline]
fn ramp(t: usize, len: usize) -> f32 {
    if t >= len {
        1.0
    } else {
        t as f32 / len as f32
    }
}

/// `sin(2*pi*p)` for a phase `p` measured in cycles.
///
/// Folds the phase into a quarter wave and evaluates a 9th-order odd
/// polynomial (maximum error about 4e-6), which is faster than `f32::sin` and
/// bit-for-bit deterministic across platforms.
#[inline]
fn sin_cyc(p: f32) -> f32 {
    let mut x = p - p.floor();
    let mut sign = 1.0;
    if x >= 0.5 {
        x -= 0.5;
        sign = -1.0;
    }
    if x > 0.25 {
        x = 0.5 - x;
    }
    let z = x * TAU;
    let z2 = z * z;
    const C3: f32 = -1.0 / 6.0;
    const C5: f32 = 1.0 / 120.0;
    const C7: f32 = -1.0 / 5040.0;
    const C9: f32 = 1.0 / 362_880.0;
    sign * z * (1.0 + z2 * (C3 + z2 * (C5 + z2 * (C7 + z2 * C9))))
}

/// Triangle wave in phase with [`sin_cyc`] (0 at `p = 0`, +1 at `p = 0.25`).
#[inline]
fn tri_cyc(p: f32) -> f32 {
    let x = p + 0.25;
    let x = x - x.floor();
    1.0 - 4.0 * (x - 0.5).abs()
}

/// PolyBLEP residual that removes most aliasing from a unit step at phase 0.
#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// Band-limited sawtooth for phase `p` in `[0, 1)` advancing `dt` cycles/sample.
#[inline]
fn saw_blep(p: f32, dt: f32) -> f32 {
    2.0 * p - 1.0 - poly_blep(p, dt)
}

/// Rational `tanh` approximation, exact at 0 and saturating to +/-1 at |x| >= 3.
#[inline]
fn fast_tanh(x: f32) -> f32 {
    if x >= 3.0 {
        1.0
    } else if x <= -3.0 {
        -1.0
    } else {
        let x2 = x * x;
        x * (27.0 + x2) / (27.0 + 9.0 * x2)
    }
}

/// Soft square: a sine pushed through a saturator. A `drive` around 1 is
/// nearly a sine, around 3 a rounded square; harmonics fall off quickly (no
/// harsh edges).
#[derive(Clone, Copy, Debug)]
struct SoftSquare {
    drive: f32,
    norm: f32,
}

impl SoftSquare {
    fn new(drive: f32) -> SoftSquare {
        let drive = drive.max(0.1);
        SoftSquare {
            drive,
            norm: 1.0 / fast_tanh(drive),
        }
    }

    #[inline]
    fn at(&self, p: f32) -> f32 {
        fast_tanh(self.drive * sin_cyc(p)) * self.norm
    }
}

/// Soft clipper: linear below the knee, then a tanh shoulder toward +/-1.
#[inline]
fn soft_clip(x: f32) -> f32 {
    const KNEE: f32 = 0.6;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        let y = KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh();
        y.copysign(x)
    }
}

/// Phase accumulator in cycles, always kept in `[0, 1)`.
#[derive(Clone, Copy, Debug, Default)]
struct Phasor {
    p: f32,
}

impl Phasor {
    fn at(p: f32) -> Phasor {
        Phasor { p: p - p.floor() }
    }

    /// Return the current phase, then advance it by `hz`.
    #[inline]
    fn tick(&mut self, hz: f32) -> f32 {
        self.step(hz / SR)
    }

    /// Return the current phase, then advance it by `inc` cycles (>= 0).
    #[inline]
    fn step(&mut self, inc: f32) -> f32 {
        let out = self.p;
        let next = self.p + inc;
        self.p = if next >= 1.0 {
            next - next.floor()
        } else {
            next
        };
        out
    }
}

/// Deterministic xorshift32 noise source.
#[derive(Clone, Debug)]
struct Noise {
    state: u32,
}

impl Noise {
    fn new(seed: u32) -> Noise {
        let s = seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
        Noise {
            state: if s == 0 { 0x1234_5678 } else { s },
        }
    }

    #[inline]
    fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// Uniform in `[0, 1)`.
    #[inline]
    fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// Uniform in `[-1, 1)`.
    #[inline]
    fn bipolar(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    /// Uniform integer in `0..n` (`n > 0`).
    fn below(&mut self, n: usize) -> usize {
        ((self.unit() * n as f32) as usize).min(n.saturating_sub(1))
    }
}

/// Exponential decay envelope: starts at 1 and falls by `1/e` every `tau` s.
#[derive(Clone, Copy, Debug)]
struct Decay {
    level: f32,
    k: f32,
}

impl Decay {
    fn new(tau: f32) -> Decay {
        Decay {
            level: 1.0,
            k: (-1.0 / (tau.max(1e-4) * SR)).exp(),
        }
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        let l = self.level;
        self.level *= self.k;
        l
    }
}

/// Attack / decay / sustain / release envelope (times in seconds).
#[derive(Clone, Copy, Debug)]
struct Adsr {
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
}

impl Adsr {
    const fn new(attack: f32, decay: f32, sustain: f32, release: f32) -> Adsr {
        Adsr {
            attack,
            decay,
            sustain,
            release,
        }
    }

    /// Level while the key is held, `t` seconds after note-on.
    #[inline]
    fn held(&self, t: f32) -> f32 {
        if t < self.attack {
            t / self.attack
        } else {
            let td = t - self.attack;
            if td < self.decay {
                let x = 1.0 - td / self.decay;
                self.sustain + (1.0 - self.sustain) * x * x
            } else {
                self.sustain
            }
        }
    }

    /// Level at `t` seconds for a note whose key is held for `gate` seconds.
    #[inline]
    fn level(&self, t: f32, gate: f32) -> f32 {
        if t < 0.0 {
            0.0
        } else if t < gate {
            self.held(t)
        } else {
            let tr = t - gate;
            if tr >= self.release {
                0.0
            } else {
                let x = 1.0 - tr / self.release;
                self.held(gate) * x * x
            }
        }
    }

    /// Total sounding length in seconds of a note held for `gate` seconds.
    fn length(&self, gate: f32) -> f32 {
        gate.max(0.0) + self.release
    }
}

/// An [`Adsr`] evaluated at control rate (every 32 samples) and linearly
/// interpolated in between: smooth, and far cheaper than per-sample maths.
#[derive(Clone, Copy, Debug)]
struct AdsrRamp {
    env: Adsr,
    gate: f32,
    level: f32,
    step: f32,
    t: usize,
}

impl AdsrRamp {
    const BLOCK: usize = 32;

    fn new(env: Adsr, gate: f32) -> AdsrRamp {
        AdsrRamp {
            env,
            gate,
            level: 0.0,
            step: 0.0,
            t: 0,
        }
    }

    /// Total length in samples.
    fn length(&self) -> usize {
        samples(self.env.length(self.gate)).max(2)
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        if self.t.is_multiple_of(Self::BLOCK) {
            let target = self
                .env
                .level((self.t + Self::BLOCK) as f32 / SR, self.gate);
            self.step = (target - self.level) / Self::BLOCK as f32;
        }
        self.t += 1;
        let out = self.level;
        self.level += self.step;
        out
    }
}

/// One-pole low-pass filter (6 dB/octave).
#[derive(Clone, Copy, Debug)]
struct Lp1 {
    /// Input coefficient `a` and feedback coefficient `b = 1 - a`.
    a: f32,
    b: f32,
    z: f32,
}

impl Lp1 {
    fn new(hz: f32) -> Lp1 {
        let mut f = Lp1 {
            a: 1.0,
            b: 0.0,
            z: 0.0,
        };
        f.set(hz);
        f
    }

    #[inline]
    fn set(&mut self, hz: f32) {
        let hz = hz.clamp(1.0, SR * 0.49);
        self.b = (-TAU * hz / SR).exp();
        self.a = 1.0 - self.b;
    }

    /// `z = b*z + a*x`: a single multiply-add on the recurrence path.
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        self.z = self.b * self.z + self.a * x;
        self.z
    }
}

/// One-pole high-pass filter (6 dB/octave).
#[derive(Clone, Copy, Debug)]
struct Hp1 {
    lp: Lp1,
}

impl Hp1 {
    fn new(hz: f32) -> Hp1 {
        Hp1 { lp: Lp1::new(hz) }
    }

    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        x - self.lp.run(x)
    }
}

/// Topology-preserving state-variable filter (Zavalishin), 12 dB/octave.
#[derive(Clone, Copy, Debug)]
struct Svf {
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    ic1: f32,
    ic2: f32,
}

/// Simultaneous outputs of one [`Svf`] step.
#[derive(Clone, Copy, Debug)]
struct SvfOut {
    low: f32,
    /// Band-pass normalised to unity gain at the centre frequency.
    band: f32,
    high: f32,
}

impl Svf {
    fn new(hz: f32, q: f32) -> Svf {
        let mut f = Svf {
            k: 1.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            ic1: 0.0,
            ic2: 0.0,
        };
        f.set(hz, q);
        f
    }

    #[inline]
    fn set(&mut self, hz: f32, q: f32) {
        let g = (PI * hz.clamp(10.0, SR * 0.45) / SR).tan();
        self.k = 1.0 / q.max(0.05);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    #[inline]
    fn run(&mut self, x: f32) -> SvfOut {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        SvfOut {
            low: v2,
            band: self.k * v1,
            high: x - self.k * v1 - v2,
        }
    }
}

/// Feedback comb filter with a slowly modulated (chorused) delay time and a
/// low-pass in the loop: the building block of the shimmer reverb.
#[derive(Clone, Debug)]
struct ModComb {
    buf: Vec<f32>,
    write: usize,
    base: f32,
    depth: f32,
    lfo: Phasor,
    lfo_hz: f32,
    /// Current (fractional) delay in samples, refreshed at control rate.
    delay: f32,
    countdown: u32,
    feedback: f32,
    damp: f32,
    z: f32,
}

impl ModComb {
    /// The chorus LFO is evaluated once per this many samples.
    const CONTROL_BLOCK: u32 = 16;

    fn new(delay: usize, depth: f32, lfo_hz: f32, lfo_phase: f32, verb: Verb) -> ModComb {
        let len = delay + depth.ceil() as usize + 4;
        ModComb {
            buf: vec![0.0; len],
            write: 0,
            base: delay as f32,
            depth,
            lfo: Phasor::at(lfo_phase),
            lfo_hz,
            delay: delay as f32,
            countdown: 0,
            feedback: verb.feedback,
            damp: verb.damp,
            z: 0.0,
        }
    }

    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        if self.countdown == 0 {
            let lfo = sin_cyc(self.lfo.tick(self.lfo_hz * Self::CONTROL_BLOCK as f32));
            self.delay = self.base + self.depth * (0.5 + 0.5 * lfo);
            self.countdown = Self::CONTROL_BLOCK;
        }
        self.countdown -= 1;
        let n = self.buf.len();
        // `delay` lies in [base, base + depth] and the buffer is longer than
        // that, so `read` is in (0, 2n): wrap by subtraction, not modulo.
        let read = self.write as f32 + n as f32 - self.delay;
        let mut i0 = read as usize;
        let frac = read - i0 as f32;
        if i0 >= n {
            i0 -= n;
        }
        let i1 = if i0 + 1 == n { 0 } else { i0 + 1 };
        let a = self.buf[i0];
        let y = a + (self.buf[i1] - a) * frac;
        self.z = y + (self.z - y) * self.damp;
        if self.z.abs() < 1e-20 {
            self.z = 0.0; // keep denormals out of the feedback path
        }
        self.buf[self.write] = x + self.z * self.feedback;
        self.write += 1;
        if self.write == n {
            self.write = 0;
        }
        y
    }
}

/// Schroeder all-pass diffuser.
#[derive(Clone, Debug)]
struct Allpass {
    buf: Vec<f32>,
    pos: usize,
    g: f32,
}

impl Allpass {
    fn new(delay: usize, g: f32) -> Allpass {
        Allpass {
            buf: vec![0.0; delay.max(1)],
            pos: 0,
            g,
        }
    }

    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let delayed = self.buf[self.pos];
        let v = x + self.g * delayed;
        self.buf[self.pos] = v;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        delayed - self.g * v
    }
}

/// Reverb settings for a canvas.
#[derive(Clone, Copy, Debug)]
struct Verb {
    /// Comb feedback (tail length), `0..1`.
    feedback: f32,
    /// Low-pass damping inside the combs, `0..1` (higher = darker tail).
    damp: f32,
    /// Wet level added to the dry bus.
    mix: f32,
}

impl Verb {
    /// Tight, short room for effects and ambience.
    const ROOM: Verb = Verb {
        feedback: 0.72,
        damp: 0.35,
        mix: 0.55,
    };
    /// Bright, glistening tail for magical effects.
    const SHIMMER: Verb = Verb {
        feedback: 0.8,
        damp: 0.12,
        mix: 0.75,
    };
    /// Lush, dreamy space for the music loops.
    const HALL: Verb = Verb {
        feedback: 0.84,
        damp: 0.28,
        mix: 1.25,
    };
    /// The same space, a little drier so jingles stay crisp.
    const STAGE: Verb = Verb {
        feedback: 0.84,
        damp: 0.28,
        mix: 0.95,
    };
}

/// Small modulated feedback-delay network ("shimmer" reverb): four chorused
/// combs in parallel, two all-pass diffusers in series, and a high-pass so
/// the tail stays airy instead of muddy.
#[derive(Clone, Debug)]
struct Shimmer {
    combs: [ModComb; 4],
    diffusers: [Allpass; 2],
    hp: Hp1,
    input_gain: f32,
}

impl Shimmer {
    /// `loop_len`: when rendering a loop, the chorus LFO rates are snapped to
    /// a whole number of cycles per loop so the reverb state is periodic too.
    fn new(verb: Verb, loop_len: Option<usize>) -> Shimmer {
        const DELAYS: [usize; 4] = [1116, 1277, 1422, 1557];
        const RATES: [f32; 4] = [0.31, 0.47, 0.59, 0.73];
        const DEPTH: f32 = 9.0;
        let snap = |hz: f32| match loop_len {
            Some(n) if n > 0 => {
                let secs = n as f32 / SR;
                (hz * secs).round().max(1.0) / secs
            }
            _ => hz,
        };
        let comb = |i: usize| ModComb::new(DELAYS[i], DEPTH, snap(RATES[i]), i as f32 * 0.25, verb);
        Shimmer {
            combs: [comb(0), comb(1), comb(2), comb(3)],
            diffusers: [Allpass::new(556, 0.5), Allpass::new(441, 0.5)],
            hp: Hp1::new(180.0),
            input_gain: 1.0 - verb.feedback,
        }
    }

    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let x = x * self.input_gain;
        let mut s = 0.0;
        for c in &mut self.combs {
            s += c.run(x);
        }
        for d in &mut self.diffusers {
            s = d.run(s);
        }
        self.hp.run(s)
    }
}

/// Feed `f` the `count` samples that cyclically precede `buf[0]` (oldest
/// first), so a stateful processor reaches the state it would have when the
/// loop comes around again.
fn feed_cyclic_tail(buf: &[f32], count: usize, mut f: impl FnMut(f32)) {
    let n = buf.len();
    if n == 0 {
        return;
    }
    let (full, part) = (count / n, count % n);
    buf[n - part..].iter().for_each(|&x| f(x));
    for _ in 0..full {
        buf.iter().for_each(|&x| f(x));
    }
}

/// Peak absolute value of a buffer.
fn peak_of(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

/// Final mastering: scrub non-finite values, normalise, soft-clip the top of
/// the range for a little glue, then normalise to exactly `peak`.
fn master(buf: &mut [f32], peak: f32) {
    for x in buf.iter_mut() {
        if !x.is_finite() {
            *x = 0.0;
        }
    }
    let m = peak_of(buf);
    if m <= 1e-9 {
        return;
    }
    let g = 1.0 / m;
    for x in buf.iter_mut() {
        *x = soft_clip(*x * g);
    }
    let m = peak_of(buf);
    if m > 1e-9 {
        let g = peak / m;
        for x in buf.iter_mut() {
            *x *= g;
        }
    }
}

// ===========================================================================
// Voices
// ===========================================================================

/// A per-note synthesiser.
trait Voice {
    /// Total length in samples, including the release tail.
    fn length(&self) -> usize;
    /// Produce the next sample.
    fn tick(&mut self) -> f32;
}

/// A one-off voice driven by a closure of the sample index.
struct FnVoice<F> {
    len: usize,
    t: usize,
    f: F,
}

impl<F: FnMut(usize) -> f32> FnVoice<F> {
    fn new(len: usize, f: F) -> FnVoice<F> {
        FnVoice { len, t: 0, f }
    }
}

impl<F: FnMut(usize) -> f32> Voice for FnVoice<F> {
    fn length(&self) -> usize {
        self.len
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        let t = self.t;
        self.t += 1;
        (self.f)(t)
    }
}

/// Two-operator FM bell / celesta. The modulation index decays faster than
/// the amplitude, so notes start bright and mellow into a pure tone.
struct BellVoice {
    carrier: Phasor,
    modulator: Phasor,
    fc: f32,
    fm: f32,
    /// Peak phase deviation in cycles.
    dev: f32,
    amp: Decay,
    bright: Decay,
    t: usize,
    attack: usize,
    gate: usize,
    release: usize,
    len: usize,
}

impl BellVoice {
    fn new(freq: f32, gate: f32, ratio: f32, index: f32, decay: f32) -> BellVoice {
        let attack = samples(0.0015).max(1);
        let release = samples(0.15).max(1);
        let gate = samples(gate).max(attack);
        let len = samples(decay * 6.0).min(gate + release).max(attack + 1);
        let fm = freq * ratio;
        // Keep the strongest sidebands below ~19 kHz (no aliasing fizz).
        let headroom = ((19_000.0 - freq) / fm.max(1.0)).clamp(0.0, 1.0);
        BellVoice {
            carrier: Phasor::default(),
            modulator: Phasor::default(),
            fc: freq,
            fm,
            dev: index * headroom / TAU,
            amp: Decay::new(decay),
            bright: Decay::new(decay * 0.3),
            t: 0,
            attack,
            gate,
            release,
            len,
        }
    }
}

impl Voice for BellVoice {
    fn length(&self) -> usize {
        self.len
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        let t = self.t;
        self.t += 1;
        let mut g = self.amp.tick() * ramp(t, self.attack);
        if t > self.gate {
            let r = (1.0 - (t - self.gate) as f32 / self.release as f32).max(0.0);
            g *= r * r;
        }
        let index = self.dev * (0.15 + 0.85 * self.bright.tick());
        let m = index * sin_cyc(self.modulator.tick(self.fm));
        sin_cyc(self.carrier.tick(self.fc) + m) * g
    }
}

/// Soft plucked tone: triangle plus a touch of octave and a tiny noise
/// "pick", through a low-pass whose cutoff closes as the note decays.
struct PluckVoice {
    phase: Phasor,
    freq: f32,
    amp: Decay,
    pick: Decay,
    noise: Noise,
    lp: Lp1,
    floor: f32,
    span: f32,
    bright_rate: f32,
    t: usize,
    gate: usize,
    release: usize,
    len: usize,
}

impl PluckVoice {
    fn new(freq: f32, gate: f32, decay: f32, bright: f32, seed: u32) -> PluckVoice {
        let release = samples(0.06).max(1);
        let gate = samples(gate).max(1);
        let floor = freq * 1.5;
        let span = freq * bright;
        PluckVoice {
            phase: Phasor::default(),
            freq,
            amp: Decay::new(decay),
            pick: Decay::new(0.0015),
            noise: Noise::new(seed),
            lp: Lp1::new(floor + span),
            floor,
            span,
            bright_rate: 1.0 / (decay * 0.35 * SR),
            t: 0,
            gate,
            release,
            len: samples(decay * 7.0).min(gate + release).max(2),
        }
    }
}

impl Voice for PluckVoice {
    fn length(&self) -> usize {
        self.len
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        let t = self.t;
        self.t += 1;
        if t & 15 == 0 {
            let b = (-(t as f32) * self.bright_rate).exp();
            self.lp.set(self.floor + self.span * b);
        }
        let p = self.phase.tick(self.freq);
        let pick = 0.3 * self.noise.bipolar() * self.pick.tick();
        let x = 0.75 * tri_cyc(p) + 0.25 * sin_cyc(2.0 * p) + pick;
        let mut g = self.amp.tick() * ramp(t, 40);
        if t > self.gate {
            let r = (1.0 - (t - self.gate) as f32 / self.release as f32).max(0.0);
            g *= r * r;
        }
        self.lp.run(x) * g
    }
}

/// Warm chord pad: per tone two detuned band-limited saws, plus a sub sine
/// under the lowest tone, all through one gentle two-pole low-pass and one
/// slow ADSR. Rendering a whole chord in one voice shares the filter and
/// envelope work.
struct PadVoice {
    saws: [Phasor; PadVoice::MAX_SAWS],
    /// Phase increments (cycles per sample) of the active saws.
    incs: [f32; PadVoice::MAX_SAWS],
    count: usize,
    sub: Phasor,
    inc_sub: f32,
    env: AdsrRamp,
    lp1: Lp1,
    lp2: Lp1,
    len: usize,
}

impl PadVoice {
    const ENV: Adsr = Adsr::new(0.35, 0.6, 0.75, 0.6);
    /// At most five chord tones (two saws each).
    const MAX_SAWS: usize = 10;

    fn new(freqs: &[f32], gate: f32, cutoff: f32, detune: f32, seed: u32) -> PadVoice {
        let mut rng = Noise::new(seed);
        let mut saws = [Phasor::default(); Self::MAX_SAWS];
        let mut incs = [0.0; Self::MAX_SAWS];
        let mut count = 0;
        let detuned = freqs
            .iter()
            .take(Self::MAX_SAWS / 2)
            .flat_map(|&f| [f * (1.0 + detune), f * (1.0 - detune)]);
        for (k, hz) in detuned.enumerate() {
            saws[k] = Phasor::at(rng.unit());
            incs[k] = hz / SR;
            count = k + 1;
        }
        let lowest = freqs.iter().copied().fold(f32::INFINITY, f32::min);
        let lowest = if lowest.is_finite() { lowest } else { 110.0 };
        let cutoff = cutoff.max(lowest * 3.0);
        let env = AdsrRamp::new(Self::ENV, gate);
        PadVoice {
            saws,
            incs,
            count,
            sub: Phasor::default(),
            inc_sub: lowest * 0.5 / SR,
            env,
            lp1: Lp1::new(cutoff),
            lp2: Lp1::new(cutoff),
            len: env.length(),
        }
    }
}

impl Voice for PadVoice {
    fn length(&self) -> usize {
        self.len
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        let e = self.env.tick();
        let mut x = 0.0;
        for (ph, &inc) in self.saws.iter_mut().zip(&self.incs).take(self.count) {
            x += saw_blep(ph.step(inc), inc);
        }
        let x = 0.5 * x + 0.35 * sin_cyc(self.sub.step(self.inc_sub));
        self.lp2.run(self.lp1.run(x)) * e
    }
}

/// Round bass: soft square plus sine, low-passed with a short filter "pluck".
struct BassVoice {
    phase: Phasor,
    inc: f32,
    shape: SoftSquare,
    env: AdsrRamp,
    lp1: Lp1,
    lp2: Lp1,
    cutoff: f32,
    t: usize,
    len: usize,
}

impl BassVoice {
    const ENV: Adsr = Adsr::new(0.004, 0.15, 0.65, 0.05);

    fn new(freq: f32, gate: f32, cutoff: f32) -> BassVoice {
        let env = AdsrRamp::new(Self::ENV, gate);
        BassVoice {
            phase: Phasor::default(),
            inc: freq / SR,
            shape: SoftSquare::new(2.2),
            env,
            lp1: Lp1::new(cutoff),
            lp2: Lp1::new(cutoff),
            cutoff,
            t: 0,
            len: env.length(),
        }
    }
}

impl Voice for BassVoice {
    fn length(&self) -> usize {
        self.len
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        let t = self.t;
        self.t += 1;
        if t & 15 == 0 {
            let c = self.cutoff * (1.0 + 2.5 * (-(t as f32) / (0.08 * SR)).exp());
            self.lp1.set(c);
            self.lp2.set(c);
        }
        let p = self.phase.step(self.inc);
        let x = 0.65 * self.shape.at(p) + 0.45 * sin_cyc(p);
        self.lp2.run(self.lp1.run(x)) * self.env.tick()
    }
}

/// Airy lead: sine with a whisper of triangle and octave, delayed vibrato.
struct LeadVoice {
    phase: Phasor,
    vibrato: Phasor,
    /// Base phase increment (cycles per sample).
    inc: f32,
    /// Current increment and its per-sample slope (vibrato at control rate).
    cur_inc: f32,
    inc_step: f32,
    env: AdsrRamp,
    t: usize,
    len: usize,
}

impl LeadVoice {
    const ENV: Adsr = Adsr::new(0.015, 0.3, 0.6, 0.2);
    /// Vibrato fades in after this many seconds...
    const VIBRATO_DELAY: f32 = 0.15;
    /// ...over this many seconds, to this depth (fraction of the pitch).
    const VIBRATO_FADE: f32 = 0.3;
    const VIBRATO_DEPTH: f32 = 0.012;
    const VIBRATO_HZ: f32 = 5.3;
    /// The vibrato is evaluated every this many samples and interpolated.
    const BLOCK: usize = 32;

    fn new(freq: f32, gate: f32) -> LeadVoice {
        let env = AdsrRamp::new(Self::ENV, gate);
        let inc = freq / SR;
        LeadVoice {
            phase: Phasor::default(),
            vibrato: Phasor::default(),
            inc,
            cur_inc: inc,
            inc_step: 0.0,
            env,
            t: 0,
            len: env.length(),
        }
    }
}

impl Voice for LeadVoice {
    fn length(&self) -> usize {
        self.len
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        if self.t.is_multiple_of(Self::BLOCK) {
            let ts = (self.t + Self::BLOCK) as f32 / SR;
            let fade_in = smoothstep((ts - Self::VIBRATO_DELAY) / Self::VIBRATO_FADE);
            let depth = Self::VIBRATO_DEPTH * fade_in;
            let vib = sin_cyc(
                self.vibrato
                    .step(Self::VIBRATO_HZ * Self::BLOCK as f32 / SR),
            );
            let target = self.inc * (1.0 + depth * vib);
            self.inc_step = (target - self.cur_inc) / Self::BLOCK as f32;
        }
        self.t += 1;
        let p = self.phase.step(self.cur_inc);
        self.cur_inc += self.inc_step;
        let x = sin_cyc(p) + 0.16 * tri_cyc(p) + 0.1 * sin_cyc(2.0 * p);
        x * self.env.tick()
    }
}

/// Soft kick: a sine with a fast downward pitch sweep and a tiny click.
struct KickVoice {
    phase: Phasor,
    pitch: Decay,
    amp: Decay,
    click: Decay,
    noise: Noise,
    lp: Lp1,
    t: usize,
    len: usize,
}

impl KickVoice {
    fn new(seed: u32) -> KickVoice {
        KickVoice {
            phase: Phasor::default(),
            pitch: Decay::new(0.028),
            amp: Decay::new(0.12),
            click: Decay::new(0.0015),
            noise: Noise::new(seed),
            lp: Lp1::new(2500.0),
            t: 0,
            len: samples(0.32),
        }
    }
}

impl Voice for KickVoice {
    fn length(&self) -> usize {
        self.len
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        let t = self.t;
        self.t += 1;
        let hz = 48.0 + 115.0 * self.pitch.tick();
        let body = sin_cyc(self.phase.tick(hz)) * self.amp.tick() * ramp(t, 44);
        body + 0.2 * self.lp.run(self.noise.bipolar()) * self.click.tick()
    }
}

/// Soft snare / clap: band-passed noise over a short tonal body.
struct SnareVoice {
    noise: Noise,
    bp: Svf,
    body: Phasor,
    noise_env: Decay,
    body_env: Decay,
    t: usize,
    len: usize,
}

impl SnareVoice {
    fn new(seed: u32) -> SnareVoice {
        SnareVoice {
            noise: Noise::new(seed),
            bp: Svf::new(1700.0, 0.8),
            body: Phasor::default(),
            noise_env: Decay::new(0.07),
            body_env: Decay::new(0.03),
            t: 0,
            len: samples(0.32),
        }
    }
}

impl Voice for SnareVoice {
    fn length(&self) -> usize {
        self.len
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        let t = self.t;
        self.t += 1;
        let n = self.bp.run(self.noise.bipolar()).band * self.noise_env.tick();
        let b = sin_cyc(self.body.tick(190.0)) * self.body_env.tick() * 0.45;
        (0.9 * n + b) * ramp(t, 35)
    }
}

/// Hi-hat: noise high-passed into the "air" band, with the top-end fizz
/// rolled off, and a short exponential decay.
struct HatVoice {
    noise: Noise,
    hp: Svf,
    lp: Lp1,
    env: Decay,
    t: usize,
    len: usize,
}

impl HatVoice {
    fn new(decay: f32, seed: u32) -> HatVoice {
        HatVoice {
            noise: Noise::new(seed),
            hp: Svf::new(6500.0, 0.7),
            lp: Lp1::new(10_000.0),
            env: Decay::new(decay),
            t: 0,
            len: samples(decay * 7.0).max(2),
        }
    }
}

impl Voice for HatVoice {
    fn length(&self) -> usize {
        self.len
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        let t = self.t;
        self.t += 1;
        let air = self.hp.run(self.noise.bipolar()).high;
        self.lp.run(air) * self.env.tick() * ramp(t, 18)
    }
}

/// Shaker: band-passed noise with a soft swell and quick decay.
struct ShakerVoice {
    noise: Noise,
    bp: Svf,
    env: Decay,
    attack: usize,
    t: usize,
    len: usize,
}

impl ShakerVoice {
    fn new(seed: u32) -> ShakerVoice {
        ShakerVoice {
            noise: Noise::new(seed),
            bp: Svf::new(5800.0, 1.2),
            env: Decay::new(0.035),
            attack: samples(0.006),
            t: 0,
            len: samples(0.15),
        }
    }
}

impl Voice for ShakerVoice {
    fn length(&self) -> usize {
        self.len
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        let t = self.t;
        self.t += 1;
        let e = if t < self.attack {
            ramp(t, self.attack)
        } else {
            self.env.tick()
        };
        self.bp.run(self.noise.bipolar()).band * e
    }
}

// ===========================================================================
// Instruments and the mix canvas
// ===========================================================================

/// Sound source of an instrument.
#[derive(Clone, Copy, Debug)]
enum Timbre {
    Bell { ratio: f32, index: f32, decay: f32 },
    Pluck { decay: f32, bright: f32 },
    Pad { cutoff: f32, detune: f32 },
    Bass { cutoff: f32 },
    Lead,
    Kick,
    Snare,
    Hat { decay: f32 },
    Shaker,
}

/// A timbre plus its routing on the canvas.
#[derive(Clone, Copy, Debug)]
struct Inst {
    timbre: Timbre,
    /// Level on the dry (or pump) bus.
    dry: f32,
    /// Level sent to the reverb.
    send: f32,
    /// Route the dry signal through the side-chain pump.
    pumped: bool,
}

impl Inst {
    const fn new(timbre: Timbre, dry: f32, send: f32) -> Inst {
        Inst {
            timbre,
            dry,
            send,
            pumped: false,
        }
    }

    const fn pumped(self) -> Inst {
        Inst {
            pumped: true,
            ..self
        }
    }
}

/// Celesta-like lead bell (harmonic FM).
const BELL_LEAD: Inst = Inst::new(
    Timbre::Bell {
        ratio: 2.0,
        index: 1.3,
        decay: 0.55,
    },
    0.55,
    0.3,
);
/// Inharmonic glass bell for long shimmering tones.
const GLASS: Inst = Inst::new(
    Timbre::Bell {
        ratio: 3.5,
        index: 0.9,
        decay: 0.8,
    },
    0.35,
    0.45,
);
/// Bright, short chime for arpeggios and pickups.
const CHIME: Inst = Inst::new(
    Timbre::Bell {
        ratio: 2.0,
        index: 1.3,
        decay: 0.3,
    },
    0.5,
    0.3,
);
/// Crystalline high chime.
const CRYSTAL: Inst = Inst::new(
    Timbre::Bell {
        ratio: 3.5,
        index: 0.6,
        decay: 0.16,
    },
    0.55,
    0.3,
);
/// Woody, playful mallet.
const MALLET: Inst = Inst::new(
    Timbre::Bell {
        ratio: 4.0,
        index: 1.1,
        decay: 0.2,
    },
    0.5,
    0.2,
);
/// Tiny high ping used for sparkles.
const SPARK: Inst = Inst::new(
    Timbre::Bell {
        ratio: 2.0,
        index: 0.5,
        decay: 0.07,
    },
    0.3,
    0.45,
);
const PLUCK: Inst = Inst::new(
    Timbre::Pluck {
        decay: 0.22,
        bright: 6.0,
    },
    0.4,
    0.2,
);
const ARP: Inst = Inst::new(
    Timbre::Pluck {
        decay: 0.12,
        bright: 5.0,
    },
    0.28,
    0.3,
);
const PAD: Inst = Inst::new(
    Timbre::Pad {
        cutoff: 1500.0,
        detune: 0.006,
    },
    0.2,
    0.15,
);
const BASS: Inst = Inst::new(Timbre::Bass { cutoff: 650.0 }, 0.35, 0.0);
const LEAD: Inst = Inst::new(Timbre::Lead, 0.4, 0.3);
const KICK: Inst = Inst::new(Timbre::Kick, 0.55, 0.0);
const SNARE: Inst = Inst::new(Timbre::Snare, 1.0, 0.6);
const HAT: Inst = Inst::new(Timbre::Hat { decay: 0.022 }, 0.2, 0.05);
const SHAKER: Inst = Inst::new(Timbre::Shaker, 0.26, 0.08);

/// Fade applied to the last few samples of every voice (about 4 ms max).
const VOICE_FADE: usize = 176;

/// One note of a score: (start beat, MIDI pitch, length in beats).
type Score = [(f32, u8, f32)];

/// A mono mix under construction.
struct Canvas {
    dry: Vec<f32>,
    send: Vec<f32>,
    /// Side-chained bus (ducked on every beat); empty until first used.
    pump: Vec<f32>,
    pump_depth: f32,
    looped: bool,
    /// Samples per beat (music canvases).
    beat_len: f32,
    /// Fade-out applied to the end of a one-shot, in seconds.
    fade_out: f32,
}

impl Canvas {
    fn with_len(len: usize, looped: bool, bpm: f32) -> Canvas {
        let len = len.max(1);
        Canvas {
            dry: vec![0.0; len],
            send: vec![0.0; len],
            pump: Vec::new(),
            pump_depth: 0.0,
            looped,
            beat_len: SR * 60.0 / bpm,
            fade_out: 0.02,
        }
    }

    /// A one-shot of `secs` seconds.
    fn oneshot(secs: f32) -> Canvas {
        Canvas::with_len(samples(secs), false, 120.0)
    }

    /// A one-shot jingle of `secs` seconds at `bpm`.
    fn jingle(secs: f32, bpm: f32) -> Canvas {
        let mut c = Canvas::with_len(samples(secs), false, bpm);
        c.fade_out = 0.15;
        c
    }

    /// A loop of exactly `len` samples.
    fn looping(len: usize) -> Canvas {
        Canvas::with_len(len, true, 120.0)
    }

    /// A loop of exactly `bars` 4/4 bars at `bpm`.
    fn music_loop(bars: usize, bpm: f32) -> Canvas {
        let len = (bars as f32 * 4.0 * SR * 60.0 / bpm).round() as usize;
        Canvas::with_len(len, true, bpm)
    }

    fn len(&self) -> usize {
        self.dry.len()
    }

    /// Sample index of a position in beats.
    fn beat(&self, beats: f32) -> usize {
        (beats.max(0.0) * self.beat_len).round() as usize
    }

    fn secs_per_beat(&self) -> f32 {
        self.beat_len / SR
    }

    /// Mix `voice` in starting at sample `start`. On a looping canvas the
    /// voice wraps around the end; on a one-shot it is cut at the end. The
    /// final few samples are faded so no voice ever ends with a step.
    fn voice<V: Voice>(&mut self, start: usize, mut voice: V, dry: f32, send: f32, pumped: bool) {
        let n = self.len();
        let mut len = voice.length();
        let mut j = if self.looped {
            start % n
        } else if start < n {
            len = len.min(n - start);
            start
        } else {
            return;
        };
        if pumped && self.pump.is_empty() {
            self.pump = vec![0.0; n];
        }
        let Canvas {
            dry: dry_bus,
            send: send_bus,
            pump: pump_bus,
            ..
        } = self;
        let bus = if pumped { pump_bus } else { dry_bus };
        let fade = (len / 8).clamp(1, VOICE_FADE);
        for i in 0..len {
            let mut s = voice.tick();
            let left = len - i;
            if left < fade {
                s *= left as f32 / fade as f32;
            }
            bus[j] += s * dry;
            send_bus[j] += s * send;
            j += 1;
            if j == n {
                j = 0;
            }
        }
    }

    /// Play one note of `inst` at sample `start`, held for `gate` seconds.
    fn note(&mut self, inst: &Inst, start: usize, midi: f32, gate: f32, vel: f32) {
        let f = midi_hz(midi);
        let seed = (start as u32).wrapping_mul(2_654_435_761) ^ midi.to_bits();
        let (dry, send, pumped) = (inst.dry * vel, inst.send * vel, inst.pumped);
        match inst.timbre {
            Timbre::Bell {
                ratio,
                index,
                decay,
            } => self.voice(
                start,
                BellVoice::new(f, gate, ratio, index, decay),
                dry,
                send,
                pumped,
            ),
            Timbre::Pluck { decay, bright } => self.voice(
                start,
                PluckVoice::new(f, gate, decay, bright, seed),
                dry,
                send,
                pumped,
            ),
            Timbre::Pad { cutoff, detune } => {
                let voice = PadVoice::new(&[f], gate, cutoff, detune, seed);
                self.voice(start, voice, dry, send, pumped)
            }
            Timbre::Bass { cutoff } => {
                self.voice(start, BassVoice::new(f, gate, cutoff), dry, send, pumped)
            }
            Timbre::Lead => self.voice(start, LeadVoice::new(f, gate), dry, send, pumped),
            Timbre::Kick => self.voice(start, KickVoice::new(seed), dry, send, pumped),
            Timbre::Snare => self.voice(start, SnareVoice::new(seed), dry, send, pumped),
            Timbre::Hat { decay } => {
                self.voice(start, HatVoice::new(decay, seed), dry, send, pumped)
            }
            Timbre::Shaker => self.voice(start, ShakerVoice::new(seed), dry, send, pumped),
        }
    }

    /// Play a chord (MIDI pitches) at sample `start`, held for `gate` seconds.
    /// Pads render the whole chord in one voice; other timbres note by note.
    fn chord(&mut self, inst: &Inst, start: usize, tones: &[f32], gate: f32, vel: f32) {
        if let Timbre::Pad { cutoff, detune } = inst.timbre {
            let mut freqs = [0.0f32; PadVoice::MAX_SAWS / 2];
            let k = tones.len().min(freqs.len());
            for (f, &m) in freqs.iter_mut().zip(tones) {
                *f = midi_hz(m);
            }
            let seed = (start as u32).wrapping_mul(2_654_435_761) ^ (k as u32);
            let voice = PadVoice::new(&freqs[..k], gate, cutoff, detune, seed);
            self.voice(start, voice, inst.dry * vel, inst.send * vel, inst.pumped);
        } else {
            for &m in tones {
                self.note(inst, start, m, gate, vel);
            }
        }
    }

    /// Play a chord at `beats`, held for `gate` beats.
    fn chord_b(&mut self, inst: &Inst, beats: f32, tones: &[u8], gate: f32, vel: f32) {
        let mut pitches = [0.0f32; 8];
        let k = tones.len().min(pitches.len());
        for (p, &m) in pitches.iter_mut().zip(tones) {
            *p = f32::from(m);
        }
        let start = self.beat(beats);
        let gate = gate * self.secs_per_beat();
        self.chord(inst, start, &pitches[..k], gate, vel);
    }

    /// Play a note at `secs` seconds, held for `gate` seconds.
    fn note_s(&mut self, inst: &Inst, secs: f32, midi: f32, gate: f32, vel: f32) {
        self.note(inst, samples(secs), midi, gate, vel);
    }

    /// Play a note at `beats`, held for `gate` beats.
    fn note_b(&mut self, inst: &Inst, beats: f32, midi: f32, gate: f32, vel: f32) {
        let start = self.beat(beats);
        let gate = gate * self.secs_per_beat();
        self.note(inst, start, midi, gate, vel);
    }

    /// Play a drum hit at `beats`.
    fn hit(&mut self, inst: &Inst, beats: f32, vel: f32) {
        self.note_b(inst, beats, 60.0, 0.0, vel);
    }

    /// Play every note of `score`, shifted by `offset` beats; each note is
    /// held for `legato` times its written length.
    fn score(&mut self, inst: &Inst, score: &Score, offset: f32, vel: f32, legato: f32) {
        for &(b, m, len) in score {
            self.note_b(inst, offset + b, f32::from(m), len * legato, vel);
        }
    }

    /// Mix down: side-chain pump, reverb, DC blocking, edge fades (one-shots)
    /// and mastering to `peak` (capped at the mixer's safe peak limits).
    fn finish(mut self, verb: Option<Verb>, peak: f32) -> Vec<f32> {
        const WARM_SECS: f32 = 2.5;
        let n = self.len();
        if !self.pump.is_empty() {
            let curve = pump_curve(self.beat_len.round().max(1.0) as usize, self.pump_depth);
            for (i, (d, p)) in self.dry.iter_mut().zip(&self.pump).enumerate() {
                *d += p * curve[i % curve.len()];
            }
        }
        if let Some(v) = verb {
            let mut rv = Shimmer::new(v, self.looped.then_some(n));
            if self.looped {
                feed_cyclic_tail(&self.send, samples(WARM_SECS), |x| {
                    rv.run(x);
                });
            }
            for (d, &s) in self.dry.iter_mut().zip(&self.send) {
                *d += v.mix * rv.run(s);
            }
        }
        let mut dc = Hp1::new(18.0);
        if self.looped {
            feed_cyclic_tail(&self.dry, samples(0.5), |x| {
                dc.run(x);
            });
        }
        for d in self.dry.iter_mut() {
            *d = dc.run(*d);
        }
        if self.looped {
            rotate_to_quiet_start(&mut self.dry, samples(0.01));
        } else {
            fade_edges(&mut self.dry, self.fade_out);
        }
        let limit = if self.looped {
            LOOP_PEAK_LIMIT
        } else {
            SFX_PEAK_LIMIT
        };
        master(&mut self.dry, peak.min(limit));
        self.dry
    }
}

/// Cyclically shift a loop by at most `window` samples either way so that it
/// begins at its quietest nearby sample (in practice a zero crossing). A
/// rotation keeps the loop periodic, and starting playback no longer clicks.
fn rotate_to_quiet_start(buf: &mut [f32], window: usize) {
    let n = buf.len();
    let window = window.min(n / 2);
    if window == 0 {
        return;
    }
    let candidates = (n - window..n).chain(0..window);
    let best = candidates
        .min_by(|&a, &b| buf[a].abs().total_cmp(&buf[b].abs()))
        .unwrap_or(0);
    buf.rotate_left(best);
}

/// Gain curve for one beat of side-chain "pumping": a smooth duck right after
/// the beat that recovers over ~0.3 s (continuous at both ends).
fn pump_curve(period: usize, depth: f32) -> Vec<f32> {
    (0..period)
        .map(|i| {
            let t = i as f32 / SR;
            let duck = (1.0 - (-t / 0.004).exp()) * (-t / 0.13).exp();
            1.0 - depth * duck
        })
        .collect()
}

/// Raised-cosine fade-in (~0.7 ms) and fade-out (`out_secs`) of a one-shot.
fn fade_edges(buf: &mut [f32], out_secs: f32) {
    let n = buf.len();
    let fin = 32.min(n / 4).max(1);
    for (i, x) in buf.iter_mut().take(fin).enumerate() {
        *x *= 0.5 - 0.5 * (PI * i as f32 / fin as f32).cos();
    }
    let fout = samples(out_secs).min(n / 4).max(1);
    for (k, x) in buf.iter_mut().rev().take(fout).enumerate() {
        // k = 0 is the very last sample (gain 0).
        *x *= 0.5 - 0.5 * (PI * k as f32 / fout as f32).cos();
    }
}

/// Settings for a scatter of tiny bell pings.
#[derive(Clone, Copy, Debug)]
struct Sparkles {
    from: f32,
    to: f32,
    count: usize,
    /// MIDI range (pings are drawn from a major pentatonic scale on C).
    low: u8,
    high: u8,
    /// Velocity at `from`; fades linearly to `vel * fade_to` at `to`.
    vel: f32,
    fade_to: f32,
}

impl Sparkles {
    /// `count` pings spread between `from` and `to` seconds.
    fn between(from: f32, to: f32, count: usize) -> Sparkles {
        Sparkles {
            from,
            to,
            count,
            low: 84,
            high: 108,
            vel: 0.5,
            fade_to: 1.0,
        }
    }

    /// Restrict the pings to the MIDI range `low..=high`.
    fn pitches(self, low: u8, high: u8) -> Sparkles {
        Sparkles {
            low: low.min(high),
            high: high.max(low),
            ..self
        }
    }

    /// Velocity at the start and the factor it reaches by the end.
    fn level(self, vel: f32, fade_to: f32) -> Sparkles {
        Sparkles {
            vel,
            fade_to,
            ..self
        }
    }

    fn render(self, c: &mut Canvas, seed: u32) {
        const PENTA: [u8; 5] = [0, 2, 4, 7, 9];
        let mut rng = Noise::new(seed);
        let count = self.count.max(1);
        for k in 0..count {
            let u = (k as f32 + 0.3 + 0.4 * rng.unit()) / count as f32;
            let t = self.from + (self.to - self.from) * u;
            let mut m = self.low;
            for _ in 0..8 {
                let octaves = usize::from((self.high - self.low) / 12 + 1);
                let octave = self.low / 12 + rng.below(octaves) as u8;
                let cand = octave * 12 + PENTA[rng.below(PENTA.len())];
                if (self.low..=self.high).contains(&cand) {
                    m = cand;
                    break;
                }
            }
            let vel = self.vel * (1.0 + (self.fade_to - 1.0) * u) * (0.7 + 0.3 * rng.unit());
            c.note_s(&SPARK, t, f32::from(m), 0.3, vel);
        }
    }
}

// ===========================================================================
// One-shot effects
// ===========================================================================

/// Pitches of the glimmer run: G major pentatonic, G5 A5 B5 D6 E6.
const GLIMMER_RUN: [u8; 5] = [79, 81, 83, 86, 88];

/// Step `step` of the glimmer run (clamped to the top note). Higher notes are
/// mastered a touch quieter so the run sounds even as it climbs.
fn glimmer(step: usize) -> Vec<f32> {
    let k = step.min(GLIMMER_RUN.len() - 1);
    mote(f32::from(GLIMMER_RUN[k]), 0.26 * (1.0 - 0.025 * k as f32))
}

/// Soft glassy chirp (~58 ms): a sine that flicks up about a semitone, with
/// a faint inharmonic "glass" partial that dies away almost immediately.
fn mote(midi: f32, peak: f32) -> Vec<f32> {
    const LEN: f32 = 0.058;
    let mut c = Canvas::oneshot(LEN);
    let f0 = midi_hz(midi);
    let (mut tone, mut glass) = (Phasor::default(), Phasor::default());
    let (mut amp, mut shine) = (Decay::new(0.016), Decay::new(0.006));
    let mut lp = Lp1::new(6000.0);
    let attack = samples(0.0015);
    let len = c.len();
    c.voice(
        0,
        FnVoice::new(len, move |t| {
            let ts = t as f32 / SR;
            let hz = f0 * (1.0 + 0.07 * (1.0 - (-ts / 0.005).exp()));
            let x = sin_cyc(tone.tick(hz)) + 0.22 * shine.tick() * sin_cyc(glass.tick(hz * 2.756));
            lp.run(x) * amp.tick() * ramp(t, attack)
        }),
        1.0,
        0.0,
        false,
    );
    c.finish(None, peak)
}

/// Rising shimmering sweep (~0.5 s) with sparkles climbing alongside.
fn flare() -> Vec<f32> {
    let mut c = Canvas::oneshot(0.8);
    let len = samples(0.5);
    let (mut a, mut b) = (Phasor::default(), Phasor::at(0.3));
    let (mut md, mut trem) = (Phasor::default(), Phasor::default());
    c.voice(
        0,
        FnVoice::new(len, move |t| {
            let u = t as f32 / len as f32;
            let hz = exp_lerp(330.0, 1320.0, u.powf(1.3));
            let env = smoothstep(u / 0.08) * (1.0 - smoothstep((u - 0.7) / 0.3));
            let tr = 1.0 - 0.3 * (0.5 + 0.5 * sin_cyc(trem.tick(12.0 + 14.0 * u)));
            let dev = (1.1 - 0.6 * u) / TAU * sin_cyc(md.tick(hz * 2.0));
            let x = sin_cyc(a.tick(hz) + dev) + 0.5 * sin_cyc(b.tick(hz * 1.006));
            x * env * tr
        }),
        0.6,
        0.45,
        false,
    );
    Sparkles::between(0.05, 0.5, 12)
        .pitches(88, 103)
        .level(0.5, 1.4)
        .render(&mut c, 0xF1A2E);
    c.finish(Some(Verb::SHIMMER), 0.42)
}

/// Quick "bloop-zap" (~0.34 s): pitch dives, then springs up past the start.
fn shade_eaten() -> Vec<f32> {
    let mut c = Canvas::oneshot(0.5);
    let len = samples(0.34);
    let dip = samples(0.1);
    let (mut car, mut md) = (Phasor::default(), Phasor::default());
    // A mildly resonant low-pass that tracks the pitch gives the "bloop".
    let mut lp = Svf::new(2600.0, 1.4);
    c.voice(
        0,
        FnVoice::new(len, move |t| {
            let (hz, index) = if t < dip {
                (exp_lerp(760.0, 190.0, t as f32 / dip as f32), 0.6)
            } else {
                let u = (t - dip) as f32 / (len - dip) as f32;
                (exp_lerp(190.0, 1520.0, u.powf(0.85)), 0.6 + 1.2 * u)
            };
            let u = t as f32 / len as f32;
            let env = smoothstep(u / 0.02) * (1.0 - smoothstep((u - 0.72) / 0.28));
            if t & 15 == 0 {
                lp.set(hz * 3.5, 1.4);
            }
            let p = car.tick(hz);
            let x = 0.6 * tri_cyc(p) + 0.5 * sin_cyc(p + index / TAU * sin_cyc(md.tick(hz * 2.0)));
            lp.run(x).low * env
        }),
        0.75,
        0.25,
        false,
    );
    c.finish(Some(Verb::ROOM), 0.32)
}

/// Triumphant sparkle (~0.9 s): a quick major arpeggio blooming into a
/// chord, with airy noise glitter.
fn full_chain() -> Vec<f32> {
    let mut c = Canvas::oneshot(1.1);
    for (k, m) in [79u8, 84, 88, 91, 96].into_iter().enumerate() {
        c.note_s(
            &CHIME,
            0.05 * k as f32,
            f32::from(m),
            0.5,
            0.75 + 0.05 * k as f32,
        );
    }
    for m in [67u8, 72, 79, 84] {
        c.note_s(&GLASS, 0.25, f32::from(m), 0.6, 0.7);
    }
    let len = samples(0.75);
    let mut rng = Noise::new(0xC4A1);
    let mut bp = Svf::new(5000.0, 0.9);
    let mut flutter = Phasor::default();
    c.voice(
        samples(0.1),
        FnVoice::new(len, move |t| {
            let u = t as f32 / len as f32;
            let env = smoothstep(u / 0.15) * (1.0 - smoothstep((u - 0.3) / 0.7));
            let f = 0.6 + 0.4 * sin_cyc(flutter.tick(17.0));
            bp.run(rng.bipolar()).band * env * f
        }),
        0.06,
        0.1,
        false,
    );
    Sparkles::between(0.3, 0.85, 8)
        .pitches(88, 103)
        .level(0.5, 0.4)
        .render(&mut c, 0xF0C);
    c.finish(Some(Verb::SHIMMER), 0.45)
}

/// Death (1.5 s): a descending warbling glide that dissolves into sparkles.
fn death() -> Vec<f32> {
    let mut c = Canvas::oneshot(1.5);
    c.fade_out = 0.08;
    let len = samples(1.05);
    let (mut car, mut wob) = (Phasor::default(), Phasor::default());
    let mut lp = Lp1::new(2800.0);
    c.voice(
        0,
        FnVoice::new(len, move |t| {
            let u = t as f32 / len as f32;
            let base = exp_lerp(880.0, 98.0, u.powf(0.85));
            let depth = 0.25 + 1.2 * u; // semitones
            let w = sin_cyc(wob.tick(6.0 + 7.0 * u));
            let hz = base * (depth * w / 12.0).exp2();
            let p = car.tick(hz);
            let x = 0.75 * sin_cyc(p) + 0.3 * tri_cyc(p);
            let env = smoothstep(u / 0.015) * (1.0 - smoothstep((u - 0.55) / 0.45));
            lp.run(x) * env
        }),
        0.8,
        0.25,
        false,
    );
    // A soft airy breath under the dissolve.
    let breath_len = samples(0.7);
    let mut rng = Noise::new(0xDEAD);
    let mut bp = Svf::new(3200.0, 1.5);
    c.voice(
        samples(0.7),
        FnVoice::new(breath_len, move |t| {
            let u = t as f32 / breath_len as f32;
            let env = smoothstep(u / 0.3) * (1.0 - smoothstep((u - 0.3) / 0.7));
            bp.run(rng.bipolar()).band * env
        }),
        0.07,
        0.12,
        false,
    );
    Sparkles::between(0.72, 1.36, 14)
        .pitches(84, 105)
        .level(0.75, 0.25)
        .render(&mut c, 0x5EED);
    c.finish(Some(Verb::SHIMMER), 0.42)
}

/// Bright rising E-major arpeggio (six notes).
fn extra_life() -> Vec<f32> {
    let mut c = Canvas::oneshot(0.95);
    for (k, m) in [76u8, 80, 83, 88, 92, 95].into_iter().enumerate() {
        c.note_s(
            &CHIME,
            0.055 * k as f32,
            f32::from(m),
            0.45,
            0.75 + 0.05 * k as f32,
        );
    }
    c.finish(Some(Verb::ROOM), 0.42)
}

/// Soft glass chime with a quieter fifth above.
fn bonus_spawn() -> Vec<f32> {
    let mut c = Canvas::oneshot(0.9);
    c.note_s(&GLASS, 0.0, 79.0, 0.7, 0.9);
    c.note_s(&GLASS, 0.035, 86.0, 0.7, 0.45);
    c.finish(Some(Verb::SHIMMER), 0.34)
}

/// Crystalline two-note chime, rising a fifth.
fn bonus_eaten() -> Vec<f32> {
    let mut c = Canvas::oneshot(0.6);
    c.note_s(&CRYSTAL, 0.0, 88.0, 0.4, 0.9);
    c.note_s(&CRYSTAL, 0.075, 95.0, 0.4, 0.9);
    c.finish(Some(Verb::ROOM), 0.4)
}

/// Tiny UI blip built from short bell notes `(time, midi)`.
fn ui_blip(notes: &[(f32, f32)], total: f32, decay: f32, peak: f32) -> Vec<f32> {
    let mut c = Canvas::oneshot(total);
    c.fade_out = 0.008;
    let inst = Inst::new(
        Timbre::Bell {
            ratio: 2.0,
            index: 0.5,
            decay,
        },
        0.9,
        0.0,
    );
    for &(t, m) in notes {
        c.note_s(&inst, t, m, total, 1.0);
    }
    c.finish(None, peak)
}

/// New-game jingle (3.85 s, D major, 150 BPM): bell lead over plucked
/// chords, a walking bass and light percussion, ending in a sparkle.
fn intro() -> Vec<f32> {
    const LEAD_LINE: &Score = &[
        (0.0, 69, 0.5),
        (0.5, 74, 0.5),
        (1.0, 78, 0.75),
        (1.75, 76, 0.25),
        (2.0, 74, 0.5),
        (2.5, 81, 1.0),
        (3.5, 79, 0.5),
        (4.0, 78, 0.5),
        (4.5, 79, 0.25),
        (4.75, 81, 0.25),
        (5.0, 83, 0.5),
        (5.5, 81, 0.5),
        (6.0, 76, 0.5),
        (6.5, 85, 0.5),
        (7.0, 86, 2.0),
    ];
    const BASS_LINE: &Score = &[
        (0.0, 38, 1.0),
        (1.0, 45, 1.0),
        (2.0, 47, 1.0),
        (3.0, 42, 1.0),
        (4.0, 43, 1.0),
        (5.0, 47, 1.0),
        (6.0, 45, 1.0),
        (7.0, 38, 2.0),
    ];
    let mut c = Canvas::jingle(3.85, 150.0);
    c.score(&BELL_LEAD, LEAD_LINE, 0.0, 0.9, 0.95);
    c.score(&LEAD, LEAD_LINE, 0.0, 0.35, 0.9);
    c.score(&BASS, BASS_LINE, 0.0, 0.8, 0.85);
    let chords: [(f32, &[u8], f32); 5] = [
        (0.0, &[62, 66, 69], 1.5),
        (2.0, &[62, 66, 71], 1.5),
        (4.0, &[62, 67, 71], 1.5),
        (6.0, &[61, 64, 69], 0.9),
        (7.0, &[62, 66, 69, 74], 2.0),
    ];
    for (b, tones, gate) in chords {
        c.chord_b(&PLUCK, b, tones, gate, 0.45);
        c.chord_b(&PAD, b, tones, gate, 0.5);
    }
    for b in [0.0, 2.0, 4.0, 6.0, 7.0] {
        c.hit(&KICK, b, 0.7);
    }
    for b in [1.0, 3.0, 5.0] {
        c.hit(&SNARE, b, 0.6);
    }
    for k in 0..14 {
        c.hit(&SHAKER, k as f32 * 0.5, if k % 2 == 1 { 1.0 } else { 0.6 });
    }
    for (k, m) in [86u8, 90, 93, 98].into_iter().enumerate() {
        c.note_b(&SPARK, 7.0 + 0.12 * k as f32, f32::from(m), 1.0, 0.6);
    }
    c.finish(Some(Verb::STAGE), 0.45)
}

/// Level-clear fanfare (2.0 s, G major): a rushing arpeggio, two pushes and a
/// ringing tonic chord.
fn level_clear() -> Vec<f32> {
    const RUN: &Score = &[
        (0.0, 67, 0.25),
        (0.25, 71, 0.25),
        (0.5, 74, 0.25),
        (0.75, 79, 0.25),
        (1.0, 83, 0.25),
        (1.25, 86, 0.25),
        (1.5, 84, 0.5),
        (2.0, 86, 0.5),
        (2.5, 91, 2.0),
    ];
    let mut c = Canvas::jingle(2.0, 150.0);
    c.score(&BELL_LEAD, RUN, 0.0, 0.9, 1.0);
    let hits: [(f32, &[u8], u8, f32); 3] = [
        (1.5, &[60, 64, 67, 72], 36, 0.45),
        (2.0, &[62, 66, 69, 74], 38, 0.45),
        (2.5, &[67, 71, 74, 79], 43, 2.0),
    ];
    for (b, tones, bass, gate) in hits {
        c.chord_b(&PLUCK, b, tones, gate, 0.5);
        c.chord_b(&PAD, b, tones, gate, 0.55);
        c.note_b(&BASS, b, f32::from(bass), gate, 0.9);
        c.hit(&KICK, b, 0.75);
    }
    for k in 0..6 {
        c.hit(&SHAKER, k as f32 * 0.25, 0.4 + 0.12 * k as f32);
    }
    Sparkles::between(1.0, 1.7, 7)
        .pitches(88, 103)
        .level(0.5, 0.4)
        .render(&mut c, 0x1E7E1);
    c.finish(Some(Verb::STAGE), 0.45)
}

/// Game over (2.5 s, A minor, 100 BPM): a gentle iv-V-i cadence.
fn game_over() -> Vec<f32> {
    const MELODY: &Score = &[
        (0.0, 72, 0.5),
        (0.5, 76, 0.5),
        (1.0, 77, 0.5),
        (1.5, 76, 0.25),
        (1.75, 74, 0.25),
        (2.0, 71, 0.5),
        (2.5, 68, 0.5),
        (3.0, 69, 1.2),
    ];
    let soft_bell = Inst::new(
        Timbre::Bell {
            ratio: 2.0,
            index: 0.8,
            decay: 0.7,
        },
        0.5,
        0.35,
    );
    let mut c = Canvas::jingle(2.5, 100.0);
    c.fade_out = 0.3;
    c.score(&soft_bell, MELODY, 0.0, 0.9, 1.0);
    let chords: [(f32, &[u8], u8, f32); 4] = [
        (0.0, &[57, 60, 64], 45, 1.0),
        (1.0, &[53, 57, 62], 41, 1.0),
        (2.0, &[52, 56, 59], 40, 1.0),
        (3.0, &[57, 60, 64], 45, 0.6),
    ];
    for (b, tones, bass, gate) in chords {
        c.chord_b(&PAD, b, tones, gate, 0.75);
        c.note_b(&BASS, b, f32::from(bass), gate, 0.6);
    }
    c.finish(Some(Verb::STAGE), 0.42)
}

/// High-score flourish (2.0 s): a pentatonic run up into a Cmaj9 bloom and a
/// final ringing high C with sparkles.
fn high_score() -> Vec<f32> {
    let mut c = Canvas::jingle(2.0, 150.0);
    for (k, m) in [72u8, 74, 76, 79, 81, 84, 86, 88, 91]
        .into_iter()
        .enumerate()
    {
        c.note_s(
            &CHIME,
            0.065 * k as f32,
            f32::from(m),
            0.2,
            0.55 + 0.04 * k as f32,
        );
    }
    for m in [72u8, 76, 79, 83, 86] {
        c.note_s(&GLASS, 0.62, f32::from(m), 0.9, 0.6);
    }
    c.chord(
        &PAD,
        samples(0.62),
        &[60.0, 64.0, 67.0, 71.0, 74.0],
        0.8,
        0.4,
    );
    c.note_s(&BASS, 0.62, 36.0, 0.8, 0.8);
    c.note_s(&KICK, 0.62, 0.0, 0.0, 0.7);
    for (k, m) in [88u8, 91, 93, 96].into_iter().enumerate() {
        c.note_s(
            &CHIME,
            1.0 + 0.08 * k as f32,
            f32::from(m),
            0.6,
            0.6 + 0.08 * k as f32,
        );
    }
    Sparkles::between(1.1, 1.75, 9)
        .pitches(88, 103)
        .level(0.5, 0.35)
        .render(&mut c, 0x41C4);
    c.finish(Some(Verb::STAGE), 0.45)
}

// ===========================================================================
// Loops
// ===========================================================================

/// Parameters of one gameplay-ambience stage.
#[derive(Clone, Copy, Debug)]
struct HumStage {
    /// Root note of the hum (MIDI).
    root: f32,
    /// Seconds per pulse (8 pulses per loop).
    pulse: f32,
    /// 0..1: extra brightness / urgency.
    urgency: f32,
}

/// Each stage is a whole tone higher, faster and brighter than the last.
const HUM_STAGES: [HumStage; 4] = [
    HumStage {
        root: 40.0,
        pulse: 0.46,
        urgency: 0.0,
    },
    HumStage {
        root: 42.0,
        pulse: 0.40,
        urgency: 0.33,
    },
    HumStage {
        root: 44.0,
        pulse: 0.34,
        urgency: 0.66,
    },
    HumStage {
        root: 46.0,
        pulse: 0.28,
        urgency: 1.0,
    },
];

/// Pulses per hum loop.
const HUM_PULSES: usize = 8;

/// Add a sine snapped to a whole number of cycles over `buf`, so it is
/// exactly periodic when `buf` loops.
fn periodic_sine(buf: &mut [f32], hz: f32, gain: f32) {
    let n = buf.len();
    if n == 0 {
        return;
    }
    let cycles = (f64::from(hz) * n as f64 / f64::from(SR)).round().max(1.0);
    let step = cycles / n as f64;
    for (i, x) in buf.iter_mut().enumerate() {
        *x += sin_cyc((i as f64 * step).fract() as f32) * gain;
    }
}

/// Low pulsing ambient hum: a quiet drone (root, fifth, octave) under eight
/// swelling, filter-opening pulses per loop.
fn hum(stage: usize) -> Vec<f32> {
    let st = HUM_STAGES[stage.min(HUM_STAGES.len() - 1)];
    let pulse = samples(st.pulse);
    let mut c = Canvas::looping(pulse * HUM_PULSES);
    let root = midi_hz(st.root);
    for (ratio, gain) in [(1.0, 0.3), (1.5, 0.08), (2.0, 0.12)] {
        periodic_sine(&mut c.dry, root * ratio, gain);
    }
    let shape = SoftSquare::new(1.4 + st.urgency);
    for k in 0..HUM_PULSES {
        let accent = if k % 2 == 0 { 1.0 } else { 0.72 };
        let len = pulse * 27 / 20;
        let attack = len * 3 / 10;
        let f = root * 2.0;
        let mut ph = Phasor::default();
        let (mut lp1, mut lp2) = (Lp1::new(f * 2.0), Lp1::new(f * 2.0));
        let bright = 3.0 + 3.0 * st.urgency;
        c.voice(
            k * pulse,
            FnVoice::new(len, move |t| {
                let env = if t < attack {
                    let s = sin_cyc(0.25 * t as f32 / attack as f32);
                    s * s
                } else {
                    let s = sin_cyc(0.25 + 0.25 * (t - attack) as f32 / (len - attack) as f32);
                    s * s
                };
                if t & 15 == 0 {
                    let cut = f * (1.5 + bright * env);
                    lp1.set(cut);
                    lp2.set(cut);
                }
                let hz = f * (1.0 + 0.012 * t as f32 / len as f32);
                let p = ph.tick(hz);
                let x = 0.6 * shape.at(p) + 0.4 * sin_cyc(p);
                lp2.run(lp1.run(x)) * env
            }),
            0.6 * accent,
            0.12 * accent,
            false,
        );
        if st.urgency > 0.5 {
            c.note(
                &SPARK,
                k * pulse,
                st.root + 36.0,
                0.1,
                0.25 * st.urgency * accent,
            );
        }
    }
    c.finish(Some(Verb::ROOM), 0.3)
}

/// Fright: a wobbly, playful bloop pattern with a vibrato whistle on top.
fn fright() -> Vec<f32> {
    const PATTERN: [u8; 8] = [55, 62, 58, 62, 55, 62, 65, 62];
    let mut c = Canvas::music_loop(1, 150.0);
    for (k, &m) in PATTERN.iter().enumerate() {
        let f = midi_hz(f32::from(m));
        let len = samples(0.17);
        let mut ph = Phasor::default();
        let shape = SoftSquare::new(2.0);
        let mut lp = Lp1::new(f * 6.0);
        let accent = if k % 2 == 0 { 1.0 } else { 0.75 };
        c.voice(
            c.beat(k as f32 * 0.5),
            FnVoice::new(len, move |t| {
                let u = t as f32 / len as f32;
                if t & 15 == 0 {
                    lp.set(f * (1.5 + 5.0 * (1.0 - u) * (1.0 - u)));
                }
                let hz = f * (1.0 - 0.12 * u);
                let env = ramp(t, 130) * (1.0 - u) * (1.0 - u);
                lp.run(shape.at(ph.tick(hz))) * env
            }),
            0.55 * accent,
            0.15,
            false,
        );
    }
    // Whistle: vibrato over a whole number of cycles, and its total phase
    // snapped to whole cycles, so it is exactly periodic over the loop.
    let n = c.len();
    let f0 = midi_hz(79.0);
    let freq = |i: usize| {
        let u = i as f32 / n as f32;
        f0 * (0.7 * sin_cyc(6.0 * u) / 12.0).exp2()
    };
    let total: f64 = (0..n).map(|i| f64::from(freq(i)) / f64::from(SR)).sum();
    let scale = total.round().max(1.0) / total;
    let mut phase = 0.0f64;
    for (i, (d, s)) in c.dry.iter_mut().zip(c.send.iter_mut()).enumerate() {
        let u = i as f32 / n as f32;
        let pulse = sin_cyc(4.0 * u) * sin_cyc(4.0 * u);
        let x = sin_cyc(phase.fract() as f32) * (0.35 + 0.65 * pulse);
        phase += f64::from(freq(i)) * scale / f64::from(SR);
        *d += 0.12 * x;
        *s += 0.06 * x;
    }
    c.finish(Some(Verb::ROOM), 0.3)
}

/// Eyes: fast airy whirring, ten band-passed noise sweeps per second.
fn eyes() -> Vec<f32> {
    const SWEEPS: usize = 10;
    let sweep = samples(0.1);
    let mut c = Canvas::looping(sweep * SWEEPS);
    for k in 0..SWEEPS {
        let len = sweep * 3 / 2;
        let mut rng = Noise::new(0xE7E5 + k as u32);
        let mut bp = Svf::new(900.0, 3.5);
        let mut ph = Phasor::default();
        c.voice(
            k * sweep,
            FnVoice::new(len, move |t| {
                let u = t as f32 / len as f32;
                if t & 7 == 0 {
                    bp.set(exp_lerp(900.0, 4000.0, u), 3.5);
                }
                let s = sin_cyc(0.5 * u);
                let env = s * s;
                let tone = 0.35 * sin_cyc(ph.tick(exp_lerp(420.0, 1260.0, u)));
                (0.8 * bp.run(rng.bipolar()).band + tone) * env
            }),
            0.5,
            0.15,
            false,
        );
    }
    c.finish(Some(Verb::ROOM), 0.26)
}

/// A chord for the loops: bass root plus four voicing tones.
#[derive(Clone, Copy, Debug)]
struct Chord {
    bass: u8,
    tones: [u8; 4],
}

const fn chord(bass: u8, tones: [u8; 4]) -> Chord {
    Chord { bass, tones }
}

/// Title music: a 12-bar D-minor synthwave loop at 112 BPM
/// (about 25.7 s) with octave bass, pumping pads, a plucked arpeggio, soft
/// drums, glass bells in the first four bars and an airy bell/sine lead after.
fn title() -> Vec<f32> {
    const DM: Chord = chord(38, [62, 65, 69, 74]);
    const BB: Chord = chord(34, [62, 65, 70, 74]);
    const F: Chord = chord(41, [60, 65, 69, 72]);
    const C: Chord = chord(36, [60, 64, 67, 72]);
    const GM: Chord = chord(43, [62, 67, 70, 74]);
    const A: Chord = chord(33, [61, 64, 69, 73]);
    const PROGRESSION: [Chord; 12] = [DM, BB, F, C, DM, BB, GM, A, BB, C, DM, A];
    const ARP_ORDER: [usize; 8] = [0, 1, 2, 3, 2, 1, 2, 3];
    const GLASS_LINE: &Score = &[
        (0.0, 89, 3.5),
        (4.0, 86, 3.5),
        (8.0, 84, 3.5),
        (12.0, 88, 3.5),
    ];
    const LEAD_LINE: &Score = &[
        // Bar 5 (Dm)
        (16.0, 69, 0.5),
        (16.5, 74, 0.5),
        (17.0, 77, 1.0),
        (18.0, 76, 0.5),
        (18.5, 74, 0.5),
        (19.0, 81, 1.0),
        // Bar 6 (Bb)
        (20.0, 79, 1.5),
        (21.5, 77, 0.5),
        (22.0, 74, 2.0),
        // Bar 7 (Gm)
        (24.0, 70, 0.5),
        (24.5, 74, 0.5),
        (25.0, 79, 1.0),
        (26.0, 77, 0.5),
        (26.5, 76, 0.5),
        (27.0, 74, 1.0),
        // Bar 8 (A)
        (28.0, 73, 1.5),
        (29.5, 76, 0.5),
        (30.0, 81, 2.0),
        // Bar 9 (Bb)
        (32.0, 77, 0.5),
        (32.5, 79, 0.5),
        (33.0, 81, 1.0),
        (34.0, 82, 0.5),
        (34.5, 81, 0.5),
        (35.0, 77, 1.0),
        // Bar 10 (C)
        (36.0, 79, 1.5),
        (37.5, 76, 0.5),
        (38.0, 72, 2.0),
        // Bar 11 (Dm)
        (40.0, 74, 0.5),
        (40.5, 77, 0.5),
        (41.0, 81, 1.0),
        (42.0, 84, 0.5),
        (42.5, 81, 0.5),
        (43.0, 77, 1.0),
        // Bar 12 (A)
        (44.0, 76, 1.0),
        (45.0, 73, 1.0),
        (46.0, 69, 2.0),
    ];

    let mut c = Canvas::music_loop(PROGRESSION.len(), 112.0);
    c.pump_depth = 0.4;
    let pad = PAD.pumped();
    for (bar, ch) in PROGRESSION.iter().enumerate() {
        let b0 = bar as f32 * 4.0;
        let voicing = [ch.tones[0] - 12, ch.tones[1] - 12, ch.tones[2] - 12];
        c.chord_b(&pad, b0, &voicing, 3.9, 0.9);
        for k in 0..8 {
            let octave = if k % 2 == 1 { 12.0 } else { 0.0 };
            let vel = if k % 2 == 0 { 1.0 } else { 0.75 };
            c.note_b(
                &BASS,
                b0 + k as f32 * 0.5,
                f32::from(ch.bass) + octave,
                0.38,
                vel,
            );
        }
        for k in 0..16 {
            let m = f32::from(ch.tones[ARP_ORDER[k % 8]]) + 12.0;
            let vel = if k % 4 == 0 { 1.0 } else { 0.7 };
            c.note_b(&ARP, b0 + k as f32 * 0.25, m, 0.2, vel);
        }
        for beat in 0..4 {
            let b = b0 + beat as f32;
            c.hit(&KICK, b, if beat % 2 == 0 { 0.9 } else { 0.65 });
            if beat % 2 == 1 {
                c.hit(&SNARE, b, 0.85);
            }
            c.hit(&HAT, b + 0.5, 1.0);
            c.hit(&HAT, b + 0.25, 0.35);
            c.hit(&HAT, b + 0.75, 0.35);
        }
    }
    c.score(&GLASS, GLASS_LINE, 0.0, 0.7, 1.0);
    c.score(&BELL_LEAD, LEAD_LINE, 0.0, 0.55, 0.9);
    c.score(&LEAD, LEAD_LINE, 0.0, 0.5, 0.95);
    c.finish(Some(Verb::HALL), 0.33)
}

/// Tempo of the interlude loop.
const INTERLUDE_BPM: f32 = 120.0;
/// Bars of 4/4 in the interlude loop: four bars at 120 BPM are exactly 8.0 s,
/// shorter than every interlude scene, so the full tune is always heard.
const INTERLUDE_BARS: usize = 4;

/// Interlude harmony, two beats per chord: F Dm | Gm C | F Bb | C7 F.
const INTERLUDE_CHORDS: [Chord; 8] = [
    chord(41, [65, 69, 72, 77]), // F
    chord(38, [62, 65, 69, 74]), // Dm
    chord(43, [62, 67, 70, 74]), // Gm
    chord(36, [60, 64, 67, 72]), // C
    chord(41, [65, 69, 72, 77]), // F
    chord(46, [62, 65, 70, 74]), // Bb
    chord(36, [58, 64, 67, 72]), // C7
    chord(41, [65, 69, 72, 77]), // F
];

/// Interlude tune: two complete two-bar phrases. The first is a question
/// that comes to rest on the dominant; the second answers it and cadences
/// home to F, held through the last bar.
const INTERLUDE_MELODY: &Score = &[
    // Bar 1 (F, Dm)
    (0.0, 84, 0.5),
    (0.5, 81, 0.5),
    (1.0, 77, 0.5),
    (1.5, 81, 0.5),
    (2.0, 86, 0.75),
    (2.75, 84, 0.25),
    (3.0, 81, 1.0),
    // Bar 2 (Gm, C): rests on G, the fifth of C
    (4.0, 79, 0.5),
    (4.5, 82, 0.5),
    (5.0, 86, 0.75),
    (5.75, 84, 0.25),
    (6.0, 82, 0.5),
    (6.5, 81, 0.5),
    (7.0, 79, 1.0),
    // Bar 3 (F, Bb)
    (8.0, 84, 0.5),
    (8.5, 81, 0.5),
    (9.0, 84, 0.5),
    (9.5, 89, 0.5),
    (10.0, 86, 0.75),
    (10.75, 84, 0.25),
    (11.0, 82, 0.5),
    (11.5, 86, 0.5),
    // Bar 4 (C7, F): the answer lands on the tonic
    (12.0, 84, 0.5),
    (12.5, 82, 0.5),
    (13.0, 79, 0.5),
    (13.5, 76, 0.5),
    (14.0, 77, 1.5),
];

/// Interlude music: a bouncy F-major tune at 120 BPM, exactly
/// 8.0 s long (one pass of [`INTERLUDE_MELODY`]): oom-pah bass, plucked
/// off-beat chords and light drums under a mallet tune, closing on a ringing
/// tonic chord with a sparkle and a one-beat breath before the loop point.
fn interlude() -> Vec<f32> {
    let mut c = Canvas::music_loop(INTERLUDE_BARS, INTERLUDE_BPM);
    let last = INTERLUDE_CHORDS.len() - 1;
    for (k, ch) in INTERLUDE_CHORDS.iter().enumerate() {
        let b0 = k as f32 * 2.0;
        let root = f32::from(ch.bass);
        if k == last {
            // Final tonic: one sustained "oom" and a ringing chord.
            c.note_b(&BASS, b0, root, 1.4, 1.0);
            c.chord_b(&PLUCK, b0, &ch.tones[..3], 1.2, 0.55);
            c.chord_b(&PAD, b0, &ch.tones[..3], 1.5, 0.45);
            for m in [77u8, 81, 84] {
                c.note_b(&GLASS, b0, f32::from(m), 1.5, 0.35);
            }
            for (j, m) in [89u8, 93, 96].into_iter().enumerate() {
                c.note_b(&SPARK, b0 + 0.33 * j as f32, f32::from(m), 0.5, 0.5);
            }
        } else {
            c.note_b(&BASS, b0, root, 0.45, 1.0);
            c.note_b(&BASS, b0 + 1.0, root + 7.0, 0.45, 0.85);
            for off in [0.5, 1.5] {
                c.chord_b(&PLUCK, b0 + off, &ch.tones[..3], 0.25, 0.55);
            }
        }
    }
    let beats = INTERLUDE_BARS as f32 * 4.0;
    for bar in 0..INTERLUDE_BARS {
        let b0 = bar as f32 * 4.0;
        c.hit(&KICK, b0, 0.6);
        c.hit(&KICK, b0 + 2.0, 0.5);
        c.hit(&SNARE, b0 + 1.0, 0.45);
        if b0 + 3.0 < beats - 1.0 {
            c.hit(&SNARE, b0 + 3.0, 0.45);
        }
    }
    // Shaker eighths up to the last beat, which is left to ring.
    for k in 0..(2 * (beats as usize - 1)) {
        c.hit(&SHAKER, k as f32 * 0.5, if k % 2 == 1 { 1.0 } else { 0.55 });
    }
    c.score(&MALLET, INTERLUDE_MELODY, 0.0, 0.8, 0.7);
    c.score(&GLASS, INTERLUDE_MELODY, 0.0, 0.15, 0.7);
    c.finish(Some(Verb::ROOM), 0.3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::READY_TIME_FIRST;

    fn secs(buf: &[f32]) -> f32 {
        buf.len() as f32 / SR
    }

    fn assert_valid(name: &str, buf: &[f32], limit: f32) {
        assert!(
            buf.len() >= 1000,
            "{name}: too short ({} samples)",
            buf.len()
        );
        assert!(
            buf.iter().all(|x| x.is_finite()),
            "{name}: non-finite sample"
        );
        let peak = peak_of(buf);
        assert!(peak <= limit + 1e-6, "{name}: peak {peak} exceeds {limit}");
        assert!(peak >= 0.1, "{name}: nearly silent (peak {peak})");
    }

    #[test]
    fn every_sfx_renders_valid_audio() {
        for sfx in Sfx::ALL {
            let buf = render_sfx(sfx);
            assert_valid(&format!("{sfx:?}"), &buf, SFX_PEAK_LIMIT);
            // One-shots start and end silently (no clicks).
            assert!(buf[0].abs() < 1e-3, "{sfx:?} starts at {}", buf[0]);
            assert!(
                buf[buf.len() - 1].abs() < 1e-3,
                "{sfx:?} ends at {}",
                buf[buf.len() - 1]
            );
        }
    }

    #[test]
    fn every_track_renders_valid_audio() {
        for track in Track::ALL {
            let buf = render_track(track);
            assert_valid(&format!("{track:?}"), &buf, LOOP_PEAK_LIMIT);
            // Loops begin at a quiet sample, so starting playback cannot click.
            assert!(buf[0].abs() < 0.01, "{track:?} starts at {}", buf[0]);
        }
    }

    #[test]
    fn quiet_start_rotation_keeps_the_loop_intact() {
        let mut buf: Vec<f32> = (0..1000)
            .map(|i| 0.5 + (i as f32 * 0.37).sin() * 0.5)
            .collect();
        buf[995] = 0.0;
        let original = buf.clone();
        rotate_to_quiet_start(&mut buf, 10);
        assert_eq!(buf[0], 0.0);
        assert_eq!(buf[5], original[0]);
        let mut sorted_a = original.clone();
        let mut sorted_b = buf.clone();
        sorted_a.sort_by(f32::total_cmp);
        sorted_b.sort_by(f32::total_cmp);
        assert_eq!(sorted_a, sorted_b);
    }

    #[test]
    fn sfx_durations_are_in_range() {
        let len = |s: Sfx| secs(&render_sfx(s));
        let intro = len(Sfx::Intro);
        assert!((3.5..=4.0).contains(&intro), "intro {intro}");
        assert!(intro <= READY_TIME_FIRST);
        assert!(len(Sfx::Death) <= 1.7);
        assert!(len(Sfx::Death) >= 1.3);
        for s in Sfx::MOTES {
            // Each glimmer-run note is a short chirp.
            assert!((0.04..=0.07).contains(&len(s)), "{s:?}: {}", len(s));
        }
        let ui = [
            Sfx::MenuMove,
            Sfx::MenuSelect,
            Sfx::MenuBack,
            Sfx::PauseOn,
            Sfx::PauseOff,
            Sfx::Key,
        ];
        for s in ui {
            assert!((0.03..=0.15).contains(&len(s)), "{s:?}: {}", len(s));
        }
        assert!((1.8..=2.2).contains(&len(Sfx::LevelClear)));
        assert!((2.3..=2.7).contains(&len(Sfx::GameOver)));
        assert!((1.8..=2.2).contains(&len(Sfx::HighScore)));
        assert!((0.5..=1.0).contains(&len(Sfx::Flare)));
        assert!((0.3..=0.6).contains(&len(Sfx::ShadeEaten)));
    }

    #[test]
    fn glimmer_run_rises_through_a_pentatonic_scale() {
        let zero_crossings = |b: &[f32]| {
            b.windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count()
        };
        let crossings: Vec<usize> = Sfx::MOTES
            .iter()
            .map(|&s| zero_crossings(&render_sfx(s)))
            .collect();
        assert!(
            crossings.windows(2).all(|w| w[1] > w[0]),
            "pitch rises: {crossings:?}"
        );
        // Major pentatonic: steps of 2, 2, 3, 2 semitones.
        let steps: Vec<u8> = GLIMMER_RUN.windows(2).map(|w| w[1] - w[0]).collect();
        assert_eq!(steps, vec![2, 2, 3, 2]);
        assert_eq!(Sfx::MOTES.len(), GLIMMER_RUN.len());
        for (k, s) in Sfx::MOTES.into_iter().enumerate() {
            assert_eq!(render_sfx(s), glimmer(k), "{s:?}");
            assert!(!s.is_music() && !s.is_ui(), "{s:?}");
        }
        // Soft: every chirp sits well under the one-shot limit.
        for s in Sfx::MOTES {
            assert!(peak_of(&render_sfx(s)) <= 0.27, "{s:?}");
        }
    }

    #[test]
    fn sfx_categories() {
        let ui: Vec<Sfx> = Sfx::ALL.into_iter().filter(|s| s.is_ui()).collect();
        assert_eq!(
            ui,
            vec![
                Sfx::MenuMove,
                Sfx::MenuSelect,
                Sfx::MenuBack,
                Sfx::PauseOn,
                Sfx::PauseOff,
                Sfx::Key
            ]
        );
        assert!(Sfx::ALL.iter().all(|s| !(s.is_ui() && s.is_music())));
        for (i, s) in Sfx::ALL.into_iter().enumerate() {
            assert_eq!(s as usize, i, "ALL follows declaration order");
        }
    }

    #[test]
    fn track_lengths_are_in_range() {
        let hums = [Track::Hum0, Track::Hum1, Track::Hum2, Track::Hum3];
        for (stage, t) in hums.into_iter().enumerate() {
            let s = secs(&render_track(t));
            assert!((1.0..=4.0).contains(&s), "hum {stage}: {s}");
        }
        let title = render_track(Track::Title);
        assert!((16.0..=32.0).contains(&secs(&title)));
        // Exactly 12 bars at 112 BPM.
        assert_eq!(title.len(), 12 * 4 * 23_625);
        // Exactly 8.0 s: four bars at 120 BPM.
        assert_eq!(
            render_track(Track::Interlude).len(),
            8 * SAMPLE_RATE as usize
        );
    }

    #[test]
    fn interlude_is_whole_phrases_ending_on_the_tonic() {
        let beats = INTERLUDE_BARS as f32 * 4.0;
        assert_eq!(beats * 60.0 / INTERLUDE_BPM, 8.0, "an exactly 8.0 s loop");
        // Interlude scenes last 8.2 s or more: one full pass is always heard.
        assert!(beats * 60.0 / INTERLUDE_BPM < 8.2);
        assert_eq!(
            INTERLUDE_CHORDS.len() as f32 * 2.0,
            beats,
            "two beats per chord"
        );
        // No note of the tune runs over the loop point, and it is in order.
        for &(b, _, len) in INTERLUDE_MELODY {
            assert!(
                b >= 0.0 && len > 0.0 && b + len <= beats,
                "note at beat {b}"
            );
        }
        assert!(INTERLUDE_MELODY
            .windows(2)
            .all(|w| w[0].0 + w[0].2 <= w[1].0 + 1e-6));
        // Phrase 1 rests on G (the dominant's fifth) at the end of bar 2; the
        // answer ends on F over an F chord, after a C7 (a perfect cadence).
        let at = |beat: f32| INTERLUDE_MELODY.iter().find(|n| n.0 == beat).map(|n| n.1);
        assert_eq!(at(7.0).map(|m| m % 12), Some(7));
        let last = INTERLUDE_MELODY[INTERLUDE_MELODY.len() - 1];
        assert_eq!(last.1 % 12, 5, "ends on F");
        assert!(
            last.0 + last.2 >= beats - 1.0,
            "the tonic rings into the last beat"
        );
        let n = INTERLUDE_CHORDS.len();
        assert_eq!(INTERLUDE_CHORDS[n - 1].bass % 12, 5, "F chord last");
        assert_eq!(
            INTERLUDE_CHORDS[n - 2].tones,
            [58, 64, 67, 72],
            "C7 before it"
        );
        assert_eq!(INTERLUDE_CHORDS[0].bass % 12, 5, "the loop restarts on F");
    }

    #[test]
    fn interlude_loop_has_no_click_at_the_seam() {
        let x = render_track(Track::Interlude);
        let n = x.len();
        // The canvas wraps tails over the loop point, so playing the buffer
        // twice in a row has no step at the seam beyond normal motion.
        let max_step = x.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
        let seam = (x[0] - x[n - 1]).abs();
        assert!(
            seam <= max_step && seam < 0.02,
            "seam step {seam} (max {max_step})"
        );
        // Both sides of the seam carry similar energy (no gap or burst).
        let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
        let w = samples(0.05);
        let (tail, head) = (rms(&x[n - w..]), rms(&x[..w]));
        assert!(tail > 1e-3 && head > 1e-3, "tail {tail} head {head}");
    }

    #[test]
    fn hum_stages_rise_in_pitch_and_tempo() {
        for w in HUM_STAGES.windows(2) {
            assert!(w[1].root > w[0].root);
            assert!(w[1].pulse < w[0].pulse);
            assert!(w[1].urgency > w[0].urgency);
        }
    }

    #[test]
    fn loop_seams_are_continuous() {
        for track in Track::ALL {
            let x = render_track(track);
            let n = x.len();
            let diffs: Vec<f32> = x.windows(2).map(|w| w[1] - w[0]).collect();
            let max_step = diffs.iter().fold(0.0f32, |m, d| m.max(d.abs()));
            let max_curve = diffs
                .windows(2)
                .fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
            let seam = x[0] - x[n - 1];
            let before = x[n - 1] - x[n - 2];
            let after = x[1] - x[0];
            assert!(
                seam.abs() <= max_step + 1e-6,
                "{track:?}: seam step {seam} > max {max_step}"
            );
            assert!(seam.abs() < 0.05, "{track:?}: seam step {seam}");
            assert!(
                (seam - before).abs() <= max_curve + 1e-6,
                "{track:?}: slope break into seam"
            );
            assert!(
                (after - seam).abs() <= max_curve + 1e-6,
                "{track:?}: slope break out of seam"
            );
        }
    }

    #[test]
    fn rendering_is_deterministic() {
        for sfx in [Sfx::Flare, Sfx::Death, Sfx::Mote2, Sfx::Intro] {
            assert_eq!(render_sfx(sfx), render_sfx(sfx), "{sfx:?}");
        }
        for track in [Track::Hum2, Track::Eyes, Track::Fright, Track::Interlude] {
            assert_eq!(render_track(track), render_track(track), "{track:?}");
        }
    }

    #[test]
    fn total_sample_budget_is_respected() {
        let sfx: usize = Sfx::ALL.iter().map(|&s| render_sfx(s).len()).sum();
        let tracks: usize = Track::ALL.iter().map(|&t| render_track(t).len()).sum();
        assert!(sfx + tracks < 4_000_000, "{} samples", sfx + tracks);
    }

    fn pcm_of(wav: &[u8]) -> Vec<i16> {
        wav[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c))
            .collect()
    }

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }

    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    #[test]
    fn wav_header_is_canonical() {
        let samples = [0.0f32, 0.5, -0.5, 1.0, -1.0];
        let wav = encode_wav(&samples, SAMPLE_RATE);
        assert_eq!(wav.len(), 44 + samples.len() * 2);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32_at(&wav, 4) as usize, wav.len() - 8);
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(u32_at(&wav, 16), 16);
        assert_eq!(u16_at(&wav, 20), 1, "PCM");
        assert_eq!(u16_at(&wav, 22), 1, "mono");
        assert_eq!(u32_at(&wav, 24), 44_100);
        assert_eq!(u32_at(&wav, 28), 88_200, "byte rate");
        assert_eq!(u16_at(&wav, 32), 2, "block align");
        assert_eq!(u16_at(&wav, 34), 16, "bits");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32_at(&wav, 40) as usize, samples.len() * 2);
        let pcm = pcm_of(&wav);
        assert_eq!(pcm, vec![0, 16384, -16384, 32767, -32767]);
    }

    #[test]
    fn wav_clamps_and_scrubs_bad_samples() {
        let wav = encode_wav(&[2.0, -3.0, f32::NAN, f32::INFINITY], 22_050);
        assert_eq!(u32_at(&wav, 24), 22_050);
        let pcm = pcm_of(&wav);
        assert_eq!(pcm, vec![32767, -32767, 0, 0]);
        let empty = encode_wav(&[], SAMPLE_RATE);
        assert_eq!(empty.len(), 44);
        assert_eq!(u32_at(&empty, 4), 36);
    }

    #[test]
    fn midi_conversion() {
        assert!((midi_hz(69.0) - 440.0).abs() < 1e-3);
        assert!((midi_hz(81.0) - 880.0).abs() < 1e-2);
        assert!((midi_hz(60.0) - 261.6256).abs() < 1e-2);
    }

    #[test]
    fn fast_sine_matches_std() {
        for i in -2000..2000 {
            let p = i as f32 * 0.001_37;
            assert!((sin_cyc(p) - (p * TAU).sin()).abs() < 2e-5, "p = {p}");
        }
    }

    #[test]
    fn oscillator_shapes() {
        assert!(tri_cyc(0.0).abs() < 1e-6);
        assert!((tri_cyc(0.25) - 1.0).abs() < 1e-6);
        assert!((tri_cyc(0.75) + 1.0).abs() < 1e-6);
        let sq = SoftSquare::new(2.5);
        assert!((sq.at(0.25) - 1.0).abs() < 1e-5);
        assert!((sq.at(0.75) + 1.0).abs() < 1e-5);
        // The PolyBLEP saw is bounded and roughly zero-mean.
        let dt = 440.0 / SR;
        let mut ph = Phasor::default();
        let saw: Vec<f32> = (0..44_100).map(|_| saw_blep(ph.tick(440.0), dt)).collect();
        assert!(saw.iter().all(|x| x.abs() <= 1.1));
        assert!((saw.iter().sum::<f32>() / saw.len() as f32).abs() < 0.01);
        assert_eq!(fast_tanh(0.0), 0.0);
        assert!((fast_tanh(1.0) - 1.0f32.tanh()).abs() < 0.03);
        assert_eq!(soft_clip(0.3), 0.3);
        assert!(soft_clip(1.0) < 1.0 && soft_clip(5.0) <= 1.0 && soft_clip(-5.0) >= -1.0);
    }

    #[test]
    fn noise_is_deterministic_and_bounded() {
        let mut a = Noise::new(7);
        let mut b = Noise::new(7);
        let mut c = Noise::new(8);
        let va: Vec<f32> = (0..1000).map(|_| a.bipolar()).collect();
        let vb: Vec<f32> = (0..1000).map(|_| b.bipolar()).collect();
        let vc: Vec<f32> = (0..1000).map(|_| c.bipolar()).collect();
        assert_eq!(va, vb);
        assert_ne!(va, vc);
        assert!(va.iter().all(|x| (-1.0..1.0).contains(x)));
        assert!((va.iter().sum::<f32>() / 1000.0).abs() < 0.1);
        assert!((0..100).all(|_| a.below(5) < 5));
    }

    #[test]
    fn envelopes() {
        let env = Adsr::new(0.1, 0.1, 0.5, 0.2);
        assert_eq!(env.level(0.0, 1.0), 0.0);
        assert!((env.level(0.1, 1.0) - 1.0).abs() < 1e-6);
        assert!((env.level(0.5, 1.0) - 0.5).abs() < 1e-6);
        assert!(env.level(1.1, 1.0) < 0.5);
        assert_eq!(env.level(1.25, 1.0), 0.0);
        assert!((env.length(1.0) - 1.2).abs() < 1e-6);
        let mut d = Decay::new(0.01);
        let first = d.tick();
        for _ in 0..samples(0.01) - 1 {
            d.tick();
        }
        assert_eq!(first, 1.0);
        assert!((d.tick() - (-1.0f32).exp()).abs() < 0.01);
    }

    #[test]
    fn filters_behave() {
        // Low-pass passes DC, high-pass removes it.
        let mut lp = Lp1::new(200.0);
        let mut hp = Hp1::new(200.0);
        let mut svf = Svf::new(500.0, 0.7);
        let mut last = (
            0.0,
            0.0,
            SvfOut {
                low: 0.0,
                band: 0.0,
                high: 0.0,
            },
        );
        for _ in 0..10_000 {
            last = (lp.run(1.0), hp.run(1.0), svf.run(1.0));
        }
        assert!((last.0 - 1.0).abs() < 1e-3);
        assert!(last.1.abs() < 1e-3);
        assert!((last.2.low - 1.0).abs() < 1e-3);
        assert!(last.2.high.abs() < 1e-3 && last.2.band.abs() < 1e-3);
        // A low-pass attenuates a tone far above its cutoff.
        let mut lp = Lp1::new(100.0);
        let mut ph = Phasor::default();
        let out = (0..10_000)
            .map(|_| lp.run(sin_cyc(ph.tick(8000.0))))
            .fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(out < 0.05);
    }

    #[test]
    fn reverb_tail_decays() {
        let mut rv = Shimmer::new(Verb::HALL, None);
        let mut energy = |range: std::ops::Range<usize>, first: bool| {
            range
                .map(|i| rv.run(if first && i == 0 { 1.0 } else { 0.0 }))
                .map(|x| x * x)
                .sum::<f32>()
        };
        let early = energy(0..samples(0.3), true);
        let late = energy(0..samples(3.0), false);
        let silent = energy(0..samples(0.3), false);
        assert!(early > 0.0 && early.is_finite());
        assert!(late.is_finite());
        assert!(silent < early * 1e-3);
    }

    #[test]
    fn looping_canvas_wraps_notes() {
        let mut c = Canvas::looping(1000);
        let len = 300;
        c.voice(900, FnVoice::new(len, |_| 1.0), 1.0, 0.0, false);
        // 100 samples at the end, 200 wrapped to the start.
        assert!(c.dry[950] > 0.99 && c.dry[150] > 0.99);
        assert_eq!(c.dry[500], 0.0);
        // The last sample of the voice (index 199) is faded to ~0.
        assert!(c.dry[199] < 0.05);
    }

    #[test]
    fn oneshot_canvas_truncates_notes() {
        let mut c = Canvas::oneshot(0.01);
        let n = c.len();
        c.voice(n - 10, FnVoice::new(1000, |_| 1.0), 1.0, 0.0, false);
        c.voice(n + 10, FnVoice::new(1000, |_| 1.0), 1.0, 0.0, false);
        assert_eq!(c.dry[0], 0.0);
        assert!(c.dry[n - 10] > 0.0);
    }

    #[test]
    fn cyclic_tail_feeds_oldest_first() {
        let buf = [1.0, 2.0, 3.0, 4.0];
        let mut seen = Vec::new();
        feed_cyclic_tail(&buf, 6, |x| seen.push(x));
        assert_eq!(seen, vec![3.0, 4.0, 1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn mastering_normalises_and_scrubs() {
        let mut buf = vec![0.0, 2.0, -4.0, f32::NAN, 1.0];
        master(&mut buf, 0.4);
        assert!(buf.iter().all(|x| x.is_finite()));
        assert!((peak_of(&buf) - 0.4).abs() < 1e-6);
        let mut silent = vec![0.0; 10];
        master(&mut silent, 0.4);
        assert!(silent.iter().all(|&x| x == 0.0));
    }

    /// Writes every sound as a WAV file into `$GLIMMER_WAV_DIR` for listening.
    /// Run with `GLIMMER_WAV_DIR=/some/dir cargo test --release --lib
    /// synth::tests::export_wavs -- --ignored`.
    #[test]
    #[ignore]
    fn export_wavs() {
        let Some(dir) = std::env::var_os("GLIMMER_WAV_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("create the output directory");
        let write = |name: String, pcm: Vec<f32>| {
            let path = dir.join(format!("{name}.wav"));
            std::fs::write(&path, encode_wav(&pcm, SAMPLE_RATE)).expect("write a WAV file");
        };
        for s in Sfx::ALL {
            write(format!("{s:?}"), render_sfx(s));
        }
        for t in Track::ALL {
            write(format!("{t:?}"), render_track(t));
        }
    }

    /// Prints timing and level statistics for every sound. Run with
    /// `cargo test --release --lib synth::tests::report -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn report() {
        use std::time::Instant;
        let stats = |name: String, f: &dyn Fn() -> Vec<f32>| {
            let t0 = Instant::now();
            let b = f();
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let rms = (b.iter().map(|x| x * x).sum::<f32>() / b.len() as f32).sqrt();
            let zc = b
                .windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count() as f32;
            let bright = zc / secs(&b) / 2.0;
            println!(
                "{name:<12} {:>6.3}s {:>8} smp {ms:>7.1} ms  peak {:.3} rms {rms:.3} \
                 crest {:>5.1}  zc-freq {bright:>6.0} Hz",
                secs(&b),
                b.len(),
                peak_of(&b),
                peak_of(&b) / rms.max(1e-9)
            );
        };
        for s in Sfx::ALL {
            stats(format!("{s:?}"), &|| render_sfx(s));
        }
        for t in Track::ALL {
            stats(format!("{t:?}"), &|| render_track(t));
        }
    }
}
