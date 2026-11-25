//! The editor: a document plus its undo history. Everything that changes the drawing goes through here.

use crate::command::{Command, execute};
use crate::doc::{Color, Document, Frame};

const HISTORY: usize = 100;

#[derive(Clone)]
enum Snapshot {
    /// One frame as it was (most edits only touch the current frame: cheap).
    Frame { index: usize, frame: Frame },
    /// The whole document (for edits that add or remove frames, change the palette or the size).
    Whole(Document),
}

pub struct Editor {
    pub doc: Document,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Goes up with every change, so the app can tell whether the drawing differs from the saved one.
    version: u64,
}

impl Editor {
    pub fn new(width: usize, height: usize) -> Self {
        Self::from_document(Document::new(width, height))
    }

    pub fn from_document(doc: Document) -> Self {
        Self { doc, undo: Vec::new(), redo: Vec::new(), version: 0 }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    fn frame_snapshot(&self) -> Snapshot {
        Snapshot::Frame { index: self.doc.current, frame: self.doc.frame().clone() }
    }

    fn remember(&mut self, snapshot: Snapshot) {
        self.undo.push(snapshot);
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.version += 1;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Marks the start of a brush stroke: all the pixels painted after this are one undo step.
    pub fn begin_stroke(&mut self) {
        let snapshot = self.frame_snapshot();
        self.remember(snapshot);
    }

    /// Runs several commands as ONE undo step (an AI answer is one step, however many commands it holds).
    /// Returns what happened to each. If none of them changed anything, nothing is added to the history.
    pub fn apply_batch(&mut self, commands: &[Command]) -> Vec<Result<String, String>> {
        let whole = commands.iter().any(|c| c.is_structural() || matches!(c, Command::SelectFrame { .. }));
        let before = if whole { Snapshot::Whole(self.doc.clone()) } else { self.frame_snapshot() };
        let results: Vec<_> = commands.iter().map(|c| execute(&mut self.doc, c)).collect();
        let changed = commands.iter().zip(&results).any(|(c, r)| c.edits() && r.is_ok());
        if changed {
            self.remember(before);
        }
        results
    }

    /// Replaces the whole drawing by one frame (an imported picture). One undo step brings the old drawing back.
    pub fn replace_drawing(&mut self, width: usize, height: usize, pixels: Vec<Color>, palette: Option<Vec<Color>>) {
        let before = Snapshot::Whole(self.doc.clone());
        let mut doc = Document::new(width, height);
        doc.palette = palette.unwrap_or_else(|| self.doc.palette.clone());
        doc.frames[0].pixels = pixels;
        self.doc = doc;
        self.remember(before);
    }

    pub fn apply(&mut self, command: &Command) -> Result<String, String> {
        self.apply_batch(std::slice::from_ref(command)).remove(0)
    }

    /// Puts back what a snapshot holds and returns a snapshot of what was there, to go the other way.
    fn swap(&mut self, snapshot: Snapshot) -> Snapshot {
        match snapshot {
            Snapshot::Frame { index, frame } => {
                let index = index.min(self.doc.frames.len() - 1);
                let current = Snapshot::Frame { index, frame: std::mem::replace(&mut self.doc.frames[index], frame) };
                self.doc.current = index;
                current
            }
            Snapshot::Whole(doc) => Snapshot::Whole(std::mem::replace(&mut self.doc, doc)),
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else { return false };
        let other = self.swap(snapshot);
        self.redo.push(other);
        self.version += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else { return false };
        let other = self.swap(snapshot);
        self.undo.push(other);
        self.version += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::TRANSPARENT;

    fn pixel(x: i32, y: i32) -> Command {
        Command::Pixel { x, y, color: "#ff0000".into() }
    }

    const RED: [u8; 4] = [255, 0, 0, 255];

    #[test]
    fn undo_and_redo_walk_through_the_edits() {
        let mut editor = Editor::new(4, 4);
        editor.apply(&pixel(0, 0)).unwrap();
        editor.apply(&pixel(1, 1)).unwrap();
        assert!(editor.undo());
        assert_eq!(editor.doc.get(1, 1), Some(TRANSPARENT));
        assert_eq!(editor.doc.get(0, 0), Some(RED));
        assert!(editor.undo());
        assert!(!editor.undo(), "nothing left to undo");
        assert_eq!(editor.doc.get(0, 0), Some(TRANSPARENT));
        assert!(editor.redo() && editor.redo());
        assert!(!editor.redo());
        assert_eq!((editor.doc.get(0, 0), editor.doc.get(1, 1)), (Some(RED), Some(RED)));
    }

    #[test]
    fn a_new_edit_forgets_the_redo_history() {
        let mut editor = Editor::new(4, 4);
        editor.apply(&pixel(0, 0)).unwrap();
        editor.undo();
        assert!(editor.can_redo());
        editor.apply(&pixel(2, 2)).unwrap();
        assert!(!editor.can_redo());
    }

    #[test]
    fn a_stroke_is_one_undo_step() {
        let mut editor = Editor::new(8, 8);
        editor.begin_stroke();
        for x in 0..5 {
            editor.doc.set(x, 0, RED);
        }
        assert!(editor.undo());
        assert!((0..5).all(|x| editor.doc.get(x, 0) == Some(TRANSPARENT)));
        assert!(!editor.can_undo());
    }

    #[test]
    fn an_ai_answer_with_many_commands_is_one_undo_step() {
        let mut editor = Editor::new(8, 8);
        let results = editor.apply_batch(&[pixel(0, 0), pixel(1, 0), pixel(99, 99), pixel(2, 0)]);
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 3);
        assert!(results[2].is_err(), "the bad command is reported, the others still ran");
        assert!(editor.undo());
        assert!((0..3).all(|x| editor.doc.get(x, 0) == Some(TRANSPARENT)));
        assert!(!editor.can_undo());
    }

    #[test]
    fn commands_that_fail_leave_no_history() {
        let mut editor = Editor::new(4, 4);
        assert!(editor.apply(&pixel(50, 50)).is_err());
        assert!(!editor.can_undo());
        // choosing a frame is not an edit either
        editor.apply(&Command::NewFrame { copy_current: false }).unwrap();
        let steps_before = editor.undo.len();
        editor.apply(&Command::SelectFrame { index: 0 }).unwrap();
        assert_eq!(editor.undo.len(), steps_before);
    }

    #[test]
    fn adding_a_frame_can_be_undone() {
        let mut editor = Editor::new(4, 4);
        editor.apply(&pixel(0, 0)).unwrap();
        editor.apply(&Command::NewFrame { copy_current: true }).unwrap();
        editor.apply(&pixel(3, 3)).unwrap();
        assert_eq!(editor.doc.frames.len(), 2);
        editor.undo(); // the pixel on frame 1
        assert_eq!(editor.doc.get(3, 3), Some(TRANSPARENT));
        editor.undo(); // the new frame
        assert_eq!((editor.doc.frames.len(), editor.doc.current), (1, 0));
        assert!(editor.redo() && editor.redo());
        assert_eq!((editor.doc.frames.len(), editor.doc.current, editor.doc.get(3, 3)), (2, 1, Some(RED)));
    }

    #[test]
    fn a_new_canvas_can_be_undone_with_its_drawing_back() {
        let mut editor = Editor::new(4, 4);
        editor.apply(&pixel(1, 1)).unwrap();
        editor.apply(&Command::NewCanvas { width: 10, height: 10 }).unwrap();
        assert_eq!(editor.doc.width, 10);
        editor.undo();
        assert_eq!((editor.doc.width, editor.doc.get(1, 1)), (4, Some(RED)));
    }

    #[test]
    fn an_imported_picture_replaces_the_drawing_and_can_be_undone() {
        let mut editor = Editor::new(4, 4);
        editor.apply(&pixel(1, 1)).unwrap();
        editor.apply(&Command::NewFrame { copy_current: false }).unwrap();
        editor.replace_drawing(2, 2, vec![RED; 4], Some(vec![RED]));
        assert_eq!((editor.doc.width, editor.doc.frames.len(), editor.doc.palette.len()), (2, 1, 1));
        editor.undo();
        assert_eq!((editor.doc.width, editor.doc.frames.len(), editor.doc.get(1, 1)), (4, 2, Some(TRANSPARENT)));
        editor.undo();
        assert_eq!((editor.doc.frames.len(), editor.doc.get(1, 1)), (1, Some(RED)), "the first drawing is still there");
        editor.redo();
        editor.redo();
        assert_eq!((editor.doc.width, editor.doc.frames.len()), (2, 1));
    }

    #[test]
    fn the_history_is_bounded() {
        let mut editor = Editor::new(4, 4);
        for i in 0..(HISTORY + 20) as i32 {
            editor.apply(&pixel(i % 4, (i / 4) % 4)).unwrap();
        }
        assert_eq!(editor.undo.len(), HISTORY);
    }
}
