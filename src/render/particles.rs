//! Lightweight particle system (visual only), positions in maze space.
//!
//! Particles are simple points with velocity and exponential drag; the
//! renderer draws them as tiny soft lights that shrink and fade with age.
//! Expanding shockwave [`Ring`]s are kept alongside. Everything is
//! deterministic (hash-based jitter from a caller-supplied seed) and both
//! pools are bounded, so nothing grows without limit and nothing allocates
//! once the pools have warmed up.

use super::gfx::{hash, hash01, lighten};
use macroquad::color::Color;
use macroquad::math::Vec2;
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub pos: Vec2,
    pub vel: Vec2,
    pub age: f32,
    pub life: f32,
    pub size: f32,
    pub color: Color,
    pub drag: f32,
}

impl Particle {
    /// Remaining life fraction, from 1 down to 0.
    pub fn remaining(&self) -> f32 {
        if self.life <= 0.0 {
            0.0
        } else {
            (1.0 - self.age / self.life).clamp(0.0, 1.0)
        }
    }
}

/// An expanding, fading shockwave ring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub pos: Vec2,
    pub age: f32,
    pub life: f32,
    /// Radius reached at the end of its life (tiles).
    pub radius: f32,
    /// Stroke width at birth (tiles).
    pub width: f32,
    pub color: Color,
}

impl Ring {
    /// Progress from 0 to 1 over the ring's life.
    pub fn progress(&self) -> f32 {
        if self.life <= 0.0 {
            1.0
        } else {
            (self.age / self.life).clamp(0.0, 1.0)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Particles {
    pub items: Vec<Particle>,
    rings: Vec<Ring>,
}

/// Default drag (1/s) of burst particles.
const BURST_DRAG: f32 = 3.2;

impl Particles {
    pub const MAX: usize = 2048;
    /// Most simultaneous shockwave rings.
    pub const MAX_RINGS: usize = 48;

    pub fn new() -> Particles {
        Particles {
            items: Vec::with_capacity(Self::MAX),
            rings: Vec::with_capacity(Self::MAX_RINGS),
        }
    }

    /// Radial burst of `count` particles.
    #[allow(clippy::too_many_arguments)]
    pub fn burst(
        &mut self,
        pos: Vec2,
        count: usize,
        speed: f32,
        life: f32,
        size: f32,
        color: Color,
        seed: u32,
    ) {
        self.spray(pos, 0.0, TAU, count, speed, life, size, color, seed);
    }

    /// Burst of `count` particles spread over the angular range
    /// `heading +/- spread / 2` (radians, screen space).
    #[allow(clippy::too_many_arguments)]
    pub fn spray(
        &mut self,
        pos: Vec2,
        heading: f32,
        spread: f32,
        count: usize,
        speed: f32,
        life: f32,
        size: f32,
        color: Color,
        seed: u32,
    ) {
        let count = count.min(Self::MAX);
        if count == 0 || life <= 0.0 {
            return;
        }
        let overflow = (self.items.len() + count).saturating_sub(Self::MAX);
        if overflow > 0 {
            self.items.drain(..overflow);
        }
        let full_circle = spread >= TAU - 1e-4;
        for i in 0..count {
            let k = hash(seed ^ (i as u32).wrapping_mul(0x9e37_79b9));
            let r = |salt: u32| hash01(k ^ salt);
            let a = if full_circle {
                (i as f32 + r(1) * 0.8) / count as f32 * TAU
            } else {
                heading + (r(1) - 0.5) * spread
            };
            let v = speed * (0.35 + 0.65 * r(2));
            let tint = if r(5) < 0.3 {
                lighten(color, 0.55)
            } else {
                color
            };
            self.items.push(Particle {
                pos,
                vel: Vec2::new(a.cos(), a.sin()) * v,
                age: 0.0,
                life: life * (0.6 + 0.4 * r(3)),
                size: size * (0.55 + 0.45 * r(4)),
                color: tint,
                drag: BURST_DRAG,
            });
        }
    }

    /// Add an expanding shockwave ring.
    pub fn ring(&mut self, pos: Vec2, radius: f32, width: f32, life: f32, color: Color) {
        if life <= 0.0 {
            return;
        }
        if self.rings.len() >= Self::MAX_RINGS {
            self.rings.remove(0);
        }
        self.rings.push(Ring {
            pos,
            age: 0.0,
            life,
            radius,
            width,
            color,
        });
    }

    /// Live shockwave rings.
    pub fn rings(&self) -> &[Ring] {
        &self.rings
    }

    pub fn update(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        for p in &mut self.items {
            p.age += dt;
            p.pos += p.vel * dt;
            p.vel *= (-p.drag * dt).exp();
        }
        self.items.retain(|p| p.age < p.life);
        for r in &mut self.rings {
            r.age += dt;
        }
        self.rings.retain(|r| r.age < r.life);
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.rings.clear();
    }

    /// True when no particle or ring is alive.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.rings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: Color = Color {
        r: 1.0,
        g: 0.5,
        b: 0.2,
        a: 1.0,
    };

    #[test]
    fn burst_spawns_and_expires() {
        let mut p = Particles::new();
        p.burst(Vec2::new(3.0, 4.0), 20, 5.0, 0.5, 0.2, C, 1);
        assert_eq!(p.items.len(), 20);
        for q in &p.items {
            assert_eq!(q.pos, Vec2::new(3.0, 4.0));
            assert!(q.life > 0.0 && q.life <= 0.5);
            assert!(q.vel.length() <= 5.0 + 1e-4 && q.vel.length() > 0.0);
        }
        p.update(0.1);
        assert!(p.items.iter().all(|q| q.pos != Vec2::new(3.0, 4.0)));
        for _ in 0..10 {
            p.update(0.1);
        }
        assert!(p.is_empty());
    }

    #[test]
    fn bursts_are_deterministic_per_seed() {
        let mut a = Particles::new();
        let mut b = Particles::new();
        a.burst(Vec2::ZERO, 8, 3.0, 1.0, 0.1, C, 42);
        b.burst(Vec2::ZERO, 8, 3.0, 1.0, 0.1, C, 42);
        assert_eq!(a.items, b.items);
        let mut c = Particles::new();
        c.burst(Vec2::ZERO, 8, 3.0, 1.0, 0.1, C, 43);
        assert_ne!(a.items, c.items);
    }

    #[test]
    fn pool_is_bounded() {
        let mut p = Particles::new();
        for s in 0..10 {
            p.burst(Vec2::ZERO, 500, 1.0, 10.0, 0.1, C, s);
        }
        assert_eq!(p.items.len(), Particles::MAX);
        for _ in 0..100 {
            p.ring(Vec2::ZERO, 2.0, 0.2, 1.0, C);
        }
        assert_eq!(p.rings().len(), Particles::MAX_RINGS);
    }

    #[test]
    fn drag_slows_particles() {
        let mut p = Particles::new();
        p.burst(Vec2::ZERO, 4, 4.0, 5.0, 0.1, C, 3);
        let v0: f32 = p.items.iter().map(|q| q.vel.length()).sum();
        p.update(0.2);
        let v1: f32 = p.items.iter().map(|q| q.vel.length()).sum();
        assert!(v1 < v0);
    }

    #[test]
    fn spray_respects_heading() {
        let mut p = Particles::new();
        p.spray(Vec2::ZERO, 0.0, 0.5, 30, 2.0, 1.0, 0.1, C, 9);
        for q in &p.items {
            let a = q.vel.y.atan2(q.vel.x);
            assert!(a.abs() <= 0.25 + 1e-4, "{a}");
        }
    }

    #[test]
    fn rings_progress_and_expire() {
        let mut p = Particles::new();
        p.ring(Vec2::ZERO, 3.0, 0.3, 0.5, C);
        p.update(0.25);
        assert!((p.rings()[0].progress() - 0.5).abs() < 1e-5);
        p.update(0.3);
        assert!(p.rings().is_empty());
        p.ring(Vec2::ZERO, 3.0, 0.3, 0.5, C);
        p.clear();
        assert!(p.is_empty());
    }
}
