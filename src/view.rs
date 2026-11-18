//! Letterboxed mapping from the virtual canvas (measured in tiles) to window
//! pixels. Everything is drawn directly at the window's resolution (no
//! offscreen upscaling), so lines and text stay crisp at any size/DPI.
//!
//! Canvas layout (in tiles, `CANVAS_W` x `CANVAS_H` = 27x36):
//! - rows `0 .. HUD_TOP_ROWS` (0..3): top HUD (scores, level)
//! - rows `3 .. 34`: the 27x31 maze
//! - rows `34 .. 36`: bottom HUD (lives, relic history)

use crate::config::{CANVAS_H, CANVAS_W, HUD_TOP_ROWS, MAZE_H, MAZE_W};
use macroquad::math::{Rect, Vec2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// Window pixels per tile.
    pub scale: f32,
    /// Window-pixel position of canvas point (0, 0).
    pub ox: f32,
    pub oy: f32,
    /// Window size in (logical) pixels.
    pub screen_w: f32,
    pub screen_h: f32,
}

impl Layout {
    /// Fraction of the window kept free around the canvas.
    pub const MARGIN: f32 = 0.02;

    /// Fit the canvas, centred, into a `screen_w` x `screen_h` window.
    pub fn compute(screen_w: f32, screen_h: f32) -> Layout {
        let sw = screen_w.max(1.0);
        let sh = screen_h.max(1.0);
        let usable_w = sw * (1.0 - 2.0 * Self::MARGIN);
        let usable_h = sh * (1.0 - 2.0 * Self::MARGIN);
        let scale = (usable_w / CANVAS_W).min(usable_h / CANVAS_H).max(0.01);
        let ox = ((sw - CANVAS_W * scale) * 0.5).round();
        let oy = ((sh - CANVAS_H * scale) * 0.5).round();
        Layout {
            scale,
            ox,
            oy,
            screen_w: sw,
            screen_h: sh,
        }
    }

    /// Canvas point (tiles) -> window pixels.
    pub fn canvas(&self, x: f32, y: f32) -> Vec2 {
        Vec2::new(self.ox + x * self.scale, self.oy + y * self.scale)
    }

    /// Maze-space point (tiles) -> window pixels.
    pub fn maze(&self, p: Vec2) -> Vec2 {
        self.canvas(p.x, p.y + HUD_TOP_ROWS)
    }

    /// Length in tiles -> pixels.
    pub fn px(&self, tiles: f32) -> f32 {
        tiles * self.scale
    }

    /// Maze area in window pixels.
    pub fn maze_rect(&self) -> Rect {
        let p = self.maze(Vec2::ZERO);
        Rect::new(
            p.x,
            p.y,
            MAZE_W as f32 * self.scale,
            MAZE_H as f32 * self.scale,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_fits_and_is_centred() {
        for (w, h) in [(594.0, 792.0), (1920.0, 1080.0), (400.0, 1200.0)] {
            let l = Layout::compute(w, h);
            // The whole canvas in window pixels.
            let r = Rect::new(l.ox, l.oy, l.px(CANVAS_W), l.px(CANVAS_H));
            assert!(r.x >= 0.0 && r.y >= 0.0);
            assert!(r.x + r.w <= w + 0.5 && r.y + r.h <= h + 0.5);
            assert!(((r.x + r.w * 0.5) - w * 0.5).abs() <= 1.0);
        }
        let l = Layout::compute(270.0 / 0.96, 360.0 / 0.96);
        assert!((l.scale - 10.0).abs() < 1e-3);
        assert_eq!(l.maze(Vec2::ZERO).y, l.oy + 3.0 * l.scale);
    }
}
