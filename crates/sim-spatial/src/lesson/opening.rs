//! Opening lessons and making scenes live: loading the lesson file, the
//! scene's sandbox, builder and recorded run (off the UI thread).
use super::*;

/// Lessons in `dir` as a lessons launch opens them: the builder on the
/// first scene's sandbox (or an empty system), its compiled scene, and
/// `slug` (default: the first readable lesson) opened, its scene activating
/// off the UI thread. The launch and a switch to lessons mode share it; the
/// last value is a lesson that did not open ("Lesson {slug}: {error}").
pub fn open_lessons(dir: PathBuf, slug: Option<String>, library: PathBuf, registry: sim_core::BehaviorRegistry) -> Result<(Learn, Builder, SpatialScene, Option<String>), String> {
    let mut learn = Learn::new(dir.clone(), library.clone(), registry.clone());
    let slug = slug.or_else(|| learn.entries.iter().find(|e| e.error.is_none()).map(|e| e.slug.clone()));
    // The builder starts on the first scene's sandbox (or an empty system);
    // opening the lesson then activates that scene off the UI thread.
    let first = slug.as_ref().and_then(|s| {
        let lesson = Lesson::load(&dir.join(s).join("lesson.md")).ok()?;
        let scene = lesson.scenes().next().map(|(_, sc)| sc.clone())?;
        runtime::sandbox(&lesson, &scene, &registry, false).ok().map(|sb| sb.path)
    });
    let initial = match first {
        Some(p) => p,
        None => {
            let p = runtime::sandbox_root().join("_empty").join("empty.system.json");
            if !p.exists() {
                sim_system::SystemStore::create(&p, &sim_system::SystemDocument::new("Lesson")).map_err(|e| format!("{}: {e}", p.display()))?;
            }
            p
        }
    };
    let builder = Builder::open(initial, library, registry)?;
    let scene = crate::builder::compiled_scene(&builder)?;
    let warning = slug.as_ref().and_then(|s| learn.open(s).err().map(|e| format!("Lesson {s}: {e}")));
    Ok((learn, builder, scene, warning))
}

impl Learn {
    /// Open a lesson by slug (or path).
    pub fn open(&mut self, slug: &str) -> Result<(), String> {
        let path = self.entries.iter().find(|e| e.slug == slug).map(|e| e.path.clone()).unwrap_or_else(|| self.dir.join(slug).join("lesson.md"));
        self.scene = None;
        self.compares.clear();
        self.thread = None;
        self.draft = None;
        self.picked = None;
        self.input = None;
        self.scroll = 0.;
        self.scroll_to = None;
        self.quiz_pick.clear();
        self.quiz_verdict.clear();
        self.quiz_text.clear();
        self.review = None;
        self.visited.insert(slug.to_string());
        self.load(&path);
        let lesson = self.lesson.clone().ok_or_else(|| self.lesson_error.clone().unwrap_or_default())?;
        let annotations = PathBuf::from(format!("{}.annotations.json", lesson.path.display()));
        self.notes = Some(Notes::new(Arc::new(lesson.slug.clone()), annotations));
        self.notes_doc = ThreadDocument::new(&lesson.slug);
        self.agent = agent::LessonAgent::open(&lesson.slug, &self.registry);
        if let Some(p) = &self.player {
            p.stop();
        }
        self.narration_part = None;
        self.load_narration();
        self.tasks.clear();
        self.labs.clear();
        self.remedy.clear();
        self.step_text.clear();
        self.confidence.clear();
        self.draw_variants();
        self.start_model();
        // The first scene not waiting on a prediction becomes live.
        let first = lesson.scenes().map(|(_, s)| s.id.clone()).find(|id| self.scene_locked(id).is_none());
        if let Some(first) = first {
            self.activate(&first);
        }
        self.status = format!("{} · {}", lesson.meta.title, lesson.path.display());
        self.dirty = true;
        Ok(())
    }

    pub(super) fn load(&mut self, path: &std::path::Path) {
        self.stamp = lesson_stamp(path);
        match Lesson::load(path) {
            Ok(l) => {
                let mut index = l.text_index();
                if let Some(a) = &self.scene {
                    if let Some(sb) = &a.sandbox {
                        if let Ok(doc) = runtime::load_system(&sb.path, &self.registry) {
                            index.with_parts(&a.id, runtime::instance_paths(&doc));
                        }
                    }
                }
                self.index = Some(index);
                self.lesson = Some(l);
                self.lesson_error = None;
                self.load_figures();
            }
            Err(e) => {
                // Keep showing the last good version; say what broke.
                self.lesson_error = Some(e.to_string());
                if self.lesson.as_ref().is_none_or(|l| l.path != path) {
                    self.lesson = None;
                    self.index = None;
                }
            }
        }
        self.dirty = true;
    }

    /// Make a scene the live one: sandbox, builder, recorded run (all off the UI thread).
    pub fn activate(&mut self, id: &str) {
        let Some(lesson) = self.lesson.clone() else { return };
        if let Some(q) = self.scene_locked(id) {
            self.status = format!("Answer the prediction `{q}` first: the scene unlocks once you commit to a guess.");
            self.scroll_to = Some(q);
            return;
        }
        let Some(scene) = lesson.scene(id).map(|s| runtime::lesson_scene(&lesson, s)) else {
            self.status = format!("No scene `{id}` in this lesson");
            return;
        };
        self.activate_scene(scene);
    }

    /// Make a prepared scene live (a lesson scene, or a task's own scene).
    pub(crate) fn activate_scene(&mut self, scene: Scene) {
        let Some(lesson) = self.lesson.clone() else { return };
        let id = scene.id.clone();
        let id = id.as_str();
        let timeline = match lesson.timeline(&scene) {
            Ok(t) => t,
            Err(e) => {
                self.status = format!("Scene {id}: {e}");
                Timeline::default()
            }
        };
        let (registry, library) = (self.registry.clone(), self.library.clone());
        let (sc, l) = (scene.clone(), lesson.clone());
        // Recordings can take a while: a dedicated thread.
        let jobs = crate::jobs::Job::streaming(crate::jobs::Pool::Dedicated, 0, "the scene recording", move |ctx| {
            let prepared = runtime::sandbox(&l, &sc, &registry, false).and_then(|sb| Builder::open(sb.path.clone(), library, registry.clone()).map(|b| (sb, Box::new(b))));
            let ok = prepared.is_ok();
            if !ctx.emit(Stage::Builder(prepared)) || !ok {
                return Ok(());
            }
            if !ctx.emit(Stage::Run(record(&l, &sc, &BTreeMap::new(), &registry, ctx))) {
                return Ok(());
            }
            if let Some(companion) = &sc.companion {
                ctx.emit(Stage::Companion(record(&l, &sc, &companion.set, &registry, ctx)));
            }
            Ok(())
        });
        self.scene = Some(ActiveScene {
            id: id.into(),
            scene,
            timeline,
            sandbox: None,
            run: None,
            error: None,
            time: 0.,
            wall: 0.,
            plan: Default::default(),
            playing: false,
            user_speed: 1.0,
            installed: false,
            jobs: Some(jobs),
            recording: true,
            framed: false,
            framed_aspect: 0.,
            camera_cue: None,
            highlight: Vec::new(),
            charts: Vec::new(),
            phase_charts: Vec::new(),
            show_pending: true,
            zoom_cue: None,
            zoomed_part: None,
            explore: false,
            overrides: BTreeMap::new(),
            slider_drag: None,
            companion: None,
            challenge: None,
            challenge_met: None,
            stop_at: None,
        });
        self.picked = None;
        self.dirty = true;
    }

    /// Record the active scene again from its sandbox, with the reader's
    /// slider values (cached when unchanged).
    pub(crate) fn rerecord(&mut self) {
        let Some(lesson) = self.lesson.clone() else { return };
        let Some(a) = self.scene.as_mut() else { return };
        let (registry, scene, overrides) = (self.registry.clone(), a.scene.clone(), a.overrides.clone());
        // Replacing the job cancels a recording still running.
        a.jobs = Some(crate::jobs::Job::streaming(crate::jobs::Pool::Dedicated, 0, "the scene recording", move |ctx| {
            ctx.emit(Stage::Run(record(&lesson, &scene, &overrides, &registry, ctx)));
            Ok(())
        }));
        a.recording = true;
        self.dirty = true;
    }

    pub(super) fn reset_sandbox(&mut self) {
        let (Some(lesson), Some(a)) = (self.lesson.clone(), self.scene.as_ref()) else { return };
        match runtime::sandbox(&lesson, &a.scene, &self.registry, true) {
            Ok(_) => {
                let id = a.id.clone();
                self.activate(&id);
                self.status = "Reset the scene's builder copy to the lesson's system.".into();
            }
            Err(e) => self.status = e,
        }
    }

    /// A part was clicked in the live scene.
    pub fn pick(&mut self, scene: &mut SpatialScene, component: &str) {
        let Some(a) = &self.scene else { return };
        if self.mode == PageMode::Annotate {
            self.draft = Some(LessonAnchor::Scene { scene: a.id.clone(), part: Some(component.into()), time_s: Some((a.time * 1000.).round() / 1000.), missing: false });
            self.thread = None;
            self.input = Some(Input { purpose: Purpose::Comment, buffer: String::new() });
        } else {
            self.picked = Some(component.into());
        }
        let _ = scene.set_selection(crate::builder::discussion::selection(scene, &[component.to_string()]));
        self.dirty = true;
    }

    pub(super) fn goto_part(&mut self, system: &str, path: &str) {
        let Some(lesson) = &self.lesson else { return };
        let same = self.scene.as_ref().is_some_and(|a| a.scene.system == system);
        let first = lesson.scenes().find(|(_, s)| s.system == system).map(|(_, s)| s.id.clone());
        if !same {
            if let Some(id) = first {
                self.activate(&id);
                self.scroll_to = Some(id);
            }
        }
        self.picked = Some(path.into());
        self.dirty = true;
    }
}

/// Record a scene from its sandbox (the learner's copy), cached by hash.
/// Record a scene from its sandbox, with `extra` parameter values on top
/// (the reader's sliders, or a companion run's `set`).
fn record(lesson: &Lesson, scene: &Scene, extra: &BTreeMap<String, f64>, registry: &sim_core::BehaviorRegistry, ctx: &crate::jobs::Ctx<Stage>) -> Result<SceneRun, String> {
    let sb = runtime::sandbox(lesson, scene, registry, false)?;
    let doc = runtime::load_system(&sb.path, registry)?;
    let mut sc = runtime::sandbox_scene(scene);
    sc.set.extend(extra.iter().map(|(k, v)| (k.clone(), *v)));
    let doc = runtime::scene_document(&doc, registry, &sc)?;
    let timeline = lesson.timeline(scene)?;
    // Reported in thousandths, as before.
    runtime::scene_run(&doc, registry, &sc, &timeline, true, Some(ctx.cancel_flag()), &|f| ctx.fraction(((f * 1000.) as u32) as f64 / 1000.0))
}
