use crate::command::{Command, CommandError};
use crate::project::Project;

/// An open project plus its undo/redo history.
#[derive(Debug, Default)]
pub struct Session {
    project: Project,
    undo_stack: Vec<Command>,
    redo_stack: Vec<Command>,
    // The most recent Command executed, for merging slider drags.
    last_executed: Option<Command>,
    // Bumped on every change; compared with `saved_revision` for "unsaved".
    revision: u64,
    saved_revision: u64,
}

impl Session {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_executed: None,
            revision: 0,
            saved_revision: 0,
        }
    }

    /// Replaces the open project (New / Open) and clears history.
    pub fn replace_project(&mut self, project: Project) {
        *self = Session::new(project);
    }

    /// True when there are changes since the last save (or since opening).
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// Records that the current state has been saved.
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }

    /// Marks the project as having unsaved changes (e.g. work recovered
    /// after a crash), until the next save.
    pub fn mark_unsaved(&mut self) {
        self.saved_revision = u64::MAX;
    }

    /// Changes whenever the project does, so callers can tell whether it
    /// changed since they last looked.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Applies a Command and records it for undo. A new edit clears redo.
    ///
    /// Repeated edits of the same thing (one slider being dragged) merge into
    /// a single undo step that restores the value from before the drag.
    pub fn execute(&mut self, command: Command) -> Result<(), CommandError> {
        let merge = self.redo_stack.is_empty()
            && self
                .last_executed
                .as_ref()
                .is_some_and(|last| last.coalesces_with(&command));
        let applied = command.clone();
        let inverse = command.apply(&mut self.project)?;
        if !merge {
            self.undo_stack.push(inverse);
        }
        self.redo_stack.clear();
        self.last_executed = Some(applied);
        self.revision += 1;
        Ok(())
    }

    /// Ends the current merge window, so the next edit gets its own undo
    /// step even if it touches the same parameter. Call when a drag ends.
    pub fn end_gesture(&mut self) {
        self.last_executed = None;
    }

    /// Reverts the most recent edit. Returns `false` if there was nothing to undo.
    pub fn undo(&mut self) -> bool {
        self.last_executed = None;
        Self::step(
            &mut self.project,
            &mut self.undo_stack,
            &mut self.redo_stack,
            &mut self.revision,
        )
    }

    /// Re-applies the most recently undone edit. Returns `false` if there was nothing to redo.
    pub fn redo(&mut self) -> bool {
        self.last_executed = None;
        Self::step(
            &mut self.project,
            &mut self.redo_stack,
            &mut self.undo_stack,
            &mut self.revision,
        )
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    fn step(
        project: &mut Project,
        from: &mut Vec<Command>,
        to: &mut Vec<Command>,
        revision: &mut u64,
    ) -> bool {
        let Some(command) = from.pop() else {
            return false;
        };
        // Inverses were produced by successful applies, so they are valid by
        // construction. If one ever fails, drop it rather than corrupt history.
        match command.apply(project) {
            Ok(inverse) => {
                to.push(inverse);
                *revision += 1;
                true
            }
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_walks_history() {
        let mut session = Session::default();
        session
            .execute(Command::SetTempo { bpm: 90.0 })
            .expect("ok");
        session
            .execute(Command::SetTempo { bpm: 150.0 })
            .expect("ok");

        assert!(session.undo());
        assert_eq!(session.project().tempo_bpm, 90.0);
        assert!(session.undo());
        assert_eq!(session.project().tempo_bpm, 120.0);
        assert!(!session.undo());

        assert!(session.redo());
        assert_eq!(session.project().tempo_bpm, 90.0);
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut session = Session::default();
        session
            .execute(Command::SetTempo { bpm: 90.0 })
            .expect("ok");
        session.undo();
        session
            .execute(Command::SetTempo { bpm: 100.0 })
            .expect("ok");
        assert!(!session.can_redo());
    }

    fn cutoff(value: f64) -> Command {
        Command::SetInstrumentParam {
            track_id: 1,
            param: "filter.cutoff_hz".into(),
            value,
        }
    }

    #[test]
    fn slider_drag_is_one_undo_step() {
        let mut session = Session::default();
        let original = session.project().clone();
        for v in [500.0, 600.0, 700.0, 800.0] {
            session.execute(cutoff(v)).expect("ok");
        }
        assert!(session.undo());
        assert_eq!(session.project(), &original);
        assert!(!session.can_undo());
    }

    #[test]
    fn end_gesture_separates_undo_steps() {
        let mut session = Session::default();
        session.execute(cutoff(500.0)).expect("ok");
        session.end_gesture();
        session.execute(cutoff(900.0)).expect("ok");
        session.undo();
        let value = session
            .project()
            .track(1)
            .expect("track")
            .instrument
            .value("filter.cutoff_hz");
        assert_eq!(value, Some(500.0));
    }

    #[test]
    fn different_params_are_separate_steps() {
        let mut session = Session::default();
        session.execute(cutoff(500.0)).expect("ok");
        session
            .execute(Command::SetInstrumentParam {
                track_id: 1,
                param: "filter.resonance".into(),
                value: 0.9,
            })
            .expect("ok");
        session.undo();
        let inst = &session.project().track(1).expect("track").instrument;
        assert_eq!(inst.value("filter.cutoff_hz"), Some(500.0));
    }

    #[test]
    fn rejected_edit_is_not_recorded() {
        let mut session = Session::default();
        assert!(session.execute(Command::SetTempo { bpm: 0.0 }).is_err());
        assert!(!session.can_undo());
    }
}
