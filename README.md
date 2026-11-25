# Skribbl

A pixel-art editor, written in Rust, where you can connect an AI and **give it orders**: draw a sword, animate a bouncing ball, change what is on the canvas. You can also turn any picture into pixel art, on your computer, without any AI.

The AI does not paint pixels by magic: it uses the **same drawing commands as the editor** (pixel, line, rectangle, ellipse, fill, frames...). So everything it does can be read, replayed and undone like anything you do yourself: one answer of the AI is one Ctrl+Z.

> [!NOTE]
> Skribbl works with the AI service **you** choose, with **your** API key. It is not affiliated with any of them.

## Features

**Editor**
- Pencil, eraser, line, rectangle, ellipse (outlined or filled), fill, colour picker
- The pencil and the eraser each have their own size (double-click the tool, or `[` and `]`)
- Zoom on the pointer, pan with the middle button, pixel grid, transparency checkerboard
- Palette (PICO-8 by default), any colour, add colours to the palette
- Undo and redo (100 steps), a brush stroke is one step
- Canvases from 1x1 to 256x256

**Animation**
- Frames: new, duplicate, delete, select; playback at 1 to 30 fps (`Space`)
- Onion skin: the previous frame in red and the next one in green, behind the current one
- Export the current frame (PNG), a spritesheet (PNG) or the animation (GIF, looping, with transparency), with a scale up to 32x

**AI panel**
- One adapter for everything that speaks the OpenAI "chat completions" API: **OpenAI, Groq, DeepSeek, Ollama, LM Studio**, or any server of your own; one adapter for **Anthropic**
- The AI receives the size of the canvas, the palette and the current frame **as text** (one character per pixel), so even a model that cannot see images knows what is drawn. A picture of the canvas can be sent too, for models that can see
- If some commands fail (a pixel outside the canvas, a wrong colour), the others still run, and the AI gets the errors and the canvas back to fix them (twice at most)
- Models that cannot call tools can answer with a JSON block instead: untick "The model can call tools"
- A "Reasoning effort" setting for models that think before they answer
- "Improve" asks the AI to refine the current drawing (outline, shading, details)

**Import an image**
- Shrinks a picture (PNG, JPG, BMP, GIF) to the size you want, picks the N best colours (median cut), or uses your palette, or keeps all the colours; optional Floyd-Steinberg dithering; live preview

**Projects**
- Save and open `.skb` files (the whole animation, the palette and the speed), with Ctrl+S / Ctrl+O, a confirmation before unsaved work is thrown away, and writes that cannot leave a half-written file

## Connecting an AI

1. Open a drawing, click **AI**, open **Connection**.
2. Choose a provider. The address is filled in for you. Type the **name of the model** yourself (model names change often, so none is hard-coded; the grey text suggests one).
3. Paste your API key and click **Save the key**. It goes into the **Windows credential store**, one per server address, and is never written to a file. A local server (Ollama, LM Studio) needs no key.
4. Ask for something: *"a red mushroom with white dots"*. `Ctrl+Enter` sends.

Tips:
- Small canvases (16 to 64 pixels) work best. Pixel art is few pixels placed well, and the AI places them with commands.
- Several short requests (then **Improve**) work better than one long, demanding prompt, especially with a model that spends its tokens on reasoning.
- Free plans have token limits per minute. If the answer is "cut short", set **Reasoning effort** to `low`, start a **New chat** (the whole conversation is sent each time) and try again.

## Build

You need [Rust](https://rustup.rs). Then:

```
cargo run --release
```

## Keys

| | |
|---|---|
| `B` `E` `L` `R` `O` `G` `I` | pencil, eraser, line, rectangle, ellipse, fill, picker (`Shift+R`, `Shift+O`: filled) |
| `[` `]` | smaller, bigger pencil or eraser |
| `Space` | play or pause the animation |
| `Ctrl+Z`, `Ctrl+Y` | undo, redo |
| `Ctrl+S`, `Ctrl+Shift+S`, `Ctrl+O` | save, save as, open |

## Honest limits

- **Windows only for now**: the API key is kept with the Windows credential store.
- The AI part has been tried for real **only with Groq** (`openai/gpt-oss-120b`). The adapters for OpenAI, DeepSeek, Anthropic, Ollama and LM Studio are written against their documentation and covered by tests of what Skribbl sends and reads, but not tried against the live services.
- The AI is not good at drawing. It draws what it can describe with simple shapes; do not expect a masterpiece, and expect better results from bigger models.
- The conversation with the AI is not saved with the drawing.
- Frames cannot be reordered, and there are no layers or selections yet.

## How it is built

| | |
|---|---|
| `doc.rs`, `draw.rs` | the drawing (frames of RGBA pixels, palette) and the shape algorithms |
| `command.rs`, `editor.rs` | the commands shared by the editor and the AI; the undo history |
| `ai.rs`, `ai_panel.rs` | the two adapters, the prompt, reading what the AI answers, the panel |
| `pixelize.rs`, `import_window.rs` | picture to pixel art (median cut, dithering) |
| `project.rs` | the `.skb` format |
| `main.rs` | the window (egui) |

```
cargo test
```

## License

MIT, see [LICENSE](LICENSE).
