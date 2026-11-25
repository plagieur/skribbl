//! The document: a canvas of RGBA pixels, one or more frames (an animation), and a palette.

pub type Color = [u8; 4];

pub const TRANSPARENT: Color = [0, 0, 0, 0];

/// The biggest canvas side. Pixel art is small, and every undo step keeps a copy of a frame.
pub const MAX_SIZE: usize = 256;

#[derive(Clone, PartialEq, Debug)]
pub struct Frame {
    pub pixels: Vec<Color>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Document {
    pub width: usize,
    pub height: usize,
    pub frames: Vec<Frame>,
    pub current: usize,
    pub palette: Vec<Color>,
}

/// The 16 colours of the PICO-8 console: a good starting palette for pixel art.
pub fn default_palette() -> Vec<Color> {
    [
        "000000", "1d2b53", "7e2553", "008751", "ab5236", "5f574f", "c2c3c7", "fff1e8", "ff004d", "ffa300", "ffec27", "00e436", "29adff", "83769c", "ff77a8", "ffccaa",
    ]
    .iter()
    .filter_map(|hex| parse_color(&format!("#{hex}")))
    .collect()
}

impl Document {
    pub fn new(width: usize, height: usize) -> Self {
        let (width, height) = (width.clamp(1, MAX_SIZE), height.clamp(1, MAX_SIZE));
        Self { width, height, frames: vec![Frame { pixels: vec![TRANSPARENT; width * height] }], current: 0, palette: default_palette() }
    }

    pub fn frame(&self) -> &Frame {
        &self.frames[self.current]
    }

    pub fn frame_mut(&mut self) -> &mut Frame {
        &mut self.frames[self.current]
    }

    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Color> {
        self.in_bounds(x, y).then(|| self.frame().pixels[y as usize * self.width + x as usize])
    }

    /// Paints one pixel of the current frame. Pixels outside the canvas are ignored; returns whether it was inside.
    pub fn set(&mut self, x: i32, y: i32, color: Color) -> bool {
        if !self.in_bounds(x, y) {
            return false;
        }
        let width = self.width;
        self.frame_mut().pixels[y as usize * width + x as usize] = color;
        true
    }
}

/// Reads `#rgb`, `#rrggbb`, `#rrggbbaa` (the `#` is optional) or `transparent`.
pub fn parse_color(text: &str) -> Option<Color> {
    let text = text.trim().to_ascii_lowercase();
    if text == "transparent" || text == "none" {
        return Some(TRANSPARENT);
    }
    let digits = text.trim_start_matches('#');
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let pair = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    match digits.len() {
        3 => {
            let nibble = |i: usize| u8::from_str_radix(&digits[i..i + 1], 16).ok().map(|n| n * 17);
            Some([nibble(0)?, nibble(1)?, nibble(2)?, 255])
        }
        6 => Some([pair(0)?, pair(2)?, pair(4)?, 255]),
        8 => Some([pair(0)?, pair(2)?, pair(4)?, pair(6)?]),
        _ => None,
    }
}

pub fn to_hex(color: Color) -> String {
    if color[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", color[0], color[1], color[2], color[3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_read_in_every_usual_form() {
        assert_eq!(parse_color("#ff0000"), Some([255, 0, 0, 255]));
        assert_eq!(parse_color("FF0000"), Some([255, 0, 0, 255]));
        assert_eq!(parse_color("#f80"), Some([255, 136, 0, 255]));
        assert_eq!(parse_color("#11223380"), Some([17, 34, 51, 128]));
        assert_eq!(parse_color("transparent"), Some(TRANSPARENT));
        assert_eq!(parse_color("  #00ff00 "), Some([0, 255, 0, 255]));
    }

    #[test]
    fn bad_colours_are_refused() {
        for bad in ["", "#", "#12", "#12345", "#gg0000", "red", "#1234567"] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn colours_round_trip_through_hex() {
        for color in [[1, 2, 3, 255], [200, 100, 50, 7], TRANSPARENT] {
            assert_eq!(parse_color(&to_hex(color)), Some(color));
        }
    }

    #[test]
    fn a_new_document_is_clamped_and_transparent() {
        let doc = Document::new(0, 9999);
        assert_eq!((doc.width, doc.height), (1, MAX_SIZE));
        assert!(doc.frame().pixels.iter().all(|p| *p == TRANSPARENT));
        assert_eq!(doc.palette.len(), 16);
    }

    #[test]
    fn pixels_outside_the_canvas_are_ignored() {
        let mut doc = Document::new(4, 4);
        assert!(doc.set(3, 3, [1, 1, 1, 255]));
        assert!(!doc.set(4, 0, [1, 1, 1, 255]));
        assert!(!doc.set(-1, 0, [1, 1, 1, 255]));
        assert_eq!(doc.get(3, 3), Some([1, 1, 1, 255]));
        assert_eq!(doc.get(4, 4), None);
    }
}
