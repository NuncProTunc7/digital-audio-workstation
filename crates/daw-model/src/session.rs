use crate::command::{Command, CommandError};
use crate::project::Project;

/// An open project plus its undo/redo history.
#[derive(Debug, Default)]
pub struct Session {
    project: Project,
    undo_stack: Vec<Command>,
    redo_stack: Vec<Command>,
}

impl Session {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Applies a Command and records it for undo. A new edit clears redo.
    pub fn execute(&mut self, command: Command) -> Result<(), CommandError> {
        let inverse = command.apply(&mut self.project)?;
        self.undo_stack.push(inverse);
        self.redo_stack.clear();
        Ok(())
    }

    /// Reverts the most recent edit. Returns `false` if there was nothing to undo.
    pub fn undo(&mut self) -> bool {
        Self::step(
            &mut self.project,
            &mut self.undo_stack,
            &mut self.redo_stack,
        )
    }

    /// Re-applies the most recently undone edit. Returns `false` if there was nothing to redo.
    pub fn redo(&mut self) -> bool {
        Self::step(
            &mut self.project,
            &mut self.redo_stack,
            &mut self.undo_stack,
        )
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    fn step(project: &mut Project, from: &mut Vec<Command>, to: &mut Vec<Command>) -> bool {
        let Some(command) = from.pop() else {
            return false;
        };
        // Inverses were produced by successful applies, so they are valid by
        // construction. If one ever fails, drop it rather than corrupt history.
        match command.apply(project) {
            Ok(inverse) => {
                to.push(inverse);
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

    #[test]
    fn rejected_edit_is_not_recorded() {
        let mut session = Session::default();
        assert!(session.execute(Command::SetTempo { bpm: 0.0 }).is_err());
        assert!(!session.can_undo());
    }
}
