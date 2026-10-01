//! Lesson edits and notes through the shared command layers.
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

    /// Submit a note command (async; the result arrives in `poll`).
    pub fn note(&mut self, label: &str, command: ThreadCommand<LessonAnchor>) -> Result<u64, String> {
        let notes = self.notes.as_mut().ok_or("no lesson is open")?;
        let id = notes.submit(command, Some(self.notes_doc.revision))?;
        self.pending.push((id, label.to_string()));
        Ok(id)
    }

    /// Threads re-attached to today's text (display only).
    pub(crate) fn threads(&self) -> BTreeMap<String, Thread<LessonAnchor>> {
        match &self.index {
            Some(index) => self.notes_doc.refreshed(index),
            None => self.notes_doc.threads.clone(),
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
        let comment = |id: String| Comment { id, author: self.author.clone(), body: body.clone(), created_at: sim_annotate::stamp(), edited_at: None, links: vec![] };
        match (&input.purpose, &self.thread, &self.draft) {
            (Purpose::EditComment(c), Some(t), _) => {
                let command = ThreadCommand::EditComment { thread: t.clone(), comment: c.clone(), body: body.clone(), edited_at: sim_annotate::stamp() };
                self.note("Edit comment", command)?;
            }
            (_, Some(t), _) => {
                let command = ThreadCommand::AddComment { thread: t.clone(), comment: comment(sim_annotate::uid("c")) };
                self.note("Reply", command)?;
            }
            (_, None, Some(anchor)) => {
                let id = sim_annotate::uid("t");
                let title: String = sim_annotate::plain_comment(&body).lines().next().unwrap_or("Note").chars().take(80).collect();
                let thread = Thread { id: id.clone(), title, resolved: false, targets: vec![anchor.clone()], comments: vec![comment(sim_annotate::uid("c"))], pin_m: None, view: None };
                self.note("New note", ThreadCommand::PutThread { thread })?;
                if std::mem::take(&mut self.ask_next) {
                    self.ask_when_saved = Some(id.clone());
                }
                self.thread = Some(id);
                self.draft = None;
            }
            _ => return Err("pick a paragraph or a part to attach the note to".into()),
        }
        self.input = None;
        Ok(())
    }
}
