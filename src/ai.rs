//! Talking to an AI. Two adapters cover everything: one for the OpenAI "chat completions" API (OpenAI, Groq, DeepSeek,
//! Ollama, LM Studio and any other server that copies it) and one for Anthropic's Messages API.
//!
//! The AI draws with the same commands as the editor. Models that can call tools get one tool, `draw`; for the
//! others the answer is a JSON block that is read from the text.

use crate::command::Command;
use crate::doc::{Document, to_hex};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Kind {
    OpenAi,
    Anthropic,
}

pub struct Preset {
    pub name: &'static str,
    pub kind: Kind,
    pub url: &'static str,
    /// An example of a model name, shown as a hint (models change often, so the user types the one they want).
    pub model_hint: &'static str,
}

pub const PRESETS: [Preset; 7] = [
    Preset { name: "Groq", kind: Kind::OpenAi, url: "https://api.groq.com/openai/v1", model_hint: "llama-3.3-70b-versatile" },
    Preset { name: "OpenAI", kind: Kind::OpenAi, url: "https://api.openai.com/v1", model_hint: "gpt-4o" },
    Preset { name: "DeepSeek", kind: Kind::OpenAi, url: "https://api.deepseek.com", model_hint: "deepseek-chat" },
    Preset { name: "Anthropic", kind: Kind::Anthropic, url: "https://api.anthropic.com", model_hint: "claude-sonnet-5-5" },
    Preset { name: "Ollama (on this computer)", kind: Kind::OpenAi, url: "http://localhost:11434/v1", model_hint: "llama3.1" },
    Preset { name: "LM Studio (on this computer)", kind: Kind::OpenAi, url: "http://localhost:1234/v1", model_hint: "the model loaded in LM Studio" },
    Preset { name: "Other (your own server)", kind: Kind::OpenAi, url: "", model_hint: "the model name" },
];

pub const CUSTOM: usize = PRESETS.len() - 1;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub preset: usize,
    pub kind: Kind,
    pub base_url: String,
    pub model: String,
    /// Use the model's tool calling. Off: the model answers with a JSON block instead.
    pub tools: bool,
    /// Send a picture of the canvas with each request (the model must understand images).
    pub send_image: bool,
    /// For models that think before they answer: "low", "medium" or "high" (empty: the server's default).
    pub reasoning: String,
}

impl Default for Config {
    fn default() -> Self {
        Self { preset: 0, kind: PRESETS[0].kind, base_url: PRESETS[0].url.into(), model: String::new(), tools: true, send_image: false, reasoning: String::new() }
    }
}

impl Config {
    fn path() -> Option<std::path::PathBuf> {
        std::env::var_os("APPDATA").map(|dir| std::path::PathBuf::from(dir).join("skribbl").join("ai.json"))
    }

    pub fn load() -> Self {
        let mut config: Self = Self::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        config.preset = config.preset.min(CUSTOM);
        config
    }

    pub fn save(&self) {
        if let Some(path) = Self::path() {
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
            if let Ok(text) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(path, text);
            }
        }
    }

    pub fn use_preset(&mut self, index: usize) {
        let preset = &PRESETS[index.min(CUSTOM)];
        self.preset = index.min(CUSTOM);
        self.kind = preset.kind;
        if index != CUSTOM {
            self.base_url = preset.url.into();
        }
        self.model.clear();
    }
}

// The API key lives in the Windows credential store, one per server address. It is never written to a file.

pub fn load_key(address: &str) -> String {
    keyring::Entry::new("skribbl", address.trim()).and_then(|e| e.get_password()).unwrap_or_default()
}

pub fn save_key(address: &str, key: &str) -> Result<(), String> {
    let entry = keyring::Entry::new("skribbl", address.trim()).map_err(|e| e.to_string())?;
    if key.trim().is_empty() { entry.delete_credential().or_else(|e| if matches!(e, keyring::Error::NoEntry) { Ok(()) } else { Err(e) }).map_err(|e| e.to_string()) } else { entry.set_password(key.trim()).map_err(|e| e.to_string()) }
}

// ---- the conversation ----

#[derive(Clone)]
pub struct Call {
    pub id: String,
    pub args: Value,
}

#[derive(Clone)]
pub enum Msg {
    /// `image` is a PNG, base64 encoded.
    User { text: String, image: Option<String> },
    Assistant { text: String, calls: Vec<Call> },
    ToolResult { id: String, text: String },
}

pub struct Reply {
    pub text: String,
    pub calls: Vec<Call>,
}

pub struct Request {
    pub config: Config,
    pub key: String,
    pub system: String,
    pub messages: Vec<Msg>,
}

const DRAW_DESCRIPTION: &str = "Draws on the canvas. Pass every command of your answer in the `commands` list, in the order they must run.";

fn draw_parameters() -> Value {
    let int = |what: &str| json!({"type": "integer", "description": what});
    json!({
        "type": "object",
        "properties": {
            "commands": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "tool": {"type": "string", "enum": ["pixel", "line", "rect", "ellipse", "fill", "clear", "set_palette", "new_canvas", "new_frame", "select_frame", "delete_frame"]},
                        "x": int("column of the pixel, or of the left side of a rect/ellipse box"),
                        "y": int("row of the pixel, or of the top side of a rect/ellipse box"),
                        "x1": int("start column of a line"), "y1": int("start row of a line"),
                        "x2": int("end column of a line"), "y2": int("end row of a line"),
                        "w": int("width of a rect/ellipse box"), "h": int("height of a rect/ellipse box"),
                        "width": int("width of a new canvas"), "height": int("height of a new canvas"),
                        "index": int("frame number, starting at 0"),
                        "color": {"type": "string", "description": "#rrggbb, #rrggbbaa or transparent"},
                        "colors": {"type": "array", "items": {"type": "string"}, "description": "the colours of a new palette"},
                        "filled": {"type": "boolean"},
                        "copy_current": {"type": "boolean", "description": "new_frame: start from a copy of the current frame"}
                    },
                    "required": ["tool"]
                }
            }
        },
        "required": ["commands"]
    })
}

pub fn system_prompt(tools: bool) -> String {
    let how = if tools {
        "Call the `draw` tool once with all the commands of your answer."
    } else {
        "Answer with ONE JSON object in a ```json block, like {\"commands\": [ ... ]}, and at most one short sentence before it."
    };
    format!(
        "You are the drawing engine of Skribbl, a pixel-art editor. You draw by giving commands.\n\
         \n\
         Coordinates: (0, 0) is the top-left pixel, x grows to the right, y grows downwards. Each message tells you the size of the canvas: stay inside it. \
         Colours are written #rrggbb, #rrggbbaa or transparent.\n\
         \n\
         Commands (use exactly these field names):\n\
         - {{\"tool\":\"pixel\",\"x\":0,\"y\":0,\"color\":\"#ff0000\"}}\n\
         - {{\"tool\":\"line\",\"x1\":0,\"y1\":0,\"x2\":9,\"y2\":4,\"color\":\"#ff0000\"}} (both ends included)\n\
         - {{\"tool\":\"rect\",\"x\":2,\"y\":2,\"w\":6,\"h\":4,\"color\":\"#ff0000\",\"filled\":true}} (x, y is the top-left corner)\n\
         - {{\"tool\":\"ellipse\",\"x\":2,\"y\":2,\"w\":6,\"h\":6,\"color\":\"#ff0000\",\"filled\":true}} (it fits in that box)\n\
         - {{\"tool\":\"fill\",\"x\":0,\"y\":0,\"color\":\"#87ceeb\"}} (paints the whole connected area that has the colour of that pixel)\n\
         - {{\"tool\":\"clear\"}} or {{\"tool\":\"clear\",\"color\":\"#000000\"}} (the current frame)\n\
         - {{\"tool\":\"set_palette\",\"colors\":[\"#000000\",\"#ffffff\"]}}\n\
         - {{\"tool\":\"new_canvas\",\"width\":32,\"height\":32}} (a new empty canvas, only when asked)\n\
         - {{\"tool\":\"new_frame\",\"copy_current\":true}} (adds a frame at the end and selects it)\n\
         - {{\"tool\":\"select_frame\",\"index\":0}} (frames are numbered from 0)\n\
         - {{\"tool\":\"delete_frame\",\"index\":0}}\n\
         \n\
         How to draw well:\n\
         - Plan before you draw: make the subject fill most of the canvas (leave a margin of 1 or 2 pixels) and centre it. A sword can be drawn on the diagonal.\n\
         - Work from the background to the details: fill the background first (only if asked, otherwise keep it transparent), then big shapes, then small details.\n\
         - Give every object a 1-pixel outline in a dark shade, fill it, add a lighter highlight on the side facing the light (top left) and a darker shade on the opposite side.\n\
         - Do not build everything from plain rectangles: use lines for diagonals, ellipses for round parts, and single pixels to round corners and add details.\n\
         - A good drawing takes 20 to 80 commands, not 5. Use a palette of 4 to 8 colours that go well together.\n\
         - Pixel art is about few pixels placed well. Use the symmetry of the subject.\n\
         - For an animation, draw the first frame, then add each next frame with new_frame (copy_current true) and change only what moves, in small steps.\n\
         - When asked to change the drawing, change it: do not redraw what stays the same.\n\
         - If the commands fail, you get the errors and the canvas back: fix them.\n\
         - If the request is impossible, say so in one sentence.\n\
         \n\
         {how}"
    )
}

// ---- describing the canvas ----

const SYMBOLS: &str = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ+-";

/// The canvas as text, so that even a model that cannot see images knows what is drawn.
pub fn describe(doc: &Document) -> String {
    let symbols: Vec<char> = SYMBOLS.chars().collect();
    let mut out = format!("Canvas: {} wide, {} high (x from 0 to {}, y from 0 to {}). ", doc.width, doc.height, doc.width - 1, doc.height - 1);
    out += &format!("Frames: {} (numbered from 0), the current one is {}.\n", doc.frames.len(), doc.current);
    let palette: Vec<String> = doc.palette.iter().enumerate().map(|(i, c)| format!("{}={}", symbols[i % symbols.len()], to_hex(*c))).collect();
    out += &format!("Palette: {}\n", palette.join(" "));
    let pixels = &doc.frame().pixels;
    if pixels.iter().all(|p| p[3] == 0) {
        out += "The current frame is empty (fully transparent).";
    } else if doc.width * doc.height <= 4096 {
        out += "Current frame, one character per pixel ('.' is transparent, '?' is a colour that is not in the palette):\n";
        for row in pixels.chunks(doc.width) {
            let line: String = row.iter().map(|p| if p[3] == 0 { '.' } else { doc.palette.iter().position(|c| c == p).map_or('?', |i| symbols[i % symbols.len()]) }).collect();
            out += &line;
            out.push('\n');
        }
    } else {
        out += "The current frame has drawings (the canvas is too big to be listed pixel by pixel).";
    }
    out
}

/// A picture of the current frame for models that can see: enlarged, over white, as a base64 PNG.
pub fn canvas_png_base64(doc: &Document) -> Option<String> {
    let scale = (256 / doc.width.max(doc.height)).max(1) as u32;
    let mut bytes = Vec::with_capacity(doc.width * doc.height * 4);
    for p in &doc.frame().pixels {
        let a = p[3] as u32;
        bytes.extend((0..3).map(|k| ((p[k] as u32 * a + 255 * (255 - a)) / 255) as u8));
        bytes.push(255);
    }
    let image = image::RgbaImage::from_raw(doc.width as u32, doc.height as u32, bytes)?;
    let image = image::imageops::resize(&image, doc.width as u32 * scale, doc.height as u32 * scale, image::imageops::FilterType::Nearest);
    let mut png = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(png))
}

// ---- reading what the AI answers ----

/// Finds the JSON in a text: in a ``` block if there is one, else from the first brace or bracket to the last.
pub fn extract_json(text: &str) -> Option<Value> {
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let body = &after[after.find('\n').map_or(0, |i| i + 1)..];
        let Some(end) = body.find("```") else { break };
        if let Ok(value) = serde_json::from_str(body[..end].trim()) {
            return Some(value);
        }
        rest = &body[end + 3..];
    }
    let start = text.find(['{', '['])?;
    let closer = if text[start..].starts_with('{') { '}' } else { ']' };
    let end = text.rfind(closer)?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&text[start..=end]).ok()
}

/// Models often write 3.0 for 3: turn the numbers that are whole into integers.
fn whole_numbers(value: &mut Value) {
    match value {
        Value::Number(n) => {
            if let Some(f) = n.as_f64().filter(|f| n.is_f64() && f.fract() == 0.0 && f.abs() < 1e9) {
                *value = json!(f as i64);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(whole_numbers),
        Value::Object(map) => map.values_mut().for_each(whole_numbers),
        _ => {}
    }
}

/// The commands in what the AI sent: `{"commands":[...]}`, a bare list, or one command. One bad command does not spoil the others.
pub fn commands_from(args: &Value) -> Vec<Result<Command, String>> {
    let mut value = match args {
        Value::String(text) => match extract_json(text) {
            Some(v) => v,
            None => return vec![Err("the arguments are not valid JSON".into())],
        },
        other => other.clone(),
    };
    whole_numbers(&mut value);
    let items = match value {
        Value::Array(items) => items,
        Value::Object(mut map) => match map.remove("commands") {
            Some(Value::Array(items)) => items,
            Some(_) => return vec![Err("`commands` must be a list".into())],
            None if map.contains_key("tool") => vec![Value::Object(map)],
            None => return vec![Err("no `commands` list found".into())],
        },
        _ => return vec![Err("expected a list of commands".into())],
    };
    items.into_iter().map(|item| serde_json::from_value::<Command>(item).map_err(|e| e.to_string())).collect()
}

// ---- the network ----

fn short(text: &str) -> String {
    let text = text.trim();
    match text.char_indices().nth(300) {
        Some((i, _)) => format!("{}...", &text[..i]),
        None => text.to_string(),
    }
}

fn error_message(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(body) else { return short(body) };
    v["error"]["message"].as_str().or_else(|| v["error"].as_str()).or_else(|| v["message"].as_str()).map_or_else(|| short(body), str::to_string)
}

fn post_json(url: &str, headers: &[(&str, String)], body: &Value) -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(180))).build().into();
    let mut request = agent.post(url);
    for (name, value) in headers {
        request = request.header(*name, value.as_str());
    }
    let mut response = request.send_json(body).map_err(|e| format!("could not reach {url}: {e}"))?;
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().map_err(|e| format!("could not read the answer: {e}"))?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {}", error_message(&text)));
    }
    serde_json::from_str(&text).map_err(|_| format!("the server did not answer with JSON: {}", short(&text)))
}

pub fn send(request: &Request) -> Result<Reply, String> {
    let config = &request.config;
    if config.base_url.trim().is_empty() {
        return Err("enter the address of the server in the connection settings".into());
    }
    if config.model.trim().is_empty() {
        return Err("enter the name of the model in the connection settings".into());
    }
    let result = match config.kind {
        Kind::OpenAi => send_openai(request),
        Kind::Anthropic => {
            if request.key.trim().is_empty() {
                return Err("paste your API key in the connection settings".into());
            }
            send_anthropic(request)
        }
    };
    let sent_image = request.messages.iter().any(|m| matches!(m, Msg::User { image: Some(_), .. }));
    result.map_err(|e| if sent_image && e.starts_with("HTTP 400") { format!("{e}\n(A picture of the canvas was sent: if this model cannot see images, untick \"Send a picture of the canvas\" in the connection settings.)") } else { e })
}

fn send_openai(request: &Request) -> Result<Reply, String> {
    let config = &request.config;
    let mut messages = vec![json!({"role": "system", "content": request.system})];
    for message in &request.messages {
        messages.push(match message {
            Msg::User { text, image: Some(image) } => json!({"role": "user", "content": [
                {"type": "text", "text": text},
                {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{image}")}}
            ]}),
            Msg::User { text, image: None } => json!({"role": "user", "content": text}),
            Msg::Assistant { text, calls } => {
                // always a string: Groq refuses a null content, even next to tool calls
                let mut m = json!({"role": "assistant", "content": text});
                if !calls.is_empty() {
                    m["tool_calls"] = calls.iter().map(|c| json!({"id": c.id, "type": "function", "function": {"name": "draw", "arguments": c.args.to_string()}})).collect();
                }
                m
            }
            Msg::ToolResult { id, text } => json!({"role": "tool", "tool_call_id": id, "content": text}),
        });
    }
    let mut body = json!({"model": config.model.trim(), "messages": messages});
    if config.tools {
        body["tools"] = json!([{"type": "function", "function": {"name": "draw", "description": DRAW_DESCRIPTION, "parameters": draw_parameters()}}]);
        body["tool_choice"] = json!("auto");
    }
    if !config.reasoning.is_empty() {
        body["reasoning_effort"] = json!(config.reasoning);
    }
    let mut headers = Vec::new();
    if !request.key.trim().is_empty() {
        headers.push(("Authorization", format!("Bearer {}", request.key.trim())));
    }
    let url = format!("{}/chat/completions", config.base_url.trim().trim_end_matches('/'));
    let answer = post_json(&url, &headers, &body)?;
    let message = &answer["choices"][0]["message"];
    if message.is_null() {
        return Err(format!("unexpected answer from the server: {}", short(&answer.to_string())));
    }
    let calls: Vec<Call> = message["tool_calls"]
        .as_array()
        .map(|list| {
            list.iter()
                .enumerate()
                .map(|(i, call)| {
                    let raw = &call["function"]["arguments"];
                    let args = match raw.as_str() {
                        Some(text) => serde_json::from_str(text).unwrap_or_else(|_| json!(text)),
                        None => raw.clone(),
                    };
                    Call { id: call["id"].as_str().map_or_else(|| format!("call_{i}"), str::to_string), args }
                })
                .collect()
        })
        .unwrap_or_default();
    let text = message["content"].as_str().unwrap_or_default().to_string();
    if calls.is_empty() && text.trim().is_empty() && answer["choices"][0]["finish_reason"] == "length" {
        return Err("the answer was cut short: the model used all its tokens (often on its own reasoning). Ask for something simpler, or try another model.".into());
    }
    Ok(Reply { text, calls })
}

fn send_anthropic(request: &Request) -> Result<Reply, String> {
    let config = &request.config;
    let mut messages: Vec<Value> = Vec::new();
    // Anthropic wants user and assistant turns to alternate: blocks of the same role are merged.
    let mut push = |role: &str, blocks: Vec<Value>| match messages.last_mut() {
        Some(last) if last["role"] == role => last["content"].as_array_mut().unwrap().extend(blocks),
        _ => messages.push(json!({"role": role, "content": blocks})),
    };
    for message in &request.messages {
        match message {
            Msg::User { text, image } => {
                let mut blocks = Vec::new();
                if let Some(image) = image {
                    blocks.push(json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": image}}));
                }
                blocks.push(json!({"type": "text", "text": text}));
                push("user", blocks);
            }
            Msg::Assistant { text, calls } => {
                let mut blocks = Vec::new();
                if !text.trim().is_empty() {
                    blocks.push(json!({"type": "text", "text": text}));
                }
                for call in calls {
                    let input = if call.args.is_object() { call.args.clone() } else { json!({}) };
                    blocks.push(json!({"type": "tool_use", "id": call.id, "name": "draw", "input": input}));
                }
                if blocks.is_empty() {
                    blocks.push(json!({"type": "text", "text": "(no answer)"}));
                }
                push("assistant", blocks);
            }
            Msg::ToolResult { id, text } => push("user", vec![json!({"type": "tool_result", "tool_use_id": id, "content": text})]),
        }
    }
    let mut body = json!({"model": config.model.trim(), "max_tokens": 8192, "system": request.system, "messages": messages});
    if config.tools {
        body["tools"] = json!([{"name": "draw", "description": DRAW_DESCRIPTION, "input_schema": draw_parameters()}]);
    }
    let headers = [("x-api-key", request.key.trim().to_string()), ("anthropic-version", "2023-06-01".to_string())];
    let base = config.base_url.trim().trim_end_matches('/');
    let url = if base.ends_with("/v1") { format!("{base}/messages") } else { format!("{base}/v1/messages") };
    let answer = post_json(&url, &headers, &body)?;
    let blocks = answer["content"].as_array().ok_or_else(|| format!("unexpected answer from the server: {}", short(&answer.to_string())))?;
    let mut reply = Reply { text: String::new(), calls: Vec::new() };
    for block in blocks {
        match block["type"].as_str() {
            Some("text") => reply.text += block["text"].as_str().unwrap_or_default(),
            Some("tool_use") => reply.calls.push(Call { id: block["id"].as_str().unwrap_or("call").to_string(), args: block["input"].clone() }),
            _ => {}
        }
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::TRANSPARENT;

    #[test]
    fn json_is_found_in_a_block_or_in_plain_text() {
        let fenced = "Here you go:\n```json\n{\"commands\":[{\"tool\":\"clear\"}]}\n```\nEnjoy!";
        assert_eq!(extract_json(fenced).unwrap()["commands"][0]["tool"], "clear");
        let bare = "Sure! {\"commands\":[{\"tool\":\"clear\"}]} done";
        assert!(extract_json(bare).is_some());
        let list = "[{\"tool\":\"clear\"}]";
        assert!(extract_json(list).unwrap().is_array());
        assert!(extract_json("no json here").is_none());
        let unmarked = "```\n[{\"tool\":\"clear\"}]\n```";
        assert!(extract_json(unmarked).is_some());
    }

    #[test]
    fn commands_are_read_from_every_shape_the_ai_may_use() {
        let object = json!({"commands": [{"tool": "pixel", "x": 1, "y": 2, "color": "#fff"}, {"tool": "clear"}]});
        assert!(commands_from(&object).iter().all(|c| c.is_ok()));
        assert_eq!(commands_from(&json!([{"tool": "clear"}])).len(), 1);
        assert!(commands_from(&json!({"tool": "clear"}))[0].is_ok());
        let text = json!("```json\n{\"commands\":[{\"tool\":\"clear\"}]}\n```");
        assert!(commands_from(&text)[0].is_ok());
        assert!(commands_from(&json!(42))[0].is_err());
        assert!(commands_from(&json!({"nothing": 1}))[0].is_err());
    }

    #[test]
    fn one_bad_command_does_not_spoil_the_others() {
        let args = json!({"commands": [{"tool": "pixel", "x": 1, "y": 1, "color": "#fff"}, {"tool": "explode"}, {"tool": "pixel", "x": 2}]});
        let parsed = commands_from(&args);
        assert!(parsed[0].is_ok());
        assert!(parsed[1].as_ref().unwrap_err().contains("explode"), "{:?}", parsed[1]);
        assert!(parsed[2].is_err(), "a missing field is an error");
    }

    #[test]
    fn whole_floats_are_accepted_as_integers() {
        let args = json!({"commands": [{"tool": "line", "x1": 0.0, "y1": 1.0, "x2": 5.0, "y2": 5.0, "color": "#fff"}]});
        assert!(commands_from(&args)[0].is_ok());
        let fractional = json!({"commands": [{"tool": "pixel", "x": 1.5, "y": 1, "color": "#fff"}]});
        assert!(commands_from(&fractional)[0].is_err());
    }

    #[test]
    fn the_canvas_is_described_for_the_ai() {
        let mut doc = Document::new(4, 3);
        assert!(describe(&doc).contains("empty"));
        doc.set(1, 1, doc.palette[8]);
        doc.set(0, 0, [1, 2, 3, 255]);
        let text = describe(&doc);
        assert!(text.contains("4 wide, 3 high"), "{text}");
        assert!(text.contains("8=#ff004d"), "{text}");
        assert!(text.contains("?...\n.8..\n....\n"), "{text}");
        assert_eq!(doc.get(2, 2), Some(TRANSPARENT));
    }

    #[test]
    fn the_canvas_picture_is_a_png() {
        let doc = Document::new(8, 8);
        let data = base64::engine::general_purpose::STANDARD.decode(canvas_png_base64(&doc).unwrap()).unwrap();
        assert_eq!(&data[1..4], b"PNG");
    }

    #[test]
    fn the_prompt_depends_on_how_the_model_answers() {
        assert!(system_prompt(true).contains("`draw` tool"));
        assert!(system_prompt(false).contains("```json"));
    }
}
