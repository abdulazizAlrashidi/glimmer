//! Procedural character / item art, drawn in window pixels so the app can
//! reuse it on the title screen, cast roll, how-to-play page and interludes.
//!
//! All designs are original:
//! - Glim (the player): a glowing mint-cyan firefly orb with a white-hot core,
//!   two translucent flapping wings and two small eyes looking where it
//!   travels; a soft halo pulses with `anim_t`.
//! - Shades: hovering hexagonal "lantern drones" in their signature colour
//!   with a single large eye (pupil looks toward `dir`), small fins and a
//!   flickering underglow. Frightened: hollow, pale indigo, jittering, and the
//!   eye turns into a spiral; `flash` swaps to white. Eyes only: just the
//!   glowing eye and a short comet trail.
//! - Relics: one glowing geometric emblem per `BonusKind`.
//!
//! Everything is built from the allocation-free primitives in
//! [`super::gfx`]; polygon outlines use fixed-size stack arrays.

use super::gfx::{self, fade, lighten, mix, shade, with_alpha, WHITE};
use crate::config::BonusKind;
use crate::font;
use crate::geom::Dir;
use crate::shade::{ShadeKind, ShadeLook};
use macroquad::color::Color;
use macroquad::math::Vec2;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

const fn rgb(r: f32, g: f32, b: f32) -> Color {
    Color { r, g, b, a: 1.0 }
}

/// Glim's signature mint-cyan.
const GLIM: Color = rgb(0.42, 1.0, 0.84);
/// Glim's warm lantern tail.
const GLIM_TAIL: Color = rgb(0.80, 1.0, 0.52);
/// Ink used for eyes and pupils.
const INK: Color = rgb(0.02, 0.07, 0.10);
/// Frightened shades.
const FRIGHT: Color = rgb(0.34, 0.50, 1.0);
const FRIGHT_FLASH: Color = rgb(0.96, 0.96, 1.0);

/// Signature colour of a shade.
pub fn shade_color(kind: ShadeKind) -> Color {
    match kind {
        ShadeKind::Rook => rgb(1.0, 0.20, 0.30),
        ShadeKind::Vex => rgb(0.72, 0.40, 1.0),
        ShadeKind::Flint => rgb(1.0, 0.66, 0.14),
        ShadeKind::Moss => rgb(0.58, 1.0, 0.26),
    }
}

/// Colour of frightened shades (`flash` = the alternate white phase).
pub fn fright_color(flash: bool) -> Color {
    if flash {
        FRIGHT_FLASH
    } else {
        FRIGHT
    }
}

/// Player colour (used for HUD life icons, logo accents...).
pub fn glim_color() -> Color {
    GLIM
}

/// Dominant colour of a relic (for particles and popups).
pub fn bonus_color(kind: BonusKind) -> Color {
    match kind {
        BonusKind::Shard => rgb(0.55, 0.95, 1.0),
        BonusKind::Prism => rgb(0.85, 0.80, 1.0),
        BonusKind::Lotus => rgb(1.0, 0.52, 0.80),
        BonusKind::Crown => rgb(1.0, 0.82, 0.30),
        BonusKind::Comet => rgb(0.55, 0.80, 1.0),
        BonusKind::Star => rgb(1.0, 0.90, 0.35),
        BonusKind::Nova => rgb(1.0, 0.62, 0.30),
        BonusKind::Singularity => rgb(0.78, 0.45, 1.0),
    }
}

/// Soft radial glow (concentric translucent discs).
pub fn glow(center: Vec2, radius: f32, color: Color, intensity: f32) {
    gfx::glow(center, radius, color, intensity);
}

// ---------------------------------------------------------------------------
// Glim
// ---------------------------------------------------------------------------

/// Draw Glim. `radius` is about 0.65 tile in game. `anim_t` drives wings/pulse.
pub fn draw_glim(center: Vec2, radius: f32, dir: Dir, anim_t: f32, alpha: f32) {
    draw_glim_ex(center, radius, dir, anim_t, alpha, 0.0);
}

/// Glim with an extra `whiten` (0..1) used by the death animation.
fn draw_glim_ex(center: Vec2, radius: f32, dir: Dir, anim_t: f32, alpha: f32, whiten: f32) {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.0 || radius <= 0.0 {
        return;
    }
    let r = radius;
    let pulse = 0.5 + 0.5 * (anim_t * 4.6).sin();
    let look = dir.vec();
    let c = center + Vec2::new(0.0, (anim_t * 5.3).sin() * r * 0.04);

    // Halo and lantern tail (behind the body, opposite to travel).
    gfx::glow(c, r * 2.9, GLIM, alpha * (0.26 + 0.10 * pulse));
    let tail = c - look * (r * 0.62);
    gfx::glow(tail, r * 1.25, GLIM_TAIL, alpha * (0.30 + 0.16 * pulse));

    // Wings: translucent, beating quickly (hind wing first, then fore wing).
    let beat = (anim_t * 21.0).sin();
    let wing_in = with_alpha(lighten(GLIM, 0.82), alpha * 0.72);
    let wing_out = with_alpha(lighten(GLIM, 0.3), alpha * 0.2);
    for side in [-1.0f32, 1.0] {
        let root = c + Vec2::new(side * r * 0.38, -r * 0.30);
        let fore = -FRAC_PI_2 + side * (0.98 + 0.38 * beat);
        let hind = fore + side * 0.62;
        for (tilt, len, width, k) in [
            (hind, r * 0.78, r * 0.26, 0.75f32),
            (fore, r * 1.08, r * 0.36, 1.0),
        ] {
            let d = Vec2::new(tilt.cos(), tilt.sin());
            let mid = root + d * (len * 0.52);
            gfx::ellipse(
                mid,
                len * 0.55,
                width,
                tilt,
                fade(wing_in, k),
                fade(wing_out, k),
            );
            // A bright vein along the wing.
            gfx::taper(
                root,
                root + d * (len * 0.95),
                r * 0.035,
                0.0,
                fade(wing_in, k),
                fade(wing_out, 0.0),
            );
        }
    }

    // Antennae with glowing tips.
    let sway = (anim_t * 3.4).sin() * r * 0.05;
    for side in [-1.0f32, 1.0] {
        let base = c + Vec2::new(side * r * 0.2, -r * 0.82);
        let tip = c + Vec2::new(side * r * 0.46 + sway, -r * 1.32);
        gfx::line(base, tip, r * 0.06, with_alpha(lighten(GLIM, 0.4), alpha));
        gfx::glow_small(tip, r * 0.38, GLIM_TAIL, alpha * (0.5 + 0.3 * pulse));
        gfx::circle(tip, r * 0.085, with_alpha(lighten(GLIM_TAIL, 0.5), alpha));
    }

    // Body: white-hot core through mint to a deeper rim.
    let hot = mix(rgb(1.0, 1.0, 0.96), WHITE, whiten);
    let body = |col: Color, k: f32| with_alpha(mix(col, WHITE, whiten * k), alpha);
    let rings = [
        (0.0, with_alpha(hot, alpha)),
        (r * 0.2, body(rgb(0.93, 1.0, 0.97), 0.95)),
        (r * 0.55, body(rgb(0.62, 1.0, 0.90), 0.9)),
        (r * 0.84, body(GLIM, 0.8)),
        (r, body(rgb(0.10, 0.64, 0.58), 0.7)),
    ];
    gfx::with_builder(|b| b.polar(c, 0.0, TAU, &rings, gfx::segs_for(r, TAU)));
    gfx::soft_ring(
        c,
        r * 0.97,
        r * 0.14,
        with_alpha(lighten(GLIM, 0.6), alpha * 0.75),
    );
    // Glossy highlight.
    gfx::ellipse(
        c + Vec2::new(-r * 0.36, -r * 0.42),
        r * 0.24,
        r * 0.12,
        -0.65,
        with_alpha(WHITE, alpha * 0.75 * (1.0 - whiten)),
        with_alpha(WHITE, 0.0),
    );

    // Eyes: side by side, shifted toward the travel direction.
    if whiten < 0.6 {
        let blink = (anim_t * 0.31).fract() > 0.965;
        let base = c + look * (r * 0.2) + Vec2::new(0.0, -r * 0.06);
        for side in [-1.0f32, 1.0] {
            let e = base + Vec2::new(side * r * 0.27, 0.0);
            let ink = with_alpha(INK, alpha * (1.0 - whiten));
            if blink {
                gfx::line(
                    e - Vec2::new(r * 0.11, 0.0),
                    e + Vec2::new(r * 0.11, 0.0),
                    r * 0.07,
                    ink,
                );
            } else {
                gfx::ellipse(e, r * 0.12, r * 0.17, 0.0, ink, ink);
                gfx::circle(
                    e + Vec2::new(-r * 0.035, -r * 0.06),
                    r * 0.045,
                    with_alpha(WHITE, alpha * 0.95),
                );
            }
        }
    }
}

/// Glim's death animation over `progress` 0..1 (flicker, swell, burst, fade).
pub fn draw_glim_death(center: Vec2, radius: f32, progress: f32) {
    let p = progress.clamp(0.0, 1.0);
    let r = radius;
    if p < 0.3 {
        // Flicker with growing intensity.
        let k = p / 0.3;
        let on = ((p * 46.0) as i32) % 2 == 0 || k < 0.25;
        let a = if on { 1.0 } else { 0.35 };
        draw_glim_ex(center, r, Dir::Down, p * 2.0, a, k * 0.3);
    } else if p < 0.56 {
        // Swell and whiten.
        let k = (p - 0.3) / 0.26;
        let rr = r * (1.0 + 0.5 * k * k);
        gfx::glow(center, rr * (2.6 + k), WHITE, 0.25 + 0.35 * k);
        draw_glim_ex(center, rr, Dir::Down, 0.6, 1.0, 0.3 + 0.7 * k);
    } else {
        // Burst: shock ring, rays and drifting sparks.
        let k = (p - 0.56) / 0.44;
        let fade_out = (1.0 - k).powf(1.4);
        let core = r * 1.5 * (1.0 - k * 2.2).max(0.0);
        if core > 0.0 {
            gfx::glow(center, core * 3.0, WHITE, 0.6 * fade_out);
            gfx::circle(center, core * 0.6, with_alpha(WHITE, fade_out));
        }
        let ring_r = r * (1.3 + 3.4 * k.sqrt());
        gfx::soft_ring(
            center,
            ring_r,
            r * (0.9 - 0.6 * k),
            with_alpha(GLIM, 0.85 * fade_out),
        );
        gfx::ring(
            center,
            ring_r,
            r * 0.07,
            with_alpha(lighten(GLIM, 0.6), 0.9 * fade_out),
        );
        for i in 0..12 {
            let a = i as f32 / 12.0 * TAU + 0.26;
            let d = Vec2::new(a.cos(), a.sin());
            let long = if i % 2 == 0 { 1.0 } else { 0.7 };
            let r0 = r * (0.8 + 2.6 * k) * long;
            let r1 = r0 + r * 1.3 * (1.0 - k) * long;
            gfx::taper(
                center + d * r0,
                center + d * r1,
                r * 0.09,
                0.0,
                with_alpha(lighten(GLIM, 0.5), fade_out),
                with_alpha(GLIM, 0.0),
            );
            let s = center + d * (r * (1.4 + 3.6 * k) * long) + Vec2::new(0.0, -r * 0.8 * k);
            let tw = 0.6 + 0.4 * (p * 50.0 + i as f32).sin();
            gfx::sparkle(
                s,
                r * 0.42 * (1.0 - 0.5 * k),
                a + k * 2.0,
                with_alpha(GLIM_TAIL, fade_out * tw),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Shades
// ---------------------------------------------------------------------------

/// Pointy-top hexagon outline (6 points) of half-size `(rx, ry)`.
fn hexagon(c: Vec2, rx: f32, ry: f32) -> [Vec2; 6] {
    std::array::from_fn(|k| {
        let a = -FRAC_PI_2 + k as f32 * TAU / 6.0;
        c + Vec2::new(a.cos() * rx, a.sin() * ry)
    })
}

/// Draw a shade. `radius` is about 0.7 tile in game.
pub fn draw_shade(
    center: Vec2,
    radius: f32,
    kind: ShadeKind,
    look: ShadeLook,
    dir: Dir,
    anim_t: f32,
) {
    if radius <= 0.0 {
        return;
    }
    let seed = kind.index() as u32 * 31 + 7;
    let phase = kind.index() as f32 * 1.7;
    let hover = (anim_t * 3.1 + phase).sin() * radius * 0.07;
    let c = center + Vec2::new(0.0, hover);
    match look {
        ShadeLook::Normal => draw_lantern(c, radius, shade_color(kind), dir, anim_t, seed),
        ShadeLook::Frightened { flash } => draw_frightened(c, radius, flash, anim_t, seed),
        ShadeLook::Eyes => draw_eyes_only(center, radius, shade_color(kind), dir, anim_t),
    }
}

fn underglow(c: Vec2, r: f32, col: Color, anim_t: f32, seed: u32, strength: f32) {
    let flicker = 0.75 + 0.25 * gfx::noise1(anim_t * 9.0, seed);
    let top = c + Vec2::new(0.0, r * 0.75);
    let bottom = c + Vec2::new(0.0, r * 1.55);
    gfx::taper(
        top,
        bottom,
        r * 0.30,
        r * 0.62,
        with_alpha(col, 0.42 * flicker * strength),
        with_alpha(col, 0.0),
    );
    gfx::glow(
        c + Vec2::new(0.0, r * 1.0),
        r * 1.0,
        col,
        0.32 * flicker * strength,
    );
}

fn draw_lantern(c: Vec2, r: f32, col: Color, dir: Dir, anim_t: f32, seed: u32) {
    gfx::glow(c, r * 2.3, col, 0.22);
    underglow(c, r, col, anim_t, seed, 1.0);

    // Fins, gently flapping.
    let flap = (anim_t * 6.0 + seed as f32).sin() * r * 0.05;
    let fin_dark = shade(col, 0.45);
    for side in [-1.0f32, 1.0] {
        let a = c + Vec2::new(side * r * 0.70, -r * 0.18);
        let b = c + Vec2::new(side * r * 0.70, r * 0.40);
        let tip = c + Vec2::new(side * r * 1.16, r * 0.52 + flap);
        gfx::with_builder(|m| {
            m.triangle([a, b, tip], [shade(col, 0.8), fin_dark, lighten(col, 0.2)])
        });
        gfx::line(a, tip, r * 0.05, lighten(col, 0.35));
    }

    // Body: a glowing glass hexagon lit from inside.
    let hex = hexagon(c, r * 0.86, r * 0.98);
    let inner = mix(col, WHITE, 0.30);
    let outer = shade(col, 0.42);
    gfx::poly_fan(c, inner, &hex, outer);
    let frame = hexagon(c, r * 0.62, r * 0.71);
    gfx::poly_outline(&frame, r * 0.05, with_alpha(shade(col, 0.32), 0.75));
    gfx::poly_outline(&hex, r * 0.09, lighten(col, 0.35));
    // Lantern ribs.
    gfx::line(
        hex[0],
        frame[0],
        r * 0.05,
        with_alpha(lighten(col, 0.3), 0.8),
    );
    gfx::line(
        hex[3],
        frame[3],
        r * 0.05,
        with_alpha(lighten(col, 0.3), 0.8),
    );

    // Antenna with a blinking tip.
    let top = hex[0];
    let tip = top + Vec2::new(0.0, -r * 0.24);
    gfx::line(top, tip, r * 0.06, lighten(col, 0.3));
    let blink = if (anim_t * 1.3 + seed as f32 * 0.1).fract() < 0.5 {
        1.0
    } else {
        0.35
    };
    gfx::glow(tip, r * 0.42, col, 0.55 * blink);
    gfx::circle(tip, r * 0.08, with_alpha(lighten(col, 0.6), blink));

    // The single eye.
    let eye_c = c + Vec2::new(0.0, -r * 0.03);
    let look = dir.vec();
    gfx::glow(eye_c, r * 0.7, WHITE, 0.14);
    gfx::circle(eye_c, r * 0.37, rgb(0.97, 0.97, 1.0));
    let iris = eye_c + look * (r * 0.13);
    gfx::radial(iris, r * 0.21, lighten(col, 0.25), shade(col, 0.6));
    gfx::circle(iris + look * (r * 0.03), r * 0.10, INK);
    gfx::circle(iris + Vec2::new(-r * 0.07, -r * 0.08), r * 0.05, WHITE);
    // A stern lid across the top of the eye.
    let lid_col = mix(inner, outer, 0.25);
    let y0 = eye_c.y - r * 0.40;
    let y1 = eye_c.y - r * 0.20;
    let x0 = eye_c.x - r * 0.42;
    let x1 = eye_c.x + r * 0.42;
    gfx::with_builder(|m| {
        m.quad(
            [
                Vec2::new(x0, y0),
                Vec2::new(x1, y0),
                Vec2::new(x1, y1 - r * 0.04),
                Vec2::new(x0, y1 + r * 0.02),
            ],
            [lid_col; 4],
        )
    });
    gfx::line(
        Vec2::new(x0 + r * 0.06, y1 + r * 0.02),
        Vec2::new(x1 - r * 0.06, y1 - r * 0.04),
        r * 0.05,
        shade(col, 0.35),
    );
}

fn draw_frightened(c: Vec2, r: f32, flash: bool, anim_t: f32, seed: u32) {
    let col = fright_color(flash);
    let jitter = Vec2::new(
        gfx::noise1(anim_t * 24.0, seed),
        gfx::noise1(anim_t * 24.0, seed + 5),
    ) * (r * 0.05);
    let c = c + jitter;
    gfx::glow(c, r * 2.1, col, 0.20);
    underglow(c, r, col, anim_t, seed, 0.55);

    for side in [-1.0f32, 1.0] {
        let a = c + Vec2::new(side * r * 0.70, -r * 0.18);
        let b = c + Vec2::new(side * r * 0.70, r * 0.40);
        let tip = c + Vec2::new(side * r * 1.10, r * 0.60 + (anim_t * 14.0).sin() * r * 0.06);
        gfx::line(a, tip, r * 0.06, with_alpha(col, 0.85));
        gfx::line(b, tip, r * 0.06, with_alpha(col, 0.85));
    }

    // Hollow body: faint glass, bright rim.
    let hex = hexagon(c, r * 0.86, r * 0.98);
    gfx::poly_fan(
        c,
        with_alpha(col, 0.30),
        &hex,
        with_alpha(shade(col, 0.6), 0.12),
    );
    gfx::poly_outline(&hex, r * 0.1, lighten(col, 0.25));

    // Spiral eye, spinning.
    let mut pts = [Vec2::ZERO; 28];
    let spin = anim_t * 5.0;
    let turns = 2.4 * PI;
    let n = pts.len();
    for (i, p) in pts.iter_mut().enumerate() {
        let k = i as f32 / (n - 1) as f32;
        let a = spin + k * turns;
        let rr = r * (0.03 + 0.30 * k);
        *p = c + Vec2::new(a.cos() * rr, a.sin() * rr - r * 0.02);
    }
    let ink = if flash {
        rgb(1.0, 0.45, 0.6)
    } else {
        lighten(col, 0.7)
    };
    gfx::polyline(&pts, r * 0.075, ink);
}

fn draw_eyes_only(center: Vec2, r: f32, col: Color, dir: Dir, anim_t: f32) {
    let look = dir.vec();
    // Comet trail behind the eye.
    for i in (1..=5).rev() {
        let k = i as f32 / 5.0;
        let p = center - look * (r * (0.25 + 0.32 * i as f32));
        gfx::glow_small(
            p,
            r * (0.55 - 0.3 * k),
            lighten(col, 0.5),
            0.5 * (1.0 - k) + 0.12,
        );
    }
    let pulse = 0.85 + 0.15 * (anim_t * 9.0).sin();
    gfx::glow(center, r * 1.4, lighten(col, 0.4), 0.42 * pulse);
    gfx::circle(center, r * 0.36, rgb(0.97, 0.97, 1.0));
    gfx::ring(center, r * 0.36, r * 0.06, lighten(col, 0.3));
    let iris = center + look * (r * 0.14);
    gfx::radial(iris, r * 0.2, lighten(col, 0.3), shade(col, 0.6));
    gfx::circle(iris + look * (r * 0.03), r * 0.095, INK);
    gfx::circle(iris + Vec2::new(-r * 0.07, -r * 0.07), r * 0.045, WHITE);
}

// ---------------------------------------------------------------------------
// Relics
// ---------------------------------------------------------------------------

/// Draw a relic emblem.
pub fn draw_bonus(center: Vec2, radius: f32, kind: BonusKind, anim_t: f32) {
    if radius <= 0.0 {
        return;
    }
    let r = radius;
    let c = center;
    let col = bonus_color(kind);
    let pulse = 0.5 + 0.5 * (anim_t * 3.0).sin();
    gfx::glow(c, r * 2.3, col, 0.26 + 0.1 * pulse);
    match kind {
        BonusKind::Shard => relic_shard(c, r, col),
        BonusKind::Prism => relic_prism(c, r, col, anim_t),
        BonusKind::Lotus => relic_lotus(c, r, col, anim_t),
        BonusKind::Crown => relic_crown(c, r, col),
        BonusKind::Comet => relic_comet(c, r, col, anim_t),
        BonusKind::Star => relic_star(c, r, col, anim_t),
        BonusKind::Nova => relic_nova(c, r, col, anim_t),
        BonusKind::Singularity => relic_singularity(c, r, col, anim_t),
    }
    // A travelling glint.
    let g = (anim_t * 0.7).fract();
    if g < 0.25 {
        let k = g / 0.25;
        let a = (k * PI).sin();
        gfx::sparkle(
            c + Vec2::new(r * 0.45, -r * 0.55),
            r * 0.55 * a,
            k,
            with_alpha(WHITE, a),
        );
    }
}

fn relic_shard(c: Vec2, r: f32, col: Color) {
    let pts = [
        c + Vec2::new(0.0, -r),
        c + Vec2::new(r * 0.40, -r * 0.40),
        c + Vec2::new(r * 0.30, r * 0.62),
        c + Vec2::new(0.0, r * 0.95),
        c + Vec2::new(-r * 0.30, r * 0.62),
        c + Vec2::new(-r * 0.40, -r * 0.40),
    ];
    gfx::poly_fan(
        c + Vec2::new(-r * 0.1, -r * 0.1),
        lighten(col, 0.75),
        &pts,
        shade(col, 0.55),
    );
    gfx::poly_outline(&pts, r * 0.07, lighten(col, 0.5));
    let facet = with_alpha(WHITE, 0.55);
    gfx::line(pts[0], c + Vec2::new(0.0, r * 0.1), r * 0.05, facet);
    gfx::line(
        c + Vec2::new(0.0, r * 0.1),
        pts[3],
        r * 0.05,
        with_alpha(WHITE, 0.3),
    );
    gfx::line(
        pts[5],
        c + Vec2::new(0.0, r * 0.1),
        r * 0.04,
        with_alpha(WHITE, 0.3),
    );
    gfx::line(
        pts[1],
        c + Vec2::new(0.0, r * 0.1),
        r * 0.04,
        with_alpha(WHITE, 0.3),
    );
    // A small companion crystal.
    let s = [
        c + Vec2::new(r * 0.55, -r * 0.05),
        c + Vec2::new(r * 0.75, r * 0.25),
        c + Vec2::new(r * 0.62, r * 0.75),
        c + Vec2::new(r * 0.42, r * 0.35),
    ];
    gfx::poly_fan(
        c + Vec2::new(r * 0.58, r * 0.3),
        lighten(col, 0.5),
        &s,
        shade(col, 0.5),
    );
}

fn relic_prism(c: Vec2, r: f32, col: Color, t: f32) {
    let tri = [
        c + Vec2::new(0.0, -r * 0.92),
        c + Vec2::new(r * 0.82, r * 0.62),
        c + Vec2::new(-r * 0.82, r * 0.62),
    ];
    // Incoming white beam and the outgoing rainbow.
    let entry = c + Vec2::new(-r * 0.38, -r * 0.05);
    gfx::line(
        c + Vec2::new(-r * 1.25, r * 0.22),
        entry,
        r * 0.08,
        with_alpha(WHITE, 0.85),
    );
    let exit = c + Vec2::new(r * 0.36, r * 0.02);
    let bands = [
        rgb(1.0, 0.3, 0.35),
        rgb(1.0, 0.75, 0.25),
        rgb(0.4, 1.0, 0.45),
        rgb(0.3, 0.8, 1.0),
        rgb(0.7, 0.45, 1.0),
    ];
    let wob = (t * 2.0).sin() * 0.04;
    for (i, b) in bands.iter().enumerate() {
        let a = -0.32 + i as f32 * 0.16 + wob;
        let end = exit + Vec2::new(a.cos(), a.sin()) * (r * 0.95);
        gfx::taper(exit, end, r * 0.03, r * 0.07, *b, with_alpha(*b, 0.15));
    }
    gfx::poly_fan(
        c + Vec2::new(0.0, r * 0.15),
        with_alpha(lighten(col, 0.6), 0.85),
        &tri,
        with_alpha(shade(col, 0.5), 0.9),
    );
    gfx::poly_outline(&tri, r * 0.08, lighten(col, 0.6));
    gfx::line(
        tri[0],
        c + Vec2::new(0.0, r * 0.62),
        r * 0.04,
        with_alpha(WHITE, 0.35),
    );
}

fn relic_lotus(c: Vec2, r: f32, col: Color, t: f32) {
    let sway = (t * 1.6).sin() * 0.04;
    let deep = shade(col, 0.55);
    for (i, &a) in [-2.2f32, -0.94, -2.6, -0.54].iter().enumerate() {
        let a = a + sway;
        let len = if i < 2 { r * 0.72 } else { r * 0.55 };
        let d = Vec2::new(a.cos(), a.sin());
        gfx::ellipse(
            c + d * (len * 0.55) + Vec2::new(0.0, r * 0.18),
            len * 0.55,
            r * 0.2,
            a,
            lighten(col, 0.25),
            deep,
        );
    }
    let up = -FRAC_PI_2 + sway;
    gfx::ellipse(
        c + Vec2::new(up.cos(), up.sin()) * (r * 0.42) + Vec2::new(0.0, r * 0.12),
        r * 0.5,
        r * 0.24,
        up,
        lighten(col, 0.6),
        shade(col, 0.7),
    );
    // Leaf pad and glowing heart.
    gfx::ellipse(
        c + Vec2::new(0.0, r * 0.62),
        r * 0.85,
        r * 0.16,
        0.0,
        rgb(0.35, 0.85, 0.55),
        rgb(0.1, 0.4, 0.3),
    );
    gfx::glow(
        c + Vec2::new(0.0, r * 0.2),
        r * 0.6,
        rgb(1.0, 0.9, 0.5),
        0.6,
    );
    gfx::circle(c + Vec2::new(0.0, r * 0.22), r * 0.12, rgb(1.0, 0.92, 0.6));
}

fn relic_crown(c: Vec2, r: f32, col: Color) {
    let y_base = c.y + r * 0.55;
    let y_band = c.y + r * 0.2;
    // Outline (star-shaped from the bottom centre, so a single fan fills it).
    let pts = [
        Vec2::new(c.x - r * 0.85, y_base),
        Vec2::new(c.x - r * 0.95, c.y - r * 0.55),
        Vec2::new(c.x - r * 0.45, c.y - r * 0.05),
        Vec2::new(c.x, c.y - r * 0.88),
        Vec2::new(c.x + r * 0.45, c.y - r * 0.05),
        Vec2::new(c.x + r * 0.95, c.y - r * 0.55),
        Vec2::new(c.x + r * 0.85, y_base),
    ];
    let hi = lighten(col, 0.5);
    let lo = shade(col, 0.55);
    gfx::poly_fan(Vec2::new(c.x, y_base), lo, &pts, hi);
    gfx::poly_outline(&pts, r * 0.07, lighten(col, 0.6));
    // Jewelled band.
    let band = [
        Vec2::new(c.x - r * 0.88, y_band),
        Vec2::new(c.x + r * 0.88, y_band),
        Vec2::new(c.x + r * 0.85, y_base),
        Vec2::new(c.x - r * 0.85, y_base),
    ];
    gfx::with_builder(|m| m.quad(band, [hi, hi, col, col]));
    gfx::line(band[0], band[1], r * 0.05, lighten(col, 0.7));
    for tip in [pts[1], pts[3], pts[5]] {
        gfx::glow_small(tip, r * 0.32, WHITE, 0.5);
        gfx::circle(tip, r * 0.11, rgb(1.0, 0.97, 0.8));
    }
    for (i, gem) in [
        rgb(1.0, 0.3, 0.45),
        rgb(0.35, 0.95, 1.0),
        rgb(1.0, 0.3, 0.45),
    ]
    .iter()
    .enumerate()
    {
        let p = Vec2::new(c.x + (i as f32 - 1.0) * r * 0.5, (y_band + y_base) * 0.5);
        gfx::glow_small(p, r * 0.35, *gem, 0.6);
        gfx::circle(p, r * 0.1, *gem);
    }
}

fn relic_comet(c: Vec2, r: f32, col: Color, t: f32) {
    let head = c + Vec2::new(r * 0.35, -r * 0.35);
    let tail_dir = Vec2::new(-0.72, 0.69);
    let n = Vec2::new(-tail_dir.y, tail_dir.x);
    let wob = (t * 7.0).sin() * r * 0.04;
    gfx::taper(
        head,
        head + tail_dir * (r * 1.55) + n * wob,
        r * 0.32,
        0.0,
        with_alpha(col, 0.85),
        with_alpha(col, 0.0),
    );
    for s in [-1.0f32, 1.0] {
        let a = head + n * (s * r * 0.22);
        gfx::taper(
            a,
            a + tail_dir * (r * 1.1) + n * (s * r * 0.15 + wob),
            r * 0.07,
            0.0,
            with_alpha(lighten(col, 0.5), 0.8),
            with_alpha(col, 0.0),
        );
    }
    gfx::glow(head, r * 0.9, lighten(col, 0.5), 0.7);
    gfx::radial(head, r * 0.3, WHITE, lighten(col, 0.3));
}

fn star_points(c: Vec2, r_out: f32, r_in: f32, n: usize, rot: f32, out: &mut [Vec2; 24]) -> usize {
    let n = n.min(12);
    for (k, p) in out.iter_mut().enumerate().take(n * 2) {
        let a = rot + k as f32 * PI / n as f32;
        let rr = if k % 2 == 0 { r_out } else { r_in };
        *p = c + Vec2::new(a.cos(), a.sin()) * rr;
    }
    n * 2
}

fn relic_star(c: Vec2, r: f32, col: Color, t: f32) {
    let mut pts = [Vec2::ZERO; 24];
    let rot = -FRAC_PI_2 + (t * 0.9).sin() * 0.15;
    let n = star_points(c, r * 0.98, r * 0.42, 5, rot, &mut pts);
    gfx::poly_fan(c, rgb(1.0, 1.0, 0.85), &pts[..n], shade(col, 0.75));
    gfx::poly_outline(&pts[..n], r * 0.07, lighten(col, 0.55));
    gfx::glow(c, r * 0.8, WHITE, 0.35);
}

fn relic_nova(c: Vec2, r: f32, col: Color, t: f32) {
    let rot = t * 0.6;
    for i in 0..8 {
        let a = rot + i as f32 * TAU / 8.0;
        let d = Vec2::new(a.cos(), a.sin());
        let len = if i % 2 == 0 { r * 1.05 } else { r * 0.62 };
        gfx::taper(
            c,
            c + d * len,
            r * 0.13,
            0.0,
            lighten(col, 0.6),
            with_alpha(col, 0.2),
        );
    }
    let pulse = 0.5 + 0.5 * (t * 5.0).sin();
    gfx::soft_ring(
        c,
        r * (0.52 + 0.06 * pulse),
        r * 0.22,
        with_alpha(lighten(col, 0.3), 0.9),
    );
    gfx::radial(c, r * 0.36, WHITE, col);
}

fn relic_singularity(c: Vec2, r: f32, col: Color, t: f32) {
    let tilt = -0.35;
    let spin = t * 2.2;
    // Accretion disk: back half, core, front half (approximated by layered ellipses).
    gfx::ellipse(
        c,
        r * 1.0,
        r * 0.36,
        tilt,
        with_alpha(rgb(1.0, 0.62, 0.3), 0.0),
        with_alpha(rgb(1.0, 0.62, 0.3), 0.85),
    );
    gfx::ellipse(
        c,
        r * 0.8,
        r * 0.26,
        tilt,
        with_alpha(col, 0.0),
        with_alpha(col, 0.9),
    );
    gfx::glow(c, r * 0.9, col, 0.5);
    gfx::circle(c, r * 0.4, rgb(0.01, 0.0, 0.03));
    gfx::ring(c, r * 0.42, r * 0.06, lighten(col, 0.6));
    // Orbiting hot spots.
    for i in 0..3 {
        let a = spin + i as f32 * TAU / 3.0;
        let (s, co) = tilt.sin_cos();
        let (x, y) = (a.cos() * r * 0.9, a.sin() * r * 0.32);
        let p = c + Vec2::new(x * co - y * s, x * s + y * co);
        if a.sin() > -0.2 {
            gfx::glow_small(p, r * 0.3, rgb(1.0, 0.85, 0.6), 0.8);
        }
    }
}

// ---------------------------------------------------------------------------
// Dots
// ---------------------------------------------------------------------------

/// A mote (small dot).
pub fn draw_mote(center: Vec2, radius: f32, color: Color) {
    gfx::with_builder(|b| mote_into(b, center, radius, color));
}

/// Tessellate a mote into an open builder (lets the renderer batch all motes
/// into a single submission).
pub fn mote_into(b: &mut gfx::Builder, center: Vec2, radius: f32, color: Color) {
    if radius <= 0.0 || color.a <= 0.0 {
        return;
    }
    b.glow_small(center, radius * 3.4, color, 0.38 * color.a);
    b.circle(center, radius, with_alpha(lighten(color, 0.45), color.a));
}

/// A flare (power dot), pulsing with `t`.
pub fn draw_flare(center: Vec2, radius: f32, t: f32, color: Color) {
    if radius <= 0.0 || color.a <= 0.0 {
        return;
    }
    let a = color.a;
    let pulse = 0.5 + 0.5 * (t * 6.0).sin();
    let r = radius * (0.92 + 0.12 * pulse);
    gfx::glow(
        center,
        r * (3.6 + 0.8 * pulse),
        color,
        (0.36 + 0.22 * pulse) * a,
    );
    gfx::sparkle(
        center,
        r * (2.3 + 0.5 * pulse),
        t * 0.7,
        with_alpha(lighten(color, 0.3), 0.75 * a),
    );
    gfx::sparkle(
        center,
        r * 1.5,
        t * 0.7 + PI * 0.25,
        with_alpha(color, 0.45 * a),
    );
    gfx::soft_ring(
        center,
        r * (1.45 + 0.35 * pulse),
        r * 0.35,
        with_alpha(color, 0.55 * a * (1.0 - 0.5 * pulse)),
    );
    gfx::radial(
        center,
        r,
        with_alpha(WHITE, a),
        with_alpha(lighten(color, 0.2), a),
    );
}

// ---------------------------------------------------------------------------
// Logo
// ---------------------------------------------------------------------------

/// One wordmark glyph from the font bitmap: a deep extrusion, a face with a
/// vertical gradient and a bright bevel on every top edge.
fn draw_wordmark_glyph(bitmap: &[u8; font::GLYPH_H], origin: Vec2, px: f32, col: Color) {
    let lit = |row: usize, c: usize| {
        row < font::GLYPH_H
            && c < font::GLYPH_W
            && bitmap[row] & (1 << (font::GLYPH_W - 1 - c)) != 0
    };
    let top_col = lighten(col, 0.3);
    let bottom_col = shade(col, 0.82);
    let deep = shade(col, 0.28);
    let bevel = with_alpha(lighten(col, 0.85), 0.95);
    let rows = font::GLYPH_H as f32;
    gfx::with_builder(|b| {
        let quad = |b: &mut gfx::Builder, x: f32, y: f32, w: f32, h: f32, c0: Color, c1: Color| {
            b.quad(
                [
                    Vec2::new(x, y),
                    Vec2::new(x + w, y),
                    Vec2::new(x + w, y + h),
                    Vec2::new(x, y + h),
                ],
                [c0, c0, c1, c1],
            );
        };
        let ext = Vec2::new(px * 0.45, px * 0.6);
        for row in 0..font::GLYPH_H {
            for c in 0..font::GLYPH_W {
                if lit(row, c) {
                    let p = origin + Vec2::new(c as f32 * px, row as f32 * px) + ext;
                    quad(b, p.x, p.y, px, px, deep, deep);
                }
            }
        }
        for row in 0..font::GLYPH_H {
            let c0 = mix(top_col, bottom_col, row as f32 / rows);
            let c1 = mix(top_col, bottom_col, (row + 1) as f32 / rows);
            for c in 0..font::GLYPH_W {
                if !lit(row, c) {
                    continue;
                }
                let p = origin + Vec2::new(c as f32 * px, row as f32 * px);
                quad(b, p.x, p.y, px, px, c0, c1);
                if row == 0 || !lit(row - 1, c) {
                    quad(b, p.x, p.y, px, px * 0.2, bevel, with_alpha(bevel, 0.0));
                }
            }
        }
    });
}

/// The "GLIMMER" title logo centred at `center`, `width` px wide, animated by `t`.
pub fn draw_logo(center: Vec2, width: f32, t: f32) {
    const WORD: &str = "GLIMMER";
    if width <= 0.0 {
        return;
    }
    let n = WORD.chars().count();
    let units = n as f32 * font::ADVANCE - 1.0;
    let px = width / units;
    let size = px * font::GLYPH_H as f32;
    let left = center.x - width * 0.5;
    let top = center.y - size * 0.5;

    // Orbit of the tiny Glim (behind the text on the upper half).
    let orbit = |a: f32| center + Vec2::new(a.cos() * width * 0.58, a.sin() * size * 0.95);
    let ang = t * 1.3;
    let glim_r = size * 0.17;
    let glim_at = orbit(ang);
    let tangent = Vec2::new(-ang.sin() * width * 0.58, ang.cos() * size * 0.95);
    let dir = if tangent.x.abs() > tangent.y.abs() {
        if tangent.x > 0.0 {
            Dir::Right
        } else {
            Dir::Left
        }
    } else if tangent.y > 0.0 {
        Dir::Down
    } else {
        Dir::Up
    };
    let behind = ang.sin() < 0.0;
    let draw_orbit = || {
        for i in 1..=10 {
            let p = orbit(ang - i as f32 * 0.07);
            gfx::glow_small(
                p,
                glim_r * (1.3 - i as f32 * 0.08),
                GLIM,
                0.32 * (1.0 - i as f32 / 11.0),
            );
        }
        draw_glim(
            glim_at,
            glim_r * if behind { 0.85 } else { 1.0 },
            dir,
            t * 1.4,
            if behind { 0.7 } else { 1.0 },
        );
    };

    // Backlight.
    gfx::ellipse(
        center,
        width * 0.62,
        size * 1.1,
        0.0,
        with_alpha(rgb(0.15, 0.55, 0.6), 0.16),
        with_alpha(rgb(0.1, 0.2, 0.5), 0.0),
    );
    if behind {
        draw_orbit();
    }

    // Letters along a mint, cyan, violet, rose gradient, with a shimmer.
    let stops = [
        rgb(0.42, 1.0, 0.84),
        rgb(0.30, 0.82, 1.0),
        rgb(0.70, 0.50, 1.0),
        rgb(1.0, 0.50, 0.80),
    ];
    let sweep = (t * 0.35).fract() * 1.8 - 0.4;
    for (i, ch) in WORD.chars().enumerate() {
        let k = i as f32 / (n - 1) as f32;
        let seg = (k * (stops.len() - 1) as f32).min((stops.len() - 1) as f32 - 1e-3);
        let j = seg as usize;
        let mut col = mix(stops[j], stops[j + 1], seg - j as f32);
        let shimmer = (1.0 - ((k - sweep).abs() * 5.0)).max(0.0);
        col = lighten(col, shimmer * 0.55);
        let x = left + i as f32 * font::ADVANCE * px;
        let bob = (t * 2.2 + i as f32 * 0.55).sin() * px * 0.35;
        let glyph_c = Vec2::new(x + px * 2.5, top + bob + size * 0.5);
        // Smooth neon bloom (the font's own halo turns blocky at this size).
        gfx::glow(glyph_c, size * 1.15, col, 0.42 + 0.25 * shimmer);
        if let Some(bitmap) = font::glyph(ch) {
            draw_wordmark_glyph(&bitmap, Vec2::new(x, top + bob), px, col);
        }
    }

    // Underline beam.
    let y = top + size + px * 2.2;
    let a = Vec2::new(left + width * 0.08, y);
    let b = Vec2::new(left + width * 0.92, y);
    gfx::soft_line(a, b, px * 1.6, with_alpha(rgb(0.4, 0.9, 1.0), 0.45));
    gfx::line(a, b, px * 0.25, with_alpha(rgb(0.8, 1.0, 1.0), 0.8));

    // Twinkles.
    for i in 0..6u32 {
        let fx = gfx::hash01(i * 7 + 1);
        let fy = gfx::hash01(i * 13 + 5);
        let p = Vec2::new(
            left + width * (0.04 + 0.92 * fx),
            top - size * 0.25 + size * 1.5 * fy,
        );
        let tw = ((t * (1.3 + fx) + fy * TAU).sin()).max(0.0).powi(6);
        if tw > 0.02 {
            gfx::sparkle(p, size * 0.32 * tw, t * 0.5, with_alpha(WHITE, tw));
        }
    }

    if !behind {
        draw_orbit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dist(a: Color, b: Color) -> f32 {
        ((a.r - b.r).powi(2) + (a.g - b.g).powi(2) + (a.b - b.b).powi(2)).sqrt()
    }

    #[test]
    fn shade_colours_are_distinct_from_each_other_and_from_glim() {
        for (i, &a) in ShadeKind::ALL.iter().enumerate() {
            assert!(
                dist(shade_color(a), glim_color()) > 0.35,
                "{a:?} too close to Glim"
            );
            assert!(dist(shade_color(a), fright_color(false)) > 0.35);
            for &b in &ShadeKind::ALL[..i] {
                assert!(
                    dist(shade_color(a), shade_color(b)) > 0.35,
                    "{a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn hexagon_is_pointy_top_and_symmetric() {
        let h = hexagon(Vec2::new(10.0, 10.0), 4.0, 5.0);
        assert!((h[0] - Vec2::new(10.0, 5.0)).length() < 1e-4);
        assert!((h[3] - Vec2::new(10.0, 15.0)).length() < 1e-4);
        for k in 1..3 {
            assert!((h[k].x - 10.0 + (h[6 - k].x - 10.0)).abs() < 1e-4);
            assert!((h[k].y - h[6 - k].y).abs() < 1e-4);
        }
    }

    #[test]
    fn star_points_alternate_radii() {
        let mut pts = [Vec2::ZERO; 24];
        let n = star_points(Vec2::ZERO, 2.0, 1.0, 5, 0.0, &mut pts);
        assert_eq!(n, 10);
        for (k, p) in pts[..n].iter().enumerate() {
            let want = if k % 2 == 0 { 2.0 } else { 1.0 };
            assert!((p.length() - want).abs() < 1e-5);
        }
    }

    #[test]
    fn every_relic_has_a_colour() {
        for k in BonusKind::ALL {
            let c = bonus_color(k);
            assert!(c.a > 0.0 && c.r + c.g + c.b > 0.5);
        }
    }
}
