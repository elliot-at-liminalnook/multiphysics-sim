//! Lesson edits through the shared command layer, and the page's drafts
//! (a note draft is posted by `threads`).
use super::*;

impl Learn {
    fn lesson_store(&self) -> Option<LessonStore> {
        self.lesson.as_ref().map(|l| LessonStore::new(&l.path))
    }

    /// Apply a lesson edit through the shared command layer.
    pub fn edit(&mut self, label: &str, edit: Edit, expected: Option<&str>) -> Result<sim_lesson::edit::Applied, String> {
        let store = self.lesson_store().ok_or("no lesson is open")?;
        let (lesson, applied) = store.apply(label, edit, expected)?;
        self.after_edit(lesson);
        self.status = format!("{label} · saved");
        Ok(applied)
    }
    pub fn undo(&mut self, redo: bool) -> Result<sim_lesson::edit::Applied, String> {
        let store = self.lesson_store().ok_or("no lesson is open")?;
        let (lesson, applied) = if redo { store.redo()? } else { store.undo()? };
        self.after_edit(lesson);
        self.status = format!("{} {}", if redo { "Redid" } else { "Undid" }, applied.label);
        Ok(applied)
    }
    fn after_edit(&mut self, lesson: Lesson) {
        let path = lesson.path.clone();
        let scene_changed = self.scene.as_ref().is_some_and(|a| lesson.scene(&a.id) != Some(&a.scene));
        self.lesson = Some(lesson);
        self.load(&path);
        if scene_changed {
            if let Some(id) = self.scene.as_ref().map(|a| a.id.clone()) {
                if self.lesson.as_ref().is_some_and(|l| l.scene(&id).is_some()) {
                    self.activate(&id);
                } else {
                    self.scene = None;
                }
            }
        }
    }

    pub(super) fn submit_draft(&mut self) -> Result<(), String> {
        let input = self.input.clone().ok_or("nothing to post")?;
        let body = input.buffer.trim().to_string();
        match input.purpose {
            Purpose::Author => {
                if !body.is_empty() {
                    self.author = body.chars().take(120).collect();
                }
                self.input = None;
                return Ok(());
            }
            Purpose::Block(block, hash) => {
                self.edit("Edit block", Edit::ReplaceBlock { block, hash, text: input.buffer.clone() }, None)?;
                self.input = None;
                return Ok(());
            }
            Purpose::NewBlock(after) => {
                self.edit("Add block", Edit::InsertAfter { block: after, text: input.buffer.clone() }, None)?;
                self.input = None;
                return Ok(());
            }
            Purpose::QuizAnswer(id) => return self.practice(LessonAction::QuizCheck(id)),
            Purpose::StepAnswer(id, i) => {
                self.step_text.insert((id.clone(), i), input.buffer.trim().to_string());
                self.input = None;
                // Enter moves to the next blank step, then checks.
                let next = self.lesson.as_ref().and_then(|l| l.quiz(&id)).and_then(|q| q.steps.iter().enumerate().skip(i + 1).find(|(_, s)| s.blank()).map(|(j, _)| j));
                return match next {
                    Some(j) => self.practice(LessonAction::StepInput(id, j)),
                    None => self.practice(LessonAction::QuizCheck(id)),
                };
            }
            Purpose::LabPrediction(id) => {
                self.labs.entry(id).or_default().prediction = input.buffer.trim().to_string();
                self.input = None;
                return Ok(());
            }
            Purpose::Reflection(id) => return self.practice(LessonAction::ReflectSave(id)),
            _ => {}
        }
        if body.is_empty() {
            return Err("write something first".into());
        }
        self.submit_thread_draft(&input.purpose, body)
    }
}
