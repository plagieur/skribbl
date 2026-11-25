//! Turns a picture into pixel art, on this computer: shrink it, choose the colours, optionally dither.

use crate::doc::{Color, MAX_SIZE, TRANSPARENT};
use image::{RgbaImage, imageops};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Colors {
    /// Keep the colours of the picture (after shrinking).
    Original,
    /// Use the colours of the palette that is already in the editor.
    Palette,
    /// Pick the best N colours for this picture (they become the palette).
    Reduce,
}

#[derive(Clone)]
pub struct Settings {
    /// The size of the longest side, in pixels.
    pub max_side: u32,
    pub colors: Colors,
    /// How many colours `Colors::Reduce` picks.
    pub count: usize,
    /// Spread the colour error to the neighbours (Floyd-Steinberg): smoother gradients, noisier look.
    pub dither: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { max_side: 32, colors: Colors::Reduce, count: 12, dither: false }
    }
}

pub struct Converted {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Color>,
    /// The new palette, when the conversion chose one.
    pub palette: Option<Vec<Color>>,
}

/// The size the picture gets: its longest side is `max_side`, the proportions are kept.
pub fn target_size(width: u32, height: u32, max_side: u32) -> (usize, usize) {
    let max_side = max_side.clamp(1, MAX_SIZE as u32) as u64;
    let (w, h) = (width.max(1) as u64, height.max(1) as u64);
    let (tw, th) = if w >= h { (max_side, (h * max_side / w).max(1)) } else { ((w * max_side / h).max(1), max_side) };
    (tw as usize, th as usize)
}

/// How far two colours look from each other (the eye is most sensitive to green).
fn distance(a: [f32; 3], b: [u8; 3]) -> f32 {
    let (dr, dg, db) = (a[0] - b[0] as f32, a[1] - b[1] as f32, a[2] - b[2] as f32);
    2.0 * dr * dr + 4.0 * dg * dg + 3.0 * db * db
}

fn nearest(palette: &[Color], rgb: [f32; 3]) -> Color {
    *palette.iter().min_by(|a, b| distance(rgb, [a[0], a[1], a[2]]).total_cmp(&distance(rgb, [b[0], b[1], b[2]]))).expect("a palette is never empty")
}

/// Median cut: split the colours into boxes along their widest channel until there are `count`, then average each box.
pub fn median_cut(colors: &[[u8; 3]], count: usize) -> Vec<Color> {
    if colors.is_empty() {
        return Vec::new();
    }
    let range = |b: &[[u8; 3]]| -> (usize, u8) {
        (0..3).map(|c| (c, b.iter().map(|p| p[c]).max().unwrap_or(0) - b.iter().map(|p| p[c]).min().unwrap_or(0))).max_by_key(|(_, r)| *r).unwrap_or((0, 0))
    };
    let mut boxes = vec![colors.to_vec()];
    while boxes.len() < count.max(1) {
        // the box that is the most spread out and can still be split
        let Some((index, (channel, spread))) = boxes.iter().enumerate().filter(|(_, b)| b.len() > 1).map(|(i, b)| (i, range(b))).max_by_key(|(_, (_, r))| *r) else { break };
        if spread == 0 {
            break;
        }
        let mut chosen = boxes.swap_remove(index);
        chosen.sort_by_key(|p| p[channel]);
        let upper = chosen.split_off(chosen.len() / 2);
        boxes.push(chosen);
        boxes.push(upper);
    }
    boxes
        .iter()
        .map(|b| {
            let n = b.len() as u32;
            let mean = |c: usize| (b.iter().map(|p| p[c] as u32).sum::<u32>() / n) as u8;
            [mean(0), mean(1), mean(2), 255]
        })
        .collect()
}

pub fn convert(image: &RgbaImage, editor_palette: &[Color], settings: &Settings) -> Converted {
    let (w, h) = target_size(image.width(), image.height(), settings.max_side);
    let small = imageops::resize(image, w as u32, h as u32, imageops::FilterType::Lanczos3);
    // half-transparent pixels are either there or not: pixel art has no soft edges
    let mut pixels: Vec<Color> = small.pixels().map(|p| if p[3] < 128 { TRANSPARENT } else { [p[0], p[1], p[2], 255] }).collect();

    let palette: Option<Vec<Color>> = match settings.colors {
        Colors::Original => None,
        Colors::Palette => Some(editor_palette.to_vec()),
        Colors::Reduce => {
            let opaque: Vec<[u8; 3]> = pixels.iter().filter(|p| p[3] > 0).map(|p| [p[0], p[1], p[2]]).collect();
            Some(median_cut(&opaque, settings.count.clamp(2, 64)))
        }
    };
    let Some(colors) = palette.as_ref().filter(|p| !p.is_empty()) else {
        return Converted { width: w, height: h, pixels, palette: None };
    };

    if settings.dither {
        let mut buffer: Vec<[f32; 3]> = pixels.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if pixels[i][3] == 0 {
                    continue;
                }
                let old = buffer[i].map(|v| v.clamp(0.0, 255.0));
                let new = nearest(colors, old);
                pixels[i] = new;
                let error = [old[0] - new[0] as f32, old[1] - new[1] as f32, old[2] - new[2] as f32];
                for (dx, dy, share) in [(1i32, 0i32, 7.0 / 16.0), (-1, 1, 3.0 / 16.0), (0, 1, 5.0 / 16.0), (1, 1, 1.0 / 16.0)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if pixels[j][3] > 0 {
                        for c in 0..3 {
                            buffer[j][c] += error[c] * share;
                        }
                    }
                }
            }
        }
    } else {
        for p in pixels.iter_mut().filter(|p| p[3] > 0) {
            *p = nearest(colors, [p[0] as f32, p[1] as f32, p[2] as f32]);
        }
    }
    let palette = if settings.colors == Colors::Reduce { palette } else { None };
    Converted { width: w, height: h, pixels, palette }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn gradient(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| Rgba([(x * 255 / w.max(1)) as u8, (y * 255 / h.max(1)) as u8, 128, 255]))
    }

    #[test]
    fn the_size_keeps_the_proportions() {
        assert_eq!(target_size(200, 100, 32), (32, 16));
        assert_eq!(target_size(100, 200, 32), (16, 32));
        assert_eq!(target_size(10, 1000, 32), (1, 32), "never smaller than one pixel");
        assert_eq!(target_size(50, 50, 9999), (MAX_SIZE, MAX_SIZE));
    }

    #[test]
    fn reducing_gives_at_most_the_asked_number_of_colours() {
        let settings = Settings { max_side: 24, colors: Colors::Reduce, count: 6, dither: false };
        let result = convert(&gradient(100, 100), &[], &settings);
        let palette = result.palette.expect("the reduced palette is returned");
        assert!(palette.len() <= 6 && palette.len() >= 2, "{}", palette.len());
        assert!(result.pixels.iter().all(|p| palette.contains(p)), "every pixel uses a colour of the palette");
        assert_eq!((result.width, result.height), (24, 24));
    }

    #[test]
    fn the_editor_palette_is_respected_with_and_without_dithering() {
        let palette = vec![[0, 0, 0, 255], [255, 255, 255, 255]];
        for dither in [false, true] {
            let settings = Settings { max_side: 16, colors: Colors::Palette, count: 4, dither };
            let result = convert(&gradient(64, 64), &palette, &settings);
            assert!(result.palette.is_none(), "the editor palette is not changed");
            assert!(result.pixels.iter().all(|p| palette.contains(p)), "dither={dither}");
        }
    }

    #[test]
    fn dithering_mixes_colours_where_plain_matching_does_not() {
        // a flat mid-grey: plain matching gives one colour everywhere, dithering a mix of both
        let grey = RgbaImage::from_pixel(16, 16, Rgba([128, 128, 128, 255]));
        let palette = vec![[0, 0, 0, 255], [255, 255, 255, 255]];
        let kinds = |dither| {
            let s = Settings { max_side: 16, colors: Colors::Palette, count: 2, dither };
            let mut seen = convert(&grey, &palette, &s).pixels;
            seen.sort();
            seen.dedup();
            seen.len()
        };
        assert_eq!(kinds(false), 1);
        assert_eq!(kinds(true), 2);
    }

    #[test]
    fn transparent_parts_stay_transparent() {
        let image = RgbaImage::from_fn(8, 8, |x, _| if x < 4 { Rgba([255, 0, 0, 255]) } else { Rgba([0, 0, 0, 0]) });
        let result = convert(&image, &[], &Settings { max_side: 8, colors: Colors::Original, count: 4, dither: false });
        assert_eq!(result.pixels[0], [255, 0, 0, 255]);
        assert_eq!(result.pixels[7], TRANSPARENT);
        assert!(result.palette.is_none());
    }

    #[test]
    fn median_cut_handles_flat_and_empty_input() {
        assert!(median_cut(&[], 8).is_empty());
        assert_eq!(median_cut(&[[10, 20, 30]; 50], 8), vec![[10, 20, 30, 255]], "one colour cannot be split");
    }
}
