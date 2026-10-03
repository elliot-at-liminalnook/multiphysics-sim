//! The one lesson action handler (clicks, keys and REST).
use super::*;
use crate::annotations::ThreadOp;

impl Learn {
    /// Everything a click (or REST) can do. Side effects that need the ECS
    /// (panels, camera) happen in the systems that read the resulting state.
    pub(crate) fn act(&mut self, action: LessonAction, scene: &mut SpatialScene) {
        if let Err(e) = self.try_act(action, scene) {
            self.status = e;
        }
    }

    /// The same handler, reporting failure to the caller (REST).
    pub(crate) fn try_act(&mut self, action: LessonAction, scene: &mut SpatialScene) -> Result<(), String> {
        let was_dirty = std::mem::replace(&mut self.dirty, true);
        {
            match action {
                LessonAction::Open(slug) => self.open(&slug)?,
                LessonAction::ToggleCategory(id) => {
                    if !self.folded.remove(&id) {
                        self.folded.insert(id);
                    }
                }
                LessonAction::Goto(block) => self.scroll_to = Some(block),
                LessonAction::Activate(id) => self.activate(&id),
                LessonAction::Play => {
                    if let Some(a) = self.scene.as_mut() {
                        if a.time >= a.duration() - 1e-9 {
                            a.seek(0.);
                        }
                        a.playing = a.run.is_some();
                    }
                }
                LessonAction::Pause => {
                    if let Some(a) = self.scene.as_mut() {
                        a.playing = false;
                    }
                }
                LessonAction::Restart => {
                    if let Some(a) = self.scene.as_mut() {
                        a.seek(0.);
                        a.framed = false;
                        a.playing = a.run.is_some();
                    }
                }
                LessonAction::Explore => {
                    let mut stop = false;
                    if let Some(a) = self.scene.as_mut() {
                        a.explore = !a.explore;
                        stop = a.explore;
                        let rules = if a.explore { sim_script::pacing::PacingRules::exploring() } else { Default::default() };
                        if let Some(run) = a.run.clone() {
                            a.plan = run.pacing_with(&a.scene, &a.timeline, &rules).unwrap_or_default();
                            let t = a.time;
                            a.seek(t);
                        }
                    }
                    if stop {
                        self.narrate(narrate::NarrateAction::Pause).ok();
                    }
                }
                LessonAction::ResetSliders => {
                    if let Some(a) = self.scene.as_mut() {
                        a.overrides.clear();
                        a.slider_drag = None;
                    }
                    self.rerecord();
                }
                LessonAction::Speed(x) => {
                    if let Some(a) = self.scene.as_mut() {
                        a.user_speed = x;
                    }
                }
                LessonAction::Seek => {}
                LessonAction::SeekTo { time, rewind } => {
                    let a = self.scene.as_mut().ok_or("no live scene")?;
                    if a.run.is_none() {
                        return Err("the scene has not finished recording".into());
                    }
                    let time = time.ok_or("seek needs time")?;
                    // A press that goes back in time is a rewind (for authors' reports).
                    let rewound = (rewind && time < a.time - 0.01 * a.duration()).then(|| a.id.clone());
                    let was_playing = a.playing;
                    a.seek(time);
                    a.playing = false;
                    // Scrubbing follows in `ui::live_text` without a page rebuild
                    // (a timebar drag seeks every frame); only pausing a playing
                    // scene changes the page (its Play/Pause button).
                    self.dirty = was_dirty || was_playing;
                    if let (Some(scene), Some(slug)) = (rewound, self.slug().map(String::from)) {
                        self.progress.block(&slug, &scene).rewinds += 1;
                    }
                }
                LessonAction::KeepMoment(time) => {
                    if let Some(a) = self.scene.as_mut().filter(|a| a.run.is_some()) {
                        if (a.time - time).abs() > 1e-9 {
                            a.seek(time);
                        }
                    }
                    self.dirty = was_dirty;
                }
                LessonAction::Pick(component) => self.pick(scene, &component),
                LessonAction::Slider { parameter, value } => {
                    let a = self.scene.as_mut().ok_or("no live scene")?;
                    // Letting go ends the drag even if the slider is gone, so
                    // `sliders` does not send the release again next frame.
                    if a.slider_drag.as_ref().is_some_and(|(p, _)| *p == parameter) {
                        a.slider_drag = None;
                    }
                    let spec = a.scene.sliders.iter().find(|s| s.parameter == parameter).ok_or_else(|| format!("the scene has no slider for `{parameter}`"))?;
                    let v = spec.snap(value);
                    a.overrides.insert(parameter, v);
                    self.rerecord();
                }
                LessonAction::OpenBuilder => {
                    let a = self.scene.as_ref().ok_or("show a scene first")?;
                    if !a.installed {
                        return Err("the scene is still loading".into());
                    }
                    self.returning = Some(Return { scroll: self.scroll });
                    self.request_screen(false);
                }
                LessonAction::ResetSandbox => self.reset_sandbox(),
                LessonAction::SaveSandbox => {
                    let (lesson, a) = (self.lesson.as_ref().ok_or("no lesson")?, self.scene.as_ref().ok_or("no scene")?);
                    let path = runtime::save_sandbox(lesson, &a.scene, &self.registry)?;
                    self.status = format!("Saved the builder copy over {}", path.display());
                    let id = a.id.clone();
                    self.activate(&id);
                }
                LessonAction::Part(system, path) => self.goto_part(&system, &path),
                LessonAction::Link(url) => sim_lesson::edit::open_in_editor(std::path::Path::new(&url)).or_else(|_| open_url(&url))?,
                LessonAction::RunCompare(id) => self.run_compare(&id)?,
                LessonAction::Mode(mode) => {
                    self.mode = mode;
                    if mode != PageMode::Edit && matches!(self.input.as_ref().map(|i| &i.purpose), Some(Purpose::Block(..) | Purpose::NewBlock(_))) {
                        self.input = None;
                    }
                    self.status = match mode {
                        PageMode::Read => "Reading. Click a part in a scene to learn about it.".into(),
                        PageMode::Annotate => "Annotate: click a paragraph, or a part in the live scene, to start a note.".into(),
                        PageMode::Edit => "Edit: each block has Edit, + and Delete; changes save to lesson.md with undo.".into(),
                    };
                }
                LessonAction::AnnotateBlock(block) => {
                    let lesson = self.lesson.as_ref().ok_or("no lesson")?;
                    let section = lesson.block(&block).map(|b| b.section.clone()).unwrap_or_default();
                    let quote = self.index.as_ref().and_then(|i| i.block_anchor(&block, &section)).ok_or("this block has no text to annotate")?;
                    self.draft = Some(LessonAnchor::Text { quote });
                    self.thread = None;
                    self.input = Some(Input { purpose: Purpose::Comment, buffer: String::new() });
                }
                LessonAction::NoteOnScene => {
                    let a = self.scene.as_ref().ok_or("show a scene first")?;
                    self.draft = Some(LessonAnchor::Scene { scene: a.id.clone(), part: None, time_s: Some((a.time * 1000.).round() / 1000.), missing: false });
                    self.thread = None;
                    self.input = Some(Input { purpose: Purpose::Comment, buffer: String::new() });
                }
                LessonAction::NoteOnPart => {
                    let (a, part) = (self.scene.as_ref().ok_or("show a scene first")?, self.picked.clone().ok_or("click a part first")?);
                    self.draft = Some(LessonAnchor::Scene { scene: a.id.clone(), part: Some(part), time_s: Some((a.time * 1000.).round() / 1000.), missing: false });
                    self.thread = None;
                    self.input = Some(Input { purpose: Purpose::Comment, buffer: String::new() });
                }
                LessonAction::ClearPart => {
                    self.picked = None;
                    let _ = scene.set_selection(sim_inspect::selection::SelectionTarget::None);
                }
                LessonAction::EditBlock(id) => {
                    let lesson = self.lesson.as_ref().ok_or("no lesson")?;
                    let b = lesson.block(&id).ok_or("that block no longer exists")?;
                    self.input = Some(Input { purpose: Purpose::Block(id.clone(), b.hash.clone()), buffer: b.text(&lesson.source).to_string() });
                }
                LessonAction::InsertAfter(after) => self.input = Some(Input { purpose: Purpose::NewBlock(after), buffer: String::new() }),
                LessonAction::DeleteBlock(id) => {
                    let hash = self.lesson.as_ref().and_then(|l| l.block(&id)).map(|b| b.hash.clone()).ok_or("that block no longer exists")?;
                    self.edit("Delete block", Edit::DeleteBlock { block: id, hash }, None)?;
                }
                LessonAction::SaveEdit | LessonAction::Submit => self.submit_draft()?,
                LessonAction::CancelEdit | LessonAction::CancelDraft => {
                    self.input = None;
                    self.draft = None;
                }
                LessonAction::OpenEditor => {
                    let lesson = self.lesson.as_ref().ok_or("no lesson")?;
                    sim_lesson::edit::open_in_editor(&lesson.path)?;
                    self.status = format!("Opened {} — changes appear here when you save.", lesson.path.display());
                }
                LessonAction::Undo => {
                    self.undo(false)?;
                }
                LessonAction::Redo => {
                    self.undo(true)?;
                }
                LessonAction::ShowAt { scene: id, time, part } => {
                    if self.scene.as_ref().is_none_or(|a| a.id != id) {
                        self.activate(&id);
                    }
                    if let (Some(a), Some(t)) = (self.scene.as_mut(), time) {
                        a.seek(t);
                        a.playing = false;
                    }
                    if let Some(p) = part {
                        self.picked = Some(p);
                    }
                    self.scroll_to = Some(id);
                }
                LessonAction::ThreadList => {
                    self.thread = None;
                    self.menu = None;
                }
                LessonAction::OpenThread(id) => {
                    self.thread = Some(id.clone());
                    self.draft = None;
                    self.menu = None;
                    self.agent.mark_read(&id);
                    // Bring its anchor into view.
                    let anchor = self.threads().get(&id).and_then(|t| t.targets.first().cloned());
                    match anchor {
                        Some(LessonAnchor::Text { .. }) => self.scroll_to = anchor.as_ref().zip(self.index.as_ref()).and_then(|(a, i)| a.block(i)).map(String::from),
                        Some(LessonAnchor::Scene { scene: s, time_s, part, .. }) => return self.try_act(LessonAction::ShowAt { scene: s, time: time_s, part }, scene),
                        None => {}
                    }
                }
                LessonAction::Compose => {
                    if self.input.is_none() {
                        if self.thread.is_none() && self.draft.is_none() {
                            return Err("switch to Annotate and click a paragraph or a part first".into());
                        }
                        self.input = Some(Input { purpose: Purpose::Comment, buffer: String::new() });
                    }
                }
                LessonAction::CommentMenu(c) => self.menu = if self.menu.as_deref() == Some(&c) { None } else { Some(c) },
                LessonAction::EditComment(c) => {
                    let t = self.thread.clone().ok_or("open a note first")?;
                    let body = self.notes_doc.threads.get(&t).and_then(|t| t.comments.iter().find(|x| x.id == c)).map(|x| x.body.clone()).ok_or("that comment no longer exists")?;
                    self.input = Some(Input { purpose: Purpose::EditComment(c), buffer: body });
                    self.menu = None;
                }
                LessonAction::DeleteComment(c) => {
                    let t = self.thread.clone().ok_or("open a note first")?;
                    self.thread_op("Delete comment", ThreadOp::DeleteComment { thread: t, comment: c })?;
                    self.menu = None;
                }
                LessonAction::Resolve => {
                    let t = self.thread.clone().ok_or("open a note first")?;
                    let resolved = !self.notes_doc.threads.get(&t).is_some_and(|x| x.resolved);
                    self.thread_op(if resolved { "Resolve" } else { "Reopen" }, ThreadOp::Resolve { thread: t, resolved })?;
                }
                LessonAction::DeleteThread => {
                    let t = self.thread.take().ok_or("open a note first")?;
                    self.thread_op("Delete note", ThreadOp::Delete { thread: t })?;
                }
                LessonAction::Author => self.input = Some(Input { purpose: Purpose::Author, buffer: self.author.clone() }),
                LessonAction::OpenOnly => self.open_only = !self.open_only,
                LessonAction::Ask => {
                    let t = self.thread.clone().ok_or("open a note first")?;
                    let input = self.agent_input(&t)?;
                    self.agent.ask(input)?;
                    self.status = "Asked Codex (read-only answer mode); the reply is posted to this note.".into();
                }
                LessonAction::AgentCancel(run) => self.agent.cancel(&run)?,
                LessonAction::Narrate(a) => {
                    let seek = matches!(a, narrate::NarrateAction::Seek { .. });
                    self.narrate(a)?;
                    // A narration-bar drag seeks every frame and follows in
                    // `narrate::live` without a page rebuild (as before).
                    if seek {
                        self.dirty = was_dirty;
                    }
                }
                a @ (LessonAction::QuizPick(..) | LessonAction::QuizCheck(_) | LessonAction::QuizReveal(_) | LessonAction::QuizInput(_) | LessonAction::ReflectInput(_) | LessonAction::ReflectSave(_) | LessonAction::SketchClear(_) | LessonAction::Review(..) | LessonAction::HintMore(_) | LessonAction::Confidence(..) | LessonAction::StepInput(..) | LessonAction::RecallTick(..) | LessonAction::ReviewSession | LessonAction::ReviewNext | LessonAction::ReviewEnd) => self.practice(a)?,
                LessonAction::ReflectFeedback(id) => {
                    // A note on the reflection, with the reader's text, answered by Codex.
                    let lesson = self.lesson.as_ref().ok_or("no lesson")?;
                    let (section, r) = lesson.blocks.iter().find_map(|b| match &b.kind { BlockKind::Reflect(r) if r.id == id => Some((b.section.clone(), r.clone())), _ => None }).ok_or("no such reflection")?;
                    let slug = lesson.slug.clone();
                    let text = self.progress.lessons.get(&slug).and_then(|p| p.reflections.get(&id)).map(|r| r.text.clone()).ok_or("save your explanation first")?;
                    let points: Vec<String> = r.key_points.iter().map(|k| format!("- {}", k.idea)).collect();
                    let body = format!("Feedback please on my explanation for “{}”:\n\n{text}\n\nKey ideas the lesson looks for:\n{}\n\nSay what is right, what is missing or wrong, and one way to improve it. Be brief and kind.", sim_annotate::plain_comment(&r.prompt).lines().next().unwrap_or(""), points.join("\n"));
                    // Anchored to the prompt's section heading (prompts are not in the text index).
                    let heading = lesson.headings().find(|(_, _, text, _)| *text == section).map(|(b, ..)| b.id.clone());
                    let index = self.index.as_ref().ok_or("the lesson is still loading")?;
                    let (_, start, end) = heading.and_then(|h| index.ranges.iter().find(|(id, ..)| *id == h).cloned()).ok_or("the prompt has no section heading to attach the note to")?;
                    let quote = sim_annotate::text::TextAnchor::capture(&index.text.text, start, end, &section)?;
                    let op = ThreadOp::Create { title: "Feedback on my explanation".into(), targets: vec![LessonAnchor::Text { quote }], body, author: self.author.clone(), links: vec![], pin_m: None, view: None };
                    let t = self.thread_op("Feedback request", op)?.thread.ok_or("the new note has no id")?;
                    self.thread = Some(t.clone());
                    self.ask_when_saved = Some(t);
                    self.status = "Saving your explanation as a note, then asking Codex for feedback…".into();
                }
                LessonAction::AskMoment => {
                    let a = self.scene.as_ref().ok_or("show a scene first")?;
                    self.draft = Some(LessonAnchor::Scene { scene: a.id.clone(), part: self.picked.clone(), time_s: Some((a.time * 1000.).round() / 1000.), missing: false });
                    self.thread = None;
                    self.input = Some(Input { purpose: Purpose::Comment, buffer: String::new() });
                    self.ask_next = true;
                    self.status = "Write your question about this moment; Codex answers with the run's values when you post it.".into();
                }
                LessonAction::TaskStart(id) => self.start_task(&id)?,
                LessonAction::TaskCheck(id) => {
                    if self.scene.as_ref().is_none_or(|a| a.id != format!("task-{id}")) {
                        self.start_task(&id)?;
                    } else {
                        self.rerecord();
                    }
                }
                LessonAction::TaskHint(id) => self.tasks.entry(id).or_default().hints += 1,
                LessonAction::LabTick(id, i) => {
                    let s = self.labs.entry(id).or_default();
                    if let Some(t) = s.ticks.get_mut(i) {
                        *t = !*t;
                    }
                }
                LessonAction::LabPredict(id) => {
                    let buffer = self.labs.get(&id).map(|s| s.prediction.clone()).unwrap_or_default();
                    self.input = Some(Input { purpose: Purpose::LabPrediction(id), buffer });
                }
                LessonAction::LabRun(id) => self.run_lab(&id)?,
                LessonAction::LabStop => self.stop_labs(),
                LessonAction::Setting(change) => self.change_setting(change)?,
                LessonAction::SliderFocus(p) => self.focus_slider = if self.focus_slider.as_deref() == Some(p.as_str()) { None } else { Some(p) },
                LessonAction::SliderStep(step, coarse) => {
                    if let Some(p) = self.focus_slider.clone() {
                        if let Some(a) = self.scene.as_mut() {
                            if let Some(spec) = a.scene.sliders.iter().find(|s| s.parameter == p).cloned() {
                                let now = a.slider_value(scene, &p).unwrap_or(spec.min);
                                let unit = spec.step.unwrap_or((spec.max - spec.min) / 100.);
                                let v = spec.snap(now + step as f64 * unit * if coarse { 10. } else { 1. });
                                a.overrides.insert(p, v);
                                self.rerecord();
                            }
                        }
                    }
                }
                LessonAction::EventStep(step) => {
                    // A scrub, like SeekTo: the page is rebuilt only to show a pause.
                    let mut was_playing = false;
                    if let Some(a) = self.scene.as_mut().filter(|a| a.run.is_some()) {
                        let events = event_times(a);
                        let now = a.time;
                        let target = if step > 0 { events.iter().copied().find(|t| *t > now + 1e-6).unwrap_or(a.duration()) } else { events.iter().rev().copied().find(|t| *t < now - 1e-3).unwrap_or(0.) };
                        a.seek(target);
                        was_playing = std::mem::replace(&mut a.playing, false);
                    }
                    self.dirty = was_dirty || was_playing;
                }
                LessonAction::ClearPick => {
                    self.draft = None;
                    self.picked = None;
                }
            }
            Ok(())
        }
    }

    /// Narration volume (0 mutes, e.g. while frames are captured).
    pub(crate) fn player_volume(&mut self, v: f32) {
        if v >= 1. && self.player.is_none() {
            return;
        }
        self.player().set_volume(v);
    }

    fn change_setting(&mut self, change: Setting) -> Result<(), String> {
        match change {
            Setting::ReducedMotion => self.settings.reduced_motion = !self.settings.reduced_motion,
            Setting::TextSize(s) => self.settings.text_scale = s.clamp(0.8, 1.6),
            Setting::NarrationSpeed(s) => self.settings.narration_speed = s.clamp(0.5, 2.0),
            Setting::Transcript => self.settings.transcript = !self.settings.transcript,
        }
        let path = self.settings_path.clone();
        self.settings.save(&path)
    }

    fn run_compare(&mut self, id: &str) -> Result<(), String> {
        let lesson = self.lesson.clone().ok_or("no lesson")?;
        let compare = lesson.compares().find(|(_, c)| c.id == id).map(|(_, c)| c.clone()).ok_or("no such comparison")?;
        let registry = self.registry.clone();
        let state = self.compares.entry(id.into()).or_default();
        if state.job.is_some() {
            return Ok(());
        }
        // A study of variants: a dedicated thread.
        state.job = Some(crate::jobs::Job::spawn(crate::jobs::Pool::Dedicated, 0, "the comparison", move |ctx| {
            runtime::load_system(&lesson.system_path(&compare.system), &registry).and_then(|doc| runtime::compare_run(&doc, &registry, &compare, true, None, &|done, total| ctx.steps(done as u64, total as u64)))
        }));
        state.result = None;
        Ok(())
    }
}

/// An http(s) link in the browser ("not a web link: {url}" otherwise),
/// through `jobs`, which reaps the opener so it never lingers as a zombie.
fn open_url(url: &str) -> Result<(), String> {
    crate::jobs::open_in_browser(url)
}
