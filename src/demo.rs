//! A small example animation, built with the same commands the AI will use: a ball bouncing on the floor.

use crate::command::Command;
use crate::editor::Editor;

/// How high the ball is above the floor in each of the 8 frames.
const LIFT: [i32; 8] = [0, 6, 11, 14, 14, 11, 6, 0];

pub fn bouncing_ball() -> Editor {
    let mut editor = Editor::new(32, 32);
    let mut commands = Vec::new();
    for (i, lift) in LIFT.iter().enumerate() {
        if i > 0 {
            commands.push(Command::NewFrame { copy_current: false });
        }
        commands.push(Command::Rect { x: 0, y: 28, w: 32, h: 4, color: "#5f574f".into(), filled: true });
        let shadow = 12 - lift / 2;
        commands.push(Command::Ellipse { x: 16 - shadow / 2, y: 28, w: shadow, h: 2, color: "#1d2b53".into(), filled: true });
        // on the floor the ball is squashed
        let (x, y, w, h) = if *lift == 0 { (11, 21, 10, 7) } else { (12, 20 - lift, 8, 8) };
        commands.push(Command::Ellipse { x, y, w, h, color: "#ff004d".into(), filled: true });
        commands.push(Command::Pixel { x: x + 2, y: y + 1, color: "#ffccaa".into() });
        commands.push(Command::Pixel { x: x + 3, y: y + 1, color: "#ffccaa".into() });
    }
    commands.push(Command::SelectFrame { index: 0 });
    editor.apply_batch(&commands);
    editor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_is_eight_frames_and_one_undo_step() {
        let mut editor = bouncing_ball();
        assert_eq!((editor.doc.frames.len(), editor.doc.current), (8, 0));
        assert!(editor.doc.frames.iter().all(|f| f.pixels.iter().any(|p| p[3] > 0)), "no frame is empty");
        assert_ne!(editor.doc.frames[0], editor.doc.frames[3], "the ball moves");
        assert!(editor.undo());
        assert_eq!(editor.doc.frames.len(), 1);
    }
}
