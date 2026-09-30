//! Practice and pacing in the Learn screen: question cards (choice,
//! numeric, predict), self-explanation prompts, mastery gating (content
//! after an unanswered question stays locked), prediction-locked scenes,
//! spaced review across lessons, and figures (SVG rasterized off the UI
//! thread, PNG/JPEG decoded there too, hot-reloaded when the file changes).
use super::*;
use crate::builder::ui::{BORDER, FAINT, HOVER_BG, Kit, Look, OK, RAISED, SUBTLE, SURFACE, TEXT, Tint, WARN, wrap};
use sim_lesson::progress::Mode;
use sim_lesson::quiz::{Answer, Quiz, QuizKind, Reflect, Verdict};

pub(crate) enum FigureState {
    Loading(Mutex<mpsc::Receiver<Result<(Image, (f32, f32)), String>>>),
    Ready { image: Handle<Image>, aspect: f32, units: (f32, f32) },
    Failed(String),
}
pub(crate) struct CachedFigure {
    pub state: FigureState,
    stamp: Option<std::time::SystemTime>,
}

fn figure_job(path: PathBuf) -> mpsc::Receiver<Result<(Image, (f32, f32)), String>> {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display())).and_then(|bytes| {
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext == "svg" {
                let r = sim_lesson::figure::rasterize_svg(&bytes, 1600)?;
                let image = Image::new(Extent3d { width: r.width, height: r.height, depth_or_array_layers: 1 }, TextureDimension::D2, r.rgba, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
                Ok((image, r.units))
            } else {
                let image = Image::from_buffer(&bytes, ImageType::Extension(&ext), CompressedImageFormats::NONE, true, ImageSampler::default(), RenderAssetUsages::default()).map_err(|e| format!("{}: {e}", path.display()))?;
                let size = image.size_f32();
                Ok((image, (size.x, size.y)))
            }
        });
        let _ = tx.send(result);
    });
    rx
}

impl Learn {
    /// Start loading every figure of the open lesson that is new or changed.
    pub(crate) fn load_figures(&mut self) {
        let Some(lesson) = &self.lesson else { return };
        for f in lesson.figures() {
            let path = lesson.figure_path(&f.src);
            let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            let fresh = self.figures.get(&path).is_some_and(|c| c.stamp == stamp);
            if !fresh {
                self.figures.insert(path.clone(), CachedFigure { state: FigureState::Loading(Mutex::new(figure_job(path))), stamp });
            }
        }
    }
    /// Collect finished figure jobs (needs the image assets).
    pub(crate) fn poll_figures(&mut self, images: &mut Assets<Image>) -> bool {
        let mut changed = false;
        for c in self.figures.values_mut() {
            let FigureState::Loading(rx) = &c.state else { continue };
            let Some(result) = rx.lock().ok().and_then(|r| r.try_recv().ok()) else { continue };
            c.state = match result {
                Ok((image, units)) => {
                    let aspect = image.width() as f32 / image.height().max(1) as f32;
                    FigureState::Ready { image: images.add(image), aspect, units }
                }
                Err(e) => FigureState::Failed(e),
            };
            changed = true;
        }
        changed
    }

    fn passed(&self, id: &str) -> bool {
        let Some(slug) = self.slug() else { return false };
        self.progress.passed(slug, id) || self.progress.lessons.get(slug).is_some_and(|p| p.reflections.contains_key(id))
    }
    /// A predict question for this scene has not been answered yet.
    pub(crate) fn scene_locked(&self, scene: &str) -> Option<String> {
        let lesson = self.lesson.as_ref()?;
        let q = lesson.prediction_for(scene)?;
        let predicted = self.progress.quiz(&lesson.slug, &q.id).is_some_and(|r| r.prediction.is_some());
        (!predicted).then(|| q.id.clone())
    }
    /// Index of the first block hidden by an unanswered gating question.
    pub(crate) fn gate(&self) -> Option<usize> {
        // A contact sheet is an authoring view: every block shows while it is captured.
        if self.frames.is_some() {
            return None;
        }
        let lesson = self.lesson.as_ref()?;
        lesson.gate(|id| self.passed(id))
    }
    fn save_progress(&mut self) {
        if let Err(e) = self.progress.save(&self.progress_path) {
            self.status = format!("Could not save progress: {e}");
        }
    }
    fn answer_for(&self, q: &Quiz) -> Result<(Answer, String), String> {
        if q.kind == QuizKind::Steps {
            let texts: Vec<String> = q.steps.iter().enumerate().filter(|(_, s)| s.blank()).map(|(i, _)| {
                let typing = self.input.as_ref().filter(|x| x.purpose == Purpose::StepAnswer(q.id.clone(), i)).map(|x| x.buffer.clone());
                typing.or_else(|| self.step_text.get(&(q.id.clone(), i)).cloned()).unwrap_or_default()
            }).collect();
            if texts.iter().any(|t| t.trim().is_empty()) {
                return Err("fill in every step first".into());
            }
            let shown = texts.join(" · ");
            return Ok((Answer::Steps(texts), shown));
        }
        if q.options.is_empty() {
            let text = self.input.as_ref().filter(|i| i.purpose == Purpose::QuizAnswer(q.id.clone())).map(|i| i.buffer.clone()).or_else(|| self.quiz_text.get(&q.id).cloned()).unwrap_or_default();
            // Numbers may carry a unit ("24 mN·m", "4300 rpm"); it is read and converted.
            sim_lesson::units::split_quantity(&text).ok_or("type a number first")?;
            Ok((Answer::Text(text.trim().to_string()), text.trim().to_string()))
        } else {
            let i = *self.quiz_pick.get(&q.id).ok_or("pick an option first")?;
            Ok((Answer::Choice(i), q.options[i].text.clone()))
        }
    }

    /// Check, reveal, pick and review actions.
    pub(crate) fn practice(&mut self, action: LessonAction) -> Result<(), String> {
        let slug = self.slug().ok_or("no lesson is open")?.to_string();
        let lesson = self.lesson.clone().ok_or("no lesson is open")?;
        let now = sim_lesson::progress::now();
        match action {
            LessonAction::QuizPick(id, i) => {
                self.quiz_pick.insert(id.clone(), i);
                self.quiz_verdict.remove(&id);
            }
            LessonAction::QuizCheck(id) => {
                let q = lesson.quiz(&id).ok_or("no such question")?.clone();
                // A sketch is its own answer: no number or option to read.
                if q.kind == QuizKind::Sketch {
                    let points = self.sketch_points(&q).ok_or("draw your curve across the chart first")?;
                    self.progress.predict(&slug, &id, &serde_json::to_string(&points).unwrap_or_default(), now);
                    self.quiz_verdict.insert(id.clone(), Verdict { correct: true, feedback: "Now run the scene and compare.".into() });
                    self.save_progress();
                    if let Some(scene) = &q.scene {
                        self.scroll_to = Some(scene.clone());
                        if self.scene.as_ref().is_none_or(|a| &a.id != scene) {
                            self.activate(scene);
                        }
                    }
                    return Ok(());
                }
                let (answer, text) = self.answer_for(&q)?;
                if self.input.as_ref().is_some_and(|i| i.purpose == Purpose::QuizAnswer(id.clone())) {
                    self.quiz_text.insert(id.clone(), text.clone());
                    self.input = None;
                }
                let reviewing = self.review.as_ref().is_some_and(|(l, q)| *l == slug && *q == id);
                if q.kind == QuizKind::Predict {
                    // Commit the prediction; it is judged after the run.
                    self.progress.predict(&slug, &id, &text, now);
                    self.quiz_verdict.insert(id.clone(), Verdict { correct: true, feedback: "Now run the scene and compare.".into() });
                    self.save_progress();
                    if let Some(scene) = &q.scene {
                        self.scroll_to = Some(scene.clone());
                        if self.scene.as_ref().is_none_or(|a| &a.id != scene) {
                            self.activate(scene);
                        }
                    }
                    return Ok(());
                }
                // This attempt's numbers (varied questions) and the model's values.
                let mut values = self.progress.quiz(&slug, &id).map(|r| r.values.clone()).unwrap_or_default();
                values.extend(self.model.given.get(&id).cloned().unwrap_or_default());
                if !q.given.is_empty() && !self.model.given.contains_key(&id) {
                    return Err("still reading the model's values; try again in a moment".into());
                }
                let mut verdict = q.check_with_values(&answer, None, &values)?;
                let confidence = self.confidence.remove(&id);
                if q.pretest {
                    // A guess before the teaching: noted, never a miss.
                    self.progress.record_with(&slug, &id, verdict.correct, &text, Mode::Learn, false, now, None);
                    self.save_progress();
                    verdict = sim_lesson::quiz::Verdict { correct: verdict.correct, feedback: if verdict.feedback.is_empty() { "Noted. Read on and compare.".into() } else { verdict.feedback } };
                    self.quiz_verdict.insert(id, verdict);
                    return Ok(());
                }
                // A wrong option that names a misconception opens its remedy.
                if let (Answer::Choice(i), false) = (&answer, verdict.correct) {
                    if let Some(r) = q.options.get(*i).and_then(|o| o.remedy.clone()) {
                        self.progress.misconception(&slug, &id, &r);
                        self.remedy.insert(id.clone(), r);
                    }
                } else if verdict.correct {
                    self.remedy.remove(&id);
                }
                self.progress.record_with(&slug, &id, verdict.correct, &text, if reviewing { Mode::Review } else { Mode::Learn }, q.reviewable(), now, confidence);
                if !verdict.correct && confidence == Some(sim_lesson::progress::Confidence::Sure) {
                    verdict.feedback = format!("{} You were sure, which makes this one worth a second look: it comes back tomorrow.", verdict.feedback).trim().to_string();
                }
                self.save_progress();
                if verdict.correct && reviewing {
                    self.status = match &self.session {
                        Some((queue, at)) if at + 1 < queue.len() => format!("Reviewed ({} of {}). Next question when you are ready.", at + 1, queue.len()),
                        Some(_) => "Review session done: every due question answered.".into(),
                        None => "Reviewed: it will come back later, spaced further out.".into(),
                    };
                    self.review = None;
                    // Varied questions get new numbers for next time.
                    self.draw_variants();
                }
                self.quiz_verdict.insert(id, verdict);
            }
            LessonAction::QuizReveal(id) => {
                let q = lesson.quiz(&id).ok_or("no such question")?;
                self.progress.reveal(&slug, &id, q.reviewable(), now);
                self.save_progress();
                self.quiz_verdict.insert(id, Verdict { correct: false, feedback: "Answer shown. This question comes back for review tomorrow.".into() });
            }
            LessonAction::QuizInput(id) => {
                let buffer = self.quiz_text.get(&id).cloned().unwrap_or_default();
                self.input = Some(Input { purpose: Purpose::QuizAnswer(id), buffer });
            }
            LessonAction::ReflectInput(id) => {
                let buffer = self.progress.lessons.get(&slug).and_then(|p| p.reflections.get(&id)).map(|r| r.text.clone()).unwrap_or_default();
                self.input = Some(Input { purpose: Purpose::Reflection(id), buffer });
            }
            LessonAction::ReflectSave(id) => {
                let text = self.input.as_ref().filter(|i| i.purpose == Purpose::Reflection(id.clone())).map(|i| i.buffer.trim().to_string()).unwrap_or_default();
                if text.split_whitespace().count() < 3 {
                    return Err("write a sentence or two first".into());
                }
                self.progress.reflect(&slug, &id, &text, now);
                self.save_progress();
                self.input = None;
            }
            LessonAction::SketchClear(id) => {
                self.sketches.remove(&id);
            }
            LessonAction::HintMore(id) => {
                let n = lesson.quiz(&id).map(|q| q.hint_ladder().len()).unwrap_or(0) as u32;
                if self.progress.quiz(&slug, &id).map(|r| r.hints_used).unwrap_or(0) < n {
                    self.progress.hint(&slug, &id);
                    self.save_progress();
                }
            }
            LessonAction::Confidence(id, level) => {
                use sim_lesson::progress::Confidence;
                let c = match level {
                    0 => Confidence::Guess,
                    1 => Confidence::FairlySure,
                    _ => Confidence::Sure,
                };
                if self.confidence.get(&id) == Some(&c) {
                    self.confidence.remove(&id);
                } else {
                    self.confidence.insert(id, c);
                }
            }
            LessonAction::StepInput(id, i) => {
                // Keep what was typed in the step being left.
                if let Some(Input { purpose: Purpose::StepAnswer(q, j), buffer }) = self.input.clone() {
                    self.step_text.insert((q, j), buffer.trim().to_string());
                }
                let buffer = self.step_text.get(&(id.clone(), i)).cloned().unwrap_or_default();
                self.input = Some(Input { purpose: Purpose::StepAnswer(id, i), buffer });
            }
            LessonAction::RecallTick(id, i) => {
                let idea = lesson.blocks.iter().find_map(|b| match &b.kind { BlockKind::Reflect(r) if r.id == id => r.key_points.get(i).map(|k| k.idea.clone()), _ => None }).ok_or("no such key point")?;
                let ticked = self.progress.lessons.entry(slug.clone()).or_default().recalled.entry(id).or_default();
                if let Some(at) = ticked.iter().position(|x| *x == idea) {
                    ticked.remove(at);
                } else {
                    ticked.push(idea);
                }
                self.save_progress();
            }
            LessonAction::ReviewSession => {
                let catalog = self.all_lessons.clone();
                let concept = |l: &str, q: &str| catalog.iter().find(|x| x.slug == l).and_then(|x| x.quiz(q).map(|qq| x.concepts_of(qq).first().cloned().unwrap_or_default())).unwrap_or_default();
                let queue = self.progress.session(now, concept);
                let first = queue.first().cloned().ok_or("nothing is due for review right now")?;
                self.status = format!("Mixed review: {} question(s) from across your lessons, interleaved.", queue.len());
                self.session = Some((queue, 0));
                return self.practice(LessonAction::Review(first.0, first.1));
            }
            LessonAction::ReviewNext => {
                let (queue, at) = self.session.clone().ok_or("no review session")?;
                match queue.get(at + 1).cloned() {
                    Some(next) => {
                        self.session = Some((queue, at + 1));
                        return self.practice(LessonAction::Review(next.0, next.1));
                    }
                    None => {
                        self.session = None;
                        self.review = None;
                        self.status = "Review session done.".into();
                    }
                }
            }
            LessonAction::ReviewEnd => {
                self.session = None;
                self.review = None;
            }
            LessonAction::Review(l, q) => {
                if self.slug() != Some(l.as_str()) {
                    self.open(&l)?;
                }
                self.quiz_pick.remove(&q);
                self.quiz_verdict.remove(&q);
                self.quiz_text.remove(&q);
                self.review = Some((l, q.clone()));
                self.scroll_to = Some(q);
            }
            _ => {}
        }
        Ok(())
    }

    /// The time window a sketch question is drawn over.
    pub(crate) fn sketch_window(&self, q: &Quiz) -> [f64; 2] {
        q.window.unwrap_or_else(|| [0., self.lesson.as_ref().and_then(|l| l.scene(q.scene.as_deref()?)).map(|s| s.run.duration_s).unwrap_or(1.)])
    }
    /// The drawn curve as [time, value] points (None until it spans most of the chart).
    pub(crate) fn sketch_points(&self, q: &Quiz) -> Option<Vec<[f64; 2]>> {
        let columns = self.sketches.get(&q.id)?;
        let [t0, t1] = self.sketch_window(q);
        let [lo, hi] = q.range?;
        let n = columns.len() as f64;
        let points: Vec<[f64; 2]> = columns.iter().enumerate().filter_map(|(i, y)| y.map(|y| [t0 + (i as f64 + 0.5) / n * (t1 - t0), lo + y as f64 * (hi - lo)])).collect();
        (points.len() as f64 >= 0.6 * n).then_some(points)
    }

    /// The sketch on a question's canvas: the stroke being drawn, or else the
    /// one committed to progress (so a locked-in curve survives a restart).
    pub(crate) fn sketch_columns(&self, id: &str) -> Option<Vec<Option<f32>>> {
        if let Some(c) = self.sketches.get(id) {
            return Some(c.clone());
        }
        let q = self.lesson.as_ref()?.quiz(id)?;
        let ([t0, t1], [lo, hi]) = (self.sketch_window(q), q.range?);
        let saved = self.progress.quiz(self.slug()?, id)?.prediction.clone()?;
        let points: Vec<[f64; 2]> = serde_json::from_str(&saved).ok()?;
        let n = SKETCH_COLUMNS;
        let mut columns = vec![None; n];
        for [t, v] in points {
            let c = ((t - t0) / (t1 - t0) * n as f64).floor();
            if (0. ..n as f64).contains(&c) {
                columns[c as usize] = Some(((v - lo) / (hi - lo)).clamp(0., 1.) as f32);
            }
        }
        Some(columns)
    }

    /// For narration `[[quiz id]]`: answered (or predicted)?
    pub(crate) fn quiz_done(&self, id: &str) -> bool {
        let Some(slug) = self.slug() else { return true };
        self.passed(id) || self.progress.quiz(slug, id).is_some_and(|r| r.prediction.is_some())
    }
}

fn badge(k: &Kit, text: &str, color: Color) -> impl Bundle {
    (Node { border_radius: BorderRadius::all(Val::Px(10.)), padding: UiRect::axes(Val::Px(8.), Val::Px(2.)), border: UiRect::all(Val::Px(1.)), ..default() }, BorderColor::all(color), children![k.text(text, 10.5, color, 2)])
}

/// A question card.
pub(super) fn quiz_card(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, b: &sim_lesson::Block, q: &Quiz, theme: &crate::markdown::Theme) {
    let slug = l.slug().unwrap_or_default();
    let record = l.progress.quiz(slug, &q.id);
    let reviewing = l.review.as_ref().is_some_and(|(s, id)| s == slug && *id == q.id);
    let verdict = l.quiz_verdict.get(&q.id);
    let predicted = record.and_then(|r| r.prediction.clone());
    // A pretest is done once guessed, right or wrong.
    let passed = record.is_some_and(|r| r.passed() || (q.pretest && r.attempts > 0)) && !reviewing;
    let wrong_tries = record.map(|r| if r.correct { 0 } else { r.attempts }).unwrap_or(0);
    let (label, color) = match (q.kind, reviewing) {
        (_, true) => ("Review · from memory", crate::ACCENT),
        (kind, _) if kind.predicts() => ("Predict first", Color::srgb(0.66, 0.55, 0.93)),
        _ if q.pretest => ("Before you read on: a guess", Color::srgb(0.66, 0.55, 0.93)),
        (QuizKind::Steps, _) => ("Finish the worked example", crate::ACCENT),
        _ => ("Check your understanding", crate::ACCENT),
    };
    // This attempt's numbers: varied values and the model's `given` values.
    let mut values = record.map(|r| r.values.clone()).unwrap_or_default();
    values.extend(l.model.given.get(&q.id).cloned().unwrap_or_default());
    col.spawn((Node { border_radius: BorderRadius::all(Val::Px(9.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.), padding: UiRect::all(Val::Px(16.)), border: UiRect::all(Val::Px(1.)), margin: UiRect::vertical(Val::Px(8.)), flex_shrink: 0., ..default() }, BackgroundColor(RAISED), BorderColor::all(if passed { OK.with_alpha(0.6) } else { color.with_alpha(0.6) }), super::ui::BlockNode(b.id.clone()))).with_children(|c| {
        c.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn(badge(k, label, color));
            let state = if passed {
                if record.is_some_and(|r| r.revealed && !r.correct) { "answer shown · review tomorrow".to_string() } else { "answered".to_string() }
            } else if let Some(p) = &predicted {
                // A sketch's prediction is its curve, drawn below, not text.
                if q.kind == QuizKind::Sketch { "your curve is locked in".to_string() } else { format!("your prediction: {p}") }
            } else if wrong_tries > 0 {
                format!("{wrong_tries} tr{} so far", if wrong_tries == 1 { "y" } else { "ies" })
            } else if q.pretest {
                "a guess is fine".into()
            } else if q.gates() && !reviewing {
                "answer to continue".into()
            } else {
                String::new()
            };
            r.spawn(k.text(state, 11., if passed { OK } else { SUBTLE }, 1));
        });
        crate::markdown::render(c, &sim_markdown::parse(&Quiz::fill(&q.question, &values)), theme, |_| None::<LessonAction>);
        if !q.vary.is_empty() {
            c.spawn(k.text("The numbers change each time this question comes back.", 10.5, FAINT, 0));
        }
        let chosen = l.quiz_pick.get(&q.id).copied();
        let show_answer = passed && !reviewing;
        if !q.options.is_empty() {
            for (i, o) in q.options.iter().enumerate() {
                let picked = chosen == Some(i);
                let judged = verdict.is_some() && picked;
                let border = if show_answer && o.correct { OK } else if judged { if o.correct { OK } else { WARN } } else if picked { crate::ACCENT } else { BORDER };
                c.spawn((Button, LessonAction::QuizPick(q.id.clone(), i), Tint { idle: if picked { HOVER_BG } else { SURFACE }, hover: HOVER_BG }, Node { border_radius: BorderRadius::all(Val::Px(6.)), padding: UiRect::axes(Val::Px(12.), Val::Px(9.)), border: UiRect::all(Val::Px(1.5)), column_gap: Val::Px(10.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }, BackgroundColor(if picked { HOVER_BG } else { SURFACE }), BorderColor::all(border))).with_children(|row| {
                    row.spawn(k.text(format!("{}", (b'A' + i as u8) as char), 12., if picked { crate::ACCENT } else { FAINT }, 2));
                    row.spawn((k.text(&o.text, 13.5, TEXT, 0), Node { flex_shrink: 1., min_width: Val::Px(0.), ..default() }));
                });
            }
        } else if q.kind == QuizKind::Sketch {
            sketch_canvas(c, k, l, q, predicted.is_some());
        } else if q.kind == QuizKind::Steps {
            // The worked example: done steps shown, blanks to fill in order.
            for (i, st) in q.steps.iter().enumerate() {
                c.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.), padding: UiRect::axes(Val::Px(10.), Val::Px(6.)), border: UiRect::left(Val::Px(2.)), flex_shrink: 0., ..default() }).insert(BorderColor::all(if st.blank() { crate::ACCENT } else { BORDER })).with_children(|row| {
                    row.spawn(k.text(format!("Step {} · {}", i + 1, st.prompt), 12.5, SUBTLE, 1));
                    match &st.worked {
                        Some(w) => {
                            crate::markdown::render(row, &sim_markdown::parse(w), theme, |_| None::<LessonAction>);
                        }
                        None => {
                            let focused = l.input.as_ref().is_some_and(|x| x.purpose == Purpose::StepAnswer(q.id.clone(), i));
                            let shown = if focused { l.input.as_ref().map(|x| x.buffer.clone()).unwrap_or_default() } else { l.step_text.get(&(q.id.clone(), i)).cloned().unwrap_or_default() };
                            row.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), ..default() }).with_children(|r| {
                                r.spawn(Node { width: Val::Px(180.), flex_direction: FlexDirection::Column, ..default() }).with_children(|w| {
                                    w.spawn(k.input(&shown, "Your number", LessonAction::StepInput(q.id.clone(), i), focused));
                                });
                                if !st.unit.is_empty() {
                                    r.spawn(k.text(&st.unit, 13., SUBTLE, 0));
                                }
                            });
                        }
                    }
                });
            }
        } else {
            let focused = l.input.as_ref().is_some_and(|i| i.purpose == Purpose::QuizAnswer(q.id.clone()));
            let shown = if focused { l.input.as_ref().map(|i| i.buffer.clone()).unwrap_or_default() } else { l.quiz_text.get(&q.id).cloned().or(predicted.clone()).unwrap_or_default() };
            c.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), ..default() }).with_children(|r| {
                r.spawn(Node { width: Val::Px(180.), flex_direction: FlexDirection::Column, ..default() }).with_children(|w| {
                    w.spawn(k.input(&shown, "Type a number", LessonAction::QuizInput(q.id.clone()), focused));
                });
                if !q.unit.is_empty() {
                    r.spawn(k.text(&q.unit, 13., SUBTLE, 0));
                }
            });
        }
        // How sure: asked before checking (graded questions only).
        let gradable = !q.kind.predicts() && !q.pretest && !show_answer;
        if gradable {
            c.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.), ..default() }).with_children(|r| {
                r.spawn(k.text("How sure are you?", 11.5, SUBTLE, 0));
                use sim_lesson::progress::Confidence;
                for (level, name, c) in [(0u8, "A guess", Confidence::Guess), (1, "Fairly sure", Confidence::FairlySure), (2, "Sure", Confidence::Sure)] {
                    r.spawn(k.button(name, LessonAction::Confidence(q.id.clone(), level), Look::Chip(l.confidence.get(&q.id) == Some(&c)), true));
                }
            });
        }
        // Actions.
        c.spawn(wrap()).with_children(|r| {
            let committed = q.kind.predicts() && predicted.is_some();
            if q.kind == QuizKind::Sketch && !committed {
                r.spawn(k.button("Clear", LessonAction::SketchClear(q.id.clone()), Look::Ghost, l.sketches.contains_key(&q.id)));
            }
            if !show_answer && !committed {
                let label = match q.kind {
                    QuizKind::Predict => "Lock in my prediction",
                    QuizKind::Sketch => "Lock in my sketch",
                    _ => "Check",
                };
                r.spawn(k.button(label, LessonAction::QuizCheck(q.id.clone()), Look::Primary, true));
            }
            // "Show me": jump the scene to the moment this question is about.
            if let Some(t) = q.moment {
                if let Some(scene) = q.scene.clone().or_else(|| l.lesson.as_ref().and_then(|x| x.scenes().next().map(|(_, s)| s.id.clone()))) {
                    r.spawn(k.button("Show me in the scene", LessonAction::ShowAt { scene, time: Some(t), part: None }, Look::Ghost, true));
                }
            }
            // The hint ladder, one rung at a time.
            let ladder = q.hint_ladder();
            let used = record.map(|r| r.hints_used as usize).unwrap_or(0).min(ladder.len());
            if used < ladder.len() && !show_answer && !q.kind.predicts() && !q.pretest {
                r.spawn(k.button(&format!("Show a hint ({} of {})", used + 1, ladder.len()), LessonAction::HintMore(q.id.clone()), Look::Ghost, true));
            }
            if wrong_tries >= 2 && !show_answer && !q.kind.predicts() {
                r.spawn(k.button("Show the answer", LessonAction::QuizReveal(q.id.clone()), Look::Ghost, true));
            }
            // In a review session: on to the next due question.
            if let (true, Some((queue, at))) = (reviewing || l.quiz_verdict.get(&q.id).is_some_and(|v| v.correct), l.session.as_ref()) {
                if queue.get(*at).is_some_and(|(ls, qid)| ls == slug && qid == &q.id) {
                    let last = at + 1 >= queue.len();
                    r.spawn(k.button(if last { "Finish the review" } else { "Next review question" }, LessonAction::ReviewNext, Look::Primary, l.quiz_verdict.get(&q.id).is_some_and(|v| v.correct)));
                    r.spawn(k.text(format!("{} of {}", at + 1, queue.len()), 11.5, FAINT, 0));
                }
            }
        });
        let ladder = q.hint_ladder();
        let used = record.map(|r| r.hints_used as usize).unwrap_or(0).min(ladder.len());
        for (i, h) in ladder[..used].iter().enumerate() {
            c.spawn(k.text(format!("Hint {}: {h}", i + 1), 12.5, SUBTLE, 0));
        }
        // Feedback: this answer's, then the hint after a miss, then the worked explanation once answered.
        if let Some(v) = verdict {
            c.spawn(Node { column_gap: Val::Px(8.), align_items: AlignItems::FlexStart, ..default() }).with_children(|r| {
                // A prediction is only committed here; the run judges it below.
                r.spawn((k.text(if q.pretest { "Noted." } else if q.kind.predicts() && v.correct { "Locked in." } else if v.correct { "Right." } else { "Not yet." }, 13., if v.correct || q.pretest { OK } else { WARN }, 2), Node { flex_shrink: 0., ..default() }));
                r.spawn((k.text(if v.feedback.is_empty() { if v.correct { "Correct." } else { "Not quite — try again." } } else { v.feedback.as_str() }, 13., TEXT, 0), Node { flex_shrink: 1., min_width: Val::Px(0.), ..default() }));
            });
            if let (false, Some(r)) = (v.correct, l.remedy.get(&q.id)) {
                super::extras::remedy_card(c, k, l, r, theme);
            }
            if q.pretest && !q.explain.is_empty() {
                crate::markdown::render(c, &sim_markdown::parse(&q.explain), theme, |_| None::<LessonAction>);
            }
        }
        // Sketches: drawn over the simulated curve once the scene has played past the window.
        if q.kind == QuizKind::Sketch {
            let played = l.scene.as_ref().filter(|a| Some(&a.id) == q.scene.as_ref()).is_some_and(|a| a.run.is_some() && a.time >= l.sketch_window(q)[1].min(a.duration()) - 1e-6);
            match (predicted.is_some(), l.sketch_results.get(&q.id).filter(|_| played)) {
                (true, Some((image, gap, _))) => {
                    c.spawn((Node { border_radius: BorderRadius::all(Val::Px(4.)), width: Val::Percent(100.), aspect_ratio: Some(720. / 200.), flex_shrink: 0., ..default() }, ImageNode::new(image.clone())));
                    let close = *gap <= q.tolerance.map(|t| match t { sim_lesson::quiz::Tolerance::Absolute(v) | sim_lesson::quiz::Tolerance::Relative(v) => v }).unwrap_or(0.15);
                    c.spawn(k.text(format!("Purple: your sketch. Teal: the simulation. {} (typical gap {:.0} % of the axis).", if close { "Your curve has the right shape and size" } else { "Your curve differs from the simulation" }, gap * 100.), 13., if close { OK } else { WARN }, 1));
                    if !q.explain.is_empty() {
                        crate::markdown::render(c, &sim_markdown::parse(&q.explain), theme, |_| None::<LessonAction>);
                    }
                }
                (true, None) => {
                    c.spawn(k.text("Now play the scene and watch how the real curve compares with yours.", 12.5, SUBTLE, 0));
                }
                _ => {
                    c.spawn(k.text("Drag across the chart to draw the curve you expect. Sketching first makes the real result stick, even when the sketch is off.", 11.5, FAINT, 0));
                }
            }
        }
        // Predictions are judged against the recorded run once it has played.
        if q.kind == QuizKind::Predict {
            if let (Some(p), Some(a)) = (&predicted, l.scene.as_ref().filter(|a| Some(&a.id) == q.scene.as_ref())) {
                if let Some(run) = a.run.as_ref().filter(|_| a.time >= q.window.map(|w| w[1]).unwrap_or(a.duration()).min(a.duration()) - 1e-6) {
                    let verdict = match (sim_lesson::units::read(p, &q.unit).ok().map(|r| r.value), q.options.is_empty()) {
                        (Some(v), true) => runtime::predicted_value(run, q).map(|sim| q.check(&Answer::Number(v), Some(sim))),
                        _ => q.options.iter().position(|o| &o.text == p).map(|i| q.check(&Answer::Choice(i), None)),
                    };
                    if let Some(Ok(v)) = verdict {
                        let sim = runtime::predicted_value(run, q).map(|x| format!("The simulation gave {x:.4} {}. ", q.unit)).unwrap_or_default();
                        c.spawn(k.text(format!("{}{}{}", sim, if v.correct { "Your prediction was right. " } else { "Your prediction was off: " }, v.feedback), 13., if v.correct { OK } else { WARN }, 1));
                        if !q.explain.is_empty() {
                            crate::markdown::render(c, &sim_markdown::parse(&q.explain), theme, |_| None::<LessonAction>);
                        }
                    }
                } else {
                    c.spawn(k.text("Now play the scene to the end and see how close you were.", 12.5, SUBTLE, 0));
                }
            } else if predicted.is_none() {
                c.spawn(k.text("Commit to a guess before you watch: predicting first makes the result stick, even when the guess is wrong.", 11.5, FAINT, 0));
            }
        } else if show_answer {
            if let Some(o) = q.options.iter().find(|o| o.correct).filter(|_| record.is_some_and(|r| r.revealed && !r.correct)) {
                c.spawn(k.text(format!("Answer: {}", o.text), 13., OK, 1));
            }
            if let Some(a) = q.answer.filter(|_| record.is_some_and(|r| r.revealed && !r.correct)) {
                c.spawn(k.text(format!("Answer: {a} {}", q.unit), 13., OK, 1));
            }
            if !q.explain.is_empty() {
                c.spawn(k.section("Why"));
                crate::markdown::render(c, &sim_markdown::parse(&q.explain), theme, |_| None::<LessonAction>);
            }
        }
    });
}

/// A self-explanation prompt.
pub(super) fn reflect_card(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, b: &sim_lesson::Block, r: &Reflect, theme: &crate::markdown::Theme) {
    let slug = l.slug().unwrap_or_default();
    let saved = l.progress.lessons.get(slug).and_then(|p| p.reflections.get(&r.id));
    let focused = l.input.as_ref().is_some_and(|i| i.purpose == Purpose::Reflection(r.id.clone()));
    col.spawn((Node { border_radius: BorderRadius::all(Val::Px(7.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.), padding: UiRect::all(Val::Px(16.)), border: UiRect::left(Val::Px(3.)), margin: UiRect::vertical(Val::Px(8.)), flex_shrink: 0., ..default() }, BackgroundColor(RAISED), BorderColor::all(Color::srgb(0.93, 0.76, 0.40)), super::ui::BlockNode(b.id.clone()))).with_children(|c| {
        let recall = r.kind == sim_lesson::quiz::ReflectKind::Recall;
        c.spawn(badge(k, if recall { "Recall first" } else { "Explain it in your own words" }, Color::srgb(0.93, 0.76, 0.40)));
        crate::markdown::render(c, &sim_markdown::parse(&r.prompt), theme, |_| None::<LessonAction>);
        let text = if focused { l.input.as_ref().map(|i| i.buffer.clone()).unwrap_or_default() } else { saved.map(|s| s.text.clone()).unwrap_or_default() };
        c.spawn((Button, LessonAction::ReflectInput(r.id.clone()), Tint { idle: SURFACE, hover: HOVER_BG }, Node { border_radius: BorderRadius::all(Val::Px(6.)), min_height: Val::Px(70.), padding: UiRect::all(Val::Px(10.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() }, BackgroundColor(SURFACE), BorderColor::all(if focused { crate::ACCENT } else { BORDER }))).with_children(|f| {
            f.spawn(k.text(if focused { format!("{text}|") } else if text.is_empty() { "Click and write two or three sentences…".into() } else { text.clone() }, 13.5, if text.is_empty() && !focused { FAINT } else { TEXT }, 0));
        });
        c.spawn(wrap()).with_children(|row| {
            if focused {
                row.spawn(k.button("Save", LessonAction::ReflectSave(r.id.clone()), Look::Primary, true));
                row.spawn(k.button("Cancel", LessonAction::CancelDraft, Look::Ghost, true));
                row.spawn(k.text("Cmd/Ctrl+Enter saves · kept on this machine only", 10.5, FAINT, 0));
            } else if saved.is_some() {
                row.spawn(k.text("Saved", 11.5, OK, 1));
            }
        });
        // After writing: which key ideas the text covers (cue words, instant, on this machine).
        if let (Some(saved), false, false) = (saved, focused, r.key_points.is_empty()) {
            let coverage = sim_lesson::quiz::coverage(&saved.text, &r.key_points);
            if recall {
                c.spawn(k.section("Tick what you remembered"));
                let ticked = l.progress.lessons.get(slug).and_then(|p| p.recalled.get(&r.id)).cloned().unwrap_or_default();
                for (i, (idea, seen)) in coverage.iter().enumerate() {
                    let on = ticked.contains(idea);
                    c.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), ..default() }).with_children(|row| {
                        row.spawn(k.button(&format!("{} {idea}", if on { "[x]" } else { "[ ]" }), LessonAction::RecallTick(r.id.clone(), i), Look::Chip(on), true));
                        if *seen && !on {
                            row.spawn(k.text("your text mentions it", 10.5, FAINT, 0));
                        }
                    });
                }
                c.spawn(k.text("Anything you did not tick is worth rereading before you go on.", 11., FAINT, 0));
            } else {
                c.spawn(k.section("What your explanation covers"));
                for (idea, seen) in &coverage {
                    c.spawn(k.text(format!("{} {idea}", if *seen { "covered:" } else { "not yet:" }), 12., if *seen { OK } else { SUBTLE }, 0));
                }
                c.spawn(k.text("A quick word check, not a grade: it only looks for the key ideas' words.", 10.5, FAINT, 0));
            }
        }
        if saved.is_some() && !focused && !recall {
            c.spawn(wrap()).with_children(|row| {
                row.spawn(k.button("Ask Codex for feedback", LessonAction::ReflectFeedback(r.id.clone()), Look::Ghost, true));
                row.spawn(k.text("posted as a note on this prompt; the reply appears there", 10.5, FAINT, 0));
            });
        }
        if saved.is_some() && !r.model_answer.is_empty() && !focused {
            c.spawn(k.section("Compare with an expert's explanation"));
            crate::markdown::render(c, &sim_markdown::parse(&r.model_answer), theme, |_| None::<LessonAction>);
        }
    });
}

/// What stands in for the locked rest of the lesson.
pub(super) fn locked(col: &mut ChildSpawnerCommands, k: &Kit, hidden: usize) {
    col.spawn((Node { border_radius: BorderRadius::all(Val::Px(9.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.), padding: UiRect::all(Val::Px(18.)), border: UiRect::all(Val::Px(1.)), align_items: AlignItems::Center, margin: UiRect::vertical(Val::Px(10.)), flex_shrink: 0., ..default() }, BorderColor::all(BORDER))).with_children(|c| {
        c.spawn(k.text("The next part unlocks when you answer the question above", 14., TEXT, 2));
        c.spawn(k.text(format!("{hidden} more block{} below. Take your time: working it out is what makes it stick.", if hidden == 1 { "" } else { "s" }), 12., SUBTLE, 0));
    });
}

/// Review list in the outline (questions due across lessons).
pub(super) fn review_outline(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn) {
    let due = l.progress.due(sim_lesson::progress::now());
    if due.is_empty() {
        return;
    }
    col.spawn(k.section(&format!("Review · {} due", due.len())));
    col.spawn(k.text("Spaced retrieval: answer from memory; each success pushes the next review further out.", 10.5, FAINT, 0));
    if due.len() > 1 && l.session.is_none() {
        col.spawn(k.button(&format!("Start a mixed review ({})", due.len()), LessonAction::ReviewSession, Look::Secondary, true));
        col.spawn(k.text("Questions from different lessons, interleaved: harder, and it sticks better.", 10.5, FAINT, 0));
    }
    if let Some((queue, at)) = &l.session {
        col.spawn(k.text(format!("Mixed review: {} of {}", at + 1, queue.len()), 11.5, crate::ACCENT, 1));
        col.spawn(k.button("End the review", LessonAction::ReviewEnd, Look::Ghost, true));
    }
    for (lesson, quiz) in due.iter().take(6) {
        let title = l.entries.iter().find(|e| &e.slug == lesson).map(|e| e.title.clone()).unwrap_or_else(|| lesson.clone());
        col.spawn(k.button(&format!("{title} · {quiz}"), LessonAction::Review(lesson.clone(), quiz.clone()), Look::Ghost, true));
    }
}

/// A figure: the image at its natural aspect ratio, and its caption.
pub(super) fn figure(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, src: &str, caption: &str) {
    let Some(lesson) = &l.lesson else { return };
    let path = lesson.figure_path(src);
    let id = std::path::Path::new(src).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    col.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.), align_items: AlignItems::Center, margin: UiRect::vertical(Val::Px(8.)), width: Val::Percent(100.), flex_shrink: 0., ..default() }, super::ui::BlockNode(format!("figure:{id}")))).with_children(|c| {
        match l.figures.get(&path).map(|f| &f.state) {
            Some(FigureState::Ready { image, aspect, units }) => {
                c.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), width: Val::Percent(100.), max_width: Val::Px(760.), aspect_ratio: Some(*aspect), ..default() }, ImageNode::new(image.clone()), super::narrate::FigureNode(id.clone(), units.0, units.1)));
            }
            Some(FigureState::Failed(e)) => {
                c.spawn(k.text(format!("Figure {src}: {e}"), 12., WARN, 0));
            }
            _ => {
                c.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), width: Val::Percent(100.), height: Val::Px(160.), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, BackgroundColor(SURFACE), children![k.text("Drawing figure…", 12., FAINT, 0)]));
            }
        }
        if !caption.is_empty() {
            c.spawn((Text::new(caption), TextFont { font: k.f.italic.clone().into(), font_size: FontSize::Px(12.5), ..default() }, TextColor(SUBTLE), TextLayout::new(Justify::Center, bevy::text::LineBreak::WordBoundary), Node { max_width: Val::Px(680.), ..default() }));
        }
    });
}

/// Columns in a sketch.
pub(crate) const SKETCH_COLUMNS: usize = 96;

/// Where the pointer draws a sketch.
#[derive(Component)]
pub(crate) struct SketchCanvas(pub String);
/// One column's dot of a sketch.
#[derive(Component)]
pub(crate) struct SketchDot(pub String, pub usize);

fn sketch_canvas(c: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, q: &Quiz, locked: bool) {
    let [t0, t1] = l.sketch_window(q);
    let [lo, hi] = q.range.unwrap_or([0., 1.]);
    let label = q.observe.clone().unwrap_or_default();
    c.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|r| {
        r.spawn(k.text(format!("{label} {}", if q.unit.is_empty() { String::new() } else { format!("({})", q.unit) }), 11.5, TEXT, 1));
        r.spawn(k.text(format!("{} … {}", crate::builder::ui::num(lo), crate::builder::ui::num(hi)), 10.5, FAINT, 0));
    });
    let mut canvas = c.spawn((Node { border_radius: BorderRadius::all(Val::Px(4.)), width: Val::Percent(100.), aspect_ratio: Some(720. / 200.), flex_shrink: 0., border: UiRect::all(Val::Px(1.)), ..default() }, BackgroundColor(Color::srgb(0.07, 0.086, 0.106)), BorderColor::all(BORDER)));
    if !locked {
        canvas.insert((Button, SketchCanvas(q.id.clone()), bevy::ui::RelativeCursorPosition::default()));
    }
    canvas.with_children(|cv| {
        for g in 1..4 {
            cv.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.), right: Val::Px(0.), top: Val::Percent(25. * g as f32), height: Val::Px(1.), ..default() }, BackgroundColor(Color::srgb(0.16, 0.19, 0.23)), Pickable::IGNORE));
        }
        for i in 0..SKETCH_COLUMNS {
            cv.spawn((Node { border_radius: BorderRadius::all(Val::Px(2.5)), position_type: PositionType::Absolute, width: Val::Px(5.), height: Val::Px(5.), margin: UiRect { left: Val::Px(-2.5), top: Val::Px(-2.5), ..default() }, display: Display::None, ..default() }, BackgroundColor(Color::srgb(0.70, 0.58, 0.96)), SketchDot(q.id.clone(), i), Pickable::IGNORE));
        }
    });
    c.spawn(k.text(format!("time {} … {} s", crate::builder::ui::num(t0), crate::builder::ui::num(t1)), 10.5, FAINT, 0));
}

/// Drawing: dragging over a canvas sets each column the pointer crosses.
pub(super) fn sketch_input(canvases: Query<(&Interaction, &bevy::ui::RelativeCursorPosition, &SketchCanvas)>, mut learn: ResMut<Learn>, mut last: Local<Option<(String, usize, f32)>>) {
    let mut drawing = false;
    for (interaction, cursor, canvas) in &canvases {
        let (Interaction::Pressed, Some(p)) = (interaction, crate::view::cursor_fraction(cursor)) else { continue };
        drawing = true;
        let n = SKETCH_COLUMNS;
        let col = ((p.x.clamp(0., 0.9999)) * n as f32) as usize;
        let y = (1. - p.y).clamp(0., 1.);
        let columns = learn.sketches.entry(canvas.0.clone()).or_insert_with(|| vec![None; n]);
        // Fill the columns skipped since the last sample, so fast strokes stay continuous.
        match &*last {
            Some((id, c0, y0)) if id == &canvas.0 && *c0 != col => {
                let (a, b) = (*c0.min(&col), *c0.max(&col));
                for c in a..=b {
                    let f = (c as f32 - *c0 as f32) / (col as f32 - *c0 as f32);
                    columns[c] = Some(y0 + (y - y0) * f);
                }
            }
            _ => columns[col] = Some(y),
        }
        *last = Some((canvas.0.clone(), col, y));
    }
    if !drawing {
        *last = None;
    }
}

/// Place the dots of every sketch where its columns are.
pub(super) fn sketch_dots(learn: Res<Learn>, mut dots: Query<(&mut Node, &SketchDot)>) {
    if !learn.is_changed() {
        return;
    }
    let mut columns = std::collections::BTreeMap::new();
    for (mut node, dot) in &mut dots {
        let y = columns.entry(dot.0.clone()).or_insert_with(|| learn.sketch_columns(&dot.0)).as_ref().and_then(|c| c.get(dot.1).copied().flatten());
        let display = if y.is_some() { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        if let Some(y) = y {
            let left = Val::Percent((dot.1 as f32 + 0.5) / SKETCH_COLUMNS as f32 * 100.);
            let top = Val::Percent((1. - y) * 100.);
            if node.left != left || node.top != top {
                node.left = left;
                node.top = top;
            }
        }
    }
}

/// The comparison image and gap for a committed sketch, once its scene has a run.
pub(super) fn sketch_result(run: &sim_runtime::lesson::SceneRun, q: &Quiz, prediction: &str, window: [f64; 2]) -> Option<(Image, f64)> {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let sketch: Vec<[f64; 2]> = serde_json::from_str(prediction).ok()?;
    let series = run.series(q.observe.as_deref()?)?;
    let range = q.range?;
    let gap = sim_lesson::quiz::sketch_error(&sketch, &series.times, &series.values, range)?;
    let simulated: Vec<[f64; 2]> = series.times.iter().zip(&series.values).filter(|(t, _)| **t >= window[0] && **t <= window[1]).map(|(t, v)| [*t, *v]).collect();
    // Pin the axes to the question's so both curves sit where the reader drew.
    let frame = [[window[0], range[0]], [window[1], range[1]]];
    let (pixels, _, _) = crate::chart::rasterize_span(&[(&frame, [18, 22, 27]), (&simulated, [77, 212, 191]), (&sketch, [178, 148, 245])], None);
    let (w, h) = crate::chart::RASTER;
    Some((Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, pixels, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default()), gap))
}
