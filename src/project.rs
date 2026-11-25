//! Saving and opening drawings: a `.skb` file is JSON that holds the size, the palette, the speed and every frame as a PNG.

use crate::command::{MAX_FRAMES, MAX_PALETTE};
use crate::doc::{Document, Frame, MAX_SIZE, default_palette, parse_color, to_hex};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::Path;

const FORMAT: &str = "skribbl";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    version: u32,
    width: usize,
    height: usize,
    /// Playback speed, in frames per second.
    fps: u32,
    current: usize,
    palette: Vec<String>,
    /// One base64 PNG per frame.
    frames: Vec<String>,
}

pub fn to_text(doc: &Document, fps: u32) -> Result<String, String> {
    let frames = doc
        .frames
        .iter()
        .map(|frame| {
            let bytes: Vec<u8> = frame.pixels.iter().flatten().copied().collect();
            let image = image::RgbaImage::from_raw(doc.width as u32, doc.height as u32, bytes).ok_or("a frame does not have the size of the canvas")?;
            let mut png = Vec::new();
            image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
            Ok(base64::engine::general_purpose::STANDARD.encode(png))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let file = File { format: FORMAT.into(), version: VERSION, width: doc.width, height: doc.height, fps, current: doc.current, palette: doc.palette.iter().map(|c| to_hex(*c)).collect(), frames };
    serde_json::to_string(&file).map_err(|e| e.to_string())
}

pub fn from_text(text: &str) -> Result<(Document, u32), String> {
    let file: File = serde_json::from_str(text).map_err(|_| "this is not a Skribbl drawing".to_string())?;
    if file.format != FORMAT {
        return Err("this is not a Skribbl drawing".into());
    }
    if file.version > VERSION {
        return Err("this drawing was saved by a newer version of Skribbl".into());
    }
    if !(1..=MAX_SIZE).contains(&file.width) || !(1..=MAX_SIZE).contains(&file.height) {
        return Err(format!("the size of the drawing is not valid (1 to {MAX_SIZE} pixels)"));
    }
    if file.frames.is_empty() || file.frames.len() > MAX_FRAMES {
        return Err(format!("a drawing holds 1 to {MAX_FRAMES} frames"));
    }
    let frames = file
        .frames
        .iter()
        .enumerate()
        .map(|(i, data)| {
            let png = base64::engine::general_purpose::STANDARD.decode(data).map_err(|_| format!("frame {i} is damaged"))?;
            let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(|_| format!("frame {i} is damaged"))?.to_rgba8();
            if (image.width() as usize, image.height() as usize) != (file.width, file.height) {
                return Err(format!("frame {i} does not have the size of the canvas"));
            }
            Ok(Frame { pixels: image.pixels().map(|p| p.0).collect() })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let palette: Vec<_> = file.palette.iter().filter_map(|c| parse_color(c)).take(MAX_PALETTE).collect();
    let current = file.current.min(frames.len() - 1);
    let doc = Document { width: file.width, height: file.height, frames, current, palette: if palette.is_empty() { default_palette() } else { palette } };
    Ok((doc, file.fps.clamp(1, 30)))
}

/// Writes next to the file first, then moves it into place: a crash cannot leave a half-written drawing.
pub fn save(path: &Path, doc: &Document, fps: u32) -> Result<(), String> {
    let text = to_text(doc, fps)?;
    let temporary = path.with_extension("skb.tmp");
    std::fs::write(&temporary, text).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        e.to_string()
    })
}

pub fn load(path: &Path) -> Result<(Document, u32), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    from_text(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Document {
        let mut doc = Document::new(5, 3);
        doc.set(1, 2, [10, 20, 30, 255]);
        doc.set(4, 0, [200, 100, 50, 77]);
        doc.frames.push(Frame { pixels: vec![[1, 2, 3, 255]; 15] });
        doc.current = 1;
        doc.palette = vec![[0, 0, 0, 255], [255, 255, 255, 255], [9, 8, 7, 128]];
        doc
    }

    #[test]
    fn a_drawing_comes_back_exactly_as_it_was_saved() {
        let doc = sample();
        let (back, fps) = from_text(&to_text(&doc, 12).unwrap()).unwrap();
        assert_eq!(back, doc);
        assert_eq!(fps, 12);
    }

    #[test]
    fn it_goes_through_a_real_file() {
        let path = std::env::temp_dir().join("skribbl-test-project.skb");
        save(&path, &sample(), 8).unwrap();
        save(&path, &sample(), 9).unwrap(); // saving over an existing file works
        let (back, fps) = load(&path).unwrap();
        assert_eq!((back, fps), (sample(), 9));
        assert!(!path.with_extension("skb.tmp").exists());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn files_that_are_not_drawings_are_refused_with_a_clear_message() {
        assert!(from_text("hello").unwrap_err().contains("not a Skribbl"));
        assert!(from_text("{}").is_err());
        let doc = sample();
        let text = to_text(&doc, 8).unwrap();
        assert!(from_text(&text.replace("\"skribbl\"", "\"other\"")).unwrap_err().contains("not a Skribbl"));
        assert!(from_text(&text.replace("\"width\":5", "\"width\":9999")).unwrap_err().contains("size"));
        assert!(from_text(&text.replace("\"width\":5", "\"width\":6")).unwrap_err().contains("size of the canvas"));
        assert!(from_text(&text.replace("\"version\":1", "\"version\":99")).unwrap_err().contains("newer"));
    }

    #[test]
    fn a_damaged_frame_is_reported_not_a_crash() {
        let text = to_text(&sample(), 8).unwrap();
        let broken = text.replacen("iVBOR", "xxxxx", 1);
        assert!(from_text(&broken).unwrap_err().contains("damaged"));
    }

    #[test]
    fn odd_values_are_brought_back_in_range() {
        let text = to_text(&sample(), 8).unwrap().replace("\"fps\":8", "\"fps\":900").replace("\"current\":1", "\"current\":50");
        let (doc, fps) = from_text(&text).unwrap();
        assert_eq!((fps, doc.current), (30, 1));
    }
}
