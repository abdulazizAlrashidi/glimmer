//! All drawing of the game world and HUD.
//!
//! The renderer draws directly in window pixels via `view::Layout` (no
//! offscreen upscaling) so everything is crisp at any window size / DPI.
//!
//! The look is "neon garden at night": a deep blue-black backdrop with a
//! faint animated starfield/haze; walls are glowing single-stroke neon tubes
//! with rounded corners (see `maze_geom`) whose colour comes from the maze
//! `theme`; motes are small soft points of light; flares pulse; the player and
//! shades glow (see `sprites`). Tunnel ends are masked so actors slide out of
//! view. Phase overlays: "GET SET!" (Ready), points popups (EatPause), death
//! animation (Dying), wall flashing (LevelClear), "GAME OVER".
//!
//! # Frame structure
//! ```text
//! begin_frame()            layout from the window size
//! draw_backdrop()          gradient + starfield + drifting bokeh
//! draw_game(game, alpha)   walls, dots, relic, actors, particles, masks, overlays
//! draw_hud(game)           scores, level, lives, relic history
//! draw_post()              scanlines + vignette (optional)
//! ```
//! `update(dt)` advances the renderer's own wall-clock timer (ambient
//! animation, particles, shake); `handle_event` reacts to gameplay events.
//!
//! Nothing here allocates per frame: wall geometry is cached per maze, all
//! tessellation goes through the reusable scratch mesh in [`gfx`], and
//! numbers are formatted into stack buffers.

pub mod gfx;
pub mod maze_geom;
pub mod particles;
pub mod sprites;
pub mod theme;

use crate::config::{
    BONUS_HISTORY_LEN, CANVAS_H, CANVAS_W, DEATH_TIME, HUD_TOP_ROWS, LEVEL_CLEAR_FLASH,
    LEVEL_CLEAR_FLASHES, LEVEL_CLEAR_HOLD, MAX_LIVES_SHOWN, POPUP_TIME,
};
use crate::font::{self, Align};
use crate::game::{Game, GameEvent, Phase};
use crate::geom::{Dir, Tile};
use crate::maze::{Cell, Maze};
use crate::shade::ShadeState;
use crate::view::Layout;
use gfx::{fade, lighten, mix, with_alpha, Builder, WHITE};
use macroquad::color::Color;
use macroquad::math::Vec2;
use macroquad::window::{clear_background, screen_height, screen_width};
use maze_geom::{Cap, MazeOutline, WallGeometry, CORNER_RADIUS};
use particles::Particles;
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use theme::Theme;

/// Player radius in tiles.
const GLIM_RADIUS: f32 = 0.65;
/// Shade radius in tiles.
const SHADE_RADIUS: f32 = 0.72;
/// Relic radius in tiles.
const RELIC_RADIUS: f32 = 0.6;
/// Mote / flare core radii in tiles.
const MOTE_RADIUS: f32 = 0.085;
const FLARE_RADIUS: f32 = 0.24;
/// Seconds the "LEVEL N / maze name" card stays up at the start of a level.
const TITLE_CARD_TIME: f32 = 1.9;
/// Relic blinks during its last seconds on the board.
const RELIC_BLINK_TIME: f32 = 1.6;
/// Moment of the death animation's burst (fraction of `DEATH_TIME`).
const DEATH_BURST_AT: f32 = 0.56;
/// Wrap period (s) of the ambient animation clock: about 4.5 hours, short
/// enough for the clock to keep under a millisecond of precision as an `f32`
/// (ambient animations jump once per period, never freeze or drift).
pub const AMBIENT_PERIOD: f64 = 16_384.0;

/// One stroke pass of the wall look.
#[derive(Clone, Copy)]
struct Pass {
    /// Stroke width in tiles.
    width: f32,
    color: Color,
    soft: bool,
}

/// Horizontal profile of a tunnel mask: (tiles outward from the board edge,
/// opacity). Opaque from just outside the edge to past the farthest an actor
/// travels (its centre wraps 1 tile beyond the edge) plus its glow.
const TUNNEL_MASK_U: [(f32, f32); 4] = [(0.0, 0.0), (0.35, 1.0), (2.3, 1.0), (3.4, 0.0)];
/// Vertical profile of a tunnel mask: (tiles from the tunnel row's centre
/// line, opacity).
const TUNNEL_MASK_V: [(f32, f32); 4] = [(-2.1, 0.0), (-1.4, 1.0), (1.4, 1.0), (2.1, 0.0)];
/// Intensity of the wall-glow tint under the board (see [`floor_glow`]).
const FLOOR_GLOW: f32 = 0.07;
/// Largest scale a big popup reaches while it pops in.
const BIG_POPUP_POP: f32 = 1.45;
/// Popups stay this far (tiles) inside the board's left and right edges.
const POPUP_MARGIN: f32 = 0.3;

/// Centre and radius (window px) of the soft glow under the board.
fn floor_glow(lay: &Layout) -> (Vec2, f32) {
    let mr = lay.maze_rect();
    (Vec2::new(mr.x + mr.w * 0.5, mr.y + mr.h * 0.5), mr.h * 0.62)
}

/// Centre for a text of width `w` centred near `x` that must fit in
/// `[lo, hi]` (centred in the range when it is wider than that).
fn clamp_centre(x: f32, w: f32, lo: f32, hi: f32) -> f32 {
    if hi - lo <= w {
        (lo + hi) * 0.5
    } else {
        x.clamp(lo + w * 0.5, hi - w * 0.5)
    }
}

/// Per-maze cached data.
struct MazeCache {
    index: usize,
    name: &'static str,
    outline: MazeOutline,
    /// Maze-space y of the "GET SET!" / "GAME OVER" banner centre.
    banner_y: f32,
    /// Maze-space y of the level title card centre.
    title_y: f32,
    /// Nest interior rectangle (maze space): min corner and size.
    nest_min: Vec2,
    nest_size: Vec2,
}

impl MazeCache {
    fn new(maze: &Maze) -> MazeCache {
        let door = maze.door;
        MazeCache {
            index: maze.index,
            name: maze.name,
            outline: maze_geom::build_outline(maze),
            banner_y: best_banner_row(maze, door.y + 4, door.y + 7, door.y + 5) as f32 + 0.5,
            title_y: door.y as f32 - 2.45,
            nest_min: Vec2::new(door.x as f32 - 2.0, door.y as f32 + 1.0),
            nest_size: Vec2::new(5.0, 3.0),
        }
    }

    fn matches(&self, maze: &Maze) -> bool {
        self.index == maze.index && self.name == maze.name
    }
}

/// The row in `lo..=hi` with the widest open span around the centre column
/// (ties prefer `preferred`, then the row nearest to it).
fn best_banner_row(maze: &Maze, lo: i32, hi: i32, preferred: i32) -> i32 {
    let cx = maze.w / 2;
    let span = |y: i32| -> i32 {
        if !(0..maze.h).contains(&y) || maze.is_wall(Tile::new(cx, y)) {
            return 0;
        }
        let mut n = 1;
        let mut d = 1;
        while d <= cx && !maze.is_wall(Tile::new(cx - d, y)) && !maze.is_wall(Tile::new(cx + d, y))
        {
            n += 2;
            d += 1;
        }
        n
    };
    let mut best = preferred;
    let mut best_span = span(preferred);
    for y in lo..=hi {
        let s = span(y);
        if s > best_span || (s == best_span && (y - preferred).abs() < (best - preferred).abs()) {
            best = y;
            best_span = s;
        }
    }
    best
}

/// Format `n` in decimal into `buf`.
fn fmt_u32(mut n: u32, buf: &mut [u8; 10]) -> &str {
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 || i == 0 {
            break;
        }
    }
    std::str::from_utf8(&buf[i..]).unwrap_or("0")
}

/// Concatenate `parts` into `buf` (truncating at a char boundary if needed).
fn concat<'a>(buf: &'a mut [u8; 48], parts: &[&str]) -> &'a str {
    let mut len = 0;
    for p in parts {
        for ch in p.chars() {
            let n = ch.len_utf8();
            if len + n > buf.len() {
                break;
            }
            ch.encode_utf8(&mut buf[len..len + n]);
            len += n;
        }
    }
    std::str::from_utf8(&buf[..len]).unwrap_or("")
}

/// Ease-out cubic.
fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub struct Renderer {
    /// Current layout; refreshed by `begin_frame`.
    pub layout: Layout,
    /// Small camera shake on deaths / big chains (user setting).
    pub shake_enabled: bool,
    /// Subtle CRT scanlines + vignette (user setting).
    pub scanlines: bool,
    /// Developer overlay: shade targets and tile grid (F3).
    pub debug_overlay: bool,
    /// Ambient animation clock in seconds, kept in `[0, AMBIENT_PERIOD)`.
    /// Accumulated in f64 so that tiny frame steps (high refresh rates) never
    /// round away, however long the session runs.
    time: f64,
    /// Theme of the last drawn game (used by the backdrop on other screens).
    theme: Theme,
    particles: Particles,
    /// Current shake amplitude in tiles.
    shake: f32,
    cache: Option<MazeCache>,
    /// Seed counter for particle bursts.
    seed: u32,
    /// Pending particle burst of the death animation (fired by `draw_game`
    /// when the animation reaches its burst, so both stay in sync).
    death_burst: Option<Vec2>,
    /// Seconds left of the HUD "extra life" highlight.
    extra_life_flash: f32,
    /// Player trail bookkeeping.
    last_player: Option<Vec2>,
    trail_acc: f32,
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer {
            layout: Layout::compute(594.0, 792.0),
            shake_enabled: true,
            scanlines: false,
            debug_overlay: false,
            time: 0.0,
            theme: theme::theme_for(0, 1),
            particles: Particles::new(),
            shake: 0.0,
            cache: None,
            seed: 0x51ed,
            death_burst: None,
            extra_life_flash: 0.0,
            last_player: None,
            trail_acc: 0.0,
        }
    }

    /// Recompute the layout from the current window size. Call first every frame.
    pub fn begin_frame(&mut self) {
        self.layout = Layout::compute(screen_width(), screen_height());
    }

    /// Wall-clock seconds accumulated by [`Renderer::update`], wrapped every
    /// [`AMBIENT_PERIOD`] seconds so the value keeps sub-millisecond
    /// precision as an `f32`.
    pub fn time(&self) -> f32 {
        self.time as f32
    }

    fn next_seed(&mut self) -> u32 {
        self.seed = self.seed.wrapping_add(0x9e37_79b9);
        gfx::hash(self.seed)
    }

    fn add_shake(&mut self, amount: f32) {
        if self.shake_enabled {
            self.shake = self.shake.max(amount);
        }
    }

    /// Spawn particles / shake in response to a gameplay event.
    pub fn handle_event(&mut self, ev: &GameEvent, game: &Game) {
        let th = theme::theme_for(game.maze.index, game.level);
        let seed = self.next_seed();
        let p = &mut self.particles;
        match *ev {
            GameEvent::MoteEaten { tile } => {
                p.burst(tile.center(), 3, 2.0, 0.26, 0.06, th.mote, seed);
            }
            GameEvent::FlareEaten { tile } => {
                let c = tile.center();
                p.burst(c, 26, 7.5, 0.6, 0.11, th.flare, seed);
                p.ring(c, 3.4, 0.35, 0.55, th.flare);
                p.ring(c, 1.8, 0.2, 0.35, WHITE);
            }
            GameEvent::ShadeEaten { kind, pos, .. } => {
                let col = sprites::shade_color(kind);
                p.burst(pos, 30, 8.0, 0.7, 0.12, col, seed);
                p.ring(pos, 2.4, 0.3, 0.45, lighten(col, 0.3));
            }
            GameEvent::FullChain { pos, .. } => {
                p.burst(pos, 64, 11.0, 1.1, 0.13, sprites::glim_color(), seed);
                p.burst(pos, 32, 6.0, 0.9, 0.1, WHITE, seed ^ 0xabcd);
                p.ring(pos, 6.0, 0.45, 0.9, sprites::glim_color());
                self.add_shake(0.24);
            }
            GameEvent::BonusEaten { kind, pos, .. } => {
                let col = sprites::bonus_color(kind);
                p.burst(pos, 30, 6.5, 0.75, 0.11, col, seed);
                p.ring(pos, 2.6, 0.3, 0.5, col);
            }
            GameEvent::BonusSpawned { kind, pos } => {
                p.burst(pos, 14, 3.0, 0.5, 0.07, sprites::bonus_color(kind), seed);
                p.ring(pos, 1.4, 0.18, 0.4, sprites::bonus_color(kind));
            }
            GameEvent::DeathAnimStarted { pos } => {
                self.death_burst = Some(pos);
            }
            GameEvent::PlayerCaught { pos } => {
                p.ring(pos, 1.6, 0.25, 0.35, WHITE);
                self.add_shake(0.18);
            }
            GameEvent::ExtraLife => {
                let pos = game.player.mover.pos;
                p.burst(pos, 36, 6.0, 0.9, 0.1, sprites::glim_color(), seed);
                p.ring(pos, 3.0, 0.3, 0.6, sprites::glim_color());
                self.extra_life_flash = 1.6;
            }
            GameEvent::LevelCleared { .. } => {
                let maze = &game.maze;
                let centre = Vec2::new(maze.w as f32 * 0.5, maze.h as f32 * 0.5);
                p.ring(game.player.mover.pos, 16.0, 0.6, 1.4, th.wall);
                for i in 0..10u32 {
                    let a = i as f32 / 10.0 * TAU;
                    let q = centre + Vec2::new(a.cos() * 9.0, a.sin() * 11.0);
                    p.burst(
                        q,
                        12,
                        4.0,
                        0.9,
                        0.09,
                        if i % 2 == 0 { th.wall } else { th.mote },
                        seed ^ i,
                    );
                }
            }
            GameEvent::LevelStarted { .. } | GameEvent::ReadyStarted { .. } => {
                self.last_player = None;
                self.death_burst = None;
            }
            _ => {}
        }
    }

    /// Advance visual-only state (particles, shake, ambient clock) by `dt`
    /// seconds of wall-clock time.
    pub fn update(&mut self, dt: f32) {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.25)
        } else {
            0.0
        };
        self.time = (self.time + f64::from(dt)).rem_euclid(AMBIENT_PERIOD);
        self.particles.update(dt);
        self.shake *= (-dt * 7.5).exp();
        if self.shake < 0.004 {
            self.shake = 0.0;
        }
        self.extra_life_flash = (self.extra_life_flash - dt).max(0.0);
    }

    /// Clear the screen and draw the animated backdrop (used by every screen).
    pub fn draw_backdrop(&self) {
        let th = self.theme;
        let l = self.layout;
        let (sw, sh) = (l.screen_w, l.screen_h);
        let t = self.time();
        clear_background(th.backdrop_bottom);
        gfx::rect_vgradient(0.0, 0.0, sw, sh, th.backdrop_top, th.backdrop_bottom);

        // Two large, slowly wandering haze clouds.
        let unit = l.scale.max(1.0);
        for (centre, r, col, k) in self.haze_clouds() {
            gfx::glow(centre, r, col, k);
        }

        // Starfield: deterministic, slowly drifting upward, twinkling.
        let star = mix(th.hud_value, th.wall, 0.25);
        gfx::with_builder(|b| {
            for i in 0..90u32 {
                let fx = gfx::hash01(i * 3 + 11);
                let fy = gfx::hash01(i * 7 + 3);
                let speed = 0.004 + 0.01 * gfx::hash01(i * 5 + 1);
                let y = (fy - t * speed).rem_euclid(1.0) * sh;
                let x = fx * sw;
                let tw = 0.5 + 0.5 * (t * (0.6 + 1.7 * gfx::hash01(i + 99)) + fy * TAU).sin();
                let size = unit * (0.035 + 0.05 * gfx::hash01(i * 17 + 5));
                let a = (0.14 + 0.36 * tw) * (0.4 + 0.6 * gfx::hash01(i * 23 + 9));
                b.circle(Vec2::new(x, y), size, with_alpha(star, a));
            }
        });

        // Out-of-focus fireflies (bokeh), in the wall glow hue so they are
        // never mistaken for motes.
        gfx::with_builder(|b| {
            for i in 0..12u32 {
                let fx = gfx::hash01(i * 31 + 7);
                let fy = gfx::hash01(i * 37 + 1);
                let ph = gfx::hash01(i * 41 + 3) * TAU;
                let x = (fx + 0.04 * (t * 0.11 + ph).sin()) * sw;
                let y = (fy + 0.05 * (t * 0.083 + ph * 1.3).cos() - t * 0.002).rem_euclid(1.0) * sh;
                let pulse = 0.5 + 0.5 * (t * 0.5 + ph).sin();
                let r = unit * (0.6 + 0.9 * gfx::hash01(i * 43 + 2));
                b.glow(Vec2::new(x, y), r, th.wall_glow, 0.05 + 0.08 * pulse);
            }
        });
    }

    /// The backdrop's haze clouds: (centre, radius, colour, intensity).
    fn haze_clouds(&self) -> [(Vec2, f32, Color, f32); 2] {
        let (th, l, t) = (self.theme, self.layout, self.time());
        std::array::from_fn(|i| {
            let k = i as f32;
            let fx = 0.3 + 0.4 * k + 0.12 * (t * 0.031 + k * 2.0).sin();
            let fy = 0.35 + 0.3 * k + 0.08 * (t * 0.023 + k).cos();
            let col = if i == 0 { th.wall_glow } else { th.hud_label };
            let centre = Vec2::new(l.screen_w * fx, l.screen_h * fy);
            (centre, l.scale.max(1.0) * 16.0, col, 0.05)
        })
    }

    /// Colour of the backdrop's smooth layers (gradient, haze, the board's
    /// floor glow) at window point `q`; `lay` is the (shaken) board layout.
    fn backdrop_at(&self, lay: &Layout, q: Vec2) -> Color {
        let th = self.theme;
        let v = (q.y / self.layout.screen_h.max(1.0)).clamp(0.0, 1.0);
        let mut c = mix(th.backdrop_top, th.backdrop_bottom, v);
        for (centre, r, col, k) in self.haze_clouds() {
            c = mix(c, col, k * gfx::glow_falloff(q.distance(centre) / r));
        }
        let (centre, r) = floor_glow(lay);
        mix(
            c,
            th.wall_glow,
            FLOOR_GLOW * gfx::glow_falloff(q.distance(centre) / r),
        )
    }

    fn shake_offset(&self) -> Vec2 {
        if self.shake <= 0.0 || !self.shake_enabled {
            return Vec2::ZERO;
        }
        let t = self.time() * 38.0;
        Vec2::new(gfx::noise1(t, 3), gfx::noise1(t, 9)) * self.layout.px(self.shake)
    }

    /// Draw the maze, dots, relic, actors, popups, particles and phase
    /// overlays. `alpha` is the fixed-step interpolation factor in `[0, 1]`.
    pub fn draw_game(&mut self, game: &Game, alpha: f32) {
        let alpha = if alpha.is_finite() {
            alpha.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let th = theme::theme_for(game.maze.index, game.level);
        self.theme = th;
        if !self.cache.as_ref().is_some_and(|c| c.matches(&game.maze)) {
            self.cache = Some(MazeCache::new(&game.maze));
        }
        let mut lay = self.layout;
        let off = self.shake_offset();
        lay.ox += off.x;
        lay.oy += off.y;

        let phase = game.phase;
        let flash = match phase {
            Phase::LevelClear { t } if t >= LEVEL_CLEAR_HOLD => {
                let k = ((t - LEVEL_CLEAR_HOLD) / LEVEL_CLEAR_FLASH
                    * (LEVEL_CLEAR_FLASHES * 2) as f32) as u32;
                k < LEVEL_CLEAR_FLASHES * 2 && k.is_multiple_of(2)
            }
            _ => false,
        };

        if let Some(cache) = &self.cache {
            self.draw_floor(&lay, cache, &th);
            self.draw_walls(&lay, &cache.outline, &th, flash);
        }
        self.draw_dots(&lay, &game.maze, &th);
        self.draw_relic(&lay, game);
        self.draw_actors(&lay, game, alpha);
        self.draw_particles(&lay);
        self.draw_tunnel_masks(&lay, &game.maze);
        self.draw_popups(&lay, game, &th);
        if let Some(cache) = &self.cache {
            self.draw_overlays(&lay, game, cache, &th);
        }
        if self.debug_overlay {
            self.draw_debug(&lay, game);
        }
    }

    // ----- world layers ---------------------------------------------------

    fn draw_floor(&self, lay: &Layout, cache: &MazeCache, th: &Theme) {
        let (centre, r) = floor_glow(lay);
        gfx::glow(centre, r, th.wall_glow, FLOOR_GLOW);
        // Warm glow inside the nest.
        let nc = lay.maze(cache.nest_min + cache.nest_size * 0.5);
        let pulse = 0.8 + 0.2 * (self.time() * 1.7).sin();
        gfx::ellipse(
            nc,
            lay.px(cache.nest_size.x * 0.55),
            lay.px(cache.nest_size.y * 0.6),
            0.0,
            with_alpha(th.door, 0.10 * pulse),
            with_alpha(th.door, 0.0),
        );
    }

    fn draw_walls(&self, lay: &Layout, o: &MazeOutline, th: &Theme, flash: bool) {
        let wall = if flash { th.wall_flash } else { th.wall };
        let glow_col = if flash { th.wall_flash } else { th.wall_glow };
        let breathe = 0.86 + 0.14 * (self.time() * 1.3).sin();

        // Translucent glass fill of thick wall interiors.
        let fill = with_alpha(mix(glow_col, wall, 0.35), if flash { 0.16 } else { 0.075 });
        gfx::with_builder(|b| draw_fills(b, lay, o, fill));

        let main = [
            Pass {
                width: 1.1,
                color: with_alpha(glow_col, 0.11 * breathe),
                soft: true,
            },
            Pass {
                width: 0.46,
                color: with_alpha(wall, 0.36),
                soft: true,
            },
            Pass {
                width: 0.13,
                color: lighten(wall, 0.12),
                soft: false,
            },
            Pass {
                width: 0.045,
                color: with_alpha(lighten(wall, 0.78), 0.9),
                soft: false,
            },
        ];
        let border = [
            Pass {
                width: 0.3,
                color: with_alpha(wall, 0.22),
                soft: true,
            },
            Pass {
                width: 0.075,
                color: with_alpha(lighten(wall, 0.2), 0.9),
                soft: false,
            },
        ];
        for pass in &border {
            gfx::with_builder(|b| stroke_pass(b, lay, &o.border, &[], *pass));
        }
        for pass in &main {
            gfx::with_builder(|b| {
                stroke_pass(b, lay, &o.walls, &o.caps, *pass);
                // Round the glow off where a hairpin closes the stroke (the
                // hairpin replaces the visible cap).
                if pass.soft {
                    stroke_caps(b, lay, &o.closed_caps, *pass);
                }
            });
        }

        // Door bars.
        let shimmer = 0.85 + 0.15 * (self.time() * 5.0).sin();
        for &(a, b) in &o.walls.doors {
            let inset = Vec2::new(0.06, 0.0);
            let (pa, pb) = (lay.maze(a + inset), lay.maze(b - inset));
            gfx::soft_line(pa, pb, lay.px(0.7), with_alpha(th.door, 0.32 * shimmer));
            gfx::line_round(pa, pb, lay.px(0.11), th.door);
            gfx::line(
                pa,
                pb,
                lay.px(0.035),
                with_alpha(lighten(th.door, 0.8), 0.9),
            );
        }
    }

    fn draw_dots(&self, lay: &Layout, maze: &Maze, th: &Theme) {
        let t = self.time();
        let mote_r = lay.px(MOTE_RADIUS);
        let flare_r = lay.px(FLARE_RADIUS);
        let twinkle = |tile: Tile| {
            gfx::hash01(
                (tile.x as u32).wrapping_mul(73_856_093) ^ (tile.y as u32).wrapping_mul(19_349_663),
            )
        };
        // All motes in one batched submission.
        gfx::with_builder(|b| {
            for (tile, cell) in maze.dots() {
                if cell == Cell::Mote {
                    let h = twinkle(tile);
                    let tw = 0.8 + 0.2 * (t * (1.6 + h) + h * TAU).sin();
                    let p = lay.maze(tile.center());
                    sprites::mote_into(b, p, mote_r * (0.92 + 0.08 * tw), with_alpha(th.mote, tw));
                }
            }
        });
        for (tile, cell) in maze.dots() {
            if cell == Cell::Flare {
                sprites::draw_flare(
                    lay.maze(tile.center()),
                    flare_r,
                    t + twinkle(tile) * 3.0,
                    th.flare,
                );
            }
        }
    }

    fn draw_relic(&self, lay: &Layout, game: &Game) {
        let Some(b) = game.bonus else {
            return;
        };
        if matches!(
            game.phase,
            Phase::Dying { .. } | Phase::GameOver { .. } | Phase::Interlude { .. }
        ) {
            return;
        }
        let left = b.duration - b.age;
        if left < RELIC_BLINK_TIME && ((self.time() * 7.0) as i32) % 2 == 1 {
            return;
        }
        let pop = ease_out(b.age / 0.35);
        let bob = (self.time() * 2.4).sin() * 0.08;
        let p = lay.maze(b.pos + Vec2::new(0.0, bob));
        sprites::draw_bonus(
            p,
            lay.px(RELIC_RADIUS) * (0.4 + 0.6 * pop),
            b.kind,
            self.time(),
        );
    }

    fn draw_actors(&mut self, lay: &Layout, game: &Game, alpha: f32) {
        let (shades_on, player_on) = match game.phase {
            Phase::Ready { .. }
            | Phase::Playing
            | Phase::Caught { .. }
            | Phase::EatPause { .. } => (true, true),
            Phase::Dying { .. } => (false, false),
            Phase::LevelClear { t } => (t < LEVEL_CLEAR_HOLD, true),
            Phase::Interlude { .. } | Phase::GameOver { .. } => (false, false),
        };
        let hidden_shade = match game.phase {
            Phase::EatPause { shade, .. } => Some(shade),
            _ => None,
        };

        if shades_on {
            for i in (0..game.shades.len()).rev() {
                let s = &game.shades[i];
                if !s.visible || hidden_shade == Some(i) {
                    continue;
                }
                let p = lay.maze(s.mover.render_pos(alpha));
                let look = game.shade_look(i);
                sprites::draw_shade(p, lay.px(SHADE_RADIUS), s.kind, look, s.mover.dir, s.anim_t);
            }
        }

        let pl = &game.player;
        let pos = pl.mover.render_pos(alpha);
        if let Phase::Dying { t } = game.phase {
            let progress = t / DEATH_TIME;
            if progress >= DEATH_BURST_AT {
                if let Some(at) = self.death_burst.take() {
                    self.spawn_death_burst(at);
                }
            }
            sprites::draw_glim_death(lay.maze(pos), lay.px(GLIM_RADIUS), progress);
            self.last_player = None;
            return;
        }
        if player_on && pl.visible {
            self.spawn_trail(game, pos);
            sprites::draw_glim(
                lay.maze(pos),
                lay.px(GLIM_RADIUS),
                pl.mover.dir,
                pl.anim_t,
                1.0,
            );
        } else {
            self.last_player = None;
        }
    }

    fn spawn_death_burst(&mut self, pos: Vec2) {
        let seed = self.next_seed();
        let col = sprites::glim_color();
        self.particles.burst(pos, 60, 9.0, 1.1, 0.13, col, seed);
        self.particles
            .burst(pos, 24, 4.0, 1.3, 0.09, WHITE, seed ^ 0x77);
        self.particles.ring(pos, 4.5, 0.4, 0.7, col);
    }

    /// Leave a faint trail of sparks behind the moving player.
    fn spawn_trail(&mut self, game: &Game, pos: Vec2) {
        let moving = matches!(game.phase, Phase::Playing) && game.player.mover.moving;
        if let (true, Some(last)) = (moving, self.last_player) {
            let d = pos.distance(last);
            if d < 1.5 {
                self.trail_acc += d;
            }
            let back = -game.player.mover.dir.vec();
            while self.trail_acc >= 0.42 {
                self.trail_acc -= 0.42;
                let seed = self.next_seed();
                let jitter =
                    Vec2::new(gfx::hash01(seed) - 0.5, gfx::hash01(seed ^ 0x55) - 0.5) * 0.25;
                self.particles.spray(
                    pos + back * 0.45 + jitter,
                    back.y.atan2(back.x),
                    1.2,
                    1,
                    0.8,
                    0.5,
                    0.055,
                    sprites::glim_color(),
                    seed,
                );
            }
        }
        self.last_player = Some(pos);
    }

    fn draw_particles(&self, lay: &Layout) {
        for r in self.particles.rings() {
            let e = r.progress();
            let k = (1.0 - e).powf(1.5);
            let p = lay.maze(r.pos);
            let rad = lay.px(r.radius) * ease_out(e);
            let w = lay.px(r.width) * (1.0 - 0.7 * e);
            gfx::soft_ring(p, rad, w * 2.5, with_alpha(r.color, 0.55 * k));
            gfx::ring(p, rad, w * 0.35, with_alpha(lighten(r.color, 0.5), 0.8 * k));
        }
        gfx::with_builder(|b| {
            for q in &self.particles.items {
                let k = q.remaining();
                let p = lay.maze(q.pos);
                let s = lay.px(q.size) * (0.35 + 0.65 * k);
                b.glow_small(p, s * 3.6, q.color, 0.6 * k);
                b.circle(p, s * 0.55, with_alpha(lighten(q.color, 0.6), k));
            }
        });
    }

    /// Masks just outside the board on tunnel rows, so actors slide out of
    /// view instead of drifting over the window margin.
    ///
    /// Each mask is a feathered patch reaching only as far as an actor and
    /// its glow can travel beyond the board ([`TUNNEL_MASK_U`] by
    /// [`TUNNEL_MASK_V`]), painted per vertex in the backdrop's own colours
    /// (see [`Renderer::backdrop_at`]), so it blends into the margin at any
    /// window size instead of painting a stripe across it.
    fn draw_tunnel_masks(&self, lay: &Layout, maze: &Maze) {
        let left = lay.maze(Vec2::ZERO).x;
        let right = lay.maze(Vec2::new(maze.w as f32, 0.0)).x;
        gfx::with_builder(|b| {
            for &row in &maze.tunnel_rows {
                let yc = lay.maze(Vec2::new(0.0, row as f32 + 0.5)).y;
                for (edge, out) in [(left, -1.0), (right, 1.0)] {
                    let mut p = [[Vec2::ZERO; 4]; 4];
                    let mut c = [[WHITE; 4]; 4];
                    for (j, &(v, av)) in TUNNEL_MASK_V.iter().enumerate() {
                        for (i, &(u, au)) in TUNNEL_MASK_U.iter().enumerate() {
                            let q = Vec2::new(edge + out * lay.px(u), yc + lay.px(v));
                            p[j][i] = q;
                            c[j][i] = with_alpha(self.backdrop_at(lay, q), au * av);
                        }
                    }
                    for j in 0..3 {
                        for i in 0..3 {
                            b.quad(
                                [p[j][i], p[j][i + 1], p[j + 1][i + 1], p[j + 1][i]],
                                [c[j][i], c[j][i + 1], c[j + 1][i + 1], c[j + 1][i]],
                            );
                        }
                    }
                }
            }
        });
    }

    fn draw_popups(&self, lay: &Layout, game: &Game, th: &Theme) {
        let mut buf = [0u8; 10];
        for p in &game.popups {
            let k = (p.age / POPUP_TIME).clamp(0.0, 1.0);
            let a = if k < 0.65 {
                1.0
            } else {
                1.0 - (k - 0.65) / 0.35
            };
            let rise = lay.px(0.75) * ease_out(k);
            let base = if p.big { 0.95 } else { 0.68 };
            let pop = if p.big {
                1.0 + (BIG_POPUP_POP - 1.0) * (1.0 - ease_out(p.age / 0.18))
            } else {
                1.0
            };
            let size = lay.px(base) * pop;
            let col = if p.big {
                gfx::lighten(sprites::glim_color(), 0.2)
            } else {
                th.flare
            };
            let pos = lay.maze(p.pos);
            let text = fmt_u32(p.points, &mut buf);
            // Keep the number on the board (a shade eaten in a tunnel sits
            // beyond its edge); clamp with the largest size the popup
            // reaches so it does not slide while it settles.
            let widest =
                font::text_width(text, lay.px(base) * if p.big { BIG_POPUP_POP } else { 1.0 });
            let x = clamp_centre(
                pos.x,
                widest,
                lay.maze(Vec2::new(POPUP_MARGIN, 0.0)).x,
                lay.maze(Vec2::new(game.maze.w as f32 - POPUP_MARGIN, 0.0))
                    .x,
            );
            font::draw_text_glow(
                text,
                x,
                pos.y - size * 0.5 - rise,
                size,
                with_alpha(col, a),
                Align::Center,
            );
        }
    }

    fn draw_overlays(&self, lay: &Layout, game: &Game, cache: &MazeCache, th: &Theme) {
        let cx = lay.maze(Vec2::new(game.maze.w as f32 * 0.5, 0.0)).x;
        match game.phase {
            Phase::Ready { t, .. } => {
                let a = ease_out(t / 0.25);
                let pulse = 0.85 + 0.15 * (self.time() * 4.0).sin();
                let size = lay.px(1.0);
                let y = lay.maze(Vec2::new(0.0, cache.banner_y)).y - size * 0.5;
                let col = with_alpha(mix(sprites::glim_color(), WHITE, 0.15), a * pulse);
                font::draw_text_glow("GET SET!", cx, y, size, col, Align::Center);
                let fresh = game.maze.dots_left() == game.maze.total_dots;
                if fresh && t < TITLE_CARD_TIME {
                    self.draw_title_card(lay, game, cache, th, t);
                }
            }
            Phase::GameOver { t } => {
                let a = ease_out(t / 0.6);
                let size = lay.px(1.0);
                let y = lay.maze(Vec2::new(0.0, cache.banner_y)).y - size * 0.5;
                let col = with_alpha(Color::new(1.0, 0.32, 0.42, 1.0), a);
                font::draw_text_glow("GAME OVER", cx, y, size, col, Align::Center);
            }
            _ => {}
        }
    }

    /// "LEVEL N" + maze name on a glass plate above the nest.
    fn draw_title_card(&self, lay: &Layout, game: &Game, cache: &MazeCache, th: &Theme, t: f32) {
        let fade_in = ease_out(t / 0.2);
        let fade_out = ((TITLE_CARD_TIME - t) / 0.35).clamp(0.0, 1.0);
        let a = fade_in * fade_out;
        if a <= 0.0 {
            return;
        }
        let name_size = lay.px(0.72);
        let level_size = lay.px(0.52);
        let name_w = font::text_width(cache.name, name_size).max(lay.px(6.0));
        let centre = lay.maze(Vec2::new(game.maze.w as f32 * 0.5, cache.title_y));
        let half = Vec2::new(name_w * 0.5 + lay.px(0.75), lay.px(0.98));
        // Rounded glass plate with a neon rim; it rises slightly as it appears.
        let lift = lay.px(0.25) * (1.0 - fade_in);
        let centre = centre + Vec2::new(0.0, lift);
        let mut pts = [Vec2::ZERO; 44];
        let n = gfx::rounded_rect(centre - half, centre + half, lay.px(0.45), 6, &mut pts);
        let glass_in = with_alpha(mix(th.backdrop_top, th.wall_glow, 0.08), 0.94 * a);
        let glass_out = with_alpha(th.backdrop_bottom, 0.94 * a);
        gfx::glow(centre, half.x * 1.25, th.wall_glow, 0.18 * a);
        gfx::poly_fan(centre, glass_in, &pts[..n], glass_out);
        gfx::poly_outline(&pts[..n], lay.px(0.16), with_alpha(th.wall_glow, 0.25 * a));
        gfx::poly_outline(
            &pts[..n],
            lay.px(0.05),
            with_alpha(lighten(th.wall, 0.2), 0.9 * a),
        );

        let mut nbuf = [0u8; 10];
        let mut tbuf = [0u8; 48];
        let num = fmt_u32(game.level, &mut nbuf);
        let level = concat(&mut tbuf, &["LEVEL ", num]);
        let ly = centre.y - lay.px(0.72);
        font::draw_text_glow(
            level,
            centre.x,
            ly,
            level_size,
            with_alpha(th.hud_label, a),
            Align::Center,
        );
        let ny = centre.y + lay.px(0.0);
        font::draw_text_glow(
            cache.name,
            centre.x,
            ny,
            name_size,
            with_alpha(th.hud_value, a),
            Align::Center,
        );
    }

    fn draw_debug(&self, lay: &Layout, game: &Game) {
        let grid = with_alpha(WHITE, 0.05);
        let mr = lay.maze_rect();
        for x in 0..=game.maze.w {
            let px = mr.x + lay.px(x as f32);
            gfx::line(Vec2::new(px, mr.y), Vec2::new(px, mr.y + mr.h), 1.0, grid);
        }
        for y in 0..=game.maze.h {
            let py = mr.y + lay.px(y as f32);
            gfx::line(Vec2::new(mr.x, py), Vec2::new(mr.x + mr.w, py), 1.0, grid);
        }
        for s in &game.shades {
            if s.state != ShadeState::Active {
                continue;
            }
            let col = sprites::shade_color(s.kind);
            let from = lay.maze(s.mover.pos);
            let to = lay.maze(s.target.center());
            gfx::line(from, to, lay.px(0.06), with_alpha(col, 0.7));
            let h = lay.px(0.36);
            let box_pts = [
                to + Vec2::new(-h, -h),
                to + Vec2::new(h, -h),
                to + Vec2::new(h, h),
                to + Vec2::new(-h, h),
            ];
            gfx::poly_outline(&box_pts, lay.px(0.08), col);
            gfx::circle(to, lay.px(0.1), col);
        }
    }

    // ----- HUD ------------------------------------------------------------

    /// Top HUD (score, high score, level) and bottom HUD (lives, relics).
    pub fn draw_hud(&self, game: &Game) {
        let th = theme::theme_for(game.maze.index, game.level);
        let l = &self.layout;
        let label = l.px(0.62);
        let value = l.px(0.92);
        let ly = l.canvas(0.0, 0.42).y;
        let vy = l.canvas(0.0, 1.35).y;
        let left = l.canvas(1.0, 0.0).x;
        let mid = l.canvas(CANVAS_W * 0.5, 0.0).x;
        let right = l.canvas(CANVAS_W - 1.0, 0.0).x;
        let mut buf = [0u8; 10];

        font::draw_text_glow("SCORE", left, ly, label, th.hud_label, Align::Left);
        font::draw_text_glow(
            fmt_u32(game.score, &mut buf),
            left,
            vy,
            value,
            th.hud_value,
            Align::Left,
        );
        font::draw_text_glow("HIGH SCORE", mid, ly, label, th.hud_label, Align::Center);
        let hs_col = if game.score > 0 && game.score >= game.high_score {
            mix(th.hud_value, th.flare, 0.5)
        } else {
            th.hud_value
        };
        font::draw_text_glow(
            fmt_u32(game.high_score, &mut buf),
            mid,
            vy,
            value,
            hs_col,
            Align::Center,
        );
        font::draw_text_glow("LEVEL", right, ly, label, th.hud_label, Align::Right);
        font::draw_text_glow(
            fmt_u32(game.level, &mut buf),
            right,
            vy,
            value,
            th.hud_value,
            Align::Right,
        );

        // Hairline separators framing the play field.
        let sep_top = l.canvas(0.0, HUD_TOP_ROWS - 0.18).y;
        let sep_bot = l.canvas(0.0, CANVAS_H - 1.86).y;
        for y in [sep_top, sep_bot] {
            let a = Vec2::new(l.canvas(0.5, 0.0).x, y);
            let b = Vec2::new(l.canvas(CANVAS_W - 0.5, 0.0).x, y);
            gfx::line_gradient(
                a,
                Vec2::new(mid, y),
                1.0,
                with_alpha(th.wall, 0.0),
                with_alpha(th.wall, 0.28),
            );
            gfx::line_gradient(
                Vec2::new(mid, y),
                b,
                1.0,
                with_alpha(th.wall, 0.28),
                with_alpha(th.wall, 0.0),
            );
        }

        // Reserve lives.
        let icon_y = CANVAS_H - 0.95;
        let n = game.lives.min(MAX_LIVES_SHOWN);
        for i in 0..n {
            let p = l.canvas(1.3 + i as f32 * 1.55, icon_y);
            let newest = i + 1 == n && self.extra_life_flash > 0.0;
            if newest {
                let k = self.extra_life_flash / 1.6;
                gfx::glow(
                    p,
                    l.px(1.6),
                    sprites::glim_color(),
                    0.6 * k * (0.5 + 0.5 * (self.time() * 12.0).sin()),
                );
            }
            sprites::draw_glim(
                p,
                l.px(0.46),
                Dir::Right,
                self.time() * 0.45 + i as f32,
                1.0,
            );
        }

        // Recent relics, most recent on the right.
        let hist = &game.bonus_history;
        let start = hist.len().saturating_sub(BONUS_HISTORY_LEN);
        for (k, &kind) in hist[start..].iter().rev().enumerate() {
            let p = l.canvas(CANVAS_W - 1.3 - k as f32 * 1.55, icon_y);
            sprites::draw_bonus(p, l.px(0.48), kind, self.time() + k as f32 * 0.7);
        }
    }

    /// Full-screen post effects (scanlines/vignette); call last each frame.
    pub fn draw_post(&self) {
        if !self.scanlines {
            return;
        }
        let (sw, sh) = (self.layout.screen_w, self.layout.screen_h);
        let line = Color::new(0.0, 0.0, 0.0, 0.11);
        gfx::with_builder(|b| {
            let mut y = 0.0;
            while y < sh {
                b.quad(
                    [
                        Vec2::new(0.0, y),
                        Vec2::new(sw, y),
                        Vec2::new(sw, y + 1.0),
                        Vec2::new(0.0, y + 1.0),
                    ],
                    [line; 4],
                );
                y += 3.0;
            }
        });
        // Vignette: an elliptical darkening toward the corners.
        const N: usize = 8;
        let alpha_at = |u: f32, v: f32| {
            let d = ((u - 0.5) * (u - 0.5) + (v - 0.5) * (v - 0.5)).sqrt()
                / std::f32::consts::FRAC_1_SQRT_2;
            let k = ((d - 0.55) / 0.45).clamp(0.0, 1.0);
            Color::new(0.0, 0.0, 0.0, 0.5 * k * k)
        };
        gfx::with_builder(|b| {
            for j in 0..N {
                for i in 0..N {
                    let (u0, u1) = (i as f32 / N as f32, (i + 1) as f32 / N as f32);
                    let (v0, v1) = (j as f32 / N as f32, (j + 1) as f32 / N as f32);
                    b.quad(
                        [
                            Vec2::new(u0 * sw, v0 * sh),
                            Vec2::new(u1 * sw, v0 * sh),
                            Vec2::new(u1 * sw, v1 * sh),
                            Vec2::new(u0 * sw, v1 * sh),
                        ],
                        [
                            alpha_at(u0, v0),
                            alpha_at(u1, v0),
                            alpha_at(u1, v1),
                            alpha_at(u0, v1),
                        ],
                    );
                }
            }
        });
    }

    /// Forget per-game visual state (particles, cached geometry, shake).
    pub fn reset(&mut self) {
        self.particles.clear();
        self.cache = None;
        self.shake = 0.0;
        self.death_burst = None;
        self.extra_life_flash = 0.0;
        self.last_player = None;
        self.trail_acc = 0.0;
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer::new()
    }
}

// ---------------------------------------------------------------------------
// Wall tessellation
// ---------------------------------------------------------------------------

/// Stops `(offset_px, colour)` across a stroke.
type Profile<'a> = &'a [(f32, Color)];

/// Stroke every primitive of `g` (plus round `caps`) with one pass.
fn stroke_pass(b: &mut Builder, lay: &Layout, g: &WallGeometry, caps: &[Cap], pass: Pass) {
    let h = lay.px(pass.width) * 0.5;
    if h <= 0.0 {
        return;
    }
    let c = pass.color;
    let z = with_alpha(c, 0.0);
    let m = fade(c, 0.4);
    let soft_rows = [(-h, z), (-h * 0.45, m), (0.0, c), (h * 0.45, m), (h, z)];
    let solid_rows = [(-h, c), (h, c)];
    let soft_half = [(0.0, c), (h * 0.45, m), (h, z)];
    let solid_half = [(0.0, c), (h, c)];
    let (rows, half): (Profile, Profile) = if pass.soft {
        (&soft_rows, &soft_half)
    } else {
        (&solid_rows, &solid_half)
    };

    for &(a, e) in &g.segments {
        b.strip(lay.maze(a), lay.maze(e), rows);
    }
    let mut rings = [(0.0f32, c); gfx::MAX_STOPS];
    for arc in &g.arcs {
        let r = lay.px(arc.radius);
        for (dst, &(off, col)) in rings.iter_mut().zip(rows) {
            *dst = ((r + off).max(0.0), col);
        }
        // Offsets ascend, so clamped radii stay non-decreasing.
        let segs = gfx::segs_for(r + h, arc.sweep);
        b.polar(
            lay.maze(arc.center),
            arc.start,
            arc.sweep,
            &rings[..rows.len()],
            segs,
        );
    }
    stroke_caps(b, lay, caps, pass);
    let dot_segs = gfx::segs_for(h, TAU);
    for &d in &g.dots {
        b.polar(lay.maze(d), 0.0, TAU, half, dot_segs);
    }
}

/// Round stroke ends (`caps`) for one pass.
fn stroke_caps(b: &mut Builder, lay: &Layout, caps: &[Cap], pass: Pass) {
    let h = lay.px(pass.width) * 0.5;
    if h <= 0.0 || caps.is_empty() {
        return;
    }
    let c = pass.color;
    let soft_half = [(0.0, c), (h * 0.45, fade(c, 0.4)), (h, with_alpha(c, 0.0))];
    let solid_half = [(0.0, c), (h, c)];
    let half: Profile = if pass.soft { &soft_half } else { &solid_half };
    let segs = gfx::segs_for(h, PI);
    for cap in caps {
        let ang = cap.dir.y.atan2(cap.dir.x);
        b.polar(lay.maze(cap.center), ang - FRAC_PI_2, PI, half, segs);
    }
}

/// Translucent fill of the wall interiors (see `maze_geom::FillCell`).
fn draw_fills(b: &mut Builder, lay: &Layout, o: &MazeOutline, color: Color) {
    const ARC_STEPS: usize = 6;
    let r = CORNER_RADIUS;
    // Corner offsets (TL, TR, BR, BL) and their arc start angles around the
    // cell centre (each rounded corner sweeps a quarter toward the next side).
    let corners = [
        Vec2::new(-0.5, -0.5),
        Vec2::new(0.5, -0.5),
        Vec2::new(0.5, 0.5),
        Vec2::new(-0.5, 0.5),
    ];
    let starts = [PI, PI * 1.5, 0.0, FRAC_PI_2];
    let mut pts = [Vec2::ZERO; 4 * (ARC_STEPS + 1)];
    for cell in &o.fills {
        let mut n = 0;
        for k in 0..4 {
            if cell.rounded & (1 << k) != 0 {
                for s in 0..=ARC_STEPS {
                    let a = starts[k] + FRAC_PI_2 * s as f32 / ARC_STEPS as f32;
                    pts[n] = lay.maze(cell.center + Vec2::new(a.cos(), a.sin()) * r);
                    n += 1;
                }
            } else {
                pts[n] = lay.maze(cell.center + corners[k]);
                n += 1;
            }
        }
        b.fan(lay.maze(cell.center), color, &pts[..n], color);
    }
    let mut sl = [Vec2::ZERO; ARC_STEPS + 1];
    for s in &o.slivers {
        let d = s.corner - s.arc_center;
        // The arc's ends sit at the arc centre +/- the two axis components of `d`.
        let a0 = Vec2::new(d.x, 0.0).normalize_or_zero();
        let a1 = Vec2::new(0.0, d.y).normalize_or_zero();
        let t0 = a0.y.atan2(a0.x);
        let mut sweep = a1.y.atan2(a1.x) - t0;
        if sweep > PI {
            sweep -= TAU;
        } else if sweep < -PI {
            sweep += TAU;
        }
        for (i, p) in sl.iter_mut().enumerate() {
            let a = t0 + sweep * i as f32 / ARC_STEPS as f32;
            *p = lay.maze(s.arc_center + Vec2::new(a.cos(), a.sin()) * r);
        }
        let corner = lay.maze(s.corner);
        for w in sl.windows(2) {
            b.triangle([corner, w[0], w[1]], [color; 3]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formatting() {
        let mut b = [0u8; 10];
        assert_eq!(fmt_u32(0, &mut b), "0");
        assert_eq!(fmt_u32(7, &mut b), "7");
        assert_eq!(fmt_u32(1600, &mut b), "1600");
        assert_eq!(fmt_u32(u32::MAX, &mut b), "4294967295");
        let mut t = [0u8; 48];
        assert_eq!(concat(&mut t, &["LEVEL ", "12"]), "LEVEL 12");
        let long = "X".repeat(100);
        assert_eq!(concat(&mut t, &[&long]).len(), 48);
    }

    #[test]
    fn banner_row_sits_on_open_floor() {
        for i in 0..Maze::builtin_count() {
            let m = Maze::builtin(i);
            let c = MazeCache::new(&m);
            let y = (c.banner_y - 0.5) as i32;
            assert!((m.door.y + 4..=m.door.y + 7).contains(&y), "maze {i}");
            assert!(!m.is_wall(Tile::new(m.w / 2, y)), "maze {i}");
            assert!(
                !m.is_wall(Tile::new(m.w / 2 - 3, y)) && !m.is_wall(Tile::new(m.w / 2 + 3, y)),
                "maze {i}"
            );
        }
    }

    #[test]
    fn renderer_construction_and_update_need_no_window() {
        let mut r = Renderer::new();
        r.update(0.1);
        r.update(f32::NAN);
        r.update(-1.0);
        assert!((r.time() - 0.1).abs() < 1e-6);
        r.reset();
        assert!(r.particles.is_empty());
        let r2 = Renderer::default();
        assert!(r2.shake_enabled && !r2.scanlines && !r2.debug_overlay);
    }

    /// Seconds the ambient clock advanced from `a` to `b` (across a wrap).
    fn advanced(a: f64, b: f64) -> f64 {
        (b - a).rem_euclid(AMBIENT_PERIOD)
    }

    #[test]
    fn ambient_clock_never_freezes_or_drifts() {
        // One minute of frames at many refresh rates, starting fresh, after a
        // long session (where an f32 accumulator used to freeze or run fast)
        // and right before the wrap.
        for hz in [60u32, 120, 144, 240, 360, 500, 1000] {
            for start in [0.0, 19.0 * 3600.0, 20.0 * 3600.0, AMBIENT_PERIOD - 30.0] {
                let mut r = Renderer::new();
                r.time = f64::rem_euclid(start, AMBIENT_PERIOD);
                let t0 = r.time;
                let mut last = r.time();
                let mut moved = 0u32;
                for _ in 0..hz * 60 {
                    r.update(1.0 / hz as f32);
                    let now = r.time();
                    assert!((0.0..AMBIENT_PERIOD as f32).contains(&now));
                    if now != last {
                        moved += 1;
                    }
                    last = now;
                }
                let dt = advanced(t0, r.time);
                assert!((dt - 60.0).abs() < 1e-3, "{hz} Hz from {start} s: {dt} s");
                // The f32 view keeps advancing every frame too.
                assert_eq!(moved, hz * 60, "{hz} Hz from {start} s");
            }
        }
    }

    #[test]
    fn ambient_clock_wraps_with_full_precision() {
        let mut r = Renderer::new();
        r.time = AMBIENT_PERIOD - 0.5;
        r.update(0.2);
        r.update(0.2);
        r.update(0.2);
        assert!((r.time() - 0.1).abs() < 1e-4, "{}", r.time());
        // Precision of the f32 view at the top of the period.
        let top = (AMBIENT_PERIOD as f32).next_down();
        assert!(top - top.next_down() < 1e-3);
    }

    #[test]
    fn popups_stay_on_the_board() {
        // Inside the range: unchanged.
        assert_eq!(clamp_centre(50.0, 20.0, 0.0, 100.0), 50.0);
        // Hanging over either edge: pulled in so the text ends at the edge.
        assert_eq!(clamp_centre(-30.0, 40.0, 10.0, 300.0), 30.0);
        assert_eq!(clamp_centre(310.0, 40.0, 10.0, 300.0), 280.0);
        // Wider than the range: centred.
        assert_eq!(clamp_centre(0.0, 500.0, 10.0, 300.0), 155.0);
        // A big chain score eaten at the left tunnel mouth stays readable.
        let lay = Layout::compute(594.0, 792.0);
        let m = Maze::builtin(0);
        let w = font::text_width("1600", lay.px(0.95) * BIG_POPUP_POP);
        let lo = lay.maze(Vec2::new(POPUP_MARGIN, 0.0)).x;
        let hi = lay.maze(Vec2::new(m.w as f32 - POPUP_MARGIN, 0.0)).x;
        for x in [-1.0, -0.9, 0.0, m.w as f32 + 0.9] {
            let c = clamp_centre(lay.maze(Vec2::new(x, 0.0)).x, w, lo, hi);
            assert!(c - w * 0.5 >= lo - 1e-3 && c + w * 0.5 <= hi + 1e-3, "{x}");
        }
    }

    #[test]
    fn tunnel_masks_stay_local_and_feathered() {
        let (u, v) = (TUNNEL_MASK_U, TUNNEL_MASK_V);
        // Transparent at every outer edge (no hard stripe edges), opaque core.
        for prof in [u, v] {
            assert_eq!(prof[0].1, 0.0);
            assert_eq!(prof[3].1, 0.0);
            assert!(prof[1].1 == 1.0 && prof[2].1 == 1.0);
            assert!(prof.windows(2).all(|w| w[0].0 < w[1].0));
        }
        // Transparent on the board edge itself, a short reach beyond it.
        assert_eq!(u[0].0, 0.0);
        assert!(u[3].0 <= 3.5);
        // The opaque core hides an actor wrapping one tile past the edge,
        // body and bright inner glow included.
        assert!(u[2].0 >= 1.0 + SHADE_RADIUS.max(GLIM_RADIUS) + 0.5);
        assert!(v[2].0 >= 1.9 * SHADE_RADIUS && -v[1].0 >= 1.9 * SHADE_RADIUS);
    }

    #[test]
    fn tunnel_masks_match_the_backdrop() {
        let mut r = Renderer::new();
        r.layout = Layout::compute(1600.0, 700.0);
        r.update(3.0);
        let lay = r.layout;
        let th = r.theme;
        // Far from the haze the mask is the plain gradient.
        let corner = Vec2::new(lay.screen_w, 0.0);
        assert!(r.backdrop_at(&lay, corner).g >= th.backdrop_top.g - 1e-6);
        // At a haze centre it carries the haze tint, like the real backdrop.
        let (centre, ..) = r.haze_clouds()[0];
        let plain = mix(th.backdrop_top, th.backdrop_bottom, centre.y / lay.screen_h);
        let c = r.backdrop_at(&lay, centre);
        let lift = |c: Color| c.r + c.g + c.b;
        assert!(lift(c) > lift(plain) + 0.02, "{c:?} vs {plain:?}");
        assert!((c.a - 1.0).abs() < 1e-6);
    }

    #[test]
    fn easing_is_clamped() {
        assert_eq!(ease_out(-1.0), 0.0);
        assert_eq!(ease_out(2.0), 1.0);
        assert!(ease_out(0.5) > 0.5);
    }
}
