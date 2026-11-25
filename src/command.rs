//! The drawing commands. The editor and the AI use the same ones: every action of the AI is a command, so it can be
//! shown, replayed and undone like anything the user does.
//!
//! Coordinates: (0, 0) is the top-left pixel, x grows to the right and y grows downwards.

use crate::doc::{Color, Document, Frame, MAX_SIZE, TRANSPARENT, parse_color};
use crate::draw;
use serde::{Deserialize, Serialize};

pub const MAX_FRAMES: usize = 64;
pub const MAX_PALETTE: usize = 64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum Command {
    Pixel { x: i32, y: i32, color: String },
    Line { x1: i32, y1: i32, x2: i32, y2: i32, color: String },
    Rect { x: i32, y: i32, w: i32, h: i32, color: String, #[serde(default)] filled: bool },
    Ellipse { x: i32, y: i32, w: i32, h: i32, color: String, #[serde(default)] filled: bool },
    Fill { x: i32, y: i32, color: String },
    Clear { #[serde(default)] color: Option<String> },
    SetPalette { colors: Vec<String> },
    NewCanvas { width: i32, height: i32 },
    NewFrame { #[serde(default)] copy_current: bool },
    SelectFrame { index: usize },
    DeleteFrame { index: usize },
}

impl Command {
    /// Commands that change more than the pixels of the current frame.
    pub fn is_structural(&self) -> bool {
        matches!(self, Command::NewCanvas { .. } | Command::NewFrame { .. } | Command::DeleteFrame { .. } | Command::SetPalette { .. })
    }

    /// Whether the command can change the drawing (choosing a frame only moves the view).
    pub fn edits(&self) -> bool {
        !matches!(self, Command::SelectFrame { .. })
    }
}

fn color(text: &str) -> Result<Color, String> {
    parse_color(text).ok_or_else(|| format!("'{text}' is not a colour: use #rrggbb, #rrggbbaa or transparent"))
}

fn painted(doc: &mut Document, pixels: Vec<(i32, i32)>, text: &str, what: &str) -> Result<String, String> {
    let color = color(text)?;
    let count = draw::paint(doc, &pixels, color);
    if count == 0 {
        return Err(format!("nothing was drawn: every pixel of the {what} is outside the {}x{} canvas (x goes from 0 to {}, y from 0 to {})", doc.width, doc.height, doc.width - 1, doc.height - 1));
    }
    Ok(format!("drew the {what} ({count} pixels)"))
}

/// Runs a command on the document. The message says what happened (or what was wrong), to show to the user or the AI.
pub fn execute(doc: &mut Document, command: &Command) -> Result<String, String> {
    match command {
        Command::Pixel { x, y, color: text } => {
            let color = color(text)?;
            if doc.set(*x, *y, color) {
                Ok(format!("set pixel ({x}, {y})"))
            } else {
                Err(format!("({x}, {y}) is outside the {}x{} canvas", doc.width, doc.height))
            }
        }
        Command::Line { x1, y1, x2, y2, color } => painted(doc, draw::line(*x1, *y1, *x2, *y2), color, "line"),
        Command::Rect { x, y, w, h, color, filled } => {
            if *w <= 0 || *h <= 0 {
                return Err("a rectangle needs a width and a height of at least 1".into());
            }
            painted(doc, draw::rect(*x, *y, *w, *h, *filled), color, "rectangle")
        }
        Command::Ellipse { x, y, w, h, color, filled } => {
            if *w <= 0 || *h <= 0 {
                return Err("an ellipse needs a width and a height of at least 1".into());
            }
            painted(doc, draw::ellipse(*x, *y, *w, *h, *filled), color, "ellipse")
        }
        Command::Fill { x, y, color: text } => {
            let new = color(text)?;
            let Some(old) = doc.get(*x, *y) else {
                return Err(format!("({x}, {y}) is outside the {}x{} canvas", doc.width, doc.height));
            };
            if old == new {
                return Ok("that area already has this colour".into());
            }
            let region = draw::flood_region(doc, *x, *y);
            let count = draw::paint(doc, &region, new);
            Ok(format!("filled an area of {count} pixels"))
        }
        Command::Clear { color: text } => {
            let color = text.as_deref().map(self::color).transpose()?.unwrap_or(TRANSPARENT);
            doc.frame_mut().pixels.fill(color);
            Ok("cleared the current frame".into())
        }
        Command::SetPalette { colors } => {
            if colors.is_empty() || colors.len() > MAX_PALETTE {
                return Err(format!("a palette holds 1 to {MAX_PALETTE} colours"));
            }
            doc.palette = colors.iter().map(|c| color(c)).collect::<Result<_, _>>()?;
            Ok(format!("palette of {} colours set", doc.palette.len()))
        }
        Command::NewCanvas { width, height } => {
            let range = 1..=MAX_SIZE as i32;
            if !range.contains(width) || !range.contains(height) {
                return Err(format!("a canvas is 1 to {MAX_SIZE} pixels wide and high"));
            }
            let palette = std::mem::take(&mut doc.palette);
            *doc = Document::new(*width as usize, *height as usize);
            doc.palette = palette;
            Ok(format!("new empty canvas of {width}x{height}"))
        }
        Command::NewFrame { copy_current } => {
            if doc.frames.len() >= MAX_FRAMES {
                return Err(format!("an animation holds at most {MAX_FRAMES} frames"));
            }
            let frame = if *copy_current { doc.frame().clone() } else { Frame { pixels: vec![TRANSPARENT; doc.width * doc.height] } };
            doc.frames.push(frame);
            doc.current = doc.frames.len() - 1;
            Ok(format!("frame {} added and selected ({} frames)", doc.current, doc.frames.len()))
        }
        Command::SelectFrame { index } => {
            if *index >= doc.frames.len() {
                return Err(format!("there is no frame {index}: frames go from 0 to {}", doc.frames.len() - 1));
            }
            doc.current = *index;
            Ok(format!("frame {index} selected"))
        }
        Command::DeleteFrame { index } => {
            if doc.frames.len() == 1 {
                return Err("a drawing needs at least one frame".into());
            }
            if *index >= doc.frames.len() {
                return Err(format!("there is no frame {index}: frames go from 0 to {}", doc.frames.len() - 1));
            }
            doc.frames.remove(*index);
            doc.current = doc.current.min(doc.frames.len() - 1);
            Ok(format!("frame {index} deleted ({} frames left)", doc.frames.len()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(x: i32, y: i32, color: &str) -> Command {
        Command::Pixel { x, y, color: color.into() }
    }

    #[test]
    fn commands_are_read_from_json_the_way_the_ai_writes_them() {
        let line: Command = serde_json::from_str(r##"{"tool":"line","x1":0,"y1":0,"x2":3,"y2":3,"color":"#ff0000"}"##).unwrap();
        assert!(matches!(line, Command::Line { x2: 3, .. }));
        // `filled` and the optional colour may be left out
        let rect: Command = serde_json::from_str(r##"{"tool":"rect","x":1,"y":1,"w":2,"h":2,"color":"#fff"}"##).unwrap();
        assert!(matches!(rect, Command::Rect { filled: false, .. }));
        let clear: Command = serde_json::from_str(r#"{"tool":"clear"}"#).unwrap();
        assert_eq!(clear, Command::Clear { color: None });
        assert!(serde_json::from_str::<Command>(r#"{"tool":"explode"}"#).is_err());
    }

    #[test]
    fn drawing_reports_what_it_did() {
        let mut doc = Document::new(8, 8);
        let ok = execute(&mut doc, &Command::Rect { x: 1, y: 1, w: 3, h: 2, color: "#00ff00".into(), filled: true }).unwrap();
        assert!(ok.contains("6 pixels"), "{ok}");
        assert_eq!(doc.get(2, 2), Some([0, 255, 0, 255]));
    }

    #[test]
    fn a_drawing_outside_the_canvas_is_an_error_the_ai_can_understand() {
        let mut doc = Document::new(8, 8);
        let error = execute(&mut doc, &pixel(20, 3, "#fff")).unwrap_err();
        assert!(error.contains("8x8"), "{error}");
        let error = execute(&mut doc, &Command::Line { x1: 50, y1: 50, x2: 60, y2: 60, color: "#fff".into() }).unwrap_err();
        assert!(error.contains("x goes from 0 to 7"), "{error}");
        // partly inside is fine
        assert!(execute(&mut doc, &Command::Line { x1: -5, y1: 0, x2: 3, y2: 0, color: "#fff".into() }).is_ok());
    }

    #[test]
    fn a_bad_colour_is_an_error_not_a_crash() {
        let mut doc = Document::new(4, 4);
        let error = execute(&mut doc, &pixel(0, 0, "purple")).unwrap_err();
        assert!(error.contains("#rrggbb"), "{error}");
        assert_eq!(doc.get(0, 0), Some(TRANSPARENT));
    }

    #[test]
    fn fill_paints_a_whole_area() {
        let mut doc = Document::new(4, 4);
        let ok = execute(&mut doc, &Command::Fill { x: 0, y: 0, color: "#123456".into() }).unwrap();
        assert!(ok.contains("16 pixels"), "{ok}");
        assert!(execute(&mut doc, &Command::Fill { x: 0, y: 0, color: "#123456".into() }).unwrap().contains("already"));
        assert!(execute(&mut doc, &Command::Fill { x: 9, y: 0, color: "#fff".into() }).is_err());
    }

    #[test]
    fn frames_can_be_added_selected_and_deleted() {
        let mut doc = Document::new(4, 4);
        execute(&mut doc, &pixel(0, 0, "#ff0000")).unwrap();
        execute(&mut doc, &Command::NewFrame { copy_current: true }).unwrap();
        assert_eq!((doc.frames.len(), doc.current), (2, 1));
        assert_eq!(doc.get(0, 0), Some([255, 0, 0, 255]), "the copy keeps the drawing");
        execute(&mut doc, &Command::NewFrame { copy_current: false }).unwrap();
        assert_eq!(doc.get(0, 0), Some(TRANSPARENT), "a new frame is empty");
        execute(&mut doc, &Command::SelectFrame { index: 0 }).unwrap();
        assert_eq!(doc.get(0, 0), Some([255, 0, 0, 255]));
        assert!(execute(&mut doc, &Command::SelectFrame { index: 7 }).is_err());
        execute(&mut doc, &Command::DeleteFrame { index: 0 }).unwrap();
        assert_eq!(doc.frames.len(), 2);
        execute(&mut doc, &Command::DeleteFrame { index: 0 }).unwrap();
        assert!(execute(&mut doc, &Command::DeleteFrame { index: 0 }).is_err(), "the last frame stays");
    }

    #[test]
    fn a_new_canvas_keeps_the_palette_and_refuses_silly_sizes() {
        let mut doc = Document::new(4, 4);
        execute(&mut doc, &Command::SetPalette { colors: vec!["#000".into(), "#fff".into()] }).unwrap();
        execute(&mut doc, &Command::NewCanvas { width: 16, height: 8 }).unwrap();
        assert_eq!((doc.width, doc.height, doc.palette.len()), (16, 8, 2));
        assert!(execute(&mut doc, &Command::NewCanvas { width: 0, height: 8 }).is_err());
        assert!(execute(&mut doc, &Command::NewCanvas { width: 100_000, height: 8 }).is_err());
        assert!(execute(&mut doc, &Command::SetPalette { colors: vec![] }).is_err());
        assert!(execute(&mut doc, &Command::SetPalette { colors: vec!["nope".into()] }).is_err());
    }

    #[test]
    fn an_animation_has_a_limit() {
        let mut doc = Document::new(2, 2);
        for _ in 1..MAX_FRAMES {
            execute(&mut doc, &Command::NewFrame { copy_current: false }).unwrap();
        }
        assert!(execute(&mut doc, &Command::NewFrame { copy_current: false }).is_err());
    }
}
