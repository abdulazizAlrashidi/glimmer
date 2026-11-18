//! Low-level vector primitives with per-vertex colour, drawn in window pixels.
//!
//! macroquad's own shape helpers allocate on every call and only support flat
//! colours. Everything here instead tessellates into one thread-local scratch
//! mesh (reused, so nothing allocates after warm-up) and submits it with
//! [`draw_mesh`], which batches into the current draw call exactly like the
//! built-in shapes (same texture, same pipeline), so draw order with fonts and
//! other shapes is preserved.
//!
//! Most shapes are built from *profiles*: lists of `(offset, colour)` stops
//! across a stroke or around a centre. A soft neon glow is just a profile
//! fading to transparent at its rim, so glows, gradient strokes and smooth
//! arcs are all one or two submissions each.

use macroquad::color::Color;
use macroquad::math::Vec2;
use macroquad::models::{draw_mesh, Mesh, Vertex};
use std::cell::RefCell;
use std::f32::consts::{PI, TAU};

/// Most ring stops a profile may have.
pub const MAX_STOPS: usize = 8;

/// Hard cap on the angular subdivisions of one primitive.
const MAX_SEGS: usize = 160;

/// Flush the scratch mesh before it exceeds macroquad's per-draw-call limits
/// (10 000 vertices / 5 000 indices by default).
const FLUSH_VERTS: usize = 3000;
const FLUSH_INDICES: usize = 4200;

thread_local! {
    static SCRATCH: RefCell<Mesh> = RefCell::new(Mesh {
        vertices: Vec::with_capacity(FLUSH_VERTS + 512),
        indices: Vec::with_capacity(FLUSH_INDICES + 1024),
        texture: None,
    });
}

/// Mesh builder over the scratch buffer.
pub struct Builder<'a> {
    mesh: &'a mut Mesh,
}

impl Builder<'_> {
    /// Push a vertex; returns its index.
    fn vertex(&mut self, p: Vec2, c: Color) -> u16 {
        let i = self.mesh.vertices.len() as u16;
        self.mesh
            .vertices
            .push(Vertex::new(p.x, p.y, 0.0, 0.0, 0.0, c));
        i
    }

    fn tri(&mut self, a: u16, b: u16, c: u16) {
        self.mesh.indices.extend_from_slice(&[a, b, c]);
    }

    /// Make room for `verts` more vertices / `indices` more indices,
    /// submitting what was built so far if needed.
    fn reserve(&mut self, verts: usize, indices: usize) {
        if self.mesh.vertices.len() + verts > FLUSH_VERTS
            || self.mesh.indices.len() + indices > FLUSH_INDICES
        {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if !self.mesh.indices.is_empty() {
            draw_mesh(self.mesh);
        }
        self.mesh.vertices.clear();
        self.mesh.indices.clear();
    }

    /// Triangle with per-vertex colours.
    pub fn triangle(&mut self, p: [Vec2; 3], c: [Color; 3]) {
        self.reserve(3, 3);
        let a = self.vertex(p[0], c[0]);
        let b = self.vertex(p[1], c[1]);
        let d = self.vertex(p[2], c[2]);
        self.tri(a, b, d);
    }

    /// Convex quad `p0 p1 p2 p3` (in order) with per-vertex colours.
    pub fn quad(&mut self, p: [Vec2; 4], c: [Color; 4]) {
        self.reserve(4, 6);
        let i: [u16; 4] = std::array::from_fn(|k| self.vertex(p[k], c[k]));
        self.tri(i[0], i[1], i[2]);
        self.tri(i[0], i[2], i[3]);
    }

    /// Polar mesh around `c`: concentric rings `(radius, colour)` (radii
    /// ascending; a first radius of 0 makes a filled fan) over the angular
    /// range `start .. start + sweep`.
    pub fn polar(&mut self, c: Vec2, start: f32, sweep: f32, rings: &[(f32, Color)], segs: usize) {
        if rings.is_empty() || rings.len() > MAX_STOPS || sweep.abs() < 1e-6 {
            return;
        }
        let n_rings = rings.len();
        let segs = segs.clamp(1, MAX_SEGS).min(FLUSH_INDICES / (6 * n_rings));
        let fan = rings[0].0 <= 0.0;
        let cols = segs + 1;
        let verts = if fan {
            1 + (n_rings - 1) * cols
        } else {
            n_rings * cols
        };
        let idx = segs * 6 * n_rings;
        self.reserve(verts, idx);
        if verts > FLUSH_VERTS || idx > FLUSH_INDICES {
            // Never happens with MAX_SEGS * MAX_STOPS, but stay safe.
            return;
        }
        let base = self.mesh.vertices.len() as u16;
        let first_ring = if fan {
            self.vertex(c, rings[0].1);
            1
        } else {
            0
        };
        for (r, col) in &rings[first_ring..] {
            for k in 0..cols {
                let a = start + sweep * k as f32 / segs as f32;
                self.vertex(c + Vec2::new(a.cos(), a.sin()) * *r, *col);
            }
        }
        let ring_base = |ri: usize| base + first_ring as u16 + (ri * cols) as u16;
        if fan {
            let rb = ring_base(0);
            for k in 0..segs as u16 {
                self.tri(base, rb + k, rb + k + 1);
            }
        }
        for ri in 0..(n_rings - first_ring).saturating_sub(1) {
            let (a, b) = (ring_base(ri), ring_base(ri + 1));
            for k in 0..segs as u16 {
                self.tri(a + k, b + k, b + k + 1);
                self.tri(a + k, b + k + 1, a + k + 1);
            }
        }
    }

    /// Straight strip from `a` to `b` with rows at perpendicular offsets
    /// `(offset_px, colour)` (ascending offsets).
    pub fn strip(&mut self, a: Vec2, b: Vec2, rows: &[(f32, Color)]) {
        let d = b - a;
        let len = d.length();
        if len < 1e-4 || rows.len() < 2 || rows.len() > MAX_STOPS {
            return;
        }
        let n = Vec2::new(-d.y, d.x) / len;
        self.reserve(rows.len() * 2, (rows.len() - 1) * 6);
        let base = self.mesh.vertices.len() as u16;
        for (off, col) in rows {
            self.vertex(a + n * *off, *col);
            self.vertex(b + n * *off, *col);
        }
        for r in 0..(rows.len() - 1) as u16 {
            let (p, q) = (base + r * 2, base + (r + 1) * 2);
            self.tri(p, p + 1, q + 1);
            self.tri(p, q + 1, q);
        }
    }

    /// Convex polygon fan: `center` coloured `inner`, the outline `outer`.
    pub fn fan(&mut self, center: Vec2, inner: Color, points: &[Vec2], outer: Color) {
        if points.len() < 2 {
            return;
        }
        self.reserve(points.len() + 1, points.len() * 3);
        let c = self.vertex(center, inner);
        let first = self.mesh.vertices.len() as u16;
        for p in points {
            self.vertex(*p, outer);
        }
        let n = points.len() as u16;
        for k in 0..n {
            self.tri(c, first + k, first + (k + 1) % n);
        }
    }
}

impl Builder<'_> {
    /// Filled ellipse with radii `(rx, ry)` rotated by `rot`, gradient from
    /// `inner` (centre) to `outer` (rim).
    pub fn ellipse(&mut self, c: Vec2, rx: f32, ry: f32, rot: f32, inner: Color, outer: Color) {
        let segs = segs_for(rx.max(ry), TAU).min(48);
        self.reserve(segs + 1, segs * 3);
        let (s, co) = rot.sin_cos();
        let center = self.vertex(c, inner);
        let first = self.mesh.vertices.len() as u16;
        for k in 0..segs {
            let a = k as f32 / segs as f32 * TAU;
            let (x, y) = (a.cos() * rx, a.sin() * ry);
            self.vertex(c + Vec2::new(x * co - y * s, x * s + y * co), outer);
        }
        let n = segs as u16;
        for k in 0..n {
            self.tri(center, first + k, first + (k + 1) % n);
        }
    }
}

/// Build geometry with `f` and submit it (the scratch mesh is reused).
pub fn with_builder(f: impl FnOnce(&mut Builder)) {
    SCRATCH.with(|cell| {
        // A re-entrant call (never expected) is simply skipped.
        let Ok(mut mesh) = cell.try_borrow_mut() else {
            return;
        };
        mesh.vertices.clear();
        mesh.indices.clear();
        let mut b = Builder { mesh: &mut mesh };
        f(&mut b);
        b.flush();
    });
}

// ---------------------------------------------------------------------------
// Colour helpers
// ---------------------------------------------------------------------------

/// `c` with alpha replaced.
pub fn with_alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, a.clamp(0.0, 1.0))
}

/// `c` with alpha multiplied by `k`.
pub fn fade(c: Color, k: f32) -> Color {
    Color::new(c.r, c.g, c.b, (c.a * k).clamp(0.0, 1.0))
}

/// Linear blend of two colours (alpha included).
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// Move `c` toward white by `t`, keeping alpha.
pub fn lighten(c: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::new(
        c.r + (1.0 - c.r) * t,
        c.g + (1.0 - c.g) * t,
        c.b + (1.0 - c.b) * t,
        c.a,
    )
}

/// Scale the RGB channels of `c` (darken for `k < 1`), keeping alpha.
pub fn shade(c: Color, k: f32) -> Color {
    Color::new(
        (c.r * k).clamp(0.0, 1.0),
        (c.g * k).clamp(0.0, 1.0),
        (c.b * k).clamp(0.0, 1.0),
        c.a,
    )
}

pub const WHITE: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

// ---------------------------------------------------------------------------
// Deterministic noise
// ---------------------------------------------------------------------------

/// Integer hash (lowbias32).
pub fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// Hash to a float in `[0, 1)`.
pub fn hash01(x: u32) -> f32 {
    (hash(x) >> 8) as f32 / (1u32 << 24) as f32
}

/// Smooth 1-D value noise in `[-1, 1]`.
pub fn noise1(t: f32, seed: u32) -> f32 {
    let i = t.floor();
    let f = t - i;
    let i = i as i32 as u32;
    let a = hash01(i.wrapping_add(seed.wrapping_mul(0x9e37_79b9)));
    let b = hash01(
        i.wrapping_add(1)
            .wrapping_add(seed.wrapping_mul(0x9e37_79b9)),
    );
    let s = f * f * (3.0 - 2.0 * f);
    (a + (b - a) * s) * 2.0 - 1.0
}

// ---------------------------------------------------------------------------
// Shapes
// ---------------------------------------------------------------------------

/// Angular subdivisions for a smooth arc of radius `r_px` over `sweep`.
pub fn segs_for(r_px: f32, sweep: f32) -> usize {
    let full = PI * (4.0 * r_px.max(1.0)).sqrt();
    ((full * sweep.abs() / TAU).ceil() as usize).clamp(3, MAX_SEGS)
}

/// Filled circle.
pub fn circle(c: Vec2, r: f32, color: Color) {
    with_builder(|b| b.circle(c, r, color));
}

/// Filled circle with a radial gradient from `inner` (centre) to `outer` (rim).
pub fn radial(c: Vec2, r: f32, inner: Color, outer: Color) {
    if r <= 0.0 {
        return;
    }
    with_builder(|b| b.polar(c, 0.0, TAU, &[(0.0, inner), (r, outer)], segs_for(r, TAU)));
}

/// Radial profile of [`glow`]: `(fraction of the radius, alpha factor)`.
const GLOW_STOPS: [(f32, f32); 5] = [
    (0.0, 1.0),
    (0.16, 0.62),
    (0.38, 0.27),
    (0.66, 0.08),
    (1.0, 0.0),
];

/// Alpha factor of [`glow`] at `f` = distance / radius (1 in the centre, 0
/// from the rim outward), interpolated like the drawn rings.
pub fn glow_falloff(f: f32) -> f32 {
    if !f.is_finite() || f >= 1.0 {
        return 0.0;
    }
    let f = f.max(0.0);
    for w in GLOW_STOPS.windows(2) {
        let ((f0, k0), (f1, k1)) = (w[0], w[1]);
        if f <= f1 {
            return k0 + (k1 - k0) * (f - f0) / (f1 - f0);
        }
    }
    0.0
}

/// Soft radial glow: `color` at `intensity` alpha in the centre fading
/// smoothly to nothing at radius `r`.
pub fn glow(c: Vec2, r: f32, color: Color, intensity: f32) {
    with_builder(|b| b.glow(c, r, color, intensity));
}

/// Cheaper two-ring glow for tiny lights (motes, particles).
pub fn glow_small(c: Vec2, r: f32, color: Color, intensity: f32) {
    with_builder(|b| b.glow_small(c, r, color, intensity));
}

impl Builder<'_> {
    /// Filled circle.
    pub fn circle(&mut self, c: Vec2, r: f32, color: Color) {
        if r > 0.0 && color.a > 0.0 {
            self.polar(c, 0.0, TAU, &[(0.0, color), (r, color)], segs_for(r, TAU));
        }
    }

    /// Soft radial glow (see [`glow`]).
    pub fn glow(&mut self, c: Vec2, r: f32, color: Color, intensity: f32) {
        if r <= 0.0 || intensity <= 0.0 {
            return;
        }
        let a = (color.a * intensity).clamp(0.0, 1.0);
        let rings = GLOW_STOPS.map(|(f, k)| (r * f, with_alpha(color, a * k)));
        self.polar(c, 0.0, TAU, &rings, segs_for(r * 0.5, TAU));
    }

    /// Cheap two-ring glow (see [`glow_small`]).
    pub fn glow_small(&mut self, c: Vec2, r: f32, color: Color, intensity: f32) {
        if r <= 0.0 || intensity <= 0.0 {
            return;
        }
        let a = (color.a * intensity).clamp(0.0, 1.0);
        let rings = [
            (0.0, with_alpha(color, a)),
            (r * 0.35, with_alpha(color, a * 0.3)),
            (r, with_alpha(color, 0.0)),
        ];
        self.polar(c, 0.0, TAU, &rings, segs_for(r * 0.4, TAU).min(16));
    }
}

/// Ring (annulus) of the given stroke `width` centred on radius `r`.
pub fn ring(c: Vec2, r: f32, width: f32, color: Color) {
    arc(c, r, 0.0, TAU, width, color);
}

/// Soft ring: alpha peaks on radius `r` and fades out over `width / 2`.
pub fn soft_ring(c: Vec2, r: f32, width: f32, color: Color) {
    soft_arc(c, r, 0.0, TAU, width, color);
}

/// Solid arc stroke.
pub fn arc(c: Vec2, r: f32, start: f32, sweep: f32, width: f32, color: Color) {
    if width <= 0.0 || color.a <= 0.0 {
        return;
    }
    let h = width * 0.5;
    let rings = [((r - h).max(0.0), color), (r + h, color)];
    with_builder(|b| b.polar(c, start, sweep, &rings, segs_for(r + h, sweep)));
}

/// Soft (glowing) arc stroke: transparent at both edges.
pub fn soft_arc(c: Vec2, r: f32, start: f32, sweep: f32, width: f32, color: Color) {
    if width <= 0.0 || color.a <= 0.0 {
        return;
    }
    let h = width * 0.5;
    let z = with_alpha(color, 0.0);
    let m = fade(color, 0.4);
    let rings = [
        ((r - h).max(0.0), z),
        ((r - h * 0.45).max(0.0), m),
        (r, color),
        (r + h * 0.45, m),
        (r + h, z),
    ];
    with_builder(|b| b.polar(c, start, sweep, &rings, segs_for(r + h, sweep)));
}

/// Solid line.
pub fn line(a: Vec2, b: Vec2, width: f32, color: Color) {
    if width <= 0.0 || color.a <= 0.0 {
        return;
    }
    let h = width * 0.5;
    with_builder(|m| m.strip(a, b, &[(-h, color), (h, color)]));
}

/// Line with round caps.
pub fn line_round(a: Vec2, b: Vec2, width: f32, color: Color) {
    line(a, b, width, color);
    circle(a, width * 0.5, color);
    circle(b, width * 0.5, color);
}

/// Soft (glowing) line: transparent at both edges.
pub fn soft_line(a: Vec2, b: Vec2, width: f32, color: Color) {
    if width <= 0.0 || color.a <= 0.0 {
        return;
    }
    let h = width * 0.5;
    let z = with_alpha(color, 0.0);
    let m = fade(color, 0.4);
    with_builder(|s| {
        s.strip(
            a,
            b,
            &[(-h, z), (-h * 0.45, m), (0.0, color), (h * 0.45, m), (h, z)],
        )
    });
}

/// Line whose colour goes from `ca` at `a` to `cb` at `b` (solid width).
pub fn line_gradient(a: Vec2, b: Vec2, width: f32, ca: Color, cb: Color) {
    let d = b - a;
    let len = d.length();
    if len < 1e-4 || width <= 0.0 {
        return;
    }
    let n = Vec2::new(-d.y, d.x) / len * (width * 0.5);
    with_builder(|m| m.quad([a + n, b + n, b - n, a - n], [ca, cb, cb, ca]));
}

/// Tapered beam from `a` (half width `wa`) to `b` (half width `wb`), colour
/// fading from `ca` to `cb`.
pub fn taper(a: Vec2, b: Vec2, wa: f32, wb: f32, ca: Color, cb: Color) {
    let d = b - a;
    let len = d.length();
    if len < 1e-4 {
        return;
    }
    let n = Vec2::new(-d.y, d.x) / len;
    with_builder(|m| {
        m.quad(
            [a + n * wa, b + n * wb, b - n * wb, a - n * wa],
            [ca, cb, cb, ca],
        )
    });
}

/// Convex polygon with a radial-ish gradient (`inner` at `center`).
pub fn poly_fan(center: Vec2, inner: Color, points: &[Vec2], outer: Color) {
    with_builder(|b| b.fan(center, inner, points, outer));
}

/// Closed polyline outline of a convex polygon (mitre-free: segments plus
/// small joint discs).
pub fn poly_outline(points: &[Vec2], width: f32, color: Color) {
    let n = points.len();
    if n < 2 {
        return;
    }
    with_builder(|b| {
        let h = width * 0.5;
        for k in 0..n {
            let (p, q) = (points[k], points[(k + 1) % n]);
            b.strip(p, q, &[(-h, color), (h, color)]);
            b.polar(p, 0.0, TAU, &[(0.0, color), (h, color)], 8);
        }
    });
}

/// Filled ellipse (see [`Builder::ellipse`]).
pub fn ellipse(c: Vec2, rx: f32, ry: f32, rot: f32, inner: Color, outer: Color) {
    if rx <= 0.0 || ry <= 0.0 {
        return;
    }
    with_builder(|b| b.ellipse(c, rx, ry, rot, inner, outer));
}

/// Open polyline with round joints.
pub fn polyline(points: &[Vec2], width: f32, color: Color) {
    if points.len() < 2 || width <= 0.0 || color.a <= 0.0 {
        return;
    }
    let h = width * 0.5;
    with_builder(|b| {
        for w in points.windows(2) {
            b.strip(w[0], w[1], &[(-h, color), (h, color)]);
        }
        for p in &points[1..points.len() - 1] {
            b.polar(*p, 0.0, TAU, &[(0.0, color), (h, color)], 6);
        }
    });
}

/// Axis-aligned rectangle with a vertical gradient.
pub fn rect_vgradient(x: f32, y: f32, w: f32, h: f32, top: Color, bottom: Color) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let p = [
        Vec2::new(x, y),
        Vec2::new(x + w, y),
        Vec2::new(x + w, y + h),
        Vec2::new(x, y + h),
    ];
    with_builder(|b| b.quad(p, [top, top, bottom, bottom]));
}

/// Four-pointed sparkle (a "twinkle" star) of radius `r`.
pub fn sparkle(c: Vec2, r: f32, angle: f32, color: Color) {
    if r <= 0.0 || color.a <= 0.0 {
        return;
    }
    let z = with_alpha(color, 0.0);
    let w = r * 0.16;
    with_builder(|b| {
        for k in 0..4 {
            let a = angle + k as f32 * PI * 0.5;
            let d = Vec2::new(a.cos(), a.sin());
            let n = Vec2::new(-d.y, d.x);
            b.quad(
                [
                    c,
                    c + d * (r * 0.28) + n * w,
                    c + d * r,
                    c + d * (r * 0.28) - n * w,
                ],
                [color, color, z, color],
            );
        }
    });
    glow_small(c, r * 0.7, color, 0.5);
}

/// Outline points of a rounded rectangle `min..max` (clockwise on screen,
/// `steps` subdivisions per corner, at most 10). Returns the point count.
pub fn rounded_rect(
    min: Vec2,
    max: Vec2,
    radius: f32,
    steps: usize,
    out: &mut [Vec2; 44],
) -> usize {
    let steps = steps.clamp(1, 10);
    let r = radius
        .min((max.x - min.x) * 0.5)
        .min((max.y - min.y) * 0.5)
        .max(0.0);
    let centres = [
        Vec2::new(max.x - r, min.y + r),
        Vec2::new(max.x - r, max.y - r),
        Vec2::new(min.x + r, max.y - r),
        Vec2::new(min.x + r, min.y + r),
    ];
    let mut n = 0;
    for (k, c) in centres.iter().enumerate() {
        let a0 = -PI * 0.5 + k as f32 * PI * 0.5;
        for s in 0..=steps {
            let a = a0 + PI * 0.5 * s as f32 / steps as f32;
            out[n] = *c + Vec2::new(a.cos(), a.sin()) * r;
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_counts_scale_with_size() {
        assert!(segs_for(2.0, TAU) >= 3);
        assert!(segs_for(400.0, TAU) > segs_for(20.0, TAU));
        assert!(segs_for(1e9, TAU) <= MAX_SEGS);
        assert!(segs_for(20.0, PI * 0.5) < segs_for(20.0, TAU));
    }

    #[test]
    fn colour_helpers() {
        let c = Color::new(0.2, 0.4, 0.6, 0.5);
        assert_eq!(with_alpha(c, 2.0).a, 1.0);
        assert!((fade(c, 0.5).a - 0.25).abs() < 1e-6);
        assert_eq!(mix(c, WHITE, 0.0), c);
        assert_eq!(mix(c, WHITE, 1.0), WHITE);
        assert_eq!(lighten(c, 1.0), Color::new(1.0, 1.0, 1.0, 0.5));
        assert!(shade(c, 10.0).b <= 1.0);
    }

    #[test]
    fn glow_falloff_matches_the_drawn_rings() {
        for (f, k) in GLOW_STOPS {
            assert!((glow_falloff(f) - k).abs() < 1e-6, "{f}");
        }
        assert!((glow_falloff(0.08) - 0.81).abs() < 1e-5);
        assert_eq!(glow_falloff(1.5), 0.0);
        assert_eq!(glow_falloff(-1.0), 1.0);
        assert_eq!(glow_falloff(f32::NAN), 0.0);
        // Monotonic falloff.
        let mut last = 1.0;
        for i in 0..=100 {
            let k = glow_falloff(i as f32 / 100.0);
            assert!(k <= last + 1e-6);
            last = k;
        }
    }

    #[test]
    fn noise_is_bounded_and_deterministic() {
        for i in 0..1000 {
            let t = i as f32 * 0.137;
            let n = noise1(t, 7);
            assert!((-1.0..=1.0).contains(&n));
            assert_eq!(n, noise1(t, 7));
            assert!((0.0..1.0).contains(&hash01(i)));
        }
        assert_ne!(hash(1), hash(2));
    }

    #[test]
    fn rounded_rect_stays_in_bounds() {
        let mut pts = [Vec2::ZERO; 44];
        let (min, max) = (Vec2::new(0.0, 0.0), Vec2::new(10.0, 4.0));
        let n = rounded_rect(min, max, 3.0, 6, &mut pts);
        assert_eq!(n, 28);
        for p in &pts[..n] {
            assert!(p.x >= -1e-4 && p.x <= 10.0 + 1e-4 && p.y >= -1e-4 && p.y <= 4.0 + 1e-4);
        }
        // Radius is limited to half the shorter side, so the ends are round.
        assert!(pts[..n]
            .iter()
            .any(|p| (p.x - 10.0).abs() < 1e-4 && (p.y - 2.0).abs() < 1e-4));
    }
}
