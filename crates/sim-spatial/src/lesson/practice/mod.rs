//! Practice and pacing in the Learn screen: question cards (choice,
//! numeric, predict), self-explanation prompts, mastery gating (content
//! after an unanswered question stays locked), prediction-locked scenes,
//! spaced review across lessons, and figures (SVG rasterized off the UI
//! thread, PNG/JPEG decoded there too, hot-reloaded when the file changes).
use super::*;
use crate::ui_kit::{ACCENT, BORDER, FAINT, HOVER_BG, Kit, Look, OK, RAISED, SUBTLE, SURFACE, TEXT, Tint, WARN, surface_point, wrap};
use bevy::ui::prelude::AccessibleLabel;
use sim_lesson::progress::Mode;
use sim_lesson::quiz::{Answer, Quiz, QuizKind, Reflect, Verdict};

mod cards;
mod sketch;

pub(super) use cards::{figure, locked, quiz_card, reflect_card, review_outline};
pub(crate) use sketch::SKETCH_COLUMNS;
pub(super) use sketch::{sketch_dots, sketch_input, sketch_result};

pub(crate) enum FigureState {
    Loading(crate::jobs::Job<(Image, (f32, f32))>),
    Ready { image: Handle<Image>, aspect: f32, units: (f32, f32) },
    Failed(String),
}
pub(crate) struct CachedFigure {
    pub state: FigureState,
    stamp: Option<std::time::SystemTime>,
}

fn figure_job(path: PathBuf) -> crate::jobs::Job<(Image, (f32, f32))> {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    // Read and decode or rasterise: CPU work.
    crate::jobs::Job::spawn(crate::jobs::Pool::Compute, 0, "the figure loader", move |_| {
        std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display())).and_then(|bytes| {
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
        })
    })
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
                self.figures.insert(path.clone(), CachedFigure { state: FigureState::Loading(figure_job(path)), stamp });
            }
        }
    }
    /// Collect finished figure jobs (needs the image assets).
    pub(crate) fn poll_figures(&mut self, images: &mut Assets<Image>) -> bool {
        let mut changed = false;
        for c in self.figures.values_mut() {
            let FigureState::Loading(job) = &c.state else { continue };
            let Some(result) = job.poll() else { continue };
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
