//! Bounded inverse-command undo shared by annotation documents. The inverse
//! of an applied command goes on the undo stack; undoing moves the inverse of
//! the inverse to redo. New edits clear redo.

/// Most recent edits kept per stack.
pub const LIMIT: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Do,
    Undo,
    Redo,
}

/// Record `inverse` for a command applied as `step`.
pub fn record<C>(undo: &mut Vec<C>, redo: &mut Vec<C>, inverse: C, step: Step) {
    match step {
        Step::Undo => redo.push(inverse),
        Step::Redo => undo.push(inverse),
        Step::Do => {
            undo.push(inverse);
            redo.clear();
        }
    }
    if undo.len() > LIMIT {
        undo.remove(0);
    }
    if redo.len() > LIMIT {
        redo.remove(0);
    }
}

/// Stored stacks must be bounded and hold no navigation or history commands.
pub fn validate<C>(undo: &[C], redo: &[C], forbidden: impl Fn(&C) -> bool) -> Result<(), String> {
    if undo.len() > LIMIT || redo.len() > LIMIT || undo.iter().chain(redo).any(forbidden) {
        return Err("invalid annotation history".into());
    }
    Ok(())
}
