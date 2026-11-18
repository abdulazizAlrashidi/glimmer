//! Interludes: short comedic vignettes shown between some levels.
//!
//! Each scene is a pure function of its clock ([`Interlude::props`]) that
//! yields the actors to draw this frame, so scenes are deterministic,
//! scrubbable (the screenshot mode jumps straight to mid-scene) and testable
//! without a window. Actors enter and leave through two glowing portals at the
//! ends of a neon corridor and shrink into them, so nothing is ever drawn
//! outside the canvas. The two portals are linked, like the maze tunnels:
//! whatever dives into one comes out of the other.
//!
//! The on-screen heading is "INTERLUDE". Every scene lasts [`MUSIC_LOOP`]
//! seconds: the interlude music is a loop of exactly that length and fades
//! out when the scene ends, so it always stops at the end of its phrase.
//!
//! Scenes:
//! 0. The Dance Lesson: Glim shows the Gloom a firefly dance (hop, hop,
//!    spin). The four shades copy it a beat out of step, then try Glim's
//!    finale, a burst of light, and only manage a fizzle that leaves them
//!    seeing stars. Glim bows out one way, the embarrassed shades scurry off
//!    the other.
//! 1. A Portal Mix-up: Glim and Moss reach for the same relic, bump heads
//!    and bounce back into their own portals, which spit each of them out of
//!    the *other* side. Moss charges again, Glim simply hops over it, Moss
//!    shoots through the portals and comes round for another pass while Glim
//!    pockets the relic and hops over it once more.
//! 2. Lights Out: the corridor goes dark and Glim tiptoes in by its own glow
//!    while pairs of eyes blink open all around. Glim flashes its light: it
//!    is a surprise party, relic cake included. Glim eats the cake and the
//!    whole party conga-lines out.

use super::ui::{self, pal};
use crate::config::{BonusKind, INTERLUDE_VARIANTS};
use crate::font::Align;
use crate::geom::Dir;
use crate::render::sprites;
use crate::shade::{ShadeKind, ShadeLook};
use crate::view::Layout;
use macroquad::math::Rect;
#[cfg(test)]
use macroquad::math::Vec2;
use macroquad::shapes::{draw_circle, draw_circle_lines, draw_ellipse, draw_ellipse_lines};
use std::f32::consts::{PI, TAU};

/// Lane centre of the corridor (canvas tiles).
const STAGE_Y: f32 = 18.0;
/// Corridor half-height (rails), tiles.
const RAIL: f32 = 2.5;
/// Portal x positions (canvas tiles).
const GATE_L: f32 = 1.0;
const GATE_R: f32 = 26.0;
/// Base sprite radii in tiles.
const GLIM_R: f32 = 0.8;
const SHADE_R: f32 = 0.88;
const RELIC_R: f32 = 0.72;
/// Lifetime of a portal flash, seconds.
const FLASH_LIFE: f32 = 0.5;
/// Lifetime of a smoke puff, seconds.
const PUFF_LIFE: f32 = 0.7;
/// Parking spot for actors that have left through the right portal.
const OFFSTAGE: f32 = 28.5;
/// Seconds before the skip hint appears.
const HINT_DELAY: f32 = 0.8;
/// Length of every scene: one pass of the interlude music loop.
pub const MUSIC_LOOP: f32 = 8.0;
/// The heading above every scene.
pub const HEADING: &str = "INTERLUDE";

/// Most actors any scene shows at once.
const MAX_PROPS: usize = 16;

/// One drawable element of a scene, positioned in canvas tiles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Prop {
    Glim {
        x: f32,
        y: f32,
        scale: f32,
        dir: Dir,
    },
    Shade {
        kind: ShadeKind,
        x: f32,
        y: f32,
        look: ShadeLook,
        dir: Dir,
        /// Size factor (eyes popping open in the dark start small).
        scale: f32,
    },
    Relic {
        kind: BonusKind,
        x: f32,
        y: f32,
    },
    /// Stars orbiting a dazed head.
    Dizzy {
        x: f32,
        y: f32,
        strength: f32,
    },
    /// A floating score label, `age` seconds old.
    Points {
        x: f32,
        y: f32,
        value: u32,
        age: f32,
    },
    /// An expanding flash ring, `age` seconds old.
    Burst {
        x: f32,
        y: f32,
        age: f32,
    },
    /// A pair of eyes blinking in the dark (a shade with the lights off).
    /// `look` (-1..1) shifts the pupils left/right.
    Peepers {
        kind: ShadeKind,
        x: f32,
        y: f32,
        scale: f32,
        look: f32,
    },
    /// A portal flaring as something comes out of it, `age` seconds old.
    PortalFlash {
        x: f32,
        age: f32,
    },
    /// A small grey puff of smoke (a failed attempt to glow), `age` s old.
    Puff {
        x: f32,
        y: f32,
        age: f32,
    },
    /// A soft halo of light (Glim's glow), radius `r` tiles.
    Glow {
        x: f32,
        y: f32,
        r: f32,
        strength: f32,
    },
    /// A shouted caption that pops in and fades out over `life` seconds.
    Label {
        text: &'static str,
        x: f32,
        y: f32,
        age: f32,
        life: f32,
    },
}

/// Fixed-capacity list of props (no allocation).
#[derive(Clone, Copy, Debug, Default)]
pub struct Props {
    items: [Option<Prop>; MAX_PROPS],
    len: usize,
}

impl Props {
    fn push(&mut self, p: Prop) {
        if self.len < MAX_PROPS {
            self.items[self.len] = Some(p);
            self.len += 1;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Prop> {
        self.items[..self.len].iter().flatten()
    }
}

/// A running interlude scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interlude {
    pub variant: u8,
    /// Seconds since the scene started.
    pub t: f32,
}

impl Interlude {
    pub fn new(variant: u8) -> Interlude {
        Interlude {
            variant: variant % INTERLUDE_VARIANTS.max(1),
            t: 0.0,
        }
    }

    /// Length of a scene in seconds (every scene is timed to one pass of
    /// the music loop, see [`MUSIC_LOOP`]).
    pub fn duration(_variant: u8) -> f32 {
        MUSIC_LOOP
    }

    /// Scene name shown under the [`HEADING`].
    pub fn title(variant: u8) -> &'static str {
        match variant % INTERLUDE_VARIANTS.max(1) {
            0 => "THE DANCE LESSON",
            1 => "A PORTAL MIX-UP",
            _ => "LIGHTS OUT",
        }
    }

    /// Stage lighting at the current time: 1 = lit, 0 = dark.
    pub fn light(&self) -> f32 {
        match self.variant {
            2 => lights_level(self.t.max(0.0)),
            _ => 1.0,
        }
    }

    pub fn advance(&mut self, dt: f32) {
        if dt.is_finite() && dt > 0.0 {
            self.t += dt;
        }
    }

    pub fn finished(&self) -> bool {
        self.t >= Self::duration(self.variant)
    }

    /// Actors to draw at the current time, back to front.
    pub fn props(&self) -> Props {
        let mut p = Props::default();
        let t = self.t.max(0.0);
        match self.variant {
            0 => dance_lesson(t, &mut p),
            1 => portal_mixup(t, &mut p),
            _ => lights_out(t, &mut p),
        }
        p
    }

    /// Draw the whole scene (the caller draws the backdrop first).
    pub fn draw(&self, l: &Layout) {
        let t = self.t;
        let dark = 1.0 - self.light();
        draw_stage(l, t);
        if dark > 0.0 {
            ui::dim_screen(l, 0.8 * dark);
        }
        let intro = ui::ease_out(ui::progress(t, 0.0, 0.6));
        ui::text_glow(
            l,
            HEADING,
            ui::CX,
            7.2,
            ui::H1,
            ui::alpha(pal::VIOLET, intro),
            Align::Center,
        );
        ui::text_glow(
            l,
            Self::title(self.variant),
            ui::CX,
            9.6,
            ui::BODY,
            ui::alpha(pal::AMBER, ui::progress(t, 0.3, 0.6)),
            Align::Center,
        );
        for prop in self.props().iter() {
            draw_prop(l, prop, t);
        }
        let hint = ui::progress(t, HINT_DELAY, 0.5);
        if hint > 0.0 {
            ui::text(
                l,
                "ENTER - SKIP",
                ui::CX,
                30.5,
                ui::SMALL,
                ui::alpha(pal::DIM, hint),
                Align::Center,
            );
        }
    }
}

/// Piecewise-linear interpolation through `(time, value)` keys, clamped at
/// both ends.
fn track(t: f32, keys: &[(f32, f32)]) -> f32 {
    let Some(&(t_first, v_first)) = keys.first() else {
        return 0.0;
    };
    if t <= t_first {
        return v_first;
    }
    for w in keys.windows(2) {
        let (t0, v0) = w[0];
        let (t1, v1) = w[1];
        if t <= t1 {
            let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
            return v0 + (v1 - v0) * f;
        }
    }
    keys.last().map_or(v_first, |k| k.1)
}

/// Gentle hover bob.
fn bob(t: f32, phase: f32) -> f32 {
    (t * 6.0 + phase).sin() * 0.08
}

/// Size factor of an actor near the portals: 0 at/through a gate, 1 inside.
fn gate_fade(x: f32, scale: f32) -> f32 {
    let len = 1.4 * scale.max(0.1);
    ((x - GATE_L).min(GATE_R - x) / len).clamp(0.0, 1.0)
}

// ----- scene scripts ---------------------------------------------------------

/// Ease-out curve shared by the scene scripts.
fn ease(x: f32) -> f32 {
    ui::ease_out(x)
}

/// Parabolic hop of height `h` between `t0` and `t1` (0 outside).
fn hop(t: f32, t0: f32, t1: f32, h: f32) -> f32 {
    if t <= t0 || t >= t1 || t1 <= t0 {
        0.0
    } else {
        -h * (((t - t0) / (t1 - t0)) * PI).sin()
    }
}

/// Facing that cycles Up -> Right -> Down -> Left `rate` times a second
/// (`reverse` spins the other way).
fn spin(t: f32, rate: f32, reverse: bool) -> Dir {
    const CW: [Dir; 4] = [Dir::Up, Dir::Right, Dir::Down, Dir::Left];
    let i = ((t.max(0.0) * rate) as usize) % CW.len();
    if reverse {
        CW[CW.len() - 1 - i]
    } else {
        CW[i]
    }
}

fn shade(p: &mut Props, kind: ShadeKind, x: f32, y: f32, look: ShadeLook, dir: Dir, scale: f32) {
    p.push(Prop::Shade {
        kind,
        x,
        y,
        look,
        dir,
        scale,
    });
}

fn glim(p: &mut Props, x: f32, y: f32, scale: f32, dir: Dir) {
    p.push(Prop::Glim { x, y, scale, dir });
}

// --- 0. The Dance Lesson -----------------------------------------------------

/// Where the pupils stand.
const LESSON_GLIM_X: f32 = 8.0;
const LESSON_SHADE_X0: f32 = 13.0;
const LESSON_SHADE_DX: f32 = 2.6;
/// Glim's demonstration, the shades' copy, the finale and the fizzle.
const LESSON_DEMO: f32 = 2.4;
const LESSON_COPY: f32 = 3.9;
const LESSON_COPY_STAGGER: f32 = 0.12;
/// Moss misses the cue and starts this much later than the others.
const LESSON_MOSS_LATE: f32 = 0.3;
const LESSON_GLOW: f32 = 5.4;
const LESSON_FIZZLE: f32 = 6.5;
/// Length of the dance routine.
const ROUTINE: f32 = 1.4;

/// The dance routine at local time `u` (0..ROUTINE): vertical offset and an
/// optional facing override. `reverse` spins the wrong way round.
fn routine(u: f32, facing: Dir, reverse: bool) -> (f32, Dir) {
    if !(0.0..ROUTINE).contains(&u) {
        return (0.0, facing);
    }
    let dy = hop(u, 0.0, 0.35, 0.9) + hop(u, 0.35, 0.7, 0.9);
    let dir = if (0.7..1.2).contains(&u) {
        spin(u - 0.7, 12.0, reverse)
    } else {
        facing
    };
    (dy, dir)
}

fn dance_lesson(t: f32, p: &mut Props) {
    let y = STAGE_Y;
    // The four pupils: in from the right portal, then the copy, the fizzle
    // and a hasty exit back through the same portal.
    for (k, &kind) in ShadeKind::ALL.iter().enumerate() {
        let kf = k as f32;
        let home = LESSON_SHADE_X0 + kf * LESSON_SHADE_DX;
        let enter = 0.6 + 0.25 * kf;
        if t < enter {
            continue;
        }
        let leave = 7.0 + 0.05 * kf;
        let mut x = track(t, &[(enter, 26.5), (2.2, home), (leave, home), (7.9, 26.5)]);
        let mut sy = y + bob(t, 1.3 + kf);
        let mut dir = if t < leave { Dir::Left } else { Dir::Right };
        let late = if kind == ShadeKind::Moss {
            LESSON_MOSS_LATE
        } else {
            0.0
        };
        let (dy, d) = routine(
            t - LESSON_COPY - kf * LESSON_COPY_STAGGER - late,
            dir,
            kind == ShadeKind::Moss,
        );
        sy += dy;
        dir = d;
        // Straining to glow, then the fizzle.
        if (LESSON_GLOW + 0.6..LESSON_FIZZLE).contains(&t) {
            x += (t * 47.0 + kf).sin() * 0.1;
        }
        let dazed = ui::progress(t, LESSON_FIZZLE, 0.2) * (1.0 - ui::progress(t, 7.3, 0.4));
        sy += 0.35 * dazed;
        shade(p, kind, x, sy, ShadeLook::Normal, dir, 1.0);
        if dazed > 0.0 && t < leave {
            p.push(Prop::Dizzy {
                x,
                y: sy - 1.25,
                strength: dazed,
            });
        }
        if (LESSON_FIZZLE..LESSON_FIZZLE + PUFF_LIFE).contains(&t) {
            p.push(Prop::Puff {
                x,
                y: sy - 0.5,
                age: t - LESSON_FIZZLE,
            });
        }
    }

    // Glim: in, a polite hop, the demonstration, the finale, a bow, out.
    let gx = track(
        t,
        &[
            (0.0, 0.5),
            (1.4, LESSON_GLIM_X),
            (6.7, LESSON_GLIM_X),
            (7.7, 0.5),
        ],
    );
    let leaving = t >= 6.7;
    let facing = if leaving { Dir::Left } else { Dir::Right };
    let (dy, dir) = routine(t - LESSON_DEMO, facing, false);
    let flash =
        ui::progress(t, LESSON_GLOW, 0.25) * (1.0 - ui::progress(t, LESSON_GLOW + 0.4, 0.4));
    let gy = y + bob(t, 0.0) + dy + hop(t, 2.0, 2.25, 0.4) + hop(t, 6.3, 6.6, 0.5);
    if flash > 0.0 {
        p.push(Prop::Glow {
            x: gx,
            y: gy,
            r: 2.0 + 2.5 * flash,
            strength: flash,
        });
    }
    glim(p, gx, gy, 1.0 + 0.2 * flash, dir);
    if (LESSON_GLOW..LESSON_GLOW + 0.6).contains(&t) {
        p.push(Prop::Burst {
            x: gx,
            y: gy,
            age: t - LESSON_GLOW,
        });
    }
}

// --- 1. A Portal Mix-up ------------------------------------------------------

const MIXUP_RELIC: BonusKind = BonusKind::Lotus;
const MIXUP_RELIC_X: f32 = 13.5;
/// The two lunge into each other here.
const BUMP: f32 = 2.8;
/// Both come back out of the opposite portals.
const SWAP_OUT: f32 = 3.9;
/// Moss charges again.
const CHARGE: f32 = 5.5;
/// Moss's charging speed (tiles per second).
const CHARGE_SPEED: f32 = 22.0;
/// Moss is in transit between the portals for this long.
const PORTAL_TRANSIT: f32 = 0.25;
/// Where Glim waits after the swap.
const MIXUP_GLIM_WAIT: f32 = 19.5;
const MIXUP_MOSS_WAIT: f32 = 7.5;

/// Moss's x during its charge (and second pass after looping through the
/// portals), or `None` while it is inside the portals.
fn mixup_charge_x(t: f32) -> Option<f32> {
    let first_out = CHARGE + (GATE_R + 0.5 - MIXUP_MOSS_WAIT) / CHARGE_SPEED;
    let second_in = first_out + PORTAL_TRANSIT;
    if t < first_out {
        Some(MIXUP_MOSS_WAIT + CHARGE_SPEED * (t - CHARGE).max(0.0))
    } else if t < second_in {
        None
    } else {
        Some((GATE_L - 0.5 + CHARGE_SPEED * (t - second_in)).min(OFFSTAGE))
    }
}

/// Time at which Moss's charge passes canvas x `x` on pass `pass` (0 or 1).
fn mixup_pass_time(x: f32, pass: u32) -> f32 {
    let first_out = CHARGE + (GATE_R + 0.5 - MIXUP_MOSS_WAIT) / CHARGE_SPEED;
    if pass == 0 {
        CHARGE + (x - MIXUP_MOSS_WAIT) / CHARGE_SPEED
    } else {
        first_out + PORTAL_TRANSIT + (x - (GATE_L - 0.5)) / CHARGE_SPEED
    }
}

fn portal_mixup(t: f32, p: &mut Props) {
    let y = STAGE_Y;
    let hop1 = mixup_pass_time(MIXUP_GLIM_WAIT, 0);
    let eat = hop1 + 0.25 + 0.45;
    let hop2 = mixup_pass_time(MIXUP_RELIC_X, 1);

    // The relic: knocked into the air by the bump, then eaten.
    if t < eat {
        let ry = y - 1.6 * (ui::progress(t, BUMP, 0.8) * PI).sin();
        p.push(Prop::Relic {
            kind: MIXUP_RELIC,
            x: MIXUP_RELIC_X,
            y: ry + bob(t, 0.7) * 0.5,
        });
    }

    // Moss.
    let dazed = ui::progress(t, SWAP_OUT + 0.4, 0.2) * (1.0 - ui::progress(t, 5.0, 0.3));
    let (mx, my, mdir) = if t < BUMP {
        let x = track(
            t,
            &[
                (0.4, 26.5),
                (2.0, 15.6),
                (2.25, 15.6),
                (2.5, 16.2),
                (BUMP, 14.3),
            ],
        );
        (x, y + bob(t, 1.1), Dir::Left)
    } else if t < SWAP_OUT {
        // Bounced back into the right portal, spinning; out of the left one.
        let x = track(
            t,
            &[(BUMP, 14.3), (3.5, GATE_R + 0.5), (SWAP_OUT, GATE_R + 0.5)],
        );
        let x = if t >= 3.5 { OFFSTAGE } else { x };
        (x, y + (t * 37.0).sin() * 0.12, spin(t, 14.0, false))
    } else if t < CHARGE {
        let k = ease(ui::progress(t, SWAP_OUT, 0.6));
        let mut x = GATE_L - 0.5 + (MIXUP_MOSS_WAIT - GATE_L + 0.5) * k;
        // Wind-up before the charge.
        x -= 0.5 * ui::progress(t, 5.25, 0.2);
        let dir = if t < SWAP_OUT + 0.4 {
            spin(t, 14.0, false)
        } else if (5.0..5.25).contains(&t) {
            if ((t - 5.0) / 0.08) as i32 % 2 == 0 {
                Dir::Left
            } else {
                Dir::Right
            }
        } else {
            Dir::Right
        };
        (x, y + bob(t, 1.1) + 0.3 * dazed, dir)
    } else {
        match mixup_charge_x(t) {
            Some(x) => (x, y, Dir::Right),
            None => (OFFSTAGE, y, Dir::Right),
        }
    };
    if t >= 0.4 {
        shade(p, ShadeKind::Moss, mx, my, ShadeLook::Normal, mdir, 1.0);
    }
    if dazed > 0.0 {
        p.push(Prop::Dizzy {
            x: mx,
            y: my - 1.25,
            strength: dazed,
        });
    }

    // Glim.
    let (gx, gy, gdir) = if t < BUMP {
        let x = track(
            t,
            &[
                (0.2, 0.5),
                (2.0, 11.4),
                (2.25, 11.4),
                (2.5, 10.8),
                (BUMP, 12.7),
            ],
        );
        (x, y + bob(t, 0.0), Dir::Right)
    } else if t < SWAP_OUT {
        let x = track(t, &[(BUMP, 12.7), (3.5, GATE_L - 0.5)]);
        let x = if t >= 3.5 { -0.5 } else { x };
        (x, y + (t * 41.0).sin() * 0.12, spin(t, 14.0, true))
    } else if t < hop1 + 0.25 {
        let k = ease(ui::progress(t, SWAP_OUT, 0.6));
        let x = GATE_R + 0.5 - (GATE_R + 0.5 - MIXUP_GLIM_WAIT) * k;
        let dir = if t < SWAP_OUT + 0.4 {
            spin(t, 14.0, true)
        } else {
            Dir::Left
        };
        let hy = hop(t, hop1 - 0.25, hop1 + 0.25, 1.8);
        (x, y + bob(t, 0.0) + 0.3 * dazed + hy, dir)
    } else if t < hop2 + 0.25 {
        let x = track(t, &[(hop1 + 0.25, MIXUP_GLIM_WAIT), (eat, MIXUP_RELIC_X)]);
        let hy = hop(t, hop2 - 0.25, hop2 + 0.25, 1.8);
        (x, y + bob(t, 0.0) + hy, Dir::Left)
    } else {
        let x = track(t, &[(hop2 + 0.25, MIXUP_RELIC_X), (7.95, GATE_L - 0.5)]);
        (x, y + bob(t, 0.0), Dir::Left)
    };
    if t >= 0.2 {
        glim(p, gx, gy, 1.0, gdir);
    }
    if dazed > 0.0 {
        p.push(Prop::Dizzy {
            x: gx,
            y: gy - 1.25,
            strength: dazed,
        });
    }

    // The bump, the two portal exits and the reward.
    if (BUMP..BUMP + 0.6).contains(&t) {
        p.push(Prop::Burst {
            x: MIXUP_RELIC_X,
            y,
            age: t - BUMP,
        });
    }
    if (SWAP_OUT - 0.1..SWAP_OUT + FLASH_LIFE - 0.1).contains(&t) {
        for gate in [GATE_L, GATE_R] {
            p.push(Prop::PortalFlash {
                x: gate,
                age: t - SWAP_OUT + 0.1,
            });
        }
    }
    let first_out = mixup_pass_time(GATE_R + 0.5, 0);
    for (at, gate) in [(first_out, GATE_R), (first_out + PORTAL_TRANSIT, GATE_L)] {
        if (at..at + FLASH_LIFE).contains(&t) {
            p.push(Prop::PortalFlash {
                x: gate,
                age: t - at,
            });
        }
    }
    if (eat..eat + 1.3).contains(&t) {
        p.push(Prop::Points {
            x: MIXUP_RELIC_X,
            y: y - 1.7,
            value: MIXUP_RELIC.points(),
            age: t - eat,
        });
    }
}

// --- 2. Lights Out -------------------------------------------------------------

const LIGHTS_ON: f32 = 4.2;
const PARTY_CAKE: BonusKind = BonusKind::Crown;
const CAKE_X: f32 = 11.5;
const CAKE_EAT: f32 = 5.7;
const CONGA: f32 = 6.0;
const CONGA_SPEED: f32 = 10.5;
const SURPRISE: &str = "SURPRISE!";
/// Where Glim stops in the dark.
const LIGHTS_GLIM_X: f32 = 9.5;

/// Each guest: where its eyes open, when, and where it stands once the
/// lights come on.
const GUESTS: [(ShadeKind, f32, f32, f32, f32); 4] = [
    // kind, eyes x, eyes dy, appear at, party x
    (ShadeKind::Rook, 15.4, -1.0, 2.1, 13.8),
    (ShadeKind::Vex, 18.4, 0.9, 2.5, 16.2),
    (ShadeKind::Flint, 4.6, -0.6, 2.9, 6.6),
    (ShadeKind::Moss, 21.2, -0.2, 3.2, 18.6),
];

/// Stage lighting for the Lights Out scene: 1 lit, 0 dark.
fn lights_level(t: f32) -> f32 {
    if t < LIGHTS_ON {
        1.0 - ease(ui::progress(t, 0.2, 0.6))
    } else {
        ui::progress(t, LIGHTS_ON, 0.12)
    }
}

fn lights_out(t: f32, p: &mut Props) {
    let y = STAGE_Y;
    let lit = t >= LIGHTS_ON;

    // The cake.
    if lit && t < CAKE_EAT {
        p.push(Prop::Relic {
            kind: PARTY_CAKE,
            x: CAKE_X,
            y: y + 0.2 + bob(t, 0.3) * 0.5,
        });
    }

    // The guests: eyes in the dark, then the party, then the conga out.
    for (k, &(kind, ex, edy, appear, party_x)) in GUESTS.iter().enumerate() {
        if t < appear {
            continue;
        }
        let kf = k as f32;
        if !lit {
            // Eyes blink open and creep closer to Glim.
            let pop = ui::ease_back(ui::progress(t, appear, 0.25));
            let creep = ui::progress(t, appear, LIGHTS_ON - appear);
            let x = ex + (party_x - ex) * creep;
            let blink = (t + kf * 0.37) % 1.3 < 0.09 && t > appear + 0.3;
            if !blink {
                p.push(Prop::Peepers {
                    kind,
                    x,
                    y: y + edy + bob(t, kf),
                    scale: pop,
                    look: ((LIGHTS_GLIM_X - x) * 0.5).clamp(-1.0, 1.0),
                });
            }
            continue;
        }
        let conga = (t - CONGA).max(0.0);
        let x = (party_x + CONGA_SPEED * conga).min(OFFSTAGE);
        let sy = y + edy * (1.0 - ui::progress(t, CONGA, 0.6));
        let party = hop(
            t,
            LIGHTS_ON + 0.1 + 0.15 * kf,
            LIGHTS_ON + 0.45 + 0.15 * kf,
            0.7,
        ) + if (LIGHTS_ON + 0.8..CONGA).contains(&t) {
            -0.5 * ((t - LIGHTS_ON) * 7.0 + kf * 1.3).sin().abs()
        } else {
            0.0
        } + if t >= CONGA {
            -0.4 * ((t - CONGA) * 9.0 + kf).sin().abs()
        } else {
            0.0
        };
        // Face Glim during the party, then the way out.
        let dir = if t >= CONGA || party_x < LIGHTS_GLIM_X {
            Dir::Right
        } else {
            Dir::Left
        };
        shade(p, kind, x, sy + party, ShadeLook::Normal, dir, 1.0);
    }

    // Glim.
    let gx = if t < CONGA {
        track(
            t,
            &[
                (0.9, 0.5),
                (2.6, LIGHTS_GLIM_X),
                (5.4, LIGHTS_GLIM_X),
                (CAKE_EAT, CAKE_X),
            ],
        )
    } else {
        (CAKE_X + CONGA_SPEED * (t - CONGA)).min(OFFSTAGE)
    };
    let gdir = if t < 2.9 || t >= CONGA {
        Dir::Right
    } else if t < LIGHTS_ON {
        // Peering left and right into the dark.
        if ((t - 2.9) / 0.35) as i32 % 2 == 0 {
            Dir::Left
        } else {
            Dir::Right
        }
    } else if (LIGHTS_ON + 0.9..5.4).contains(&t) {
        spin(t, 10.0, false)
    } else {
        Dir::Right
    };
    let tremble = if (3.4..LIGHTS_ON).contains(&t) {
        (t * 53.0).sin() * 0.07
    } else {
        0.0
    };
    let jump = hop(t, LIGHTS_ON, LIGHTS_ON + 0.5, 1.5);
    let conga_hop = if t >= CONGA {
        -0.4 * ((t - CONGA) * 9.0 + 4.0).sin().abs()
    } else {
        0.0
    };
    let gy = y + bob(t, 0.0) + jump + conga_hop;
    if !lit && t >= 0.9 {
        p.push(Prop::Glow {
            x: gx,
            y: gy,
            r: 2.6,
            strength: 1.0 - lights_level(t),
        });
    }
    if t >= 0.9 {
        glim(p, gx + tremble, gy, 1.0, gdir);
    }

    if (LIGHTS_ON..LIGHTS_ON + 0.6).contains(&t) {
        p.push(Prop::Burst {
            x: gx,
            y: gy,
            age: t - LIGHTS_ON,
        });
    }
    if (LIGHTS_ON + 0.05..CONGA + 0.4).contains(&t) {
        p.push(Prop::Label {
            text: SURPRISE,
            x: ui::CX,
            y: STAGE_Y - RAIL - 1.5,
            age: t - LIGHTS_ON - 0.05,
            life: CONGA + 0.35 - LIGHTS_ON,
        });
    }
    if (CAKE_EAT..CAKE_EAT + 1.3).contains(&t) {
        p.push(Prop::Points {
            x: CAKE_X,
            y: y - 1.7,
            value: PARTY_CAKE.points(),
            age: t - CAKE_EAT,
        });
    }
}

// ----- drawing -----------------------------------------------------------------

fn draw_stage(l: &Layout, t: f32) {
    let shimmer = 0.75 + 0.25 * ui::pulse(t, 0.35);
    for &dy in &[-RAIL, RAIL] {
        let ry = STAGE_Y + dy;
        ui::line(
            l,
            GATE_L,
            ry,
            GATE_R,
            ry,
            0.34,
            ui::alpha(pal::VIOLET, 0.10 * shimmer),
        );
        ui::line(
            l,
            GATE_L,
            ry,
            GATE_R,
            ry,
            0.16,
            ui::alpha(pal::VIOLET, 0.30 * shimmer),
        );
        ui::line(
            l,
            GATE_L,
            ry,
            GATE_R,
            ry,
            0.06,
            ui::alpha(ui::mix(pal::VIOLET, pal::INK, 0.4), 0.9 * shimmer),
        );
    }
    // Faint lane dashes drifting along the floor.
    let n = 12;
    for i in 0..n {
        let x = GATE_L + 1.0 + ((i as f32 * 2.0 + t * 0.6) % (GATE_R - GATE_L - 2.0));
        ui::line(
            l,
            x,
            STAGE_Y + RAIL - 0.55,
            x + 0.5,
            STAGE_Y + RAIL - 0.55,
            0.05,
            ui::alpha(pal::FAINT, 0.5),
        );
    }
    // Portals.
    for (i, &gx) in [GATE_L, GATE_R].iter().enumerate() {
        let c = l.canvas(gx, STAGE_Y);
        let h = l.px(RAIL * 0.95);
        let w = l.px(0.55 + 0.08 * ui::pulse(t + i as f32 * 0.5, 0.8));
        let th = l.px(0.07).max(1.0);
        draw_ellipse_lines(
            c.x,
            c.y,
            w * 1.6,
            h * 1.04,
            0.0,
            th * 3.0,
            ui::alpha(pal::MINT, 0.10),
        );
        draw_ellipse_lines(c.x, c.y, w, h, 0.0, th, ui::alpha(pal::MINT, 0.85));
        ui::sparkle_field(
            l,
            Rect::new(gx - 0.6, STAGE_Y - RAIL, 1.2, RAIL * 2.0),
            5,
            31 + i as u32,
            t,
            0.8,
        );
    }
}

fn draw_prop(l: &Layout, prop: &Prop, t: f32) {
    match *prop {
        Prop::Glim { x, y, scale, dir } => {
            let f = gate_fade(x, scale);
            if f > 0.0 {
                sprites::draw_glim(l.canvas(x, y), l.px(GLIM_R * scale * f), dir, t, 1.0);
            }
        }
        Prop::Shade {
            kind,
            x,
            y,
            look,
            dir,
            scale,
        } => {
            let f = gate_fade(x, 1.0) * scale.max(0.0);
            if f > 0.0 {
                sprites::draw_shade(l.canvas(x, y), l.px(SHADE_R * f), kind, look, dir, t);
            }
        }
        Prop::Relic { kind, x, y } => {
            let f = gate_fade(x, 1.0);
            if f > 0.0 {
                sprites::draw_bonus(l.canvas(x, y), l.px(RELIC_R * f), kind, t);
            }
        }
        Prop::Dizzy { x, y, strength } => {
            for i in 0..3 {
                let a = t * 5.0 + i as f32 * TAU / 3.0;
                let sx = x + a.cos() * 0.85;
                let sy = y + a.sin() * 0.3;
                ui::sparkle(l, sx, sy, 0.22, ui::alpha(pal::AMBER, strength));
            }
        }
        Prop::Points { x, y, value, age } => {
            let a = 1.0 - ui::progress(age, 0.8, 0.5);
            let rise = ui::ease_out(ui::progress(age, 0.0, 1.0)) * 0.9;
            let label = PointsLabel::new(value);
            ui::text_glow(
                l,
                label.as_str(),
                x,
                y - rise,
                ui::BODY,
                ui::alpha(pal::MINT, a),
                Align::Center,
            );
        }
        Prop::Burst { x, y, age } => {
            let k = ui::progress(age, 0.0, 0.6);
            let c = l.canvas(x, y);
            let r = l.px(0.4 + 3.2 * ui::ease_out(k));
            let a = 1.0 - k;
            draw_circle(c.x, c.y, r * 0.8, ui::alpha(pal::INK, 0.18 * a));
            draw_circle_lines(
                c.x,
                c.y,
                r,
                l.px(0.12).max(1.0),
                ui::alpha(pal::MINT, 0.9 * a),
            );
            sprites::glow(c, r * 1.2, pal::MINT, a);
        }
        Prop::Peepers {
            kind,
            x,
            y,
            scale,
            look,
        } => {
            let s = scale.max(0.0) * gate_fade(x, 1.0);
            if s <= 0.0 {
                return;
            }
            let tint = sprites::shade_color(kind);
            sprites::glow(l.canvas(x, y), l.px(0.8 * s), tint, 0.5);
            for side in [-1.0, 1.0] {
                let c = l.canvas(x + side * 0.24 * s, y);
                let (w, h) = (l.px(0.15 * s), l.px(0.2 * s));
                draw_ellipse(c.x, c.y, w, h, 0.0, ui::mix(pal::INK, tint, 0.25));
                let pupil = l.canvas(x + side * 0.24 * s + look * 0.06 * s, y + 0.03 * s);
                draw_circle(
                    pupil.x,
                    pupil.y,
                    l.px(0.085 * s),
                    ui::mix(tint, pal::SHADOW, 0.6),
                );
            }
        }
        Prop::PortalFlash { x, age } => {
            let k = ui::progress(age, 0.0, FLASH_LIFE);
            let a = 1.0 - k;
            if a > 0.0 {
                let c = l.canvas(x, STAGE_Y);
                let h = l.px(RAIL * 0.95);
                let w = l.px(0.55 * (1.0 + 0.8 * ease(k)));
                draw_ellipse(c.x, c.y, w, h, 0.0, ui::alpha(pal::MINT, 0.3 * a));
                draw_ellipse_lines(
                    c.x,
                    c.y,
                    w,
                    h,
                    0.0,
                    l.px(0.14).max(1.0),
                    ui::alpha(pal::INK, 0.9 * a),
                );
            }
        }
        Prop::Puff { x, y, age } => {
            let k = ui::progress(age, 0.0, PUFF_LIFE);
            let a = (1.0 - k) * 0.7;
            for i in 0..5 {
                let ang = -PI * 0.5 + (i as f32 - 2.0) * 0.55;
                let d = 0.2 + 0.9 * ease(k);
                let c = l.canvas(x + ang.cos() * d, y + ang.sin() * d - 0.5 * k);
                let r = l.px(0.16 + 0.22 * k);
                draw_circle(c.x, c.y, r, ui::alpha(pal::FAINT, a));
                draw_circle(c.x, c.y, r * 0.55, ui::alpha(pal::DIM, a * 0.6));
            }
        }
        Prop::Glow { x, y, r, strength } => {
            if strength > 0.0 {
                sprites::glow(l.canvas(x, y), l.px(r), sprites::glim_color(), strength);
            }
        }
        Prop::Label {
            text,
            x,
            y,
            age,
            life,
        } => {
            let pop = ui::ease_back(ui::progress(age, 0.0, 0.3));
            let a = 1.0 - ui::progress(age, life - 0.35, 0.35);
            if pop > 0.0 && a > 0.0 {
                let size = ui::H1 * (0.5 + 0.5 * pop);
                ui::text_glow(
                    l,
                    text,
                    x,
                    y - size * 0.5,
                    size,
                    ui::alpha(pal::AMBER, a),
                    Align::Center,
                );
            }
        }
    }
}

/// Small stack-allocated decimal label for score popups.
struct PointsLabel {
    buf: [u8; 10],
    start: usize,
}

impl PointsLabel {
    fn new(mut v: u32) -> PointsLabel {
        let mut buf = [b'0'; 10];
        let mut i = buf.len();
        loop {
            i -= 1;
            buf[i] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 || i == 0 {
                break;
            }
        }
        PointsLabel { buf, start: i }
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.buf[self.start..]).unwrap_or("")
    }
}

/// Canvas position of a prop.
#[cfg(test)]
fn prop_pos(p: &Prop) -> Vec2 {
    match *p {
        Prop::Glim { x, y, .. }
        | Prop::Shade { x, y, .. }
        | Prop::Relic { x, y, .. }
        | Prop::Dizzy { x, y, .. }
        | Prop::Points { x, y, .. }
        | Prop::Burst { x, y, .. }
        | Prop::Puff { x, y, .. }
        | Prop::Peepers { x, y, .. }
        | Prop::Glow { x, y, .. }
        | Prop::Label { x, y, .. } => Vec2::new(x, y),
        Prop::PortalFlash { x, .. } => Vec2::new(x, STAGE_Y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_fit_the_music_loop() {
        for v in 0..INTERLUDE_VARIANTS {
            let d = Interlude::duration(v);
            assert!((6.0..=9.0).contains(&d), "variant {v}: {d}");
            // Ends on the music's phrase: one pass of the 8 s loop.
            assert!((d - MUSIC_LOOP).abs() < 0.05, "variant {v}: {d}");
            assert!(!Interlude::title(v).is_empty());
        }
        assert_eq!(MUSIC_LOOP, 8.0);
        assert_eq!(HEADING, "INTERLUDE");
        assert_eq!(Interlude::new(INTERLUDE_VARIANTS).variant, 0);
        let titles: Vec<_> = (0..INTERLUDE_VARIANTS).map(Interlude::title).collect();
        for (i, a) in titles.iter().enumerate() {
            assert!(!titles[..i].contains(a), "duplicate title {a}");
        }
    }

    /// Visit every frame of every scene at 60 fps.
    fn each_frame(mut f: impl FnMut(u8, f32, &Props)) {
        for v in 0..INTERLUDE_VARIANTS {
            let mut s = Interlude::new(v);
            while s.t <= Interlude::duration(v) + 0.5 {
                f(v, s.t, &s.props());
                s.advance(1.0 / 60.0);
            }
        }
    }

    #[test]
    fn no_scene_restages_a_chase_reversal() {
        // Glimmer's interludes never grow the hero into a giant and never
        // show a frightened shade (no "the chaser becomes the chased" gag).
        each_frame(|v, t, props| {
            for p in props.iter() {
                match *p {
                    Prop::Glim { scale, .. } => {
                        assert!(scale <= 1.25, "variant {v} t {t}: giant Glim {p:?}")
                    }
                    Prop::Shade { look, .. } => assert!(
                        !matches!(look, ShadeLook::Frightened { .. }),
                        "variant {v} t {t}: frightened shade {p:?}"
                    ),
                    _ => {}
                }
            }
        });
    }

    #[test]
    fn actors_move_smoothly_while_visible() {
        // No visible actor jumps across the stage between two frames except
        // by going through a portal (where it is hidden).
        let mut prev: Option<(u8, Props)> = None;
        each_frame(|v, t, props| {
            if let Some((pv, pp)) = &prev {
                if *pv == v {
                    let actors = |ps: &Props| -> Vec<(u8, Vec2, f32)> {
                        ps.iter()
                            .filter_map(|p| match *p {
                                Prop::Glim { x, y, scale, .. } => {
                                    Some((9, Vec2::new(x, y), gate_fade(x, scale)))
                                }
                                Prop::Shade {
                                    kind, x, y, scale, ..
                                } => Some((
                                    kind.index() as u8,
                                    Vec2::new(x, y),
                                    gate_fade(x, 1.0) * scale,
                                )),
                                _ => None,
                            })
                            .collect()
                    };
                    for (id, a, fa) in actors(pp) {
                        for (id2, b, fb) in actors(props) {
                            if id == id2 && fa > 0.0 && fb > 0.0 {
                                assert!(
                                    (a - b).length() < 0.75,
                                    "variant {v} t {t}: actor {id} jumped {a} -> {b}"
                                );
                            }
                        }
                    }
                }
            }
            prev = Some((v, *props));
        });
    }

    #[test]
    fn props_never_hit_the_capacity() {
        each_frame(|v, t, props| {
            let n = props.iter().count();
            assert!(n < MAX_PROPS, "variant {v} t {t}: {n} props");
        });
    }

    #[test]
    fn lights_out_goes_dark_then_lights_up_for_good() {
        let light = |v: u8, t: f32| Interlude { variant: v, t }.light();
        for t in [0.0, 2.0, 5.0, 8.0] {
            assert_eq!(light(0, t), 1.0);
            assert_eq!(light(1, t), 1.0);
        }
        assert_eq!(light(2, 0.0), 1.0);
        assert!(light(2, 2.0) < 0.05);
        assert!(light(2, LIGHTS_ON - 0.01) < 0.05);
        assert_eq!(light(2, LIGHTS_ON + 0.5), 1.0);
        assert_eq!(light(2, MUSIC_LOOP), 1.0);
        each_frame(|v, t, _| {
            let l = light(v, t);
            assert!((0.0..=1.0).contains(&l), "variant {v} t {t}: {l}");
        });
    }

    #[test]
    fn mixup_charge_loops_through_the_portals() {
        // Moss leaves through the right portal and comes back in from the
        // left one, and Glim is in the air each time Moss passes under it.
        let first = mixup_pass_time(GATE_R + 0.5, 0);
        assert!(mixup_charge_x(first - 0.01).is_some());
        assert!(mixup_charge_x(first + PORTAL_TRANSIT * 0.5).is_none());
        let back = mixup_charge_x(first + PORTAL_TRANSIT + 0.01).unwrap();
        assert!(back < GATE_L + 0.5);
        for (x, pass) in [(MIXUP_GLIM_WAIT, 0), (MIXUP_RELIC_X, 1)] {
            let t = mixup_pass_time(x, pass);
            let s = Interlude { variant: 1, t };
            let gy = s
                .props()
                .iter()
                .find_map(|p| match *p {
                    Prop::Glim { y, .. } => Some(y),
                    _ => None,
                })
                .unwrap();
            assert!(gy < STAGE_Y - 1.2, "pass {pass}: Glim at {gy}");
        }
        assert!(mixup_pass_time(GATE_R + 0.5, 1) < MUSIC_LOOP);
    }

    #[test]
    fn track_interpolates_and_clamps() {
        let k = [(1.0, 10.0), (3.0, 30.0), (4.0, 30.0)];
        assert_eq!(track(0.0, &k), 10.0);
        assert_eq!(track(2.0, &k), 20.0);
        assert_eq!(track(3.5, &k), 30.0);
        assert_eq!(track(99.0, &k), 30.0);
        assert_eq!(track(1.0, &[]), 0.0);
        assert_eq!(track(5.0, &[(0.0, 7.0)]), 7.0);
    }

    #[test]
    fn props_stay_finite_and_on_stage() {
        for v in 0..INTERLUDE_VARIANTS {
            let mut s = Interlude::new(v);
            let d = Interlude::duration(v);
            while s.t <= d + 0.5 {
                for p in s.props().iter() {
                    let pos = prop_pos(p);
                    assert!(
                        pos.x.is_finite() && pos.y.is_finite(),
                        "variant {v} t {}: {p:?}",
                        s.t
                    );
                    assert!(
                        (-3.0..=30.0).contains(&pos.x),
                        "variant {v} t {}: {p:?}",
                        s.t
                    );
                    assert!(
                        (STAGE_Y - RAIL - 2.0..=STAGE_Y + RAIL).contains(&pos.y),
                        "{p:?}"
                    );
                }
                s.advance(1.0 / 30.0);
            }
            assert!(s.finished());
        }
    }

    #[test]
    fn scenes_start_and_end_with_an_empty_stage() {
        let visible = |s: &Interlude| {
            s.props().iter().any(|p| match *p {
                Prop::Glim { x, scale, .. } => gate_fade(x, scale) > 0.0,
                Prop::Shade { x, scale, .. } => gate_fade(x, 1.0) * scale > 0.0,
                _ => false,
            })
        };
        for v in 0..INTERLUDE_VARIANTS {
            let mut s = Interlude::new(v);
            assert!(!visible(&s), "variant {v} starts with an actor on stage");
            s.t = Interlude::duration(v);
            assert!(!visible(&s), "variant {v} ends with an actor on stage");
            // Mostly busy in between (actors may briefly all be inside the
            // portals).
            let frames = (Interlude::duration(v) * 60.0) as u32;
            let busy = (0..frames)
                .filter(|&f| {
                    s.t = f as f32 / 60.0;
                    visible(&s)
                })
                .count() as u32;
            assert!(busy * 10 >= frames * 8, "variant {v}: busy {busy}/{frames}");
        }
    }

    #[test]
    fn gate_fade_is_zero_outside_and_one_inside() {
        assert_eq!(gate_fade(GATE_L, 1.0), 0.0);
        assert_eq!(gate_fade(GATE_R + 3.0, 1.0), 0.0);
        assert_eq!(gate_fade(13.5, 1.0), 1.0);
        assert!(gate_fade(GATE_L + 0.7, 1.0) > 0.0 && gate_fade(GATE_L + 0.7, 1.0) < 1.0);
    }

    #[test]
    fn points_label_formats() {
        assert_eq!(PointsLabel::new(0).as_str(), "0");
        assert_eq!(PointsLabel::new(500).as_str(), "500");
        assert_eq!(PointsLabel::new(u32::MAX).as_str(), "4294967295");
    }

    #[test]
    fn advance_ignores_bad_dt() {
        let mut s = Interlude::new(1);
        s.advance(f32::NAN);
        s.advance(-1.0);
        assert_eq!(s.t, 0.0);
        s.advance(0.5);
        assert_eq!(s.t, 0.5);
    }
}
