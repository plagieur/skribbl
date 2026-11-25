//! The AI side panel: the connection settings, the conversation, and the loop that applies what the AI answers.

use crate::ai::{self, Call, Config, Msg, Reply, Request};
use crate::command::Command;
use crate::editor::Editor;
use eframe::egui::{self, Color32, Key, Margin, RichText};
use serde_json::Value;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

/// How many times the AI is shown its mistakes and asked again, after one answer.
const MAX_FIXES: usize = 2;

enum Entry {
    User(String),
    Ai(String),
    Applied { count: usize, errors: Vec<String> },
    Note(String),
    Error(String),
}

pub struct AiPanel {
    pub open: bool,
    config: Config,
    key: String,
    key_note: Option<String>,
    messages: Vec<Msg>,
    log: Vec<Entry>,
    input: String,
    pending: Option<Receiver<Result<Reply, String>>>,
    fixes: usize,
    settings_open: bool,
}

impl AiPanel {
    pub fn new() -> Self {
        let config = Config::load();
        let key = ai::load_key(&config.base_url);
        Self { open: false, settings_open: key.is_empty() && config.model.is_empty(), config, key, key_note: None, messages: Vec::new(), log: Vec::new(), input: String::new(), pending: None, fixes: 0 }
    }

    fn submit(&mut self, ctx: &egui::Context, editor: &Editor) {
        let prompt = self.input.trim().to_string();
        if prompt.is_empty() || self.pending.is_some() {
            return;
        }
        // only the latest picture of the canvas is worth sending
        for message in &mut self.messages {
            if let Msg::User { image, .. } = message {
                *image = None;
            }
        }
        let text = format!("{}\n\nRequest: {prompt}", ai::describe(&editor.doc));
        let image = if self.config.send_image { ai::canvas_png_base64(&editor.doc) } else { None };
        self.messages.push(Msg::User { text, image });
        self.log.push(Entry::User(prompt));
        self.input.clear();
        self.fixes = 0;
        self.start(ctx);
    }

    fn start(&mut self, ctx: &egui::Context) {
        let request = Request { config: self.config.clone(), key: self.key.clone(), system: ai::system_prompt(self.config.tools), messages: self.messages.clone() };
        let (sender, receiver) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(ai::send(&request));
            ctx.request_repaint();
        });
        self.pending = Some(receiver);
    }

    fn poll(&mut self, ctx: &egui::Context, editor: &mut Editor) {
        let Some(receiver) = &self.pending else { return };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("the request stopped unexpectedly".to_string()),
        };
        self.pending = None;
        match result {
            Ok(reply) => self.handle(ctx, editor, reply),
            Err(e) => self.log.push(Entry::Error(e)),
        }
    }

    /// Runs what the AI answered as ONE undo step, then tells it how it went.
    fn handle(&mut self, ctx: &egui::Context, editor: &mut Editor, reply: Reply) {
        if !reply.text.trim().is_empty() {
            self.log.push(Entry::Ai(reply.text.trim().to_string()));
        }
        // where the commands are: in the tool calls, or (if there are none) in the text
        let from_tools = self.config.tools && !reply.calls.is_empty();
        let sources: Vec<(String, Value)> = if from_tools {
            reply.calls.iter().map(|c| (c.id.clone(), c.args.clone())).collect()
        } else {
            ai::extract_json(&reply.text).map(|v| (String::new(), v)).into_iter().collect()
        };
        self.messages.push(Msg::Assistant { text: reply.text.clone(), calls: if from_tools { reply.calls.clone() } else { Vec::<Call>::new() } });
        if sources.is_empty() {
            let note = if reply.text.trim().is_empty() { "The AI answered with nothing." } else { "No commands in this answer: nothing was drawn." };
            self.log.push(Entry::Note(note.into()));
            return;
        }

        let parsed: Vec<Vec<Result<Command, String>>> = sources.iter().map(|(_, args)| ai::commands_from(args)).collect();
        let valid: Vec<Command> = parsed.iter().flatten().filter_map(|c| c.as_ref().ok().cloned()).collect();
        let mut results = editor.apply_batch(&valid).into_iter();

        let mut errors = Vec::new();
        let mut count = 0;
        let mut feedback = Vec::new();
        for (list, (id, _)) in parsed.iter().zip(&sources) {
            let mut lines = Vec::new();
            for (n, command) in list.iter().enumerate() {
                let outcome = match command {
                    Ok(_) => results.next().unwrap_or_else(|| Err("not run".into())),
                    Err(e) => Err(e.clone()),
                };
                match outcome {
                    Ok(message) => {
                        count += 1;
                        lines.push(format!("command {n}: ok, {message}"));
                    }
                    Err(e) => {
                        lines.push(format!("command {n}: ERROR, {e}"));
                        errors.push(format!("command {n}: {e}"));
                    }
                }
            }
            feedback.push((id.clone(), lines.join("\n")));
        }
        self.log.push(Entry::Applied { count, errors: errors.clone() });

        let canvas = if errors.is_empty() { String::new() } else { format!("\n\nThe canvas now:\n{}", ai::describe(&editor.doc)) };
        if from_tools {
            let last = feedback.len().saturating_sub(1);
            for (i, (id, text)) in feedback.into_iter().enumerate() {
                self.messages.push(Msg::ToolResult { id, text: if i == last { format!("{text}{canvas}") } else { text } });
            }
        } else if !errors.is_empty() {
            let text = feedback.into_iter().map(|(_, t)| t).collect::<Vec<_>>().join("\n");
            self.messages.push(Msg::User { text: format!("Results of your commands:\n{text}{canvas}"), image: None });
        }
        if !errors.is_empty() && self.fixes < MAX_FIXES {
            self.fixes += 1;
            self.log.push(Entry::Note(format!("{} command(s) failed: the AI is asked to fix them.", errors.len())));
            self.start(ctx);
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, editor: &mut Editor) {
        self.poll(ui.ctx(), editor);
        if !self.open {
            return;
        }
        egui::Panel::right("ai").resizable(true).default_size(340.0).min_size(260.0).frame(egui::Frame::new().inner_margin(Margin::same(10))).show(ui, |ui| {
            ui.label(RichText::new("AI").strong().size(16.0));
            self.settings(ui);
            ui.separator();
            let input_height = 130.0;
            egui::ScrollArea::vertical().auto_shrink(false).stick_to_bottom(true).max_height((ui.available_height() - input_height).max(60.0)).show(ui, |ui| {
                self.chat(ui);
            });
            ui.separator();
            self.input_box(ui, editor);
        });
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Connection").default_open(self.settings_open).show(ui, |ui| {
            let before = self.config.clone();
            let mut address_edited = false;
            egui::ComboBox::from_label("Provider").selected_text(ai::PRESETS[self.config.preset].name).show_ui(ui, |ui| {
                for (i, preset) in ai::PRESETS.iter().enumerate() {
                    if ui.selectable_label(self.config.preset == i, preset.name).clicked() {
                        self.config.use_preset(i);
                        address_edited = true;
                    }
                }
            });
            if self.config.preset == ai::CUSTOM {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.config.kind, ai::Kind::OpenAi, "OpenAI-style");
                    ui.selectable_value(&mut self.config.kind, ai::Kind::Anthropic, "Anthropic-style");
                });
            }
            ui.label("Address");
            let address = ui.add(egui::TextEdit::singleline(&mut self.config.base_url).desired_width(f32::INFINITY).hint_text("https://..."));
            address_edited |= address.lost_focus() && self.config.base_url != before.base_url;
            ui.label("Model");
            let hint = ai::PRESETS[self.config.preset].model_hint;
            ui.add(egui::TextEdit::singleline(&mut self.config.model).desired_width(f32::INFINITY).hint_text(hint));
            if self.config.kind == ai::Kind::OpenAi {
                let shown = if self.config.reasoning.is_empty() { "default" } else { self.config.reasoning.as_str() }.to_string();
                egui::ComboBox::from_label("Reasoning effort").selected_text(shown).show_ui(ui, |ui| {
                    for (value, label) in [("", "default"), ("low", "low"), ("medium", "medium"), ("high", "high")] {
                        if ui.selectable_label(self.config.reasoning == value, label).clicked() {
                            self.config.reasoning = value.into();
                        }
                    }
                }).response.on_hover_text("For models that think before answering (like GPT-OSS). \"low\" leaves more room for the drawing itself. Models that do not think may refuse it: keep \"default\" for them.");
            }
            ui.label("API key");
            ui.add(egui::TextEdit::singleline(&mut self.key).password(true).desired_width(f32::INFINITY).hint_text("paste it here (not needed for a local server)"));
            ui.horizontal(|ui| {
                if ui.button("Save the key").clicked() {
                    self.key_note = Some(match ai::save_key(&self.config.base_url, &self.key) {
                        Ok(()) if self.key.trim().is_empty() => "Key removed.".into(),
                        Ok(()) => "Key saved in the Windows credential store.".into(),
                        Err(e) => format!("Could not save the key: {e}"),
                    });
                }
                if ui.button("Forget it").clicked() {
                    self.key.clear();
                    self.key_note = Some(match ai::save_key(&self.config.base_url, "") {
                        Ok(()) => "Key removed.".into(),
                        Err(e) => format!("Could not remove the key: {e}"),
                    });
                }
            });
            if let Some(note) = &self.key_note {
                ui.label(RichText::new(note).weak().size(11.0));
            }
            ui.checkbox(&mut self.config.tools, "The model can call tools").on_hover_text("Turn it off for a model that cannot: it then answers with JSON commands, which Skribbl reads from its text.");
            ui.checkbox(&mut self.config.send_image, "Send a picture of the canvas").on_hover_text("Only for models that understand images. The canvas is always sent as text too.");
            if address_edited {
                self.key = ai::load_key(&self.config.base_url);
                self.key_note = None;
            }
            let c = &self.config;
            if (c.preset, c.kind, &c.base_url, &c.model, c.tools, c.send_image, &c.reasoning) != (before.preset, before.kind, &before.base_url, &before.model, before.tools, before.send_image, &before.reasoning) {
                self.config.save();
            }
        });
    }

    fn chat(&mut self, ui: &mut egui::Ui) {
        if self.log.is_empty() && self.pending.is_none() {
            ui.label(RichText::new("Ask for a drawing or an animation, or for a change to what is on the canvas. Each answer is one undo step (Ctrl+Z).").weak());
        }
        for entry in &self.log {
            match entry {
                Entry::User(text) => {
                    ui.label(RichText::new("You").strong());
                    ui.label(text);
                }
                Entry::Ai(text) => {
                    ui.label(RichText::new("AI").strong().color(Color32::from_rgb(41, 173, 255)));
                    ui.label(text);
                }
                Entry::Applied { count, errors } => {
                    ui.label(RichText::new(format!("{count} command(s) applied")).color(Color32::from_rgb(0, 200, 90)).size(12.0));
                    for error in errors {
                        ui.label(RichText::new(error).color(Color32::from_rgb(255, 90, 90)).size(12.0));
                    }
                }
                Entry::Note(text) => {
                    ui.label(RichText::new(text).weak().size(12.0));
                }
                Entry::Error(text) => {
                    ui.label(RichText::new(text).color(Color32::from_rgb(255, 90, 90)));
                }
            }
            ui.add_space(4.0);
        }
        if self.pending.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("The AI is drawing...").weak());
            });
        }
    }

    fn input_box(&mut self, ui: &mut egui::Ui, editor: &Editor) {
        // a long prompt scrolls inside the box instead of pushing the buttons out of the panel
        let edit = egui::ScrollArea::vertical()
            .id_salt("ai_input")
            .max_height(84.0)
            .show(ui, |ui| ui.add(egui::TextEdit::multiline(&mut self.input).desired_rows(3).desired_width(f32::INFINITY).hint_text("Describe what to draw. Ctrl+Enter sends.")))
            .inner;
        let send_key = edit.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
        ui.horizontal(|ui| {
            if self.pending.is_some() {
                if ui.button("Stop").clicked() {
                    self.pending = None;
                    self.log.push(Entry::Note("Stopped.".into()));
                }
            } else if ui.add_enabled(!self.input.trim().is_empty(), egui::Button::new("Send")).clicked() || (send_key && !self.input.trim().is_empty()) {
                self.submit(ui.ctx(), editor);
            }
            if self.pending.is_none() && !self.log.is_empty() && ui.button("Improve").on_hover_text("Ask the AI to refine the drawing: outline, shading, details").clicked() {
                self.input = "Improve the current drawing: keep the same subject, add a dark outline, shading, a highlight and more details.".into();
                self.submit(ui.ctx(), editor);
            }
            if ui.button("New chat").on_hover_text("Forget the conversation (the drawing stays)").clicked() {
                self.pending = None;
                self.messages.clear();
                self.log.clear();
            }
        });
    }
}
