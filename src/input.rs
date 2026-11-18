//! Keyboard input mapping (arrows/WASD to move; Enter/Space confirm; Esc/P
//! pause/back; M mute; F11 or Alt+Enter fullscreen (also Ctrl+Cmd+F on
//! macOS); F3 debug overlay).
//!
//! [`Input::poll`] samples macroquad's keyboard and mouse state once per
//! frame and condenses it into [`Actions`] (what the menus and global
//! shortcuts care about) plus the steering direction for the game
//! ([`Input::game_dir`]).
//!
//! # Window events
//! Typed characters and focus changes come from a macroquad *input
//! subscriber* (the raw miniquad event stream) rather than the plain
//! character queue, because only the event stream says whether a character
//! is an OS auto-repeat (a held key must not type `AAAA` or a row of blanks
//! into a name) and whether the window lost focus. miniquad 0.4 reports focus
//! loss through `window_minimized_event` (macOS: the window resigned key
//! status or was moved; Windows/X11: focus change); it has no separate
//! minimise notification, so a window without a drawable area also counts as
//! [`Actions::focus_lost`].
//!
//! # Steering model
//! Every movement key that is currently held sits on a small stack; the most
//! recently pressed one wins, so rolling from one arrow onto the next feels
//! natural and releasing the newer key falls back to the older one. A key that
//! was pressed *and released within one frame* is remembered as a tap until a
//! simulation tick has consumed it ([`Input::consume_tap`]), so very short
//! presses are never lost even when a frame runs no tick.
//!
//! The pure building blocks ([`DirStack`], [`KeyRepeat`]) hold no macroquad
//! state and are unit-tested.

use crate::geom::Dir;
use macroquad::input::utils::{register_input_subscriber, repeat_all_miniquad_input};
use macroquad::input::{
    get_char_pressed, get_last_key_pressed, is_key_down, is_key_pressed, is_key_released,
    is_mouse_button_pressed, mouse_position, KeyCode, MouseButton,
};
use macroquad::math::Vec2;
use macroquad::miniquad::{EventHandler, KeyMods};
use macroquad::window::{screen_height, screen_width};

/// Seconds a navigation key must be held before it starts repeating.
pub const REPEAT_DELAY: f32 = 0.35;
/// Seconds between repeats once repeating.
pub const REPEAT_RATE: f32 = 0.08;

/// Movement keys and the direction each one steers. Index = key id used by
/// [`DirStack`].
const MOVE_KEYS: [(KeyCode, Dir); 8] = [
    (KeyCode::Up, Dir::Up),
    (KeyCode::Left, Dir::Left),
    (KeyCode::Down, Dir::Down),
    (KeyCode::Right, Dir::Right),
    (KeyCode::W, Dir::Up),
    (KeyCode::A, Dir::Left),
    (KeyCode::S, Dir::Down),
    (KeyCode::D, Dir::Right),
];

/// Arrow keys in navigation order (up, down, left, right).
const ARROW_NAV: [KeyCode; 4] = [KeyCode::Up, KeyCode::Down, KeyCode::Left, KeyCode::Right];
/// WASD in navigation order (up, down, left, right).
const WASD_NAV: [KeyCode; 4] = [KeyCode::W, KeyCode::S, KeyCode::A, KeyCode::D];

/// Most characters buffered from the text-input queue per frame.
const MAX_CHARS: usize = 16;

/// Mouse movement (window pixels) below which the pointer counts as still.
const MOUSE_EPSILON: f32 = 0.5;

/// Ordered set of held movement keys, most recently pressed last.
///
/// Fixed capacity, no allocation. Key ids are small integers (indices into
/// the movement key table); ids `>= CAP` are ignored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirStack {
    keys: [u8; DirStack::CAP],
    len: usize,
}

impl DirStack {
    /// Maximum number of simultaneously tracked keys.
    pub const CAP: usize = 8;

    pub const fn new() -> DirStack {
        DirStack {
            keys: [0; DirStack::CAP],
            len: 0,
        }
    }

    /// `key` went down: it becomes the most recent entry (moved to the top if
    /// it was already present).
    pub fn press(&mut self, key: u8) {
        if usize::from(key) >= Self::CAP {
            return;
        }
        self.release(key);
        if self.len < Self::CAP {
            self.keys[self.len] = key;
            self.len += 1;
        }
    }

    /// `key` went up: remove it, keeping the order of the others.
    pub fn release(&mut self, key: u8) {
        if let Some(i) = self.as_slice().iter().position(|&k| k == key) {
            self.keys.copy_within(i + 1..self.len, i);
            self.len -= 1;
        }
    }

    /// Keep only the keys for which `held` returns true (drops keys whose
    /// release event was missed, e.g. after the window lost focus).
    pub fn retain(&mut self, held: impl Fn(u8) -> bool) {
        let mut n = 0;
        for i in 0..self.len {
            let k = self.keys[i];
            if held(k) {
                self.keys[n] = k;
                n += 1;
            }
        }
        self.len = n;
    }

    /// The most recently pressed key still held.
    pub fn top(&self) -> Option<u8> {
        self.as_slice().last().copied()
    }

    fn as_slice(&self) -> &[u8] {
        &self.keys[..self.len]
    }
}

/// Auto-repeat for one navigation key: fires on the initial press, then after
/// [`REPEAT_DELAY`] every [`REPEAT_RATE`] seconds while held.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KeyRepeat {
    held: bool,
    timer: f32,
}

impl KeyRepeat {
    pub const fn new() -> KeyRepeat {
        KeyRepeat {
            held: false,
            timer: 0.0,
        }
    }

    /// Feed one frame. `pressed`: the key went down this frame; `down`: it is
    /// held now; `dt`: frame time in seconds. Returns whether the action fires
    /// this frame (at most once per frame, so a long hitch never bursts).
    pub fn update(&mut self, pressed: bool, down: bool, dt: f32) -> bool {
        if pressed {
            self.held = down;
            self.timer = REPEAT_DELAY;
            return true;
        }
        if !down || !self.held {
            self.held = false;
            return false;
        }
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.timer -= dt;
        if self.timer <= 0.0 {
            // Re-arm relative to the overshoot, but never schedule in the past.
            self.timer = (self.timer + REPEAT_RATE).max(REPEAT_RATE * 0.25);
            true
        } else {
            false
        }
    }

    /// Forget any held state (e.g. when a screen consumes the key).
    pub fn reset(&mut self) {
        *self = KeyRepeat::new();
    }
}

/// Everything the menus and global shortcuts need from one frame of input.
/// All flags are edge-triggered (true for one frame) unless noted.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Actions {
    /// Navigation from arrows **or** WASD, with key repeat.
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Navigation from the arrow keys only, with key repeat (name entry,
    /// where letters are typed).
    pub arrow_up: bool,
    pub arrow_down: bool,
    pub arrow_left: bool,
    pub arrow_right: bool,
    /// Enter / keypad Enter / Space (not Alt+Enter).
    pub confirm: bool,
    /// Enter / keypad Enter only (not Alt+Enter).
    pub enter: bool,
    /// Escape or Backspace.
    pub back: bool,
    pub escape: bool,
    pub backspace: bool,
    /// Escape or P.
    pub pause: bool,
    /// F11 or Alt+Enter (also Ctrl+Cmd+F on macOS).
    pub fullscreen: bool,
    /// M.
    pub mute: bool,
    /// F3.
    pub debug: bool,
    /// Q.
    pub quit: bool,
    /// Any key went down this frame (or a mouse button was clicked).
    pub any: bool,
    /// Left mouse button went down this frame.
    pub click: bool,
    /// The pointer moved this frame.
    pub mouse_moved: bool,
    /// The window lost keyboard focus, was minimised or has no drawable area
    /// this frame (auto-pause). Not set by any key.
    pub focus_lost: bool,
}

/// Collects what the menus need from the raw window event stream: typed
/// characters (OS auto-repeats and shortcut chords dropped) and focus loss.
/// Pure: unit-tested by calling the [`EventHandler`] methods directly.
#[derive(Clone, Copy, Debug)]
pub struct EventSink {
    chars: [char; MAX_CHARS],
    n_chars: usize,
    /// A focus-loss / minimise event arrived.
    pub focus_lost: bool,
}

impl Default for EventSink {
    fn default() -> Self {
        EventSink::new()
    }
}

impl EventSink {
    pub const fn new() -> EventSink {
        EventSink {
            chars: ['\0'; MAX_CHARS],
            n_chars: 0,
            focus_lost: false,
        }
    }

    /// Characters collected since the last [`EventSink::clear`].
    pub fn chars(&self) -> &[char] {
        &self.chars[..self.n_chars]
    }

    pub fn clear(&mut self) {
        self.n_chars = 0;
        self.focus_lost = false;
    }
}

impl EventHandler for EventSink {
    fn update(&mut self) {}

    fn draw(&mut self) {}

    fn char_event(&mut self, character: char, keymods: KeyMods, repeat: bool) {
        // A held key repeats; Ctrl/Cmd chords are shortcuts, not text.
        if repeat || keymods.ctrl || keymods.logo || character.is_control() {
            return;
        }
        if self.n_chars < MAX_CHARS {
            self.chars[self.n_chars] = character;
            self.n_chars += 1;
        }
    }

    fn window_minimized_event(&mut self) {
        self.focus_lost = true;
    }
}

/// Per-frame input sampler. See the module docs.
#[derive(Clone, Debug)]
pub struct Input {
    /// This frame's actions.
    pub act: Actions,
    /// Pointer position in window (logical) pixels.
    pub mouse: Vec2,
    stack: DirStack,
    tap: Option<Dir>,
    arrow_rep: [KeyRepeat; 4],
    wasd_rep: [KeyRepeat; 4],
    /// This frame's window events (typed characters, focus loss).
    events: EventSink,
    /// macroquad input-subscriber id, registered on the first [`Input::poll`]
    /// (registration needs the window, so tests never touch it).
    subscriber: Option<usize>,
    last_mouse: Option<Vec2>,
}

impl Default for Input {
    fn default() -> Self {
        Input::new()
    }
}

impl Input {
    pub fn new() -> Input {
        Input {
            act: Actions::default(),
            mouse: Vec2::ZERO,
            stack: DirStack::new(),
            tap: None,
            arrow_rep: [KeyRepeat::new(); 4],
            wasd_rep: [KeyRepeat::new(); 4],
            events: EventSink::new(),
            subscriber: None,
            last_mouse: None,
        }
    }

    /// Sample keyboard and mouse. Call exactly once per frame, before any
    /// screen logic. `dt` is the frame time (drives key repeat).
    pub fn poll(&mut self, dt: f32) {
        self.poll_movement();

        let mut act = Actions::default();
        let mut nav = [false; 4];
        let mut arrows = [false; 4];
        for i in 0..4 {
            let a = ARROW_NAV[i];
            let w = WASD_NAV[i];
            arrows[i] = self.arrow_rep[i].update(is_key_pressed(a), is_key_down(a), dt);
            let wasd = self.wasd_rep[i].update(is_key_pressed(w), is_key_down(w), dt);
            nav[i] = arrows[i] || wasd;
        }
        [act.up, act.down, act.left, act.right] = nav;
        [
            act.arrow_up,
            act.arrow_down,
            act.arrow_left,
            act.arrow_right,
        ] = arrows;

        let alt = is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt);
        let enter_pressed = is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter);
        // macOS standard "Enter Full Screen" chord.
        let mac_chord = cfg!(target_os = "macos")
            && (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl))
            && (is_key_down(KeyCode::LeftSuper) || is_key_down(KeyCode::RightSuper))
            && is_key_pressed(KeyCode::F);
        act.fullscreen = is_key_pressed(KeyCode::F11) || (alt && enter_pressed) || mac_chord;
        act.enter = enter_pressed && !alt;
        act.confirm = act.enter || is_key_pressed(KeyCode::Space);
        act.escape = is_key_pressed(KeyCode::Escape);
        act.backspace = is_key_pressed(KeyCode::Backspace);
        act.back = act.escape || act.backspace;
        act.pause = act.escape || is_key_pressed(KeyCode::P);
        act.mute = is_key_pressed(KeyCode::M);
        act.debug = is_key_pressed(KeyCode::F3);
        act.quit = is_key_pressed(KeyCode::Q);

        // Mouse.
        let (mx, my) = mouse_position();
        let pos = if mx.is_finite() && my.is_finite() {
            Vec2::new(mx, my)
        } else {
            self.mouse
        };
        act.mouse_moved = self
            .last_mouse
            .is_some_and(|last| (pos - last).length_squared() > MOUSE_EPSILON * MOUSE_EPSILON);
        self.last_mouse = Some(pos);
        self.mouse = pos;
        act.click = is_mouse_button_pressed(MouseButton::Left);
        act.any = get_last_key_pressed().is_some() || act.click;

        // Text and focus come from the event stream (see the module docs).
        // The plain character queue is still drained so it never grows.
        self.events.clear();
        let id = *self
            .subscriber
            .get_or_insert_with(register_input_subscriber);
        repeat_all_miniquad_input(&mut self.events, id);
        while get_char_pressed().is_some() {}
        let (w, h) = (screen_width(), screen_height());
        act.focus_lost = self.events.focus_lost || !(w > 0.0 && h > 0.0);

        self.act = act;
    }

    fn poll_movement(&mut self) {
        for (i, &(key, dir)) in MOVE_KEYS.iter().enumerate() {
            // `i < MOVE_KEYS.len() == DirStack::CAP`, so it always fits a u8.
            let id = i as u8;
            if is_key_pressed(key) {
                if is_key_down(key) {
                    self.stack.press(id);
                    self.tap = None;
                } else {
                    // Pressed and released within this frame.
                    self.tap = Some(dir);
                }
            }
            if is_key_released(key) {
                self.stack.release(id);
            }
        }
        self.stack.retain(|id| {
            MOVE_KEYS
                .get(usize::from(id))
                .is_some_and(|&(k, _)| is_key_down(k))
        });
    }

    /// Direction to feed the simulation: an unconsumed tap (the most recent
    /// input) or else the most recently pressed held key.
    pub fn game_dir(&self) -> Option<Dir> {
        self.tap.or_else(|| {
            self.stack
                .top()
                .and_then(|id| MOVE_KEYS.get(usize::from(id)))
                .map(|&(_, d)| d)
        })
    }

    /// A simulation tick has seen the current tap; forget it.
    pub fn consume_tap(&mut self) {
        self.tap = None;
    }

    /// Characters typed this frame (text-input order). OS key repeats and
    /// Ctrl/Cmd chords are left out.
    pub fn chars(&self) -> &[char] {
        self.events.chars()
    }

    /// Forget held/tapped movement and key-repeat state, e.g. on a screen
    /// change, so input from one screen never leaks into the next.
    pub fn reset(&mut self) {
        self.tap = None;
        for r in self.arrow_rep.iter_mut().chain(self.wasd_rep.iter_mut()) {
            r.reset();
        }
        self.act = Actions::default();
        self.events.clear();
    }

    /// Clear this frame's actions without sampling (used by the screenshot
    /// mode, which never reads the real keyboard).
    pub fn clear_frame(&mut self) {
        self.act = Actions::default();
        self.events.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Inspection helpers the game itself never needs.
    impl DirStack {
        fn clear(&mut self) {
            self.len = 0;
        }

        fn len(&self) -> usize {
            self.len
        }

        fn is_empty(&self) -> bool {
            self.len == 0
        }
    }

    #[test]
    fn stack_latest_press_wins_and_release_falls_back() {
        let mut s = DirStack::new();
        assert_eq!(s.top(), None);
        s.press(1);
        s.press(3);
        assert_eq!(s.top(), Some(3));
        s.press(1); // re-press moves to the top without duplicating
        assert_eq!(s.top(), Some(1));
        assert_eq!(s.len(), 2);
        s.release(1);
        assert_eq!(s.top(), Some(3));
        s.release(3);
        assert!(s.is_empty());
        s.release(5); // releasing an absent key is harmless
        assert!(s.is_empty());
    }

    #[test]
    fn stack_retain_and_capacity() {
        let mut s = DirStack::new();
        for k in 0..20u8 {
            s.press(k); // ids >= CAP are ignored
        }
        assert_eq!(s.len(), DirStack::CAP);
        assert_eq!(s.top(), Some(7));
        s.retain(|k| k % 2 == 0);
        assert_eq!(s.len(), 4);
        assert_eq!(s.top(), Some(6));
        s.clear();
        assert_eq!(s.top(), None);
    }

    #[test]
    fn repeat_fires_on_press_then_after_delay_at_rate() {
        let mut r = KeyRepeat::new();
        let dt = 0.01;
        assert!(r.update(true, true, dt));
        let mut fires = 0;
        let mut first_repeat_at = None;
        let mut t = 0.0;
        for _ in 0..100 {
            t += dt;
            if r.update(false, true, dt) {
                fires += 1;
                first_repeat_at.get_or_insert(t);
            }
        }
        let first = first_repeat_at.expect("repeats while held");
        assert!(
            (first - REPEAT_DELAY).abs() < 0.02,
            "first repeat at {first}"
        );
        // Remaining (1.0 - 0.35) s at 0.08 s per repeat is about 8-9 repeats.
        assert!((8..=10).contains(&fires), "fires = {fires}");
        // Releasing stops it.
        assert!(!r.update(false, false, dt));
        assert!(!r.update(false, false, 1.0));
    }

    #[test]
    fn repeat_never_bursts_on_a_hitch() {
        let mut r = KeyRepeat::new();
        assert!(r.update(true, true, 0.016));
        assert!(r.update(false, true, 5.0));
        // After a hitch the next repeat is still spaced out.
        assert!(!r.update(false, true, 0.001));
        assert!(!r.update(false, true, f32::NAN));
    }

    #[test]
    fn tap_within_one_frame_fires_once() {
        let mut r = KeyRepeat::new();
        assert!(r.update(true, false, 0.016));
        assert!(!r.update(false, false, 0.5));
        assert!(!r.update(false, true, 0.5)); // held without a fresh press
    }

    #[test]
    fn move_key_table_fits_stack() {
        assert_eq!(MOVE_KEYS.len(), DirStack::CAP);
    }

    fn mods() -> KeyMods {
        KeyMods {
            shift: false,
            ctrl: false,
            alt: false,
            logo: false,
        }
    }

    #[test]
    fn event_sink_drops_auto_repeats_and_chords() {
        let mut e = EventSink::new();
        e.char_event('a', mods(), false);
        // Holding Space: the OS keeps sending repeated ' ' characters.
        for _ in 0..5 {
            e.char_event(' ', mods(), true);
        }
        e.char_event(
            'B',
            KeyMods {
                shift: true,
                ..mods()
            },
            false,
        );
        e.char_event(
            'f',
            KeyMods {
                ctrl: true,
                logo: true,
                ..mods()
            },
            false,
        );
        e.char_event(
            'q',
            KeyMods {
                logo: true,
                ..mods()
            },
            false,
        );
        e.char_event('\r', mods(), false);
        assert_eq!(e.chars(), &['a', 'B']);
        assert!(!e.focus_lost);
        e.clear();
        assert!(e.chars().is_empty());
    }

    #[test]
    fn event_sink_reports_focus_loss_and_caps_chars() {
        let mut e = EventSink::new();
        e.window_minimized_event();
        assert!(e.focus_lost);
        for _ in 0..MAX_CHARS * 2 {
            e.char_event('x', mods(), false);
        }
        assert_eq!(e.chars().len(), MAX_CHARS);
        e.clear();
        assert!(!e.focus_lost);
    }

    #[test]
    fn input_reset_forgets_typed_text() {
        let mut i = Input::new();
        i.events.char_event('z', mods(), false);
        assert_eq!(i.chars(), &['z']);
        i.reset();
        assert!(i.chars().is_empty());
        i.events.char_event('z', mods(), false);
        i.clear_frame();
        assert!(i.chars().is_empty() && !i.act.focus_lost);
    }
}
