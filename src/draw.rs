//! Drawing algorithms. They only compute which pixels a shape covers; painting them is up to the caller.

use crate::doc::{Color, Document};

/// A straight line from one pixel to another, both ends included (Bresenham).
pub fn line(x1: i32, y1: i32, x2: i32, y2: i32) -> Vec<(i32, i32)> {
    let (dx, dy) = ((x2 - x1).abs(), -(y2 - y1).abs());
    let (sx, sy) = (if x1 < x2 { 1 } else { -1 }, if y1 < y2 { 1 } else { -1 });
    let (mut x, mut y, mut error) = (x1, y1, dx + dy);
    let mut out = Vec::new();
    loop {
        out.push((x, y));
        if x == x2 && y == y2 {
            return out;
        }
        let doubled = 2 * error;
        if doubled >= dy {
            error += dy;
            x += sx;
        }
        if doubled <= dx {
            error += dx;
            y += sy;
        }
    }
}

/// A rectangle with its top-left corner at (x, y), `w` wide and `h` high.
pub fn rect(x: i32, y: i32, w: i32, h: i32, filled: bool) -> Vec<(i32, i32)> {
    if w <= 0 || h <= 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for py in y..y + h {
        for px in x..x + w {
            if filled || px == x || px == x + w - 1 || py == y || py == y + h - 1 {
                out.push((px, py));
            }
        }
    }
    out
}

/// An ellipse that fits in the box with top-left corner (x, y), `w` wide and `h` high.
pub fn ellipse(x: i32, y: i32, w: i32, h: i32, filled: bool) -> Vec<(i32, i32)> {
    if w <= 0 || h <= 0 {
        return Vec::new();
    }
    let (cx, cy) = (x as f32 + w as f32 / 2.0, y as f32 + h as f32 / 2.0);
    let (rx, ry) = (w as f32 / 2.0, h as f32 / 2.0);
    let inside = |px: i32, py: i32, rx: f32, ry: f32| {
        if rx <= 0.0 || ry <= 0.0 {
            return false;
        }
        let (dx, dy) = ((px as f32 + 0.5 - cx) / rx, (py as f32 + 0.5 - cy) / ry);
        dx * dx + dy * dy <= 1.0
    };
    let mut out = Vec::new();
    for py in y..y + h {
        for px in x..x + w {
            if inside(px, py, rx, ry) && (filled || !inside(px, py, rx - 1.0, ry - 1.0)) {
                out.push((px, py));
            }
        }
    }
    out
}

/// Every pixel connected (up, down, left, right) to (x, y) that has the same colour as it.
pub fn flood_region(doc: &Document, x: i32, y: i32) -> Vec<(i32, i32)> {
    let Some(target) = doc.get(x, y) else { return Vec::new() };
    let mut seen = vec![false; doc.width * doc.height];
    let mut stack = vec![(x, y)];
    let mut out = Vec::new();
    while let Some((px, py)) = stack.pop() {
        if !doc.in_bounds(px, py) {
            continue;
        }
        let index = py as usize * doc.width + px as usize;
        if seen[index] || doc.frame().pixels[index] != target {
            continue;
        }
        seen[index] = true;
        out.push((px, py));
        stack.extend([(px + 1, py), (px - 1, py), (px, py + 1), (px, py - 1)]);
    }
    out
}

/// Paints `pixels` with `color`; returns how many were inside the canvas.
pub fn paint(doc: &mut Document, pixels: &[(i32, i32)], color: Color) -> usize {
    pixels.iter().filter(|(x, y)| doc.set(*x, *y, color)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_includes_both_ends_and_is_continuous() {
        let pixels = line(0, 0, 5, 2);
        assert_eq!(pixels.first(), Some(&(0, 0)));
        assert_eq!(pixels.last(), Some(&(5, 2)));
        for pair in pixels.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!((a.0 - b.0).abs() <= 1 && (a.1 - b.1).abs() <= 1, "gap between {a:?} and {b:?}");
        }
        assert_eq!(pixels.len(), 6);
    }

    #[test]
    fn lines_work_in_every_direction() {
        assert_eq!(line(3, 3, 3, 3), [(3, 3)]);
        assert_eq!(line(0, 0, 0, 3), [(0, 0), (0, 1), (0, 2), (0, 3)]);
        assert_eq!(line(3, 0, 0, 0), [(3, 0), (2, 0), (1, 0), (0, 0)]);
        assert_eq!(line(0, 0, 3, 3), [(0, 0), (1, 1), (2, 2), (3, 3)]);
        assert_eq!(line(3, 3, 0, 0).len(), 4);
    }

    #[test]
    fn rectangles_can_be_outlined_or_filled() {
        assert_eq!(rect(1, 1, 3, 3, true).len(), 9);
        let outline = rect(1, 1, 3, 3, false);
        assert_eq!(outline.len(), 8);
        assert!(!outline.contains(&(2, 2)));
        assert_eq!(rect(0, 0, 1, 1, false), [(0, 0)]);
        assert!(rect(0, 0, 0, 5, true).is_empty());
        assert!(rect(0, 0, -2, 5, true).is_empty());
    }

    #[test]
    fn a_circle_is_round_and_symmetric() {
        let disc = ellipse(0, 0, 8, 8, true);
        assert!(disc.contains(&(3, 3)) && disc.contains(&(4, 4)));
        assert!(!disc.contains(&(0, 0)), "the corner of the box is outside the circle");
        for &(x, y) in &disc {
            assert!(disc.contains(&(7 - x, y)) && disc.contains(&(x, 7 - y)), "not symmetric at {x},{y}");
        }
        let ring = ellipse(0, 0, 8, 8, false);
        assert!(!ring.contains(&(3, 3)), "the middle of a ring is empty");
        assert!(ring.len() < disc.len());
    }

    #[test]
    fn a_one_pixel_ellipse_is_one_pixel() {
        assert_eq!(ellipse(2, 2, 1, 1, true), [(2, 2)]);
        assert!(ellipse(0, 0, 0, 4, true).is_empty());
    }

    #[test]
    fn the_fill_stops_at_a_different_colour() {
        let mut doc = Document::new(5, 5);
        // a wall of black down the middle
        for y in 0..5 {
            doc.set(2, y, [0, 0, 0, 255]);
        }
        let left = flood_region(&doc, 0, 0);
        assert_eq!(left.len(), 10);
        assert!(left.iter().all(|(x, _)| *x < 2));
        // filling the wall itself only reaches the wall
        assert_eq!(flood_region(&doc, 2, 2).len(), 5);
        assert!(flood_region(&doc, 9, 9).is_empty());
    }

    #[test]
    fn painting_counts_only_the_pixels_inside() {
        let mut doc = Document::new(4, 4);
        let drawn = paint(&mut doc, &[(0, 0), (3, 3), (4, 4), (-1, 2)], [9, 9, 9, 255]);
        assert_eq!(drawn, 2);
    }
}
