mod ai;
mod ai_panel;
mod command;
mod doc;
mod draw;
mod editor;
mod import_window;
mod pixelize;
mod project;

use command::Command;
use doc::{Color, MAX_SIZE, TRANSPARENT, to_hex};
use editor::Editor;
use serde::{Deserialize, Serialize};
use eframe::egui::{self, Color32, Key, Margin, Modifiers, PointerButton, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, TextureOptions, Vec2, vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Pencil,
    Eraser,
    Line,
    Rect,
    RectFilled,
    Ellipse,
    EllipseFilled,
    Fill,
    Picker,
}

impl Tool {
    const ALL: [(Tool, &'static str, &'static str); 9] = [
        (Tool::Pencil, "Pencil", "B"),
        (Tool::Eraser, "Eraser", "E"),
        (Tool::Line, "Line", "L"),
        (Tool::Rect, "Rectangle", "R"),
        (Tool::RectFilled, "Filled rectangle", "Shift+R"),
        (Tool::Ellipse, "Ellipse", "O"),
        (Tool::EllipseFilled, "Filled ellipse", "Shift+O"),
        (Tool::Fill, "Fill", "G"),
        (Tool::Picker, "Colour picker", "I"),
    ];
}

/// A mouse drag in progress on the canvas.
struct Drag {
    start: (i32, i32),
    last: (i32, i32),
}

/// What replaces or closes the current drawing.
#[derive(Clone, Copy)]
enum Leave {
    Open,
    Create,
    Quit,
}

#[derive(Clone, Copy)]
enum Export {
    Png,
    Sheet,
    Gif,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Menu,
    Size,
    Options,
    Editor,
}

/// The user's settings, kept in %APPDATA%\skribbl\options.json.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Options {
    grid: bool,
    ui_scale: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self { grid: true, ui_scale: 1.0 }
    }
}

impl Options {
    fn path() -> Option<std::path::PathBuf> {
        std::env::var_os("APPDATA").map(|dir| std::path::PathBuf::from(dir).join("skribbl").join("options.json"))
    }

    fn load() -> Self {
        let mut options: Self = Self::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        options.ui_scale = options.ui_scale.clamp(0.8, 1.6);
        options
    }

    fn save(&self) {
        if let Some(path) = Self::path() {
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
            if let Ok(text) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(path, text);
            }
        }
    }
}

const MAX_BRUSH: i32 = 32;
const TITLE_WIDTH: f32 = 41.0;
const INK: Color32 = Color32::from_rgb(30, 31, 36);

/// The letters of the menu, in a 5x7 pixel font.
fn glyph(letter: char) -> [&'static str; 7] {
    match letter {
        'A' => [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
        'B' => ["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."],
        'C' => [".###.", "#...#", "#....", "#....", "#....", "#...#", ".###."],
        'E' => ["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
        'I' => ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "#####"],
        'K' => ["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"],
        'L' => ["#....", "#....", "#....", "#....", "#....", "#....", "#####"],
        'N' => ["#...#", "##..#", "##..#", "#.#.#", "#..##", "#..##", "#...#"],
        'O' => [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
        'P' => ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."],
        'Q' => [".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"],
        'R' => ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"],
        'S' => [".####", "#....", "#....", ".###.", "....#", "....#", "####."],
        'T' => ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."],
        'U' => ["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
        _ => [".....", ".....", ".....", ".....", ".....", ".....", "....."],
    }
}

fn hash(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}

/// A number from 0 to 1 out of a hash.
fn unit(h: u32) -> f32 {
    (h % 10_000) as f32 / 10_000.0
}

/// A faint pixel grid where palette-coloured pixels light up and fade out here and there.
fn menu_background(painter: &egui::Painter, area: Rect, t: f32) {
    let g = 24.0;
    let line = Stroke::new(1.0, Color32::from_gray(238));
    let mut x = area.left();
    while x <= area.right() {
        painter.vline(x, area.y_range(), line);
        x += g;
    }
    let mut y = area.top();
    while y <= area.bottom() {
        painter.hline(area.x_range(), y, line);
        y += g;
    }
    let (cols, rows) = ((area.width() / g) as u32 + 1, (area.height() / g) as u32 + 1);
    let palette = doc::default_palette();
    for k in 0..36u32 {
        let period = 3.0 + (k % 5) as f32 * 0.7;
        let u = t / period + unit(hash(k, 1));
        let epoch = u.floor();
        let h = hash(k, epoch as u32 ^ 0x55);
        let fade = ((u - epoch) * std::f32::consts::PI).sin();
        let c = palette[(h >> 16) as usize % palette.len()];
        let at = area.min + vec2((h % cols) as f32 * g, ((h >> 8) % rows) as f32 * g);
        painter.rect_filled(Rect::from_min_size(at, Vec2::splat(g)), 0.0, Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (fade * 200.0) as u8));
    }
}

/// A menu button: square, bordered, with its text in pixels; it inverts when the mouse is over it.
fn pixel_button(ui: &mut egui::Ui, text: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(240.0, 54.0), Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let pressed = response.is_pointer_button_down_on();
    let rect = if pressed { rect.translate(vec2(2.0, 2.0)) } else { rect };
    let painter = ui.painter();
    painter.rect_filled(rect.translate(vec2(4.0, 4.0)), 0.0, Color32::from_rgb(41, 173, 255).gamma_multiply(if hovered { 1.0 } else { 0.35 }));
    painter.rect_filled(rect, 0.0, if hovered { INK } else { Color32::WHITE });
    painter.rect_stroke(rect, 0.0, Stroke::new(3.0, INK), egui::StrokeKind::Inside);
    let cell = 3.0;
    let width = (text.chars().count() as f32 * 6.0 - 1.0) * cell;
    let origin = rect.center() - vec2(width, 7.0 * cell) / 2.0;
    let color = if hovered { Color32::WHITE } else { INK };
    for (i, letter) in text.chars().enumerate() {
        for (row, line) in glyph(letter).iter().enumerate() {
            for (col, ch) in line.chars().enumerate() {
                if ch == '#' {
                    let p = origin + vec2((i * 6 + col) as f32 * cell, row as f32 * cell);
                    painter.rect_filled(Rect::from_min_size(p, Vec2::splat(cell)), 0.0, color);
                }
            }
        }
    }
    response.clicked()
}

struct App {
    pencil: i32,
    eraser: i32,
    brush_open: Option<Tool>,
    ai: ai_panel::AiPanel,
    import: Option<import_window::ImportWindow>,
    /// Where the drawing is saved (none: never saved).
    path: Option<std::path::PathBuf>,
    saved_version: u64,
    /// Something the user asked for that may need the unsaved work to be dealt with first.
    request: Option<Leave>,
    confirm: Option<Leave>,
    quit_ok: bool,
    last_title: String,
    playing: bool,
    fps: u32,
    next_tick: f64,
    onion: bool,
    export_scale: u32,
    pending_export: Option<Export>,
    screen: Screen,
    menu_t0: Option<f64>,
    back: Screen,
    dark: bool,
    options: Options,
    editor: Editor,
    tool: Tool,
    color: Color,
    zoom: f32,
    pan: Vec2,
    needs_fit: bool,
    grid: bool,
    drag: Option<Drag>,
    hover: Option<(i32, i32)>,
    texture: Option<TextureHandle>,
    new_size: (usize, usize),
    note: Option<String>,
}

fn color32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(30, 31, 36);
    v.window_fill = Color32::from_rgb(38, 40, 46);
    v.extreme_bg_color = Color32::from_rgb(24, 25, 29);
    ctx.set_visuals(v);
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        let options = Options::load();
        cc.egui_ctx.set_zoom_factor(options.ui_scale);
        Self {
            pencil: 1,
            eraser: 1,
            brush_open: None,
            ai: ai_panel::AiPanel::new(),
            import: None,
            path: None,
            saved_version: 0,
            request: None,
            confirm: None,
            quit_ok: false,
            last_title: String::new(),
            playing: false,
            fps: 8,
            next_tick: 0.0,
            onion: true,
            export_scale: 1,
            pending_export: None,
            screen: Screen::Menu,
            menu_t0: None,
            back: Screen::Menu,
            dark: false,
            grid: options.grid,
            options,
            editor: Editor::new(32, 32),
            tool: Tool::Pencil,
            color: [255, 0, 77, 255],
            zoom: 16.0,
            pan: Vec2::ZERO,
            needs_fit: true,
            drag: None,
            hover: None,
            texture: None,
            new_size: (32, 32),
            note: None,
        }
    }

    /// The current frame as an image, over a checkerboard that shows what is transparent.
    fn render(&self) -> egui::ColorImage {
        let doc = &self.editor.doc;
        let mut bytes = Vec::with_capacity(doc.width * doc.height * 4);
        // onion skin: the previous frame in red and the next one in green, faintly, behind the current one
        let onion = self.onion && !self.playing;
        let prev = if onion && doc.current > 0 { Some(&doc.frames[doc.current - 1]) } else { None };
        let next = if onion && doc.current + 1 < doc.frames.len() { Some(&doc.frames[doc.current + 1]) } else { None };
        for y in 0..doc.height {
            for x in 0..doc.width {
                let i = y * doc.width + x;
                let back: u32 = if (x + y) % 2 == 0 { 62 } else { 78 };
                let mut rgb = [back; 3];
                let mut put = |p: Color, tint: Option<[u32; 3]>, percent: u32| {
                    let c = match tint {
                        Some(t) => [(p[0] as u32 + t[0]) / 2, (p[1] as u32 + t[1]) / 2, (p[2] as u32 + t[2]) / 2],
                        None => [p[0] as u32, p[1] as u32, p[2] as u32],
                    };
                    let a = p[3] as u32 * percent / 100;
                    for k in 0..3 {
                        rgb[k] = (c[k] * a + rgb[k] * (255 - a)) / 255;
                    }
                };
                if let Some(f) = next {
                    put(f.pixels[i], Some([0, 230, 80]), 35);
                }
                if let Some(f) = prev {
                    put(f.pixels[i], Some([255, 60, 60]), 35);
                }
                put(doc.frame().pixels[i], None, 100);
                bytes.extend(rgb.map(|v| v as u8));
                bytes.push(255);
            }
        }
        egui::ColorImage::from_rgba_unmultiplied([doc.width, doc.height], &bytes)
    }

    fn cell_at(&self, origin: Pos2, pos: Pos2) -> (i32, i32) {
        (((pos.x - origin.x) / self.zoom).floor() as i32, ((pos.y - origin.y) / self.zoom).floor() as i32)
    }

    fn brush_color(&self) -> Color {
        if self.tool == Tool::Eraser { TRANSPARENT } else { self.color }
    }

    fn shape_command(&self, start: (i32, i32), end: (i32, i32)) -> Option<Command> {
        let color = to_hex(self.color);
        let (x, y) = (start.0.min(end.0), start.1.min(end.1));
        let (w, h) = ((start.0 - end.0).abs() + 1, (start.1 - end.1).abs() + 1);
        match self.tool {
            Tool::Line => Some(Command::Line { x1: start.0, y1: start.1, x2: end.0, y2: end.1, color }),
            Tool::Rect => Some(Command::Rect { x, y, w, h, color, filled: false }),
            Tool::RectFilled => Some(Command::Rect { x, y, w, h, color, filled: true }),
            Tool::Ellipse => Some(Command::Ellipse { x, y, w, h, color, filled: false }),
            Tool::EllipseFilled => Some(Command::Ellipse { x, y, w, h, color, filled: true }),
            _ => None,
        }
    }

    fn press(&mut self, cell: (i32, i32)) {
        self.playing = false;
        match self.tool {
            Tool::Pencil | Tool::Eraser => {
                self.editor.begin_stroke();
                let color = self.brush_color();
                self.stamp(cell, color);
                self.drag = Some(Drag { start: cell, last: cell });
            }
            Tool::Fill => {
                if let Err(e) = self.editor.apply(&Command::Fill { x: cell.0, y: cell.1, color: to_hex(self.color) }) {
                    self.note = Some(e);
                }
            }
            Tool::Picker => {
                if let Some(c) = self.editor.doc.get(cell.0, cell.1) {
                    if c[3] > 0 {
                        self.color = c;
                    }
                }
            }
            _ => self.drag = Some(Drag { start: cell, last: cell }),
        }
    }

    fn drag_to(&mut self, cell: (i32, i32)) {
        let Some(last) = self.drag.as_ref().map(|d| d.last) else { return };
        if matches!(self.tool, Tool::Pencil | Tool::Eraser) {
            let color = self.brush_color();
            for (x, y) in draw::line(last.0, last.1, cell.0, cell.1) {
                self.stamp((x, y), color);
            }
        }
        if let Some(drag) = &mut self.drag {
            drag.last = cell;
        }
    }

    /// Paints a square of `brush` x `brush` pixels around a point.
    fn stamp(&mut self, (x, y): (i32, i32), color: Color) {
        let size = self.brush_size();
        let off = (size - 1) / 2;
        for dy in 0..size {
            for dx in 0..size {
                self.editor.doc.set(x - off + dx, y - off + dy, color);
            }
        }
    }

    /// How many squares wide the current tool paints (only the pencil and the eraser have a size, each its own).
    fn brush_size(&self) -> i32 {
        match self.tool {
            Tool::Pencil => self.pencil,
            Tool::Eraser => self.eraser,
            _ => 1,
        }
    }

    fn brush_slot(&mut self, tool: Tool) -> Option<&mut i32> {
        match tool {
            Tool::Pencil => Some(&mut self.pencil),
            Tool::Eraser => Some(&mut self.eraser),
            _ => None,
        }
    }

    fn brush_window(&mut self, ctx: &egui::Context, tool: Tool) {
        let mut open = true;
        let mut size = self.brush_slot(tool).map_or(1, |s| *s);
        let title = if tool == Tool::Eraser { "Eraser size" } else { "Pencil size" };
        egui::Window::new(title).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::LEFT_TOP, [190.0, 60.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Squares");
                ui.add(egui::DragValue::new(&mut size).range(1..=MAX_BRUSH));
            });
            ui.horizontal(|ui| {
                for n in [1, 2, 3, 4, 6, 8, 12, 16] {
                    if ui.selectable_label(size == n, n.to_string()).clicked() {
                        size = n;
                    }
                }
            });
            ui.label(RichText::new("Keys [ and ] change the size of the tool in use.").weak().size(11.0));
        });
        if let Some(slot) = self.brush_slot(tool) {
            *slot = size;
        }
        self.brush_open = open.then_some(tool);
    }

    fn release(&mut self, cell: (i32, i32)) {
        let Some(drag) = self.drag.take() else { return };
        if let Some(command) = self.shape_command(drag.start, cell) {
            if let Err(e) = self.editor.apply(&command) {
                self.note = Some(e);
            }
        }
    }

    fn shortcuts(&mut self, ui: &mut egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return; // typing in a field (the text AI box has its own undo)
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S)) {
            self.save(true);
        } else if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            self.save(false);
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::O)) {
            self.request = Some(Leave::Open);
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)) || ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Y)) {
            self.editor.redo();
        } else if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Z)) {
            self.editor.undo();
        }
        if ui.input(|i| i.key_pressed(Key::Space)) {
            self.toggle_play();
        }
        let step = ui.input(|i| i.key_pressed(Key::CloseBracket) as i32 - i.key_pressed(Key::OpenBracket) as i32);
        if let (true, Some(slot)) = (step != 0, self.brush_slot(self.tool)) {
            *slot = (*slot + step).clamp(1, MAX_BRUSH);
        }
        let shift = ui.input(|i| i.modifiers.shift);
        let pressed = |ui: &egui::Ui, key: Key| ui.input(|i| i.key_pressed(key) && !i.modifiers.command);
        if pressed(ui, Key::B) {
            self.tool = Tool::Pencil;
        } else if pressed(ui, Key::E) {
            self.tool = Tool::Eraser;
        } else if pressed(ui, Key::L) {
            self.tool = Tool::Line;
        } else if pressed(ui, Key::R) {
            self.tool = if shift { Tool::RectFilled } else { Tool::Rect };
        } else if pressed(ui, Key::O) {
            self.tool = if shift { Tool::EllipseFilled } else { Tool::Ellipse };
        } else if pressed(ui, Key::G) {
            self.tool = Tool::Fill;
        } else if pressed(ui, Key::I) {
            self.tool = Tool::Picker;
        }
    }

    fn export(&mut self, kind: Export) {
        let (title, ext, name) = match kind {
            Export::Png => ("Export the current frame", "png", "drawing.png"),
            Export::Sheet => ("Export a spritesheet", "png", "spritesheet.png"),
            Export::Gif => ("Export the animation", "gif", "animation.gif"),
        };
        let Some(path) = rfd::FileDialog::new().set_title(title).add_filter(ext, &[ext]).set_file_name(name).save_file() else { return };
        self.note = Some(match self.write_export(kind, &path) {
            Ok(()) => format!("Exported {}", path.display()),
            Err(e) => format!("Could not export: {e}"),
        });
    }

    fn write_export(&self, kind: Export, path: &std::path::Path) -> Result<(), String> {
        use image::{Delay, RgbaImage, imageops};
        let doc = &self.editor.doc;
        let (w, h, scale) = (doc.width as u32, doc.height as u32, self.export_scale);
        let picture = |i: usize| -> Result<RgbaImage, String> {
            let bytes: Vec<u8> = doc.frames[i].pixels.iter().flatten().copied().collect();
            let img = RgbaImage::from_raw(w, h, bytes).ok_or("the image data is not the right size")?;
            Ok(if scale > 1 { imageops::resize(&img, w * scale, h * scale, imageops::FilterType::Nearest) } else { img })
        };
        let count = doc.frames.len();
        match kind {
            Export::Png => picture(doc.current)?.save(path).map_err(|e| e.to_string()),
            Export::Sheet => {
                if w * scale * count as u32 > 16_384 {
                    return Err("the spritesheet would be too wide: lower the scale".into());
                }
                let mut sheet = RgbaImage::new(w * scale * count as u32, h * scale);
                for i in 0..count {
                    imageops::replace(&mut sheet, &picture(i)?, (i as u32 * w * scale) as i64, 0);
                }
                sheet.save(path).map_err(|e| e.to_string())
            }
            Export::Gif => {
                let delay = Delay::from_numer_denom_ms(1000, self.fps);
                let frames = (0..count).map(|i| picture(i).map(|img| image::Frame::from_parts(img, 0, 0, delay))).collect::<Result<Vec<_>, _>>()?;
                let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
                let mut encoder = image::codecs::gif::GifEncoder::new(file);
                encoder.set_repeat(image::codecs::gif::Repeat::Infinite).map_err(|e| e.to_string())?;
                encoder.encode_frames(frames).map_err(|e| e.to_string())
            }
        }
    }

    fn dirty(&self) -> bool {
        self.editor.version() != self.saved_version
    }

    fn mark_saved(&mut self) {
        self.saved_version = self.editor.version();
    }

    /// Saves the drawing (asking where, the first time or for "Save as"). Returns whether it was saved.
    fn save(&mut self, as_new: bool) -> bool {
        let known = if as_new { None } else { self.path.clone() };
        let path = match known {
            Some(path) => path,
            None => {
                let Some(chosen) = rfd::FileDialog::new().set_title("Save the drawing").add_filter("Skribbl drawing", &["skb"]).set_file_name("drawing.skb").save_file() else { return false };
                if chosen.extension().is_none() { chosen.with_extension("skb") } else { chosen }
            }
        };
        match project::save(&path, &self.editor.doc, self.fps) {
            Ok(()) => {
                self.note = Some(format!("Saved {}", path.display()));
                self.path = Some(path);
                self.mark_saved();
                true
            }
            Err(e) => {
                self.note = Some(format!("Could not save: {e}"));
                false
            }
        }
    }

    /// Puts a drawing in the editor, as if it had just been opened.
    fn install(&mut self, editor: Editor, path: Option<std::path::PathBuf>) {
        self.editor = editor;
        self.path = path;
        self.mark_saved();
        self.needs_fit = true;
        self.grid = self.options.grid;
        self.note = None;
        self.playing = false;
        self.screen = Screen::Editor;
    }

    fn perform(&mut self, ctx: &egui::Context, what: Leave) {
        match what {
            Leave::Create => self.install(Editor::new(self.new_size.0, self.new_size.1), None),
            Leave::Open => {
                let Some(path) = rfd::FileDialog::new().set_title("Open a drawing").add_filter("Skribbl drawing", &["skb"]).pick_file() else { return };
                match project::load(&path) {
                    Ok((doc, fps)) => {
                        self.fps = fps;
                        self.install(Editor::from_document(doc), Some(path));
                    }
                    Err(e) => self.note = Some(format!("Could not open {}: {e}", path.display())),
                }
            }
            Leave::Quit => {
                self.quit_ok = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Asks before something that would throw away unsaved work.
    fn confirm_window(&mut self, ctx: &egui::Context) {
        let Some(what) = self.confirm else { return };
        let mut choice = None;
        egui::Window::new("Unsaved changes").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("The drawing has changes that are not saved.");
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(Some(true));
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(Some(false));
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(None);
                }
            });
        });
        match choice {
            Some(Some(true)) => {
                if self.save(false) {
                    self.confirm = None;
                    self.perform(ctx, what);
                }
            }
            Some(Some(false)) => {
                self.confirm = None;
                self.perform(ctx, what);
            }
            Some(None) => self.confirm = None,
            None => {}
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = if self.screen == Screen::Editor {
            let name = self.path.as_ref().and_then(|p| p.file_stem()).map_or_else(|| "Untitled".to_string(), |n| n.to_string_lossy().into_owned());
            format!("{name}{} - Skribbl", if self.dirty() { " *" } else { "" })
        } else {
            "Skribbl".to_string()
        };
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    fn pick_image(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_title("Choose a picture").add_filter("Pictures", &["png", "jpg", "jpeg", "bmp", "gif"]).pick_file() else { return };
        match image::open(&path) {
            Ok(picture) => {
                let name = path.file_name().map_or_else(|| "picture".to_string(), |n| n.to_string_lossy().into_owned());
                self.import = Some(import_window::ImportWindow::new(name, picture.to_rgba8()));
            }
            Err(e) => self.note = Some(format!("Could not open the picture: {e}")),
        }
    }

    fn import_window(&mut self, ctx: &egui::Context) {
        let Some(mut window) = self.import.take() else { return };
        match window.show(ctx, &self.editor.doc.palette) {
            import_window::Outcome::Open => self.import = Some(window),
            import_window::Outcome::Cancel => {}
            import_window::Outcome::Apply(done) => {
                self.editor.replace_drawing(done.width, done.height, done.pixels, done.palette);
                self.needs_fit = true;
                self.playing = false;
            }
        }
    }

    fn run(&mut self, command: Command) {
        if let Err(e) = self.editor.apply(&command) {
            self.note = Some(e);
        }
    }

    fn toggle_play(&mut self) {
        self.playing = !self.playing;
        self.next_tick = 0.0;
    }

    /// While playing, moves to the next frame whenever it is time.
    fn advance(&mut self, ui: &egui::Ui) {
        if !self.playing {
            return;
        }
        let count = self.editor.doc.frames.len();
        if count < 2 {
            self.playing = false;
            return;
        }
        let now = ui.input(|i| i.time);
        let interval = 1.0 / self.fps.max(1) as f64;
        if now >= self.next_tick {
            let index = (self.editor.doc.current + 1) % count;
            self.run(Command::SelectFrame { index });
            self.next_tick = now + interval;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(interval / 4.0));
    }

    fn timeline(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("timeline").frame(egui::Frame::new().inner_margin(Margin::symmetric(10, 8))).show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button(if self.playing { "Pause" } else { "Play" }).on_hover_text("Space").clicked() {
                    self.toggle_play();
                }
                ui.label("fps");
                ui.add(egui::DragValue::new(&mut self.fps).range(1..=30));
                ui.checkbox(&mut self.onion, "Onion skin").on_hover_text("Shows the previous frame in red and the next one in green");
                ui.separator();
                let current = self.editor.doc.current;
                if ui.button("New frame").clicked() {
                    self.run(Command::NewFrame { copy_current: false });
                }
                if ui.button("Duplicate").on_hover_text("A new frame that starts as a copy of this one").clicked() {
                    self.run(Command::NewFrame { copy_current: true });
                }
                if ui.add_enabled(self.editor.doc.frames.len() > 1, egui::Button::new("Delete frame")).clicked() {
                    self.run(Command::DeleteFrame { index: current });
                }
            });
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for i in 0..self.editor.doc.frames.len() {
                        let button = egui::Button::new((i + 1).to_string()).min_size(vec2(34.0, 28.0)).selected(i == self.editor.doc.current);
                        if ui.add(button).clicked() {
                            self.run(Command::SelectFrame { index: i });
                        }
                    }
                });
            });
        });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("top").frame(egui::Frame::new().inner_margin(Margin::symmetric(10, 8))).show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Menu").clicked() {
                    self.screen = Screen::Menu;
                }
                if ui.button("New").clicked() {
                    self.screen = Screen::Size;
                    self.back = Screen::Editor;
                }
                if ui.button("Open").on_hover_text("Ctrl+O").clicked() {
                    self.request = Some(Leave::Open);
                }
                if ui.button("Save").on_hover_text("Ctrl+S").clicked() {
                    self.save(false);
                }
                if ui.button("Save as").on_hover_text("Ctrl+Shift+S").clicked() {
                    self.save(true);
                }
                ui.separator();
                if ui.button("Import image").on_hover_text("Turn a picture into pixel art").clicked() {
                    self.pick_image();
                }
                ui.menu_button("Export", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Scale");
                        ui.add(egui::DragValue::new(&mut self.export_scale).range(1..=32).suffix("x"));
                    });
                    for (kind, text) in [(Export::Png, "Current frame (PNG)"), (Export::Sheet, "Spritesheet (PNG)"), (Export::Gif, "Animation (GIF)")] {
                        if ui.button(text).clicked() {
                            self.pending_export = Some(kind);
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui.add_enabled(self.editor.can_undo(), egui::Button::new("Undo")).on_hover_text("Ctrl+Z").clicked() {
                    self.editor.undo();
                }
                if ui.add_enabled(self.editor.can_redo(), egui::Button::new("Redo")).on_hover_text("Ctrl+Y").clicked() {
                    self.editor.redo();
                }
                ui.separator();
                ui.toggle_value(&mut self.ai.open, "AI");
                ui.separator();
                ui.checkbox(&mut self.grid, "Grid");
                if ui.button("Fit").clicked() {
                    self.needs_fit = true;
                }
                ui.label(RichText::new(format!("{:.0}%", self.zoom * 100.0)).weak());
            });
        });
    }

    fn side_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("tools").exact_size(170.0).resizable(false).frame(egui::Frame::new().inner_margin(Margin::same(10))).show(ui, |ui| {
            ui.label(RichText::new("Tools").strong());
            for (tool, name, key) in Tool::ALL {
                let sized = matches!(tool, Tool::Pencil | Tool::Eraser);
                let size = if tool == Tool::Eraser { self.eraser } else { self.pencil };
                let label = if sized { format!("{name} {size}x{size}   ({key})") } else { format!("{name}   ({key})") };
                let response = ui.selectable_label(self.tool == tool, label);
                if sized {
                    response.clone().on_hover_text("Double-click to choose how many squares it fills");
                }
                if response.clicked() {
                    self.tool = tool;
                }
                if sized && response.double_clicked() {
                    self.tool = tool;
                    self.brush_open = Some(tool);
                }
            }
            ui.add_space(10.0);
            ui.label(RichText::new("Colour").strong());
            let mut picked = color32(self.color);
            if egui::color_picker::color_edit_button_srgba(ui, &mut picked, egui::color_picker::Alpha::OnlyBlend).changed() {
                self.color = picked.to_srgba_unmultiplied();
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Palette").strong());
            let palette = self.editor.doc.palette.clone();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for swatch in &palette {
                    let (rect, response) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                    ui.painter().rect_filled(rect, 3.0, color32(*swatch));
                    if *swatch == self.color {
                        ui.painter().rect_stroke(rect.expand(1.0), 3.0, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
                    }
                    if response.on_hover_text(to_hex(*swatch)).clicked() {
                        self.color = *swatch;
                    }
                }
            });
            if ui.small_button("Add the colour to the palette").clicked() && !palette.contains(&self.color) {
                let mut colors: Vec<String> = palette.iter().map(|c| to_hex(*c)).collect();
                colors.push(to_hex(self.color));
                if let Err(e) = self.editor.apply(&Command::SetPalette { colors }) {
                    self.note = Some(e);
                }
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status").frame(egui::Frame::new().inner_margin(Margin::symmetric(10, 6))).show(ui, |ui| {
            ui.horizontal(|ui| {
                let doc = &self.editor.doc;
                ui.label(RichText::new(format!("{}x{}", doc.width, doc.height)).weak());
                ui.label(RichText::new(format!("frame {} of {}", doc.current + 1, doc.frames.len())).weak());
                if let Some((x, y)) = self.hover.filter(|(x, y)| doc.in_bounds(*x, *y)) {
                    ui.label(RichText::new(format!("({x}, {y})")).weak());
                }
                if let Some(note) = &self.note {
                    ui.separator();
                    ui.label(note);
                }
            });
        });
    }

    /// The launch screens: a white canvas, the title drawn in pixels from the middle upwards, the buttons below.
    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::no_frame().frame(egui::Frame::new().fill(Color32::WHITE)).show(ui, |ui| {
            let area = ui.max_rect();
            let middle = area.center().y;
            let now = ui.input(|i| i.time);
            let t = (now - *self.menu_t0.get_or_insert(now)) as f32;
            ui.ctx().request_repaint();
            menu_background(ui.painter(), area, t);

            // the title: its pixels light up one by one, then the letters float gently
            let cell = ((area.width() * 0.8 / TITLE_WIDTH).min(area.height() * 0.4 / 7.0)).floor().max(2.0);
            let left = area.center().x - TITLE_WIDTH * cell / 2.0;
            let top = middle - 7.0 * cell - 12.0;
            let ramp = ((t - 2.2) / 1.0).clamp(0.0, 1.0);
            let mut cells = Vec::new();
            for (i, letter) in "SKRIBBL".chars().enumerate() {
                let wave = ((t - 2.2) * 2.0 + i as f32 * 0.7).sin() * cell * 0.3 * ramp;
                for (row, line) in glyph(letter).iter().enumerate() {
                    for (col, ch) in line.chars().enumerate() {
                        let delay = unit(hash((i * 35 + row * 5 + col) as u32, 7)) * 1.8;
                        if ch == '#' && t >= delay {
                            let p = Pos2::new(left + (i * 6 + col) as f32 * cell, top + row as f32 * cell + wave);
                            cells.push(Rect::from_min_size(p, Vec2::splat(cell)));
                        }
                    }
                }
            }
            for r in &cells {
                ui.painter().rect_filled(r.translate(Vec2::splat(cell * 0.45)), 0.0, Color32::from_rgb(41, 173, 255));
            }
            for r in &cells {
                ui.painter().rect_filled(*r, 0.0, INK);
            }
            let below = Rect::from_min_max(Pos2::new(area.left(), middle + 12.0), area.right_bottom());
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(below).layout(egui::Layout::top_down(egui::Align::Center)));
            ui.spacing_mut().item_spacing.y = 12.0;
            match self.screen {
                Screen::Menu => {
                    if pixel_button(&mut ui, "START") {
                        self.screen = Screen::Size;
                        self.back = Screen::Menu;
                    }
                    if pixel_button(&mut ui, "OPEN") {
                        self.request = Some(Leave::Open);
                    }
                    if pixel_button(&mut ui, "OPTIONS") {
                        self.screen = Screen::Options;
                    }
                    if pixel_button(&mut ui, "QUIT") {
                        self.request = Some(Leave::Quit);
                    }
                }
                Screen::Size => {
                    ui.label(RichText::new("Canvas size").size(20.0));
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 4.0 * 84.0 - 3.0 * 8.0) / 2.0);
                        for size in [16, 32, 64, 128] {
                            let button = egui::Button::new(format!("{size}x{size}")).min_size(vec2(84.0, 30.0)).selected(self.new_size == (size, size));
                            if ui.add(button).clicked() {
                                self.new_size = (size, size);
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 170.0) / 2.0);
                        ui.add(egui::DragValue::new(&mut self.new_size.0).range(1..=MAX_SIZE));
                        ui.label("x");
                        ui.add(egui::DragValue::new(&mut self.new_size.1).range(1..=MAX_SIZE));
                        ui.label("pixels");
                    });
                    if pixel_button(&mut ui, "CREATE") {
                        self.request = Some(Leave::Create);
                    }
                    if pixel_button(&mut ui, "BACK") {
                        self.screen = self.back;
                    }
                }
                Screen::Options => {
                    let mut changed = ui.checkbox(&mut self.options.grid, "Show the grid in new drawings").changed();
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 330.0) / 2.0);
                        ui.label("Interface size");
                        if ui.add(egui::Slider::new(&mut self.options.ui_scale, 0.8..=1.6)).drag_stopped() {
                            changed = true;
                        }
                    });
                    if changed {
                        ui.ctx().set_zoom_factor(self.options.ui_scale);
                        self.options.save();
                    }
                    if pixel_button(&mut ui, "BACK") {
                        self.screen = Screen::Menu;
                    }
                }
                Screen::Editor => {}
            }
        });
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::no_frame().frame(egui::Frame::new().fill(Color32::from_rgb(22, 23, 27))).show(ui, |ui| {
            let area = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(area, Sense::click_and_drag());
            let painter = ui.painter_at(area);
            let (w, h) = (self.editor.doc.width as f32, self.editor.doc.height as f32);

            if self.needs_fit {
                self.zoom = ((area.width() / w).min(area.height() / h) * 0.9).floor().clamp(1.0, 64.0);
                self.pan = Vec2::ZERO;
                self.needs_fit = false;
            }
            let origin_for = |zoom: f32, pan: Vec2| area.center() - vec2(w, h) * zoom / 2.0 + pan;

            // zoom around the pointer, pan with the middle button
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if let (true, Some(pointer)) = (scroll != 0.0, response.hover_pos()) {
                    let before = origin_for(self.zoom, self.pan);
                    let under = (pointer - before) / self.zoom;
                    self.zoom = (self.zoom * (scroll * 0.0015).exp()).clamp(1.0, 64.0);
                    self.pan += pointer - under * self.zoom - origin_for(self.zoom, self.pan);
                }
            }
            if response.dragged_by(PointerButton::Middle) {
                self.pan += response.drag_delta();
            }

            let origin = origin_for(self.zoom, self.pan);
            let canvas_rect = Rect::from_min_size(origin, vec2(w, h) * self.zoom);

            // the picture
            let image = self.render();
            match &mut self.texture {
                Some(texture) => texture.set(image, TextureOptions::NEAREST),
                None => self.texture = Some(ui.ctx().load_texture("canvas", image, TextureOptions::NEAREST)),
            }
            if let Some(texture) = &self.texture {
                painter.image(texture.id(), canvas_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            painter.rect_stroke(canvas_rect, 0.0, Stroke::new(1.0, Color32::from_gray(90)), egui::StrokeKind::Outside);
            if self.grid && self.zoom >= 6.0 {
                let line = Stroke::new(1.0, Color32::from_black_alpha(70));
                for x in 1..self.editor.doc.width {
                    let px = origin.x + x as f32 * self.zoom;
                    painter.line_segment([Pos2::new(px, canvas_rect.top()), Pos2::new(px, canvas_rect.bottom())], line);
                }
                for y in 1..self.editor.doc.height {
                    let py = origin.y + y as f32 * self.zoom;
                    painter.line_segment([Pos2::new(canvas_rect.left(), py), Pos2::new(canvas_rect.right(), py)], line);
                }
            }

            // the mouse
            let pointer = ui.input(|i| i.pointer.latest_pos());
            self.hover = pointer.filter(|_| response.hovered()).map(|p| self.cell_at(origin, p));
            let (pressed, down, released) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released()));
            if let Some(pos) = pointer {
                let cell = self.cell_at(origin, pos);
                if pressed && response.hovered() {
                    self.press(cell);
                } else if down && self.drag.is_some() {
                    self.drag_to(cell);
                }
                if released {
                    self.release(cell);
                }
            }

            // a ghost of the shape being dragged
            if let (Some(drag), Some(pos)) = (&self.drag, pointer) {
                let end = self.cell_at(origin, pos);
                let ghost = match self.shape_command(drag.start, end) {
                    Some(Command::Line { x1, y1, x2, y2, .. }) => draw::line(x1, y1, x2, y2),
                    Some(Command::Rect { x, y, w, h, filled, .. }) => draw::rect(x, y, w, h, filled),
                    Some(Command::Ellipse { x, y, w, h, filled, .. }) => draw::ellipse(x, y, w, h, filled),
                    _ => Vec::new(),
                };
                let tint = color32(self.color).gamma_multiply(0.7);
                for (x, y) in ghost {
                    if self.editor.doc.in_bounds(x, y) {
                        painter.rect_filled(Rect::from_min_size(origin + vec2(x as f32, y as f32) * self.zoom, Vec2::splat(self.zoom)), 0.0, tint);
                    }
                }
            }
            if let Some((x, y)) = self.hover.filter(|(x, y)| self.editor.doc.in_bounds(*x, *y)) {
                let (size, off) = if matches!(self.tool, Tool::Pencil | Tool::Eraser) { (self.brush_size(), (self.brush_size() - 1) / 2) } else { (1, 0) };
                let cell = Rect::from_min_size(origin + vec2((x - off) as f32, (y - off) as f32) * self.zoom, Vec2::splat(self.zoom * size as f32));
                painter.rect_stroke(cell, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_ok && self.dirty() && self.confirm.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Leave::Quit);
        }
        if let Some(what) = self.request.take() {
            // a drawing that is open and changed is not thrown away without asking (Open and Quit also leave the menu)
            if self.dirty() && self.confirm.is_none() { self.confirm = Some(what) } else { self.perform(&ctx, what) }
        }
        self.update_title(&ctx);
        self.confirm_window(&ctx);
        let editing = self.screen == Screen::Editor;
        if editing != self.dark {
            self.dark = editing;
            if editing { theme(ui.ctx()) } else { ui.ctx().set_visuals(egui::Visuals::light()) }
        }
        if editing {
            self.menu_t0 = None;
        }
        if !editing {
            self.menu(ui);
            return;
        }
        self.advance(ui);
        self.shortcuts(ui);
        self.top_bar(ui);
        if let Some(kind) = self.pending_export.take() {
            self.export(kind);
        }
        self.status_bar(ui);
        self.timeline(ui);
        self.side_bar(ui);
        self.ai.show(ui, &mut self.editor);
        self.canvas(ui);
        self.import_window(&ctx);
        if let Some(tool) = self.brush_open {
            self.brush_window(&ctx, tool);
        }
    }
}

fn main() -> eframe::Result {
    let viewport = egui::ViewportBuilder::default().with_title("Skribbl").with_inner_size([1100.0, 760.0]).with_min_inner_size([640.0, 440.0]);
    eframe::run_native("Skribbl", eframe::NativeOptions { viewport, ..Default::default() }, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
