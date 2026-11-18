//! Vertical text menus: selection model, layout/hit-testing and drawing.

use super::ui::{self, pal};
use crate::font::Align;
use crate::input::Actions;
use crate::view::Layout;
use macroquad::color::Color;
use macroquad::math::{Rect, Vec2};

/// Selection state of a menu with `len` entries (wraps around).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Menu {
    pub selected: usize,
    len: usize,
}

impl Menu {
    pub fn new(len: usize) -> Menu {
        Menu {
            selected: 0,
            len: len.max(1),
        }
    }

    pub fn prev(&mut self) {
        self.selected = (self.selected + self.len - 1) % self.len;
    }

    pub fn next(&mut self) {
        self.selected = (self.selected + 1) % self.len;
    }

    /// Select entry `i` (ignored when out of range). Returns whether the
    /// selection changed.
    pub fn select(&mut self, i: usize) -> bool {
        if i < self.len && i != self.selected {
            self.selected = i;
            true
        } else {
            false
        }
    }
}

/// What one frame of input did to a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuEvent {
    None,
    /// The selection moved (play the move sound).
    Moved,
    /// Entry activated with Enter/Space or a click.
    Activated(usize),
}

/// Apply keyboard navigation plus mouse hover/click. `hover` is the entry
/// under the pointer, if any. Hover only changes the selection when the
/// pointer actually moved, so a resting mouse never fights the keyboard.
pub fn navigate(menu: &mut Menu, act: &Actions, hover: Option<usize>) -> MenuEvent {
    if act.click {
        if let Some(i) = hover {
            menu.select(i);
            return MenuEvent::Activated(i);
        }
    }
    if act.confirm {
        return MenuEvent::Activated(menu.selected);
    }
    let mut moved = false;
    if act.up {
        menu.prev();
        moved = true;
    }
    if act.down {
        menu.next();
        moved = true;
    }
    if !moved && act.mouse_moved {
        if let Some(i) = hover {
            moved = menu.select(i);
        }
    }
    if moved {
        MenuEvent::Moved
    } else {
        MenuEvent::None
    }
}

/// Placement of a centred vertical list, in canvas tiles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ListLayout {
    /// Horizontal centre of the items.
    pub cx: f32,
    /// Top of the first item's glyph box.
    pub y0: f32,
    /// Distance between consecutive items.
    pub spacing: f32,
    /// Glyph height.
    pub size: f32,
}

impl ListLayout {
    pub fn item_y(&self, i: usize) -> f32 {
        self.y0 + i as f32 * self.spacing
    }

    /// Clickable area of item `i` (tiles): the label plus room for the marker.
    pub fn item_rect(&self, i: usize, label: &str) -> Rect {
        let w = ui::width(label, self.size) + 3.0;
        let h = self.spacing.max(self.size);
        let cy = self.item_y(i) + self.size * 0.5;
        Rect::new(self.cx - w * 0.5, cy - h * 0.5, w, h)
    }

    /// The item under canvas point `p`.
    pub fn hit(&self, labels: &[&str], p: Vec2) -> Option<usize> {
        labels
            .iter()
            .enumerate()
            .find(|(i, l)| self.item_rect(*i, l).contains(p))
            .map(|(i, _)| i)
    }
}

/// Draw a centred list; the selected entry glows, pulses and carries the
/// Glim marker.
pub fn draw_list(l: &Layout, labels: &[&str], menu: &Menu, list: &ListLayout, t: f32) {
    draw_list_with(l, labels, menu, list, t, pal::MINT);
}

/// [`draw_list`] with the selected entry in `accent` (e.g. a warning colour
/// while it waits for confirmation).
pub fn draw_list_with(
    l: &Layout,
    labels: &[&str],
    menu: &Menu,
    list: &ListLayout,
    t: f32,
    accent: Color,
) {
    for (i, label) in labels.iter().enumerate() {
        let y = list.item_y(i);
        if i == menu.selected {
            let p = ui::pulse(t, 1.2);
            let size = list.size * (1.0 + 0.05 * p);
            let y = y - (size - list.size) * 0.5;
            let color = ui::mix(accent, pal::INK, 0.25 * p);
            ui::text_glow(l, label, list.cx, y, size, color, Align::Center);
            let half = ui::width(label, size) * 0.5;
            ui::marker(l, list.cx - half - 1.05, y + size * 0.5, size * 0.42, t);
        } else {
            ui::text(l, label, list.cx, y, list.size, pal::DIM, Align::Center);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act() -> Actions {
        Actions::default()
    }

    #[test]
    fn menu_wraps_both_ways() {
        let mut m = Menu::new(3);
        m.prev();
        assert_eq!(m.selected, 2);
        m.next();
        assert_eq!(m.selected, 0);
        assert!(!m.select(7));
        assert!(m.select(1));
        assert!(!m.select(1));
        let mut empty = Menu::new(0);
        empty.next();
        empty.prev();
        assert_eq!(empty.selected, 0);
    }

    #[test]
    fn navigate_keyboard_and_mouse() {
        let mut m = Menu::new(4);
        let a = Actions {
            down: true,
            ..act()
        };
        assert_eq!(navigate(&mut m, &a, None), MenuEvent::Moved);
        assert_eq!(m.selected, 1);
        let a = Actions {
            confirm: true,
            ..act()
        };
        assert_eq!(navigate(&mut m, &a, Some(3)), MenuEvent::Activated(1));
        // A resting pointer does not steal the selection...
        assert_eq!(navigate(&mut m, &act(), Some(3)), MenuEvent::None);
        // ...a moving one does.
        let a = Actions {
            mouse_moved: true,
            ..act()
        };
        assert_eq!(navigate(&mut m, &a, Some(3)), MenuEvent::Moved);
        assert_eq!(m.selected, 3);
        let a = Actions {
            click: true,
            ..act()
        };
        assert_eq!(navigate(&mut m, &a, Some(0)), MenuEvent::Activated(0));
        assert_eq!(m.selected, 0);
        // Clicking outside every entry does nothing.
        assert_eq!(navigate(&mut m, &a, None), MenuEvent::None);
    }

    #[test]
    fn list_hit_testing() {
        let list = ListLayout {
            cx: 13.5,
            y0: 10.0,
            spacing: 1.6,
            size: 0.9,
        };
        let labels = ["PLAY", "HIGH SCORES", "QUIT"];
        let centre = |i: usize| Vec2::new(13.5, list.item_y(i) + 0.45);
        assert_eq!(list.hit(&labels, centre(0)), Some(0));
        assert_eq!(list.hit(&labels, centre(2)), Some(2));
        assert_eq!(list.hit(&labels, Vec2::new(1.0, list.item_y(1))), None);
        assert_eq!(list.hit(&labels, Vec2::new(13.5, 30.0)), None);
    }
}
