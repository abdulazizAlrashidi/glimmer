//! Procedurally drawn window icon (a glowing firefly orb on a dark rounded
//! square), produced at 16, 32 and 64 px.
//!
//! The artwork is described analytically in a resolution-independent unit
//! square (`u` and `v` in [0, 1], `v` pointing down) and rasterised with 4x4
//! supersampling per pixel, so every size is anti-aliased and the result is
//! bit-for-bit deterministic. Layers, back to front:
//!
//! 1. a deep-navy rounded-square tile with a vertical gradient and a faint
//!    mint rim light,
//! 2. a few distant fireflies (tiny soft specks),
//! 3. two translucent wings with brighter edges,
//! 4. a mint-cyan halo (additive),
//! 5. two fine antennae,
//! 6. the orb itself, white-hot at the centre and mint at its edge.

use macroquad::miniquad::conf::Icon;

/// Supersampling grid per pixel axis (4x4 = 16 samples per pixel).
const SUPERSAMPLE: usize = 4;

type Rgb = [f32; 3];

const WHITE: Rgb = [1.0, 1.0, 1.0];
/// Glim's mint-cyan.
const MINT: Rgb = [0.36, 1.0, 0.84];
const NAVY_TOP: Rgb = [0.075, 0.10, 0.23];
const NAVY_BOTTOM: Rgb = [0.018, 0.026, 0.085];
const RIM: Rgb = [0.32, 0.78, 0.84];
const WING: Rgb = [0.72, 0.95, 1.0];

/// Transparent margin around the tile and its corner radius (unit square).
const TILE_MARGIN: f32 = 0.055;
const TILE_RADIUS: f32 = 0.2;
/// Width of the inner rim light.
const RIM_WIDTH: f32 = 0.035;

const ORB_CENTER: (f32, f32) = (0.5, 0.57);

/// Wing ellipse: centre offset from the orb (mirrored for the left wing),
/// semi-axes and tilt (radians, raising the outer tip).
const WING_OFFSET: (f32, f32) = (0.165, -0.15);
const WING_AXES: (f32, f32) = (0.17, 0.08);
const WING_TILT: f32 = 0.72;

/// Antennae: from the top of the orb up and outward (mirrored), and width.
const ANTENNA_FROM: (f32, f32) = (0.035, -0.13);
const ANTENNA_TO: (f32, f32) = (0.085, -0.27);
const ANTENNA_WIDTH: f32 = 0.014;

/// Distant fireflies: centre and radius.
const SPECKS: [(f32, f32, f32); 4] = [
    (0.20, 0.22, 0.022),
    (0.83, 0.25, 0.016),
    (0.79, 0.83, 0.020),
    (0.21, 0.80, 0.014),
];

/// Size-dependent tuning. Small icons drop sub-pixel details (specks,
/// antennae) and exaggerate the silhouette (bigger orb, bolder wings) so they
/// stay legible, the usual trick for hand-tuned small icons.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Detail {
    orb_radius: f32,
    /// Multiplier of the additive halo.
    halo: f32,
    /// Wing alpha in the middle of the wing and extra alpha at its edge.
    wing_fill: f32,
    wing_edge: f32,
    /// Scale applied to the wing ellipse.
    wing_scale: f32,
    /// Multiplier of the rim light.
    rim: f32,
    specks: bool,
    antennae: bool,
}

impl Detail {
    fn for_size(size: usize) -> Detail {
        match size {
            0..=23 => Detail {
                orb_radius: 0.19,
                halo: 0.8,
                wing_fill: 0.42,
                wing_edge: 0.22,
                wing_scale: 1.12,
                rim: 0.5,
                specks: false,
                antennae: false,
            },
            24..=47 => Detail {
                orb_radius: 0.16,
                halo: 0.9,
                wing_fill: 0.3,
                wing_edge: 0.3,
                wing_scale: 1.05,
                rim: 0.8,
                specks: true,
                antennae: false,
            },
            _ => Detail {
                orb_radius: 0.15,
                halo: 1.0,
                wing_fill: 0.24,
                wing_edge: 0.34,
                wing_scale: 1.0,
                rim: 1.0,
                specks: true,
                antennae: true,
            },
        }
    }
}

/// Build the window icon.
pub fn window_icon() -> Icon {
    Icon {
        small: render_fixed(16),
        medium: render_fixed(32),
        big: render_fixed(64),
    }
}

/// Render a `size` x `size` icon into a fixed-size array (`N` must be
/// `size * size * 4`).
fn render_fixed<const N: usize>(size: usize) -> [u8; N] {
    let mut out = [0u8; N];
    for (dst, src) in out.iter_mut().zip(render_rgba(size)) {
        *dst = src;
    }
    out
}

/// Render the icon artwork as `size` x `size` RGBA8 (row-major). Pure, so
/// it's easy to test.
///
/// Colours are straight (non-premultiplied) alpha, as every platform's
/// window-icon API expects.
pub fn render_rgba(size: usize) -> Vec<u8> {
    let Some(len) = size.checked_mul(size).and_then(|n| n.checked_mul(4)) else {
        return Vec::new();
    };
    let mut out = vec![0u8; len];
    if size == 0 {
        return out;
    }
    let detail = Detail::for_size(size);
    let step = 1.0 / (size * SUPERSAMPLE) as f32;
    let samples = (SUPERSAMPLE * SUPERSAMPLE) as f32;
    let (pixels, _) = out.as_chunks_mut::<4>();
    for (i, px) in pixels.iter_mut().enumerate() {
        let (x, y) = (i % size, i / size);
        // Premultiplied accumulation: colour * coverage, then coverage.
        let mut acc: Rgb = [0.0; 3];
        let mut covered = 0.0f32;
        for sy in 0..SUPERSAMPLE {
            let v = ((y * SUPERSAMPLE + sy) as f32 + 0.5) * step;
            for sx in 0..SUPERSAMPLE {
                let u = ((x * SUPERSAMPLE + sx) as f32 + 0.5) * step;
                if let Some(c) = shade(u, v, detail) {
                    for (a, c) in acc.iter_mut().zip(c) {
                        *a += c;
                    }
                    covered += 1.0;
                }
            }
        }
        if covered > 0.0 {
            for (dst, a) in px.iter_mut().zip(acc) {
                *dst = to_u8(a / covered);
            }
            px[3] = to_u8(covered / samples);
        }
    }
    out
}

/// Colour of the artwork at unit-square point `(u, v)`, `None` outside the tile.
fn shade(u: f32, v: f32, detail: Detail) -> Option<Rgb> {
    let d = rounded_square_sdf(u - 0.5, v - 0.5, 0.5 - TILE_MARGIN, TILE_RADIUS);
    if d > 0.0 {
        return None;
    }

    // 1. Tile: vertical navy gradient plus a rim light that is stronger at the top.
    let mut col = mix(NAVY_TOP, NAVY_BOTTOM, smoothstep(0.05, 0.95, v));
    let rim = smoothstep(-RIM_WIDTH, 0.0, d) * (0.25 + 0.35 * (1.0 - v)) * detail.rim;
    col = mix(col, RIM, rim);

    // 2. Distant fireflies.
    if detail.specks {
        for (sx, sy, r) in SPECKS {
            let k = gauss(dist(u, v, sx, sy), r);
            col = add(col, MINT, 0.75 * k);
        }
    }

    let (ox, oy) = ORB_CENTER;
    let (dx, dy) = (u - ox, v - oy);

    // 3. Wings (mirror the point into the right half, test the right wing).
    let (wx, wy) = (dx.abs() - WING_OFFSET.0, dy - WING_OFFSET.1);
    let (sin, cos) = WING_TILT.sin_cos();
    // Rotate by +tilt so the wing's long axis points up and outward.
    let (lx, ly) = (wx * cos - wy * sin, wx * sin + wy * cos);
    let (ax, ay) = (
        WING_AXES.0 * detail.wing_scale,
        WING_AXES.1 * detail.wing_scale,
    );
    let e = (lx / ax).powi(2) + (ly / ay).powi(2);
    if e <= 1.0 {
        let alpha = detail.wing_fill + detail.wing_edge * smoothstep(0.5, 1.0, e);
        col = mix(col, WING, alpha);
    }

    // 4. Halo: a tight bright glow plus a wide soft one (additive light).
    let r = dx.hypot(dy);
    let glow = (0.85 * gauss(r, 0.16) + 0.22 * gauss(r, 0.34)) * detail.halo;
    col = add(col, MINT, glow);

    // 5. Antennae.
    if detail.antennae {
        let (from, to) = (ANTENNA_FROM, ANTENNA_TO);
        let ad = segment_dist(dx.abs(), dy, from.0, from.1, to.0, to.1);
        if ad <= ANTENNA_WIDTH * 0.5 {
            col = mix(col, MINT, 0.85);
        }
    }

    // 6. The orb: white-hot core fading to mint at the rim.
    if r <= detail.orb_radius {
        let t = smoothstep(0.2, 1.0, r / detail.orb_radius);
        col = mix(WHITE, MINT, t * t);
    }

    Some(col.map(|c| c.clamp(0.0, 1.0)))
}

/// Signed distance from `(x, y)` (relative to the centre) to a rounded square
/// of half-size `half` and corner radius `radius`; negative inside.
fn rounded_square_sdf(x: f32, y: f32, half: f32, radius: f32) -> f32 {
    let qx = x.abs() - (half - radius);
    let qy = y.abs() - (half - radius);
    let (ox, oy) = (qx.max(0.0), qy.max(0.0));
    ox.hypot(oy) + qx.max(qy).min(0.0) - radius
}

/// Distance from `(px, py)` to the segment from `(ax, ay)` to `(bx, by)`.
fn segment_dist(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (abx, aby) = (bx - ax, by - ay);
    let (apx, apy) = (px - ax, py - ay);
    let len2 = abx * abx + aby * aby;
    let t = if len2 > 0.0 {
        ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    dist(px, py, ax + abx * t, ay + aby * t)
}

fn dist(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    (ax - bx).hypot(ay - by)
}

/// Gaussian falloff with characteristic radius `r` (1 at the centre).
fn gauss(d: f32, r: f32) -> f32 {
    (-(d / r).powi(2)).exp()
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn add(a: Rgb, b: Rgb, k: f32) -> Rgb {
    [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k]
}

fn to_u8(x: f32) -> u8 {
    // Clamped to 0..=255 before the cast, so the conversion is exact.
    (x.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZES: [usize; 3] = [16, 32, 64];

    fn pixel(buf: &[u8], size: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * size + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn buffer_sizes() {
        for size in [0, 1, 2, 16, 32, 48, 64] {
            assert_eq!(render_rgba(size).len(), size * size * 4, "size {size}");
        }
        assert!(render_rgba(usize::MAX).is_empty(), "overflow is handled");
    }

    #[test]
    fn centre_is_bright_and_opaque() {
        for size in SIZES {
            let buf = render_rgba(size);
            let [r, g, b, a] = pixel(&buf, size, size / 2, size / 2);
            assert_eq!(a, 255, "size {size}");
            assert!(
                r > 200 && g > 230 && b > 210,
                "size {size}: {:?}",
                (r, g, b)
            );
        }
    }

    #[test]
    fn corners_are_transparent() {
        for size in SIZES {
            let buf = render_rgba(size);
            let last = size - 1;
            for (x, y) in [(0, 0), (last, 0), (0, last), (last, last)] {
                assert_eq!(
                    pixel(&buf, size, x, y)[3],
                    0,
                    "size {size} corner ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn tile_is_dark_and_opaque_away_from_the_orb() {
        let size = 64;
        let buf = render_rgba(size);
        // Bottom centre, inside the tile but far from the orb and specks.
        let [r, g, b, a] = pixel(&buf, size, size / 2, size - 8);
        assert_eq!(a, 255);
        assert!(r < 90 && g < 110 && b < 120, "{:?}", (r, g, b));
        assert!(b >= r, "the backdrop is navy, not warm");
        // Edge midpoints are covered by the tile (with a small margin).
        assert_eq!(pixel(&buf, size, size / 2, 5)[3], 255);
        assert_eq!(pixel(&buf, size, 5, size / 2)[3], 255);
        assert_eq!(pixel(&buf, size, size / 2, 0)[3], 0);
    }

    #[test]
    fn wings_lighten_the_tile_symmetrically() {
        let size = 64;
        let buf = render_rgba(size);
        let luma = |p: [u8; 4]| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]);
        // Outer part of each wing (tip region) vs. the bare tile below it.
        let tip_x = ((ORB_CENTER.0 + WING_OFFSET.0 + 0.06) * size as f32) as usize;
        let tip_y = ((ORB_CENTER.1 + WING_OFFSET.1 - 0.05) * size as f32) as usize;
        let right = pixel(&buf, size, tip_x, tip_y);
        let left = pixel(&buf, size, size - 1 - tip_x, tip_y);
        let bare = pixel(&buf, size, size / 2, size - 8);
        assert!(luma(right) > luma(bare) + 60, "{right:?} vs {bare:?}");
        assert!(
            luma(left).abs_diff(luma(right)) <= 6,
            "{left:?} vs {right:?}"
        );
    }

    #[test]
    fn deterministic() {
        for size in SIZES {
            assert_eq!(render_rgba(size), render_rgba(size));
        }
    }

    #[test]
    fn window_icon_matches_render() {
        let icon = window_icon();
        assert_eq!(icon.small.as_slice(), render_rgba(16).as_slice());
        assert_eq!(icon.medium.as_slice(), render_rgba(32).as_slice());
        assert_eq!(icon.big.as_slice(), render_rgba(64).as_slice());
    }

    #[test]
    fn small_sizes_use_simplified_artwork() {
        let (small, medium, big) = (
            Detail::for_size(16),
            Detail::for_size(32),
            Detail::for_size(64),
        );
        assert!(!small.specks && !small.antennae);
        assert!(medium.specks && !medium.antennae);
        assert!(big.specks && big.antennae);
        assert!(small.orb_radius > medium.orb_radius && medium.orb_radius >= big.orb_radius);
        assert_eq!(Detail::for_size(512), big);
    }

    #[test]
    fn sdf_sign() {
        assert!(rounded_square_sdf(0.0, 0.0, 0.45, 0.2) < 0.0);
        assert!(rounded_square_sdf(0.5, 0.5, 0.45, 0.2) > 0.0);
        assert!((rounded_square_sdf(0.45, 0.0, 0.45, 0.2)).abs() < 1e-6);
    }
}
