//! Thin wrapper around [`Audio`] that adds the session mute switch, only
//! forwards music changes when the track actually changes, and can run
//! without any audio at all (screenshot mode).
//!
//! Ambience is forwarded every frame on purpose: the mixer advances its
//! clock-timed fades inside `set_ambience`.

use crate::audio::Audio;
use crate::game::{GameEvent, LoopHint};
use crate::save::Settings;
use crate::synth::{Sfx, Track};

/// Settings volumes are `0..=10`.
const VOLUME_STEPS: f32 = 10.0;

pub struct SoundOut {
    audio: Option<Audio>,
    muted: bool,
    music: Option<Track>,
}

impl SoundOut {
    /// No audio output at all (every call is a no-op).
    pub fn disabled() -> SoundOut {
        SoundOut {
            audio: None,
            muted: true,
            music: None,
        }
    }

    pub fn new(audio: Audio, muted: bool) -> SoundOut {
        SoundOut {
            audio: Some(audio),
            muted,
            music: None,
        }
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    /// Session-only mute (never saved); re-applies the volumes.
    pub fn set_muted(&mut self, muted: bool, settings: &Settings) {
        self.muted = muted;
        self.apply(settings);
    }

    /// Push the settings' volumes (or silence while muted) to the mixer.
    pub fn apply(&mut self, settings: &Settings) {
        let (master, music, sfx) = mixer_volumes(settings, self.muted);
        if let Some(a) = self.audio.as_mut() {
            a.set_volumes(master, music, sfx);
        }
    }

    pub fn play(&mut self, sfx: Sfx) {
        if let Some(a) = self.audio.as_mut() {
            a.play(sfx);
        }
    }

    pub fn event(&mut self, ev: &GameEvent) {
        if let Some(a) = self.audio.as_mut() {
            a.handle_event(ev);
        }
    }

    /// Keep the gameplay loop in sync; call every frame (drives fades).
    pub fn ambience(&mut self, hint: LoopHint) {
        if let Some(a) = self.audio.as_mut() {
            a.set_ambience(hint);
        }
    }

    /// Desired music track; cheap to call every frame.
    pub fn music(&mut self, track: Option<Track>) {
        if track == self.music {
            return;
        }
        self.music = track;
        if let Some(a) = self.audio.as_mut() {
            a.set_music(track);
        }
    }

    /// Silence gameplay sound for the pause menu: the ambience and every
    /// gameplay one-shot and jingle stop; UI blips keep playing.
    pub fn stop_effects(&mut self) {
        if let Some(a) = self.audio.as_mut() {
            a.stop_effects();
        }
    }

    /// Stop everything (loops, music, one-shots).
    pub fn stop_all(&mut self) {
        self.music = None;
        if let Some(a) = self.audio.as_mut() {
            a.stop_all();
        }
    }
}

/// `(master, music, sfx)` mixer gains in `[0, 1]` for `settings`.
pub fn mixer_volumes(settings: &Settings, muted: bool) -> (f32, f32, f32) {
    let s = settings.clamped();
    let v = |x: u8| (f32::from(x) / VOLUME_STEPS).clamp(0.0, 1.0);
    let master = if muted { 0.0 } else { v(s.master_volume) };
    (master, v(s.music_volume), v(s.sfx_volume))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumes_are_normalised_and_mute_silences_master() {
        let s = Settings {
            master_volume: 10,
            music_volume: 5,
            sfx_volume: 0,
            ..Settings::default()
        };
        assert_eq!(mixer_volumes(&s, false), (1.0, 0.5, 0.0));
        assert_eq!(mixer_volumes(&s, true).0, 0.0);
        let wild = Settings {
            master_volume: 200,
            ..Settings::default()
        };
        assert_eq!(mixer_volumes(&wild, false).0, 1.0);
    }

    #[test]
    fn disabled_output_ignores_everything() {
        let mut s = SoundOut::disabled();
        s.play(Sfx::MenuMove);
        s.music(Some(Track::Title));
        s.ambience(LoopHint::Fright);
        s.event(&GameEvent::PlayStarted);
        s.stop_effects();
        s.stop_all();
        assert!(s.muted());
    }
}
