//! The "Import an image" window: choose how the picture becomes pixel art, with a live preview.

use crate::doc::Color;
use crate::pixelize::{self, Colors, Converted, Settings};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, TextureHandle, TextureOptions, Vec2, vec2};
use image::RgbaImage;

pub enum Outcome {
    Open,
    Cancel,
    Apply(Converted),
}

pub struct ImportWindow {
    name: String,
    image: RgbaImage,
    settings: Settings,
    result: Option<Converted>,
    texture: Option<TextureHandle>,
    dirty: bool,
}

impl ImportWindow {
    pub fn new(name: String, image: RgbaImage) -> Self {
        let longest = image.width().max(image.height());
        let settings = Settings { max_side: longest.clamp(8, 32), ..Settings::default() };
        Self { name, image, settings, result: None, texture: None, dirty: true }
    }

    fn refresh(&mut self, ctx: &egui::Context, palette: &[Color]) {
        let result = pixelize::convert(&self.image, palette, &self.settings);
        // the preview shows what is transparent as a mid grey
        let bytes: Vec<u8> = result.pixels.iter().flat_map(|p| if p[3] == 0 { [70, 70, 78, 255] } else { *p }).collect();
        let picture = egui::ColorImage::from_rgba_unmultiplied([result.width, result.height], &bytes);
        match &mut self.texture {
            Some(texture) => texture.set(picture, TextureOptions::NEAREST),
            None => self.texture = Some(ctx.load_texture("import_preview", picture, TextureOptions::NEAREST)),
        }
        self.result = Some(result);
        self.dirty = false;
    }

    pub fn show(&mut self, ctx: &egui::Context, palette: &[Color]) -> Outcome {
        if self.dirty {
            self.refresh(ctx, palette);
        }
        let mut open = true;
        let mut outcome = Outcome::Open;
        egui::Window::new("Import an image").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(RichText::new(format!("{} ({}x{})", self.name, self.image.width(), self.image.height())).weak());
            let before = self.settings.clone();
            ui.horizontal(|ui| {
                ui.label("Longest side");
                ui.add(egui::DragValue::new(&mut self.settings.max_side).range(4..=crate::doc::MAX_SIZE as u32).suffix(" px"));
                if let Some(r) = &self.result {
                    ui.label(RichText::new(format!("-> {}x{}", r.width, r.height)).weak());
                }
            });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.settings.colors, Colors::Reduce, "Pick the best colours");
                ui.selectable_value(&mut self.settings.colors, Colors::Palette, "Use my palette");
                ui.selectable_value(&mut self.settings.colors, Colors::Original, "Keep all colours");
            });
            if self.settings.colors == Colors::Reduce {
                ui.horizontal(|ui| {
                    ui.label("Colours");
                    ui.add(egui::Slider::new(&mut self.settings.count, 2..=64));
                });
            }
            ui.add_enabled(self.settings.colors != Colors::Original, egui::Checkbox::new(&mut self.settings.dither, "Dither (smoother gradients, noisier look)"));
            if (before.max_side, before.colors, before.count, before.dither) != (self.settings.max_side, self.settings.colors, self.settings.count, self.settings.dither) {
                self.dirty = true;
            }

            if let (Some(texture), Some(r)) = (&self.texture, &self.result) {
                let scale = (256.0 / r.width.max(r.height) as f32).max(1.0).floor().min(24.0);
                let (rect, _) = ui.allocate_exact_size(vec2(r.width as f32, r.height as f32) * scale + Vec2::splat(0.0), Sense::hover());
                ui.painter().image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            ui.label(RichText::new("This replaces the current drawing (Ctrl+Z brings it back).").weak().size(11.0));
            ui.horizontal(|ui| {
                if ui.button("Import").clicked() {
                    if let Some(result) = self.result.take() {
                        outcome = Outcome::Apply(result);
                    }
                }
                if ui.button("Cancel").clicked() {
                    outcome = Outcome::Cancel;
                }
            });
        });
        if !open {
            outcome = Outcome::Cancel;
        }
        outcome
    }
}
