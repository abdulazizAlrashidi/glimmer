//! Built-in neon pixel font (an original 5x7 glyph set), so no font files
//! are needed.
//!
//! `size` is the height in window pixels of the 7-row glyph box (one font
//! pixel = `size / 7`). Each glyph advances 6 font pixels (5 + 1 spacing),
//! a space included. `y` is the top of the glyph box. Lowercase letters
//! render as uppercase and unknown characters render as a hollow box. The
//! font covers at least A-Z, 0-9, space and
//! `. , : ; ! ? ' " - + = / \ ( ) [ ] < > _ % & # * @ ©` plus arrows ← ↑ → ↓.
//!
//! The glyphs are a geometric design meant to look like neon
//! tubing: a constant one-pixel stroke, softened (cut) corners on round
//! letters, an arched `M`/`W` pair that reads like bent glass tubing, and a
//! zero with a centre dot so it never gets confused with `O`.
//!
//! Each glyph is drawn as a handful of solid rectangles: horizontal runs of
//! lit pixels are merged, and identical runs on consecutive rows are merged
//! vertically (a vertical stem becomes a single rectangle). When a font pixel
//! is at least two window pixels wide every rectangle edge is snapped to the
//! window pixel grid, so text stays sharp at any scale without changing its
//! layout width. Nothing here allocates.

use macroquad::color::Color;
use macroquad::shapes::draw_rectangle;

/// Glyph box size in font pixels.
pub const GLYPH_W: usize = 5;
pub const GLYPH_H: usize = 7;
/// Horizontal advance per character in font pixels.
pub const ADVANCE: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// One glyph bitmap: 7 rows, top to bottom, low 5 bits used, bit 4 = leftmost.
type Bitmap = [u8; GLYPH_H];

/// Mask of the five meaningful bits in a glyph row.
const ROW_MASK: u8 = 0b1_1111;

/// Font pixels at or above this many window pixels are snapped to the grid.
const SNAP_MIN_PX: f32 = 2.0;

/// Halo passes of [`draw_text_glow`], drawn outermost first:
/// `(spread in font pixels, alpha multiplier)`.
const GLOW_PASSES: [(f32, f32); 3] = [(1.45, 0.07), (0.85, 0.12), (0.4, 0.2)];

/// How far the glow core is lightened toward white (0 = text colour, 1 = white).
const GLOW_CORE_LIGHTEN: f32 = 0.45;

// ---------------------------------------------------------------------------
// Glyph table
// ---------------------------------------------------------------------------

/// The glyph bitmaps, one row per line so the shapes are visible in the
/// source (`1` = lit). Kept out of rustfmt's reach to preserve the layout.
#[rustfmt::skip]
mod glyphs {
    use super::Bitmap;

    /// Drawn for characters the font does not cover.
    pub(super) const MISSING: Bitmap = [
        0b11111,
        0b10001,
        0b10001,
        0b10001,
        0b10001,
        0b10001,
        0b11111,
    ];

    pub(super) const SPACE: Bitmap = [
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
    ];

    // Letters -----------------------------------------------------------------
    pub(super) const A: Bitmap = [
        0b01110,
        0b10001,
        0b10001,
        0b11111,
        0b10001,
        0b10001,
        0b10001,
    ];

    pub(super) const B: Bitmap = [
        0b11110,
        0b10001,
        0b10001,
        0b11110,
        0b10001,
        0b10001,
        0b11110,
    ];

    pub(super) const C: Bitmap = [
        0b01110,
        0b10001,
        0b10000,
        0b10000,
        0b10000,
        0b10001,
        0b01110,
    ];

    pub(super) const D: Bitmap = [
        0b11110,
        0b10001,
        0b10001,
        0b10001,
        0b10001,
        0b10001,
        0b11110,
    ];

    pub(super) const E: Bitmap = [
        0b11111,
        0b10000,
        0b10000,
        0b11110,
        0b10000,
        0b10000,
        0b11111,
    ];

    pub(super) const F: Bitmap = [
        0b11111,
        0b10000,
        0b10000,
        0b11110,
        0b10000,
        0b10000,
        0b10000,
    ];

    pub(super) const G: Bitmap = [
        0b01110,
        0b10001,
        0b10000,
        0b10111,
        0b10001,
        0b10001,
        0b01110,
    ];

    pub(super) const H: Bitmap = [
        0b10001,
        0b10001,
        0b10001,
        0b11111,
        0b10001,
        0b10001,
        0b10001,
    ];

    pub(super) const I: Bitmap = [
        0b01110,
        0b00100,
        0b00100,
        0b00100,
        0b00100,
        0b00100,
        0b01110,
    ];

    pub(super) const J: Bitmap = [
        0b00001,
        0b00001,
        0b00001,
        0b00001,
        0b00001,
        0b10001,
        0b01110,
    ];

    pub(super) const K: Bitmap = [
        0b10001,
        0b10010,
        0b10100,
        0b11000,
        0b10100,
        0b10010,
        0b10001,
    ];

    pub(super) const L: Bitmap = [
        0b10000,
        0b10000,
        0b10000,
        0b10000,
        0b10000,
        0b10000,
        0b11111,
    ];

    pub(super) const M: Bitmap = [
        0b01010,
        0b10101,
        0b10101,
        0b10101,
        0b10001,
        0b10001,
        0b10001,
    ];

    pub(super) const N: Bitmap = [
        0b10001,
        0b10001,
        0b11001,
        0b10101,
        0b10011,
        0b10001,
        0b10001,
    ];

    pub(super) const O: Bitmap = [
        0b01110,
        0b10001,
        0b10001,
        0b10001,
        0b10001,
        0b10001,
        0b01110,
    ];

    pub(super) const P: Bitmap = [
        0b11110,
        0b10001,
        0b10001,
        0b11110,
        0b10000,
        0b10000,
        0b10000,
    ];

    pub(super) const Q: Bitmap = [
        0b01110,
        0b10001,
        0b10001,
        0b10001,
        0b10101,
        0b10010,
        0b01101,
    ];

    pub(super) const R: Bitmap = [
        0b11110,
        0b10001,
        0b10001,
        0b11110,
        0b10100,
        0b10010,
        0b10001,
    ];

    pub(super) const S: Bitmap = [
        0b01111,
        0b10000,
        0b10000,
        0b01110,
        0b00001,
        0b00001,
        0b11110,
    ];

    pub(super) const T: Bitmap = [
        0b11111,
        0b00100,
        0b00100,
        0b00100,
        0b00100,
        0b00100,
        0b00100,
    ];

    pub(super) const U: Bitmap = [
        0b10001,
        0b10001,
        0b10001,
        0b10001,
        0b10001,
        0b10001,
        0b01110,
    ];

    pub(super) const V: Bitmap = [
        0b10001,
        0b10001,
        0b10001,
        0b10001,
        0b10001,
        0b01010,
        0b00100,
    ];

    pub(super) const W: Bitmap = [
        0b10001,
        0b10001,
        0b10001,
        0b10101,
        0b10101,
        0b10101,
        0b01010,
    ];

    pub(super) const X: Bitmap = [
        0b10001,
        0b10001,
        0b01010,
        0b00100,
        0b01010,
        0b10001,
        0b10001,
    ];

    pub(super) const Y: Bitmap = [
        0b10001,
        0b10001,
        0b01010,
        0b00100,
        0b00100,
        0b00100,
        0b00100,
    ];

    pub(super) const Z: Bitmap = [
        0b11111,
        0b00001,
        0b00010,
        0b00100,
        0b01000,
        0b10000,
        0b11111,
    ];

    // Digits ------------------------------------------------------------------
    pub(super) const D0: Bitmap = [
        0b01110,
        0b10001,
        0b10001,
        0b10101,
        0b10001,
        0b10001,
        0b01110,
    ];

    pub(super) const D1: Bitmap = [
        0b00100,
        0b01100,
        0b00100,
        0b00100,
        0b00100,
        0b00100,
        0b01110,
    ];

    pub(super) const D2: Bitmap = [
        0b01110,
        0b10001,
        0b00001,
        0b00010,
        0b00100,
        0b01000,
        0b11111,
    ];

    pub(super) const D3: Bitmap = [
        0b11110,
        0b00001,
        0b00001,
        0b01110,
        0b00001,
        0b00001,
        0b11110,
    ];

    pub(super) const D4: Bitmap = [
        0b00010,
        0b00110,
        0b01010,
        0b10010,
        0b11111,
        0b00010,
        0b00010,
    ];

    pub(super) const D5: Bitmap = [
        0b11111,
        0b10000,
        0b11110,
        0b00001,
        0b00001,
        0b10001,
        0b01110,
    ];

    pub(super) const D6: Bitmap = [
        0b00110,
        0b01000,
        0b10000,
        0b11110,
        0b10001,
        0b10001,
        0b01110,
    ];

    pub(super) const D7: Bitmap = [
        0b11111,
        0b00001,
        0b00010,
        0b00100,
        0b01000,
        0b01000,
        0b01000,
    ];

    pub(super) const D8: Bitmap = [
        0b01110,
        0b10001,
        0b10001,
        0b01110,
        0b10001,
        0b10001,
        0b01110,
    ];

    pub(super) const D9: Bitmap = [
        0b01110,
        0b10001,
        0b10001,
        0b01111,
        0b00001,
        0b00010,
        0b01100,
    ];

    // Punctuation & symbols ---------------------------------------------------
    pub(super) const PERIOD: Bitmap = [
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b00100,
    ];

    pub(super) const COMMA: Bitmap = [
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b00100,
        0b01000,
    ];

    pub(super) const COLON: Bitmap = [
        0b00000,
        0b00100,
        0b00000,
        0b00000,
        0b00000,
        0b00100,
        0b00000,
    ];

    pub(super) const SEMICOLON: Bitmap = [
        0b00000,
        0b00100,
        0b00000,
        0b00000,
        0b00000,
        0b00100,
        0b01000,
    ];

    pub(super) const EXCLAIM: Bitmap = [
        0b00100,
        0b00100,
        0b00100,
        0b00100,
        0b00100,
        0b00000,
        0b00100,
    ];

    pub(super) const QUESTION: Bitmap = [
        0b01110,
        0b10001,
        0b00001,
        0b00010,
        0b00100,
        0b00000,
        0b00100,
    ];

    pub(super) const APOSTROPHE: Bitmap = [
        0b00100,
        0b00100,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
    ];

    pub(super) const QUOTE: Bitmap = [
        0b01010,
        0b01010,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
    ];

    pub(super) const BACKTICK: Bitmap = [
        0b01000,
        0b00100,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
    ];

    pub(super) const MINUS: Bitmap = [
        0b00000,
        0b00000,
        0b00000,
        0b01110,
        0b00000,
        0b00000,
        0b00000,
    ];

    pub(super) const PLUS: Bitmap = [
        0b00000,
        0b00100,
        0b00100,
        0b11111,
        0b00100,
        0b00100,
        0b00000,
    ];

    pub(super) const EQUALS: Bitmap = [
        0b00000,
        0b00000,
        0b11111,
        0b00000,
        0b11111,
        0b00000,
        0b00000,
    ];

    pub(super) const SLASH: Bitmap = [
        0b00001,
        0b00001,
        0b00010,
        0b00100,
        0b01000,
        0b10000,
        0b10000,
    ];

    pub(super) const BACKSLASH: Bitmap = [
        0b10000,
        0b10000,
        0b01000,
        0b00100,
        0b00010,
        0b00001,
        0b00001,
    ];

    pub(super) const PAREN_L: Bitmap = [
        0b00010,
        0b00100,
        0b01000,
        0b01000,
        0b01000,
        0b00100,
        0b00010,
    ];

    pub(super) const PAREN_R: Bitmap = [
        0b01000,
        0b00100,
        0b00010,
        0b00010,
        0b00010,
        0b00100,
        0b01000,
    ];

    pub(super) const BRACKET_L: Bitmap = [
        0b01110,
        0b01000,
        0b01000,
        0b01000,
        0b01000,
        0b01000,
        0b01110,
    ];

    pub(super) const BRACKET_R: Bitmap = [
        0b01110,
        0b00010,
        0b00010,
        0b00010,
        0b00010,
        0b00010,
        0b01110,
    ];

    pub(super) const BRACE_L: Bitmap = [
        0b00011,
        0b00100,
        0b00100,
        0b01000,
        0b00100,
        0b00100,
        0b00011,
    ];

    pub(super) const BRACE_R: Bitmap = [
        0b11000,
        0b00100,
        0b00100,
        0b00010,
        0b00100,
        0b00100,
        0b11000,
    ];

    pub(super) const LESS: Bitmap = [
        0b00010,
        0b00100,
        0b01000,
        0b10000,
        0b01000,
        0b00100,
        0b00010,
    ];

    pub(super) const GREATER: Bitmap = [
        0b01000,
        0b00100,
        0b00010,
        0b00001,
        0b00010,
        0b00100,
        0b01000,
    ];

    pub(super) const UNDERSCORE: Bitmap = [
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
        0b11111,
    ];

    pub(super) const PERCENT: Bitmap = [
        0b11001,
        0b11001,
        0b00010,
        0b00100,
        0b01000,
        0b10011,
        0b10011,
    ];

    pub(super) const AMPERSAND: Bitmap = [
        0b01100,
        0b10010,
        0b10100,
        0b01000,
        0b10101,
        0b10010,
        0b01101,
    ];

    pub(super) const HASH: Bitmap = [
        0b01010,
        0b01010,
        0b11111,
        0b01010,
        0b11111,
        0b01010,
        0b01010,
    ];

    pub(super) const ASTERISK: Bitmap = [
        0b00000,
        0b00100,
        0b10101,
        0b01110,
        0b10101,
        0b00100,
        0b00000,
    ];

    pub(super) const AT: Bitmap = [
        0b01110,
        0b10001,
        0b10111,
        0b10101,
        0b10111,
        0b10000,
        0b01111,
    ];

    pub(super) const COPYRIGHT: Bitmap = [
        0b01110,
        0b10001,
        0b10110,
        0b10101,
        0b10110,
        0b10001,
        0b01110,
    ];

    pub(super) const DOLLAR: Bitmap = [
        0b00100,
        0b01111,
        0b10100,
        0b01110,
        0b00101,
        0b11110,
        0b00100,
    ];

    pub(super) const CARET: Bitmap = [
        0b00100,
        0b01010,
        0b10001,
        0b00000,
        0b00000,
        0b00000,
        0b00000,
    ];

    pub(super) const TILDE: Bitmap = [
        0b00000,
        0b00000,
        0b01000,
        0b10101,
        0b00010,
        0b00000,
        0b00000,
    ];

    pub(super) const PIPE: Bitmap = [
        0b00100,
        0b00100,
        0b00100,
        0b00100,
        0b00100,
        0b00100,
        0b00100,
    ];

    pub(super) const MIDDLE_DOT: Bitmap = [
        0b00000,
        0b00000,
        0b00000,
        0b00100,
        0b00000,
        0b00000,
        0b00000,
    ];

    pub(super) const BULLET: Bitmap = [
        0b00000,
        0b00000,
        0b01110,
        0b01110,
        0b01110,
        0b00000,
        0b00000,
    ];

    pub(super) const TIMES: Bitmap = [
        0b00000,
        0b10001,
        0b01010,
        0b00100,
        0b01010,
        0b10001,
        0b00000,
    ];

    // Arrows ------------------------------------------------------------------
    pub(super) const ARROW_LEFT: Bitmap = [
        0b00000,
        0b00100,
        0b01000,
        0b11111,
        0b01000,
        0b00100,
        0b00000,
    ];

    pub(super) const ARROW_RIGHT: Bitmap = [
        0b00000,
        0b00100,
        0b00010,
        0b11111,
        0b00010,
        0b00100,
        0b00000,
    ];

    pub(super) const ARROW_UP: Bitmap = [
        0b00100,
        0b01110,
        0b10101,
        0b00100,
        0b00100,
        0b00100,
        0b00100,
    ];

    pub(super) const ARROW_DOWN: Bitmap = [
        0b00100,
        0b00100,
        0b00100,
        0b00100,
        0b10101,
        0b01110,
        0b00100,
    ];
}

use glyphs::*;

/// Bitmap for `c` (7 rows, low 5 bits per row, bit 4 = leftmost column),
/// `None` if the font has no glyph for it.
pub fn glyph(c: char) -> Option<[u8; GLYPH_H]> {
    let bitmap = match c.to_ascii_uppercase() {
        ' ' => SPACE,
        'A' => A,
        'B' => B,
        'C' => C,
        'D' => D,
        'E' => E,
        'F' => F,
        'G' => G,
        'H' => H,
        'I' => I,
        'J' => J,
        'K' => K,
        'L' => L,
        'M' => M,
        'N' => N,
        'O' => O,
        'P' => P,
        'Q' => Q,
        'R' => R,
        'S' => S,
        'T' => T,
        'U' => U,
        'V' => V,
        'W' => W,
        'X' => X,
        'Y' => Y,
        'Z' => Z,
        '0' => D0,
        '1' => D1,
        '2' => D2,
        '3' => D3,
        '4' => D4,
        '5' => D5,
        '6' => D6,
        '7' => D7,
        '8' => D8,
        '9' => D9,
        '.' => PERIOD,
        ',' => COMMA,
        ':' => COLON,
        ';' => SEMICOLON,
        '!' => EXCLAIM,
        '?' => QUESTION,
        '\'' | '\u{2019}' => APOSTROPHE,
        '"' => QUOTE,
        '`' => BACKTICK,
        '-' | '\u{2212}' => MINUS,
        '+' => PLUS,
        '=' => EQUALS,
        '/' => SLASH,
        '\\' => BACKSLASH,
        '(' => PAREN_L,
        ')' => PAREN_R,
        '[' => BRACKET_L,
        ']' => BRACKET_R,
        '{' => BRACE_L,
        '}' => BRACE_R,
        '<' => LESS,
        '>' => GREATER,
        '_' => UNDERSCORE,
        '%' => PERCENT,
        '&' => AMPERSAND,
        '#' => HASH,
        '*' => ASTERISK,
        '@' => AT,
        '\u{00A9}' => COPYRIGHT,
        '$' => DOLLAR,
        '^' => CARET,
        '~' => TILDE,
        '|' => PIPE,
        '\u{00B7}' => MIDDLE_DOT,
        '\u{2022}' => BULLET,
        '\u{00D7}' => TIMES,
        '\u{2190}' => ARROW_LEFT,
        '\u{2191}' => ARROW_UP,
        '\u{2192}' => ARROW_RIGHT,
        '\u{2193}' => ARROW_DOWN,
        _ => return None,
    };
    Some(bitmap)
}

// ---------------------------------------------------------------------------
// Glyph to rectangle decomposition
// ---------------------------------------------------------------------------

/// A solid block of lit font pixels inside one glyph box (font-pixel units).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Block {
    col: u8,
    row: u8,
    w: u8,
    h: u8,
}

/// Upper bound on blocks per glyph: at most 3 runs in each of the 7 five-bit
/// rows.
const MAX_BLOCKS: usize = 21;

/// Fixed-capacity block list (no heap allocation in the draw path).
struct Blocks {
    items: [Block; MAX_BLOCKS],
    len: usize,
}

impl Blocks {
    fn as_slice(&self) -> &[Block] {
        &self.items[..self.len]
    }
}

/// Bit for column `col` (0 = leftmost) in a glyph row.
const fn col_bit(col: u8) -> u8 {
    1 << (GLYPH_W as u8 - 1 - col)
}

/// Decompose a bitmap into blocks: maximal horizontal runs, with identical
/// maximal runs on consecutive rows merged into one taller block.
fn glyph_blocks(bitmap: &Bitmap) -> Blocks {
    let mut out = Blocks {
        items: [Block::default(); MAX_BLOCKS],
        len: 0,
    };
    // Bits of each row already covered by a block started on an earlier row.
    let mut used = [0u8; GLYPH_H];
    for row in 0..GLYPH_H {
        let bits = bitmap[row] & ROW_MASK;
        let mut col = 0u8;
        while (col as usize) < GLYPH_W {
            if bits & col_bit(col) == 0 {
                col += 1;
                continue;
            }
            let start = col;
            let mut mask = 0u8;
            while (col as usize) < GLYPH_W && bits & col_bit(col) != 0 {
                mask |= col_bit(col);
                col += 1;
            }
            if used[row] & mask == mask {
                continue; // continuation of a block from the row above
            }
            // A row continues this block if it holds exactly the same maximal run.
            let halo = (mask | (mask << 1) | (mask >> 1)) & ROW_MASK;
            let mut h = 1usize;
            while row + h < GLYPH_H && bitmap[row + h] & halo == mask {
                used[row + h] |= mask;
                h += 1;
            }
            if out.len < MAX_BLOCKS {
                out.items[out.len] = Block {
                    col: start,
                    row: row as u8,
                    w: col - start,
                    h: h as u8,
                };
                out.len += 1;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Layout & drawing
// ---------------------------------------------------------------------------

/// Window pixels per font pixel for a given `size`; 0 for non-positive/NaN sizes.
fn font_px(size: f32) -> f32 {
    if size.is_finite() && size > 0.0 {
        size / GLYPH_H as f32
    } else {
        0.0
    }
}

/// Width in font pixels of `n` characters (no trailing spacing column).
fn width_in_font_px(n: usize) -> f32 {
    if n == 0 {
        0.0
    } else {
        n as f32 * ADVANCE - 1.0
    }
}

/// Width in pixels of `text` at `size` (no trailing spacing column).
pub fn text_width(text: &str, size: f32) -> f32 {
    width_in_font_px(text.chars().count()) * font_px(size)
}

/// Resolved placement of a string: top-left origin, font pixel size and
/// whether edges snap to the window pixel grid.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    ox: f32,
    oy: f32,
    px: f32,
    snap: bool,
}

fn place(text: &str, x: f32, y: f32, size: f32, align: Align) -> Option<Placement> {
    let px = font_px(size);
    if px <= 0.0 || !x.is_finite() || !y.is_finite() {
        return None;
    }
    let w = text_width(text, size);
    let left = match align {
        Align::Left => x,
        Align::Center => x - w * 0.5,
        Align::Right => x - w,
    };
    let snap = px >= SNAP_MIN_PX;
    let (ox, oy) = if snap {
        (left.round(), y.round())
    } else {
        (left, y)
    };
    Some(Placement { ox, oy, px, snap })
}

/// Map a span `[start, start + len)` in font pixels to `(position, length)` in
/// window pixels. With `snap`, both edges are rounded independently, so
/// adjacent spans tile the pixel grid with neither gaps nor overlaps.
fn span(origin: f32, start: f32, len: f32, px: f32, snap: bool) -> (f32, f32) {
    let a = origin + start * px;
    let b = origin + (start + len) * px;
    let (a, b) = if snap { (a.round(), b.round()) } else { (a, b) };
    (a, b - a)
}

/// Draw every block of `text`, each enlarged by `grow` window pixels per side.
fn draw_blocks(text: &str, p: Placement, grow: f32, color: Color) {
    let snap = p.snap && grow == 0.0;
    for (i, ch) in text.chars().enumerate() {
        if ch == ' ' {
            continue;
        }
        let bitmap = glyph(ch).unwrap_or(MISSING);
        let base = i as f32 * ADVANCE;
        for b in glyph_blocks(&bitmap).as_slice() {
            let (x, w) = span(p.ox, base + f32::from(b.col), f32::from(b.w), p.px, snap);
            let (y, h) = span(p.oy, f32::from(b.row), f32::from(b.h), p.px, snap);
            draw_rectangle(x - grow, y - grow, w + 2.0 * grow, h + 2.0 * grow, color);
        }
    }
}

/// Draw crisp flat text. `x` is interpreted according to `align`.
pub fn draw_text(text: &str, x: f32, y: f32, size: f32, color: Color, align: Align) {
    if color.a <= 0.0 {
        return;
    }
    if let Some(p) = place(text, x, y, size, align) {
        draw_blocks(text, p, 0.0, color);
    }
}

/// Draw text with a soft neon glow halo in the same hue plus a bright core.
pub fn draw_text_glow(text: &str, x: f32, y: f32, size: f32, color: Color, align: Align) {
    if color.a <= 0.0 {
        return;
    }
    let Some(p) = place(text, x, y, size, align) else {
        return;
    };
    for (spread, alpha) in GLOW_PASSES {
        let halo = Color::new(color.r, color.g, color.b, color.a * alpha);
        draw_blocks(text, p, spread * p.px, halo);
    }
    draw_blocks(text, p, 0.0, lighten(color, GLOW_CORE_LIGHTEN));
}

/// Move `c` toward white by `t` (0..=1), keeping its alpha.
fn lighten(c: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::new(
        c.r + (1.0 - c.r) * t,
        c.g + (1.0 - c.g) * t,
        c.b + (1.0 - c.b) * t,
        c.a,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character the game needs the font to cover.
    const REQUIRED: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .,:;!?'\"-+=/\\()[]<>_%&#*@\u{00A9}\u{2190}\u{2191}\u{2192}\u{2193}";
    /// Optional extras the font also provides.
    const EXTRAS: &str = "`{}$^~|\u{00B7}\u{2022}\u{00D7}\u{2019}\u{2212}";

    fn all_glyph_chars() -> impl Iterator<Item = char> {
        REQUIRED.chars().chain(EXTRAS.chars())
    }

    #[test]
    fn covers_every_required_char() {
        for c in REQUIRED.chars() {
            assert!(glyph(c).is_some(), "missing glyph for {c:?}");
        }
        for c in EXTRAS.chars() {
            assert!(glyph(c).is_some(), "missing glyph for {c:?}");
        }
    }

    #[test]
    fn unknown_chars_have_no_glyph_and_draw_a_hollow_box() {
        for c in ['é', '\n', '\t', '€', '中', '\u{0}'] {
            assert_eq!(glyph(c), None, "{c:?}");
        }
        // The fallback box is hollow: full border, empty interior.
        assert_eq!(MISSING[0], ROW_MASK);
        assert_eq!(MISSING[GLYPH_H - 1], ROW_MASK);
        for row in &MISSING[1..GLYPH_H - 1] {
            assert_eq!(*row, 0b10001);
        }
    }

    #[test]
    fn lowercase_maps_to_uppercase() {
        for c in 'a'..='z' {
            assert_eq!(glyph(c), glyph(c.to_ascii_uppercase()), "{c}");
            assert!(glyph(c).is_some());
        }
    }

    #[test]
    fn every_glyph_fits_in_five_bits() {
        for c in all_glyph_chars() {
            let g = glyph(c).expect("covered");
            for row in g {
                assert_eq!(row & !ROW_MASK, 0, "{c:?} uses bits beyond column 5");
            }
        }
    }

    #[test]
    fn visible_glyphs_are_not_blank_and_space_is() {
        assert_eq!(glyph(' '), Some([0; GLYPH_H]));
        for c in all_glyph_chars().filter(|&c| c != ' ') {
            let g = glyph(c).expect("covered");
            assert!(g.iter().any(|&r| r != 0), "{c:?} is blank");
        }
    }

    #[test]
    fn letters_and_digits_are_all_distinct() {
        let chars: Vec<char> = ('A'..='Z').chain('0'..='9').collect();
        for (i, &a) in chars.iter().enumerate() {
            for &b in &chars[i + 1..] {
                assert_ne!(glyph(a), glyph(b), "{a} and {b} share a bitmap");
            }
        }
        // The distinctive zero must not look like the letter O.
        assert_ne!(glyph('0'), glyph('O'));
        // And no symbol may masquerade as the missing-glyph box.
        for c in all_glyph_chars() {
            assert_ne!(glyph(c), Some(MISSING), "{c:?}");
        }
    }

    #[test]
    fn width_math() {
        assert_eq!(text_width("AB", 7.0), 11.0);
        assert_eq!(text_width("A", 7.0), 5.0);
        assert_eq!(text_width("", 7.0), 0.0);
        assert_eq!(text_width(" ", 7.0), 5.0);
        assert_eq!(text_width("HELLO", 14.0), 2.0 * (5.0 * 6.0 - 1.0));
        // Width counts characters, not bytes.
        assert_eq!(text_width("\u{2190}\u{00A9}", 7.0), 11.0);
        // Degenerate sizes never produce negative or NaN widths.
        assert_eq!(text_width("ABC", 0.0), 0.0);
        assert_eq!(text_width("ABC", -7.0), 0.0);
        assert_eq!(text_width("ABC", f32::NAN), 0.0);
    }

    /// Rasterise blocks back into a bitmap, failing on any overlap.
    fn rasterise(blocks: &[Block]) -> Bitmap {
        let mut out = [0u8; GLYPH_H];
        for b in blocks {
            assert!(b.w > 0 && b.h > 0);
            assert!(usize::from(b.col + b.w) <= GLYPH_W && usize::from(b.row + b.h) <= GLYPH_H);
            for row in b.row..b.row + b.h {
                for col in b.col..b.col + b.w {
                    let bit = col_bit(col);
                    assert_eq!(out[usize::from(row)] & bit, 0, "overlapping blocks");
                    out[usize::from(row)] |= bit;
                }
            }
        }
        out
    }

    #[test]
    fn blocks_exactly_cover_every_glyph() {
        for c in all_glyph_chars() {
            let g = glyph(c).expect("covered");
            let blocks = glyph_blocks(&g);
            assert_eq!(rasterise(blocks.as_slice()), g, "{c:?}");
        }
        assert_eq!(rasterise(glyph_blocks(&MISSING).as_slice()), MISSING);
    }

    #[test]
    fn blocks_merge_runs_and_stems() {
        // I: top bar, one stem, bottom bar.
        assert_eq!(glyph_blocks(&I).len, 3);
        // L: one stem (rows 0..6) plus the bottom bar.
        assert_eq!(glyph_blocks(&L).len, 2);
        // T: bar plus a single 6-row stem.
        let t = glyph_blocks(&T);
        assert_eq!(t.len, 2);
        assert_eq!(
            t.items[1],
            Block {
                col: 2,
                row: 1,
                w: 1,
                h: 6
            }
        );
        // Pipe is one tall block; space has none.
        assert_eq!(glyph_blocks(&PIPE).len, 1);
        assert_eq!(glyph_blocks(&SPACE).len, 0);
    }

    #[test]
    fn placement_respects_alignment_and_snapping() {
        let left = place("AB", 10.0, 20.0, 7.0, Align::Left).expect("valid");
        assert_eq!(
            (left.ox, left.oy, left.px, left.snap),
            (10.0, 20.0, 1.0, false)
        );
        let center = place("AB", 10.0, 20.0, 7.0, Align::Center).expect("valid");
        assert_eq!(center.ox, 10.0 - 5.5);
        let right = place("AB", 10.0, 20.0, 7.0, Align::Right).expect("valid");
        assert_eq!(right.ox, -1.0);
        // Large text snaps its origin to whole pixels.
        let big = place("AB", 10.3, 20.6, 21.0, Align::Left).expect("valid");
        assert!(big.snap);
        assert_eq!((big.ox, big.oy), (10.0, 21.0));
        // Invalid sizes or positions draw nothing.
        assert!(place("AB", 0.0, 0.0, 0.0, Align::Left).is_none());
        assert!(place("AB", f32::NAN, 0.0, 7.0, Align::Left).is_none());
    }

    #[test]
    fn snapped_spans_tile_the_pixel_grid() {
        let (origin, px) = (3.0, 2.6);
        let mut prev_end = None;
        for col in 0..20 {
            let (x, w) = span(origin, col as f32, 1.0, px, true);
            assert_eq!(x.fract(), 0.0);
            assert_eq!(w.fract(), 0.0);
            assert!((2.0..=3.0).contains(&w));
            if let Some(end) = prev_end {
                assert_eq!(x, end, "snapped spans must neither gap nor overlap");
            }
            prev_end = Some(x + w);
        }
        // A merged run covers exactly the union of its pixels.
        let (x, w) = span(origin, 2.0, 3.0, px, true);
        let (x0, _) = span(origin, 2.0, 1.0, px, true);
        let (x2, w2) = span(origin, 4.0, 1.0, px, true);
        assert_eq!((x, x + w), (x0, x2 + w2));
    }

    #[test]
    fn lighten_moves_toward_white() {
        let c = lighten(Color::new(0.2, 0.4, 1.0, 0.5), 0.5);
        assert!((c.r - 0.6).abs() < 1e-6);
        assert!((c.g - 0.7).abs() < 1e-6);
        assert!((c.b - 1.0).abs() < 1e-6);
        assert_eq!(c.a, 0.5);
    }
}
