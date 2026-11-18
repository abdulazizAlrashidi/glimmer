//! Shared UI look: palette, text helpers, panels, bars and small decorations.
//!
//! Every position and size here is in **canvas tiles** (see [`crate::view`]);
//! the helpers convert to window pixels through the current [`Layout`], so all
//! screens scale with the window. Text is automatically shrunk so it never
//! overflows the canvas.

use crate::config::CANVAS_W;
use crate::font::{self, Align};
use crate::geom::Dir;
use crate::render::sprites;
use crate::view::Layout;
use macroquad::color::Color;
use macroquad::math::{Rect, Vec2};
use macroquad::shapes::{draw_circle, draw_line, draw_rectangle, draw_triangle};

/// Text sizes (glyph height in tiles).
pub const H1: f32 = 1.25;
pub const MENU: f32 = 0.95;
pub const BODY: f32 = 0.68;
pub const SMALL: f32 = 0.52;
pub const TINY: f32 = 0.44;

/// Horizontal centre of the canvas.
pub const CX: f32 = CANVAS_W * 0.5;

/// Keep text at least this far (tiles) from the canvas edges.
const EDGE_PAD: f32 = 0.35;

/// Animation clocks step back by this many seconds once they pass
/// `CLOCK_WRAP + CLOCK_KEEP` (see [`tick_clock`]).
pub const CLOCK_WRAP: f32 = 1800.0;
/// A wrapped clock never falls below this, so one-off intro animations
/// (all far shorter) never replay.
pub const CLOCK_KEEP: f32 = 120.0;

/// Advance an animation clock by `dt` seconds (non-finite or negative `dt`
/// is ignored) and keep it bounded. An `f32` clock left to run for hours
/// loses precision: past 65 536 s one ulp is 1/128 s, so frame steps round
/// away and every animation driven by it stutters and finally freezes. A
/// wrapped clock stays below `CLOCK_WRAP + CLOCK_KEEP` (one ulp is about
/// 0.12 ms) at the cost of one small phase jump every half hour.
pub fn tick_clock(t: &mut f32, dt: f32) {
    if dt.is_finite() && dt > 0.0 {
        *t += dt;
    }
    if !t.is_finite() {
        *t = CLOCK_KEEP;
    }
    if *t >= CLOCK_WRAP + CLOCK_KEEP {
        *t = CLOCK_KEEP + (*t - CLOCK_KEEP).rem_euclid(CLOCK_WRAP);
    }
}

/// UI palette ("neon garden at night").
pub mod pal {
    use macroquad::color::Color;

    pub const INK: Color = Color::new(0.88, 0.92, 1.0, 1.0);
    pub const DIM: Color = Color::new(0.58, 0.63, 0.82, 1.0);
    pub const FAINT: Color = Color::new(0.36, 0.40, 0.60, 1.0);
    pub const MINT: Color = Color::new(0.42, 1.0, 0.86, 1.0);
    pub const VIOLET: Color = Color::new(0.74, 0.55, 1.0, 1.0);
    pub const AMBER: Color = Color::new(1.0, 0.76, 0.32, 1.0);
    pub const CRIMSON: Color = Color::new(1.0, 0.38, 0.44, 1.0);
    pub const GREEN: Color = Color::new(0.50, 0.96, 0.48, 1.0);
    pub const PANEL: Color = Color::new(0.015, 0.02, 0.06, 0.88);
    pub const SHADOW: Color = Color::new(0.0, 0.0, 0.02, 1.0);
}

/// `c` with its alpha multiplied by `a`.
pub fn alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, c.a * a.clamp(0.0, 1.0))
}

/// Linear blend from `a` to `b`.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// Smooth 0 -> 1 -> 0 pulse with period `1 / speed` seconds.
pub fn pulse(t: f32, speed: f32) -> f32 {
    0.5 - 0.5 * (t * speed * std::f32::consts::TAU).cos()
}

/// Ease-out cubic on `[0, 1]` (clamped).
pub fn ease_out(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

/// Ease-out with a small overshoot ("pop"), clamped input.
pub fn ease_back(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

/// Fraction of a `len`-second window starting at `start` that has elapsed.
pub fn progress(t: f32, start: f32, len: f32) -> f32 {
    if len <= 0.0 {
        if t >= start {
            1.0
        } else {
            0.0
        }
    } else {
        ((t - start) / len).clamp(0.0, 1.0)
    }
}

/// Deterministic hash of `(i, salt)` to `[0, 1)`.
pub fn hash01(i: u32, salt: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// The size (<= `size`) at which `text` is at most `max_w` wide (same units).
pub fn fit_size(text: &str, size: f32, max_w: f32) -> f32 {
    let w = font::text_width(text, size);
    if w <= max_w || w <= 0.0 || max_w <= 0.0 {
        if max_w <= 0.0 {
            0.0
        } else {
            size
        }
    } else {
        size * (max_w / w)
    }
}

/// Widest text that fits at canvas x `x` with `align` without leaving the canvas.
fn room(x: f32, align: Align) -> f32 {
    let w = match align {
        Align::Left => CANVAS_W - x - EDGE_PAD,
        Align::Center => 2.0 * x.min(CANVAS_W - x) - 2.0 * EDGE_PAD,
        Align::Right => x - EDGE_PAD,
    };
    w.max(0.0)
}

/// Width of `text` in tiles at `size`.
pub fn width(text: &str, size: f32) -> f32 {
    font::text_width(text, size)
}

/// Flat text at canvas position (`y` = top of the glyph box), shrunk to fit.
pub fn text(l: &Layout, s: &str, x: f32, y: f32, size: f32, color: Color, align: Align) {
    text_max(l, s, x, y, size, room(x, align), color, align);
}

/// Flat text limited to `max_w` tiles wide (and to the canvas).
#[allow(clippy::too_many_arguments)]
pub fn text_max(
    l: &Layout,
    s: &str,
    x: f32,
    y: f32,
    size: f32,
    max_w: f32,
    color: Color,
    align: Align,
) {
    let size = fit_size(s, size, max_w.min(room(x, align)));
    if size <= 0.0 {
        return;
    }
    let p = l.canvas(x, y);
    font::draw_text(s, p.x, p.y, l.px(size), color, align);
}

/// Glowing text at canvas position, shrunk to fit.
pub fn text_glow(l: &Layout, s: &str, x: f32, y: f32, size: f32, color: Color, align: Align) {
    let size = fit_size(s, size, room(x, align));
    if size <= 0.0 {
        return;
    }
    let p = l.canvas(x, y);
    font::draw_text_glow(s, p.x, p.y, l.px(size), color, align);
}

/// Text vertically centred on `cy` instead of top-aligned.
pub fn text_mid(l: &Layout, s: &str, x: f32, cy: f32, size: f32, color: Color, align: Align) {
    text(l, s, x, cy - size * 0.5, size, color, align);
}

/// A screen heading: glowing, centred, with a thin underline flourish.
pub fn heading(l: &Layout, s: &str, y: f32, color: Color, t: f32) {
    text_glow(l, s, CX, y, H1, color, Align::Center);
    let w = width(s, H1).min(CANVAS_W - 4.0) * 0.5 + 1.2;
    let ly = y + H1 + 0.45;
    let shimmer = 0.55 + 0.25 * pulse(t, 0.4);
    line(
        l,
        CX - w,
        ly,
        CX + w,
        ly,
        0.06,
        alpha(color, 0.35 * shimmer),
    );
    line(
        l,
        CX - w * 0.55,
        ly,
        CX + w * 0.55,
        ly,
        0.05,
        alpha(color, 0.8 * shimmer),
    );
    diamond(l, CX - w - 0.35, ly, 0.16, alpha(color, 0.8));
    diamond(l, CX + w + 0.35, ly, 0.16, alpha(color, 0.8));
}

/// Footer key hints, centred at `y`.
pub fn hint(l: &Layout, s: &str, y: f32) {
    text(l, s, CX, y, SMALL, pal::DIM, Align::Center);
}

/// Line between canvas points, `thick` in tiles.
pub fn line(l: &Layout, x1: f32, y1: f32, x2: f32, y2: f32, thick: f32, color: Color) {
    let a = l.canvas(x1, y1);
    let b = l.canvas(x2, y2);
    draw_line(a.x, a.y, b.x, b.y, l.px(thick).max(1.0), color);
}

/// Filled rectangle in canvas tiles.
pub fn rect(l: &Layout, r: Rect, color: Color) {
    let p = l.canvas(r.x, r.y);
    draw_rectangle(p.x, p.y, l.px(r.w), l.px(r.h), color);
}

/// Small filled diamond centred at a canvas point.
pub fn diamond(l: &Layout, x: f32, y: f32, r: f32, color: Color) {
    let c = l.canvas(x, y);
    let r = l.px(r);
    let (n, e, s, w) = (
        Vec2::new(c.x, c.y - r),
        Vec2::new(c.x + r, c.y),
        Vec2::new(c.x, c.y + r),
        Vec2::new(c.x - r, c.y),
    );
    draw_triangle(n, e, s, color);
    draw_triangle(n, s, w, color);
}

/// Corner points of a rectangle with chamfered (cut) corners, clockwise.
fn chamfer_points(l: &Layout, r: Rect, cut: f32) -> [Vec2; 8] {
    let c = cut.min(r.w * 0.5).min(r.h * 0.5).max(0.0);
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
    [
        l.canvas(x0 + c, y0),
        l.canvas(x1 - c, y0),
        l.canvas(x1, y0 + c),
        l.canvas(x1, y1 - c),
        l.canvas(x1 - c, y1),
        l.canvas(x0 + c, y1),
        l.canvas(x0, y1 - c),
        l.canvas(x0, y0 + c),
    ]
}

/// A translucent panel with chamfered corners and a glowing neon edge.
pub fn panel(l: &Layout, r: Rect, edge: Color) {
    let pts = chamfer_points(l, r, 0.6);
    let centre = l.canvas(r.x + r.w * 0.5, r.y + r.h * 0.5);
    for i in 0..pts.len() {
        draw_triangle(centre, pts[i], pts[(i + 1) % pts.len()], pal::PANEL);
    }
    let wide = l.px(0.22).max(2.0);
    let thin = l.px(0.06).max(1.0);
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        draw_line(a.x, a.y, b.x, b.y, wide, alpha(edge, 0.12));
    }
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        draw_line(a.x, a.y, b.x, b.y, thin, alpha(edge, 0.85));
    }
}

/// Darken the whole window (behind overlays).
pub fn dim_screen(l: &Layout, amount: f32) {
    draw_rectangle(0.0, 0.0, l.screen_w, l.screen_h, alpha(pal::SHADOW, amount));
}

/// Horizontal bar of `segments` cells, the first `value` lit.
/// Returns nothing; see [`bar_hit`] for mouse picking.
#[allow(clippy::too_many_arguments)]
pub fn segmented_bar(
    l: &Layout,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    value: u8,
    segments: u8,
    color: Color,
    active: bool,
) {
    let n = f32::from(segments.max(1));
    let gap = (w / n) * 0.22;
    let seg_w = (w - gap * (n - 1.0)) / n;
    for i in 0..segments.max(1) {
        let sx = x + f32::from(i) * (seg_w + gap);
        // Segments grow slightly taller toward the right, like a level meter.
        let sh = h * (0.55 + 0.45 * (f32::from(i) + 1.0) / n);
        let sy = y + (h - sh);
        let r = Rect::new(sx, sy, seg_w, sh);
        if i < value {
            let glow = Rect::new(sx - 0.08, sy - 0.08, seg_w + 0.16, sh + 0.16);
            rect(l, glow, alpha(color, if active { 0.30 } else { 0.16 }));
            rect(
                l,
                r,
                if active {
                    color
                } else {
                    mix(color, pal::DIM, 0.45)
                },
            );
        } else {
            rect(l, r, alpha(pal::FAINT, 0.35));
        }
    }
}

/// Which value (0..=segments) a click at canvas x `px` sets on a bar drawn by
/// [`segmented_bar`] at `x` with width `w`; `None` if outside horizontally.
pub fn bar_hit(px: f32, x: f32, w: f32, segments: u8) -> Option<u8> {
    let segments = segments.max(1);
    let slop = 0.6;
    if !(x - slop..=x + w + slop).contains(&px) {
        return None;
    }
    let f = ((px - x) / w).clamp(0.0, 1.0);
    // Clicking the left edge region selects 0, otherwise the segment under it.
    let v = (f * f32::from(segments)).ceil();
    Some(v.clamp(0.0, f32::from(segments)) as u8)
}

/// The selection marker: a tiny Glim at a canvas point.
pub fn marker(l: &Layout, x: f32, cy: f32, r: f32, t: f32) {
    let bob = (t * 5.0).sin() * 0.06;
    sprites::draw_glim(l.canvas(x + bob, cy), l.px(r), Dir::Right, t, 1.0);
}

/// Twinkling 4-point sparkle (a small cross of light) at a canvas point.
pub fn sparkle(l: &Layout, x: f32, y: f32, r: f32, color: Color) {
    if r <= 0.0 || color.a <= 0.0 {
        return;
    }
    let c = l.canvas(x, y);
    let rp = l.px(r);
    let thin = (rp * 0.18).max(1.0);
    draw_circle(c.x, c.y, rp * 0.55, alpha(color, 0.18));
    draw_line(c.x - rp, c.y, c.x + rp, c.y, thin, color);
    draw_line(c.x, c.y - rp, c.x, c.y + rp, thin, color);
    draw_circle(c.x, c.y, (rp * 0.22).max(1.0), mix(color, pal::INK, 0.6));
}

/// A field of twinkling sparkles inside `area` (tiles). `count` sparkles,
/// `salt` varies the pattern, `t` animates, `strength` scales brightness.
pub fn sparkle_field(l: &Layout, area: Rect, count: u32, salt: u32, t: f32, strength: f32) {
    if strength <= 0.0 {
        return;
    }
    for i in 0..count {
        let x = area.x + hash01(i, salt) * area.w;
        let y = area.y + hash01(i, salt ^ 0x5bd1) * area.h;
        let speed = 0.6 + hash01(i, salt ^ 0x1234) * 1.4;
        let phase = hash01(i, salt ^ 0x77aa);
        let tw = pulse(t * speed + phase, 1.0).powi(3);
        let hue = match i % 3 {
            0 => pal::MINT,
            1 => pal::VIOLET,
            _ => pal::INK,
        };
        let r = 0.12 + 0.22 * tw;
        sparkle(l, x, y, r, alpha(hue, tw * strength));
    }
}

/// Convert a window-pixel point to canvas tiles.
pub fn to_canvas(l: &Layout, p: Vec2) -> Vec2 {
    let s = l.scale.max(1e-3);
    Vec2::new((p.x - l.ox) / s, (p.y - l.oy) / s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_size_shrinks_only_when_needed() {
        let s = "HELLO";
        let w = font::text_width(s, 1.0);
        assert_eq!(fit_size(s, 1.0, w + 1.0), 1.0);
        let shrunk = fit_size(s, 1.0, w * 0.5);
        assert!((font::text_width(s, shrunk) - w * 0.5).abs() < 1e-4);
        assert_eq!(fit_size(s, 1.0, 0.0), 0.0);
        assert_eq!(fit_size("", 1.0, 3.0), 1.0);
    }

    #[test]
    fn room_respects_canvas() {
        assert!(room(CX, Align::Center) <= CANVAS_W);
        assert!(room(0.0, Align::Right) == 0.0);
        assert!(room(CANVAS_W, Align::Left) == 0.0);
        assert!(room(2.0, Align::Left) < CANVAS_W - 2.0);
    }

    #[test]
    fn easing_and_progress_are_bounded() {
        for i in 0..=20 {
            let x = i as f32 / 20.0;
            assert!((0.0..=1.0).contains(&ease_out(x)));
            assert!((0.0..=1.2).contains(&ease_back(x)));
            assert!((0.0..=1.0).contains(&pulse(x * 3.0, 0.7)));
        }
        assert_eq!(ease_out(1.0), 1.0);
        assert!((ease_back(1.0) - 1.0).abs() < 1e-5);
        assert_eq!(progress(0.5, 1.0, 2.0), 0.0);
        assert_eq!(progress(2.0, 1.0, 2.0), 0.5);
        assert_eq!(progress(9.0, 1.0, 2.0), 1.0);
        assert_eq!(progress(1.0, 1.0, 0.0), 1.0);
    }

    #[test]
    fn hash_is_deterministic_and_in_range() {
        for i in 0..500 {
            let h = hash01(i, 42);
            assert!((0.0..1.0).contains(&h));
            assert_eq!(h, hash01(i, 42));
        }
        assert_ne!(hash01(1, 2), hash01(2, 1));
    }

    #[test]
    fn clocks_stay_bounded_and_precise() {
        let mut t = 0.0;
        tick_clock(&mut t, 1.5);
        tick_clock(&mut t, f32::NAN);
        tick_clock(&mut t, -3.0);
        assert_eq!(t, 1.5);
        // Days of 360 Hz frames: the clock keeps moving by whole frames.
        let dt = 1.0 / 360.0;
        let mut t = CLOCK_WRAP + CLOCK_KEEP - 0.5;
        for _ in 0..1000 {
            let before = t;
            tick_clock(&mut t, dt);
            assert!((CLOCK_KEEP..CLOCK_WRAP + CLOCK_KEEP).contains(&t), "{t}");
            let step = t - before;
            assert!(
                (step - dt).abs() < 1e-4 || (step + CLOCK_WRAP - dt).abs() < 1e-2,
                "step {step}"
            );
        }
        let mut t = f32::INFINITY;
        tick_clock(&mut t, 0.1);
        assert_eq!(t, CLOCK_KEEP);
        let mut t = 1.0e9;
        tick_clock(&mut t, 0.1);
        assert!(t < CLOCK_WRAP + CLOCK_KEEP);
    }

    #[test]
    fn bar_hit_maps_positions() {
        assert_eq!(bar_hit(-5.0, 0.0, 10.0, 10), None);
        assert_eq!(bar_hit(0.0, 0.0, 10.0, 10), Some(0));
        assert_eq!(bar_hit(0.5, 0.0, 10.0, 10), Some(1));
        assert_eq!(bar_hit(9.9, 0.0, 10.0, 10), Some(10));
        assert_eq!(bar_hit(10.4, 0.0, 10.0, 10), Some(10));
    }
}
