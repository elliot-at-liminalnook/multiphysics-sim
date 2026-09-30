//! Learn mode: lessons (Markdown with embedded system scenes) read in the
//! physical viewer, around the same scene, builder and runtime.
//!
//! - The lesson text is `sim_lesson`; scene runs, claims, sandboxes and
//!   comparisons are `sim_runtime::lesson`. Nothing here simulates.
//! - One scene is live at a time. Activating it copies its system into a
//!   sandbox, opens the builder on that copy (off the UI thread) and records
//!   the scene's run on the shared runtime (cached by hash). The 3D view is
//!   drawn inside the scene's card by pointing the camera viewport at the
//!   card's rectangle; playback replays the recorded frames.
//! - "Open in builder" shows the builder on the sandbox; "‹ lesson" comes
//!   back, and the scene re-records if the copy changed.
//! - Lesson notes are `sim_annotate` threads anchored to quoted text or to
//!   scene parts (`sim_lesson::LessonAnchor`), in `lesson.md.annotations.json`.
use crate::app::actions::Act;
use crate::builder::Builder;
use crate::{Orbit, SpatialScene};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use sim_annotate::{Comment, Thread, ThreadCommand, ThreadDocument};
use sim_lesson::edit::{Edit, LessonStore};
use sim_lesson::{BlockKind, Lesson, LessonAnchor, LessonIndex, Scene};
use sim_runtime::lesson::{self as runtime, CompareRun, Sandbox, SceneRun};
use sim_script::presentation::{CameraSpec, Timeline};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

mod agent;
pub(crate) mod extras;
pub(crate) mod frames;
pub(crate) mod narrate;
pub(crate) mod practice;
pub(crate) mod actions;
mod ui;

pub type Notes = sim_annotate::store::Store<ThreadDocument<LessonAnchor>>;

/// How clicks on the page behave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageMode {
    #[default]
    Read,
    /// Clicking a paragraph or a part starts a note on it.
    Annotate,
    /// Blocks show edit, insert and delete.
    Edit,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Purpose {
    Comment,
    Author,
    /// Editing block (id, hash).
    Block(String, String),
    /// New block after (id) or at the top.
    NewBlock(Option<String>),
    /// Editing an existing comment of the selected thread.
    EditComment(String),
    /// Typing a numeric answer or prediction.
    QuizAnswer(String),
    /// Writing a self-explanation.
    Reflection(String),
    /// A blank step of a steps question: (question, step index).
    StepAnswer(String, usize),
    /// The reader's prediction for a lab step.
    LabPrediction(String),
}

#[derive(Clone, Debug)]
pub(crate) struct Input {
    pub purpose: Purpose,
    pub buffer: String,
}

#[derive(Component, Clone, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LessonAction {
    Open(String),
    /// Fold or unfold a category in the lesson list.
    ToggleCategory(String),
    Goto(String),
    Activate(String),
    Play,
    Pause,
    Restart,
    Speed(f64),
    /// The timebar's marker: pressing it is read as [`LessonAction::SeekTo`]
    /// by `seek` (the time comes from the pointer).
    Seek,
    /// Seek the live scene to `time` (s) and pause. `rewind`: count a jump
    /// back as a rewind (the press that starts a timebar drag; REST never
    /// counts). `None`: "seek needs time", after the scene checks.
    SeekTo { time: Option<f64>, rewind: bool },
    /// A click on a lesson chart: keep the scene at that moment (s). Unlike
    /// `SeekTo` it does not pause and does not rebuild the page (the chart's
    /// hover already shows the moment; this keeps it when the pointer leaves).
    KeepMoment(f64),
    /// A click on a part of the live scene (`pick_part`): a note draft on it in
    /// Annotate mode, else the picked part.
    Pick(String),
    /// Set a slider (snapped to its step) and re-record, like letting go of it.
    Slider { parameter: String, value: f64 },
    OpenBuilder,
    ResetSandbox,
    SaveSandbox,
    Part(String, String),
    Link(String),
    RunCompare(String),
    Mode(PageMode),
    AnnotateBlock(String),
    NoteOnScene,
    NoteOnPart,
    ClearPart,
    EditBlock(String),
    InsertAfter(Option<String>),
    DeleteBlock(String),
    SaveEdit,
    CancelEdit,
    OpenEditor,
    Undo,
    Redo,
    ShowAt { scene: String, time: Option<f64>, part: Option<String> },
    ThreadList,
    OpenThread(String),
    Compose,
    Submit,
    CancelDraft,
    CommentMenu(String),
    EditComment(String),
    DeleteComment(String),
    Resolve,
    DeleteThread,
    Author,
    OpenOnly,
    Ask,
    AgentCancel(String),
    Narrate(narrate::NarrateAction),
    QuizPick(String, usize),
    QuizCheck(String),
    QuizReveal(String),
    QuizInput(String),
    ReflectInput(String),
    ReflectSave(String),
    /// Wipe a sketch question's drawing.
    SketchClear(String),
    /// Back to the lesson's parameter values.
    ResetSliders,
    /// Switch between the guided (narrated, paced) scene and free exploration.
    Explore,
    /// Open a lesson at a question due for spaced review.
    Review(String, String),
    /// Reveal the next hint of a question's ladder.
    HintMore(String),
    /// How sure the reader is about the answer they are about to check (0 guess, 1 fairly sure, 2 sure).
    Confidence(String, u8),
    /// Type into a blank step of a steps question.
    StepInput(String, usize),
    /// Recall prompts: tick a key point as remembered.
    RecallTick(String, usize),
    /// Ask Codex for feedback on a saved self-explanation (posted as a note).
    ReflectFeedback(String),
    /// Start a note on the live scene at this moment (and the picked part), and ask Codex once it is saved.
    AskMoment,
    TaskStart(String),
    TaskCheck(String),
    TaskHint(String),
    LabTick(String, usize),
    LabPredict(String),
    LabRun(String),
    /// A mixed review session across lessons; next question; stop.
    ReviewSession,
    ReviewNext,
    ReviewEnd,
    /// Reading and motion preferences.
    Setting(Setting),
    /// Keyboard focus for a slider (←/→ then nudge it).
    SliderFocus(String),
    /// ←/→ with a focused slider: nudge it by one step (true: Shift, ten steps).
    SliderStep(i8, bool),
    /// ←/→: the previous (-1) or next (+1) event of the live scene.
    EventStep(i8),
    /// Escape outside a draft: drop the note draft's target and the picked part.
    ClearPick,
}

/// Reader preferences (saved per machine, like progress).
#[derive(Component, Clone, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Setting {
    ReducedMotion,
    TextSize(f32),
    NarrationSpeed(f32),
    Transcript,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    /// No camera glides, orbiting, explode animation or motion blur.
    pub reduced_motion: bool,
    /// Interface text scale (UiScale).
    pub text_scale: f32,
    /// Narration playback rate (time-stretched, pitch kept).
    pub narration_speed: f32,
    /// Show the narration's words under the player.
    pub transcript: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self { reduced_motion: false, text_scale: 1.0, narration_speed: 1.0, transcript: false }
    }
}
impl Settings {
    pub fn path() -> PathBuf {
        // SIM_LESSON_SETTINGS, else the workspace's runs/; with no root, the sandbox's parent (reported at launch).
        std::env::var_os("SIM_LESSON_SETTINGS").map(PathBuf::from).unwrap_or_else(|| crate::workspace::path("runs/lessons/settings.json").unwrap_or_else(|_| sim_runtime::lesson::sandbox_root().with_file_name("settings.json")))
    }
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        sim_annotate::store::write_atomic(path, &serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?)
    }
}

/// Where the reader was when they opened the builder.
#[derive(Clone, Debug)]
struct Return {
    scroll: f32,
}

pub(crate) struct Chart {
    pub key: String,
    pub unit: String,
    pub image: Handle<Image>,
    pub range: (f64, f64),
    pub window: (f64, f64),
}

/// An operating-point plot: the whole path of `y` against `x`, and the
/// sample-at-playhead drawn as a dot over it.
pub(crate) struct PhaseChart {
    pub title: String,
    pub x: (String, String),
    pub y: (String, String),
    /// Observable IDs of x and y.
    pub ids: (String, String),
    pub image: Handle<Image>,
    pub range: (f64, f64),
    pub window: (f64, f64),
}

enum Stage {
    /// Sandbox + builder ready.
    Builder(Result<(Sandbox, Box<Builder>), String>),
    Run(Result<SceneRun, String>),
    /// The scene's companion run (split or ghost view).
    Companion(Result<SceneRun, String>),
}

pub struct ActiveScene {
    pub id: String,
    pub scene: Scene,
    pub timeline: Timeline,
    pub sandbox: Option<Sandbox>,
    pub run: Option<Arc<SceneRun>>,
    pub error: Option<String>,
    /// Simulated time shown (read it; move it with [`ActiveScene::seek`]).
    pub time: f64,
    /// Wall-clock position in the pacing plan (s).
    wall: f64,
    /// How the recorded run plays on screen: holds to read and look,
    /// automatic slow motion where plotted quantities change fast.
    pub(crate) plan: sim_script::pacing::PlaybackPlan,
    pub playing: bool,
    pub user_speed: f64,
    /// The builder for this scene's sandbox is installed.
    pub installed: bool,
    /// The scene's preparation and recordings, streamed as stages (progress:
    /// the recording's fraction). Dropping or replacing it cancels the run.
    jobs: Option<crate::jobs::Job<(), Stage>>,
    recording: bool,
    framed: bool,
    /// The view's aspect ratio when it was framed: a large change (the card
    /// finishing its layout, a resized window) frames it again.
    framed_aspect: f32,
    camera_cue: Option<f64>,
    highlight: Vec<String>,
    pub(crate) charts: Vec<Chart>,
    pub(crate) phase_charts: Vec<PhaseChart>,
    /// The scene's `show` layers still need applying to the view.
    pub(crate) show_pending: bool,
    /// The scene zoom cue (by time) last acted on.
    zoom_cue: Option<f64>,
    /// The part the camera last flew to (a clicked link or part).
    zoomed_part: Option<String>,
    /// Free exploration: no reading holds, narration paused, hints shown.
    pub(crate) explore: bool,
    /// The reader's slider values (`instance/path.parameter` → value).
    pub(crate) overrides: BTreeMap<String, f64>,
    /// A slider being dragged, and its value so far.
    pub(crate) slider_drag: Option<(String, f64)>,
    /// The companion run, beside or under this one.
    pub(crate) companion: Option<Arc<SceneRun>>,
    /// The challenge's goals judged on the latest run.
    pub(crate) challenge: Option<Vec<runtime::CheckResult>>,
    /// A challenge just met with these values, to record in progress.
    challenge_met: Option<String>,
    /// Stop playback at this time (narration `play-until`).
    pub(crate) stop_at: Option<f64>,
}
impl ActiveScene {
    pub fn duration(&self) -> f64 {
        self.run.as_ref().map(|r| r.frames.last().map(|f| f.time).unwrap_or(r.duration_s).max(1e-9)).unwrap_or(self.scene.run.duration_s)
    }
    pub fn progress(&self) -> Option<f64> {
        self.recording.then(|| self.jobs.as_ref().and_then(|j| j.progress().fraction).unwrap_or(0.))
    }
    /// Show simulated time `t` (a caption there is held again for reading).
    pub fn seek(&mut self, t: f64) {
        self.time = t.clamp(0., self.duration());
        self.wall = self.plan.wall_at(self.time);
        self.camera_cue = None;
    }
    /// Show on-screen playback time `w` (the pacing plan's clock).
    pub fn seek_wall(&mut self, w: f64) {
        let end = self.plan.duration();
        self.wall = w.clamp(0., end.max(0.));
        self.time = if self.wall >= end { self.duration() } else { self.plan.sim_at(self.wall).clamp(0., self.duration()) };
        self.camera_cue = None;
    }
    /// Position in on-screen playback time.
    pub fn wall_time(&self) -> f64 {
        self.wall
    }
    /// Playback pace at the current position.
    pub fn pace(&self) -> sim_script::pacing::Pace {
        self.plan.pace_at(self.wall)
    }
}

#[derive(Default)]
pub(crate) struct CompareState {
    /// The comparison run (progress: variants done of total).
    job: Option<crate::jobs::Job<CompareRun>>,
    pub result: Option<Result<CompareRun, String>>,
}

#[derive(Resource)]
pub struct Learn {
    pub dir: PathBuf,
    /// The lesson screen is showing (else the builder is).
    pub active: bool,
    pub entries: Vec<sim_lesson::index::Entry>,
    /// Lesson categories (`categories.yaml`), in order.
    pub categories: Vec<sim_lesson::categories::Category>,
    /// Categories folded in the lesson list.
    pub(crate) folded: BTreeSet<String>,
    pub lesson: Option<Lesson>,
    pub lesson_error: Option<String>,
    pub index: Option<LessonIndex>,
    pub mode: PageMode,
    pub scene: Option<ActiveScene>,
    pub status: String,
    pub(crate) compares: BTreeMap<String, CompareState>,
    pub(crate) input: Option<Input>,
    pub(crate) thread: Option<String>,
    pub(crate) menu: Option<String>,
    pub(crate) draft: Option<LessonAnchor>,
    pub(crate) picked: Option<String>,
    pub(crate) hover_part: Option<String>,
    pub(crate) open_only: bool,
    pub(crate) author: String,
    pub(crate) notes: Option<Notes>,
    pub(crate) notes_doc: ThreadDocument<LessonAnchor>,
    pending: Vec<(u64, String)>,
    pub(crate) agent: agent::LessonAgent,
    pub(crate) dirty: bool,
    pub(crate) scroll: f32,
    pub(crate) scroll_to: Option<String>,
    pub(crate) visited: BTreeSet<String>,
    returning: Option<Return>,
    library: PathBuf,
    registry: sim_core::BehaviorRegistry,
    stamp: Option<std::time::SystemTime>,
    checked: f64,
    scanned: f64,
    /// Show this panel set next frame (screen switch).
    pub(crate) switched: bool,
    /// The lesson screen (true) or the builder (false) was asked for: a
    /// mode switch to Lessons or Build (`app::switch`), whose OnEnter/OnExit
    /// call `show`. `active` is written only by `show`, so it always matches
    /// the mode.
    screen_request: Option<bool>,
    pub(crate) ui_revision: u64,
    /// The lesson's narrated explainer, when it has `explainer.md`.
    pub narration: Option<narrate::Narration>,
    /// Part the narration is pointing at (glows in the scene).
    pub(crate) narration_part: Option<String>,
    pub(crate) player: Option<sim_voice::player::Player>,
    /// The learner's answers, predictions, reflections and review schedule.
    pub progress: sim_lesson::progress::Progress,
    pub(crate) progress_path: PathBuf,
    pub(crate) quiz_pick: BTreeMap<String, usize>,
    pub(crate) quiz_verdict: BTreeMap<String, sim_lesson::quiz::Verdict>,
    pub(crate) quiz_text: BTreeMap<String, String>,
    /// Sketch questions: the reader's curve, one height (0 bottom … 1 top) per column.
    pub(crate) sketches: BTreeMap<String, Vec<Option<f32>>>,
    /// Sketch questions: the drawn curve over the simulated one, and the gap.
    pub(crate) sketch_results: BTreeMap<String, (Handle<Image>, f64, String)>,
    /// (lesson, question) being reviewed.
    pub(crate) review: Option<(String, String)>,
    pub(crate) figures: BTreeMap<PathBuf, practice::CachedFigure>,
    /// Numbers, equation terms and measured comparisons from the model.
    pub(crate) model: extras::ModelState,
    pub(crate) tasks: BTreeMap<String, extras::TaskState>,
    pub(crate) labs: BTreeMap<String, extras::LabState>,
    pub(crate) tracking: extras::Tracking,
    /// Confidence chosen for a question before checking it.
    pub(crate) confidence: BTreeMap<String, sim_lesson::progress::Confidence>,
    /// Steps questions: typed text per (question, step).
    pub(crate) step_text: BTreeMap<(String, usize), String>,
    /// Question → the remedy its last wrong pick pointed at.
    pub(crate) remedy: BTreeMap<String, String>,
    /// A mixed review session: the queue and the position in it.
    pub(crate) session: Option<(Vec<(String, String)>, usize)>,
    /// The concept map and every lesson (for mastery and the next lesson).
    pub(crate) concepts: sim_lesson::concepts::ConceptMap,
    pub(crate) all_lessons: Vec<Lesson>,
    pub settings: Settings,
    pub(crate) settings_path: PathBuf,
    /// A note being saved that should go to Codex once it exists.
    pub(crate) ask_when_saved: Option<String>,
    /// The note being written should go to Codex once posted ("Ask about this moment").
    pub(crate) ask_next: bool,
    /// The slider ←/→ adjust, when one has keyboard focus.
    pub(crate) focus_slider: Option<String>,
    /// A contact sheet being captured (REST `lesson_frames`).
    pub(crate) frames: Option<frames::FrameJob>,
}

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

fn lesson_stamp(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Learn {
    pub fn new(dir: PathBuf, library: PathBuf, registry: sim_core::BehaviorRegistry) -> Self {
        let entries = sim_lesson::index::scan(&dir);
        let categories = sim_lesson::categories::load(&dir).unwrap_or_default();
        Self {
            dir,
            active: true,
            entries,
            categories,
            folded: BTreeSet::new(),
            lesson: None,
            lesson_error: None,
            index: None,
            mode: PageMode::Read,
            scene: None,
            status: "Choose a lesson on the left.".into(),
            compares: BTreeMap::new(),
            input: None,
            thread: None,
            menu: None,
            draft: None,
            picked: None,
            hover_part: None,
            open_only: true,
            author: std::env::var("USER").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| "Reader".into()),
            notes: None,
            notes_doc: ThreadDocument::new("lesson"),
            pending: Vec::new(),
            agent: agent::LessonAgent::default(),
            dirty: true,
            scroll: 0.,
            scroll_to: None,
            visited: BTreeSet::new(),
            returning: None,
            library,
            registry,
            stamp: None,
            checked: 0.,
            scanned: 0.,
            switched: true,
            screen_request: None,
            ui_revision: 0,
            narration: None,
            narration_part: None,
            player: None,
            progress: sim_lesson::progress::Progress::load(&sim_lesson::progress::path()),
            progress_path: sim_lesson::progress::path(),
            quiz_pick: BTreeMap::new(),
            quiz_verdict: BTreeMap::new(),
            quiz_text: BTreeMap::new(),
            sketches: BTreeMap::new(),
            sketch_results: BTreeMap::new(),
            review: None,
            figures: BTreeMap::new(),
            model: Default::default(),
            tasks: BTreeMap::new(),
            labs: BTreeMap::new(),
            tracking: Default::default(),
            confidence: BTreeMap::new(),
            step_text: BTreeMap::new(),
            remedy: BTreeMap::new(),
            session: None,
            concepts: Default::default(),
            all_lessons: Vec::new(),
            settings: Settings::load(&Settings::path()),
            settings_path: Settings::path(),
            ask_when_saved: None,
            ask_next: false,
            focus_slider: None,
            frames: None,
        }
        .with_catalog()
    }

    /// Load the concept map and every lesson (for mastery and suggestions).
    fn with_catalog(mut self) -> Self {
        self.refresh_catalog();
        self
    }
    pub(crate) fn refresh_catalog(&mut self) {
        self.concepts = sim_lesson::concepts::load(&self.dir).unwrap_or_default();
        self.all_lessons = self.entries.iter().filter_map(|e| Lesson::load(&e.path).ok()).collect();
    }

    /// The question keyboard answers go to: the one being reviewed, else the
    /// first choice question on the page that is not yet passed.
    pub(crate) fn open_question(&self) -> Option<String> {
        let lesson = self.lesson.as_ref()?;
        if let Some((l, q)) = &self.review {
            if l == &lesson.slug {
                return Some(q.clone());
            }
        }
        let gate = self.gate().unwrap_or(usize::MAX);
        lesson.blocks.iter().take(gate).find_map(|b| match &b.kind {
            BlockKind::Quiz(q) if !q.options.is_empty() && !q.kind.predicts() && !self.progress.quiz(&lesson.slug, &q.id).is_some_and(|r| r.passed() || (q.pretest && r.attempts > 0)) => Some(q.id.clone()),
            _ => None,
        })
    }

    /// Give every varied question of the open lesson its numbers for this attempt.
    pub(crate) fn draw_variants(&mut self) {
        let Some(lesson) = self.lesson.clone() else { return };
        let mut changed = false;
        for (_, q) in lesson.quizzes().filter(|(_, q)| !q.vary.is_empty()) {
            let before = self.progress.quiz(&lesson.slug, &q.id).map(|r| r.values.clone());
            let now = self.progress.variant(&lesson.slug, q);
            changed |= before.as_ref() != Some(&now);
        }
        if changed {
            let path = self.progress_path.clone();
            let _ = self.progress.save(&path);
        }
    }

    pub fn slug(&self) -> Option<&str> {
        self.lesson.as_ref().map(|l| l.slug.as_str())
    }

    /// Ask for the lesson screen (true) or the builder (false): a switch to
    /// Lessons or Build mode through the one mode-switch handler.
    pub fn request_screen(&mut self, learn: bool) {
        self.screen_request = Some(learn);
    }
    pub(crate) fn screen_request(&self) -> Option<bool> {
        self.screen_request
    }
    pub(crate) fn take_screen_request(&mut self) -> Option<bool> {
        self.screen_request.take()
    }
    /// The screen shown, or asked for and not switched to yet.
    pub fn screen_lesson(&self) -> bool {
        self.screen_request.unwrap_or(self.active)
    }
    /// What leaving the lessons (for a mode other than Build) would lose.
    pub(crate) fn switch_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if self.input.is_some() {
            blockers.push("a lesson text draft is open: submit or cancel it".to_string());
        }
        if self.frames.is_some() {
            blockers.push("a lesson_frames contact sheet is being captured: wait for it".to_string());
        }
        blockers
    }

    /// Show the lesson screen (true) or the builder (false): the lesson's
    /// own bookkeeping, run by the Lessons mode's OnEnter/OnExit.
    pub fn show(&mut self, learn: bool) {
        if self.active == learn {
            return;
        }
        self.active = learn;
        self.switched = true;
        self.dirty = true;
        if learn {
            if let Some(r) = self.returning.take() {
                self.scroll = r.scroll;
            }
            // The learner may have changed the sandbox: record again (cached if unchanged).
            self.rerecord();
        }
    }

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

    fn load(&mut self, path: &std::path::Path) {
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

    fn reset_sandbox(&mut self) {
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

    fn goto_part(&mut self, system: &str, path: &str) {
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

    fn submit_draft(&mut self) -> Result<(), String> {
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
                    self.note("Delete comment", ThreadCommand::DeleteComment { thread: t, comment: c })?;
                    self.menu = None;
                }
                LessonAction::Resolve => {
                    let t = self.thread.clone().ok_or("open a note first")?;
                    let resolved = !self.notes_doc.threads.get(&t).is_some_and(|x| x.resolved);
                    self.note(if resolved { "Resolve" } else { "Reopen" }, ThreadCommand::Resolve { thread: t, resolved })?;
                }
                LessonAction::DeleteThread => {
                    let t = self.thread.take().ok_or("open a note first")?;
                    self.note("Delete note", ThreadCommand::DeleteThread { id: t })?;
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
                    let t = sim_annotate::uid("t");
                    let thread = Thread { id: t.clone(), title: "Feedback on my explanation".into(), resolved: false, targets: vec![LessonAnchor::Text { quote }], comments: vec![Comment { id: sim_annotate::uid("c"), author: self.author.clone(), body, created_at: sim_annotate::stamp(), edited_at: None, links: vec![] }], pin_m: None, view: None };
                    self.note("Feedback request", ThreadCommand::PutThread { thread })?;
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

fn open_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("not a web link: {url}"));
    }
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(opener).arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

pub struct LearnPlugin;
/// Lessons: the lesson screen over the builder. Its systems run while a
/// lesson is open in the Builder scope (Lessons, and Build while the builder
/// is shown over the lesson: recordings, narration and the lesson model keep
/// going, and the page hides itself when `active` is false), in their
/// original order.
impl Plugin for LearnPlugin {
    fn build(&self, app: &mut App) {
        let open = || in_state(crate::app::ModeScope::Builder).and_then(resource_exists::<Learn>);
        crate::app::actions::register::<actions::LessonCommand>(app);
        // Keys, buttons, the timebars and slider releases write lesson actions;
        // the one handler applies them and REST's (in build and lessons:
        // without a lesson, REST is told so).
        app.add_systems(Update, (actions::keys, actions::buttons, seek, narrate::seek, sliders).chain().after(crate::app::actions::serve).in_set(crate::app::ViewerSet::Input).run_if(open()))
            .add_systems(Update, actions::apply.in_set(crate::app::ViewerSet::Actions).run_if(in_state(crate::app::ModeScope::Builder)));
        app.add_systems(
            Update,
            (poll, practice::sketch_input, slider_live, chart_hover, ui::rebuild, ui::scroll, viewport, narrate::tick, playback, ui::live_text, narrate::live, narrate::overlay, practice::sketch_dots)
                .chain()
                .before(crate::camera_viewport)
                .in_set(crate::app::ViewerSet::SimSync)
                .run_if(open()),
        );
        app.add_systems(Update, (extras::live_equations, extras::track_blocks, apply_settings, frames::step).after(ui::rebuild).in_set(crate::app::ViewerSet::SimSync).run_if(open()));
    }
}

/// Reader preferences that live outside the page: text size and reduced motion.
fn apply_settings(learn: Res<Learn>, mut ui_scale: ResMut<UiScale>, mut scene: ResMut<SpatialScene>) {
    if !learn.is_changed() {
        return;
    }
    let s = learn.settings.text_scale.clamp(0.8, 1.6);
    if (ui_scale.0 - s).abs() > 1e-3 {
        ui_scale.0 = s;
    }
    if scene.reduced_motion != learn.settings.reduced_motion {
        scene.reduced_motion = learn.settings.reduced_motion;
    }
}

/// Jobs, file watching, notes and the agent.
#[allow(clippy::too_many_arguments)]
fn poll(
    mut commands: Commands,
    time: Res<Time>,
    mut learn: ResMut<Learn>,
    builder: Option<ResMut<Builder>>,
    mut scene: ResMut<SpatialScene>,
    mut images: ResMut<Assets<Image>>,
) {
    let now = time.elapsed_secs_f64();
    // Lesson file and index.
    if now - learn.checked > 0.5 {
        learn.checked = now;
        if let Some(path) = learn.lesson.as_ref().map(|l| l.path.clone()) {
            if lesson_stamp(&path) != learn.stamp {
                let before = learn.lesson.as_ref().map(|l| l.source.clone());
                learn.load(&path);
                if learn.lesson.as_ref().map(|l| l.source.clone()) != before {
                    learn.status = "Reloaded: lesson.md changed on disk.".into();
                    // A changed scene block re-records.
                    let changed = learn.scene.as_ref().and_then(|a| learn.lesson.as_ref().map(|l| l.scene(&a.id) != Some(&a.scene)));
                    if changed == Some(true) {
                        if let Some(id) = learn.scene.as_ref().map(|a| a.id.clone()) {
                            learn.activate(&id);
                        }
                    }
                }
            }
        }
        learn.load_figures();
        if now - learn.scanned > 3.0 {
            learn.scanned = now;
            let entries = sim_lesson::index::scan(&learn.dir);
            if entries != learn.entries {
                learn.entries = entries;
                learn.dirty = true;
            }
            let categories = sim_lesson::categories::load(&learn.dir).unwrap_or_default();
            if categories != learn.categories {
                learn.categories = categories;
                learn.dirty = true;
            }
        }
    }
    // Notes.
    if let Some(notes) = &learn.notes {
        let doc = notes.document();
        let error = notes.error();
        if doc.revision != learn.notes_doc.revision || doc.subject != learn.notes_doc.subject {
            learn.notes_doc = doc;
            learn.dirty = true;
        }
        if let Some(e) = error.filter(|e| !learn.status.contains(e.as_str())) {
            learn.status = format!("Notes: {e}");
            learn.dirty = true;
        }
    }
    let pending = std::mem::take(&mut learn.pending);
    for (id, label) in pending {
        match learn.notes.as_mut().and_then(|n| n.result(id)) {
            None => learn.pending.push((id, label)),
            Some(Ok(doc)) => {
                learn.notes_doc = doc;
                learn.status = format!("{label} · saved");
                learn.dirty = true;
            }
            Some(Err(e)) => {
                learn.status = format!("{label}: {e}");
                learn.dirty = true;
            }
        }
    }
    agent::tick(&mut learn, now);
    if learn.poll_figures(&mut images) {
        learn.dirty = true;
    }
    // Comparisons.
    let mut finished = false;
    for state in learn.compares.values_mut() {
        let received = state.job.as_ref().and_then(crate::jobs::Job::poll);
        if let Some(result) = received {
            state.result = Some(result);
            state.job = None;
            finished = true;
        }
    }
    if finished {
        learn.dirty = true;
    }
    // Model values, bench results, and notes waiting to go to Codex.
    if learn.poll_model(&mut images) {
        learn.dirty = true;
        let ids: Vec<String> = learn.labs.iter().filter(|(_, s)| s.result.as_ref().is_some_and(|r| r.is_ok())).map(|(id, _)| id.clone()).collect();
        for id in ids {
            learn.record_lab(&id);
        }
    }
    if let Some(t) = learn.ask_when_saved.clone() {
        if learn.notes_doc.threads.contains_key(&t) {
            learn.ask_when_saved = None;
            match learn.agent_input(&t).and_then(|input| learn.agent.ask(input)) {
                Ok(_) => learn.status = "Asked Codex about this moment; the reply is posted to the note.".into(),
                Err(e) => learn.status = format!("Codex: {e}"),
            }
            learn.dirty = true;
        }
    }
    // The active scene's jobs.
    let label = learn.lesson.as_ref().map(|l| l.meta.title.clone());
    let mut builder = builder;
    let Some(a) = learn.scene.as_mut() else { return };
    let stage = match a.jobs.as_ref().and_then(crate::jobs::Job::next_update) {
        Some(stage) => Some(stage),
        // Every stage is queued before the job's result and the closures only
        // return Ok, so an error with nothing queued is a panic: show it as the
        // run's error instead of leaving the scene "recording" forever.
        None => match a.jobs.as_ref().and_then(crate::jobs::Job::poll) {
            Some(Err(e)) => Some(Stage::Run(Err(e))),
            _ => None,
        },
    };
    let mut parts = None;
    let mut judged: Option<Arc<SceneRun>> = None;
    let mut changed = stage.is_some();
    let received = matches!(stage, Some(Stage::Builder(Ok(_))));
    match stage {
        Some(Stage::Builder(Ok((sb, mut b)))) => {
            let _ = b.set_level(&a.scene.level);
            b.lesson = label;
            if let Ok(doc) = runtime::load_system(&sb.path, b.registry()) {
                parts = Some((a.id.clone(), runtime::instance_paths(&doc)));
            }
            a.sandbox = Some(sb);
            a.installed = false;
            a.framed = false;
            commands.insert_resource(*b);
        }
        Some(Stage::Builder(Err(e))) => {
            a.error = Some(e);
            a.recording = false;
            a.jobs = None;
        }
        Some(Stage::Run(Ok(run))) => {
            a.recording = false;
            a.error = None;
            a.charts = charts(&run, &a.scene, &a.timeline, &mut images);
            a.phase_charts = phase_charts(&run, &a.scene, &mut images);
            let rules = if a.explore { sim_script::pacing::PacingRules::exploring() } else { Default::default() };
            a.plan = run.pacing_with(&a.scene, &a.timeline, &rules).unwrap_or_default();
            a.challenge = a.scene.challenge.as_ref().map(|c| runtime::check_claims(&run, &c.win));
            if !a.overrides.is_empty() && a.challenge.as_ref().is_some_and(|r| r.iter().all(|x| x.passed)) {
                a.challenge_met = Some(serde_json::to_string(&a.overrides).unwrap_or_default());
            }
            a.run = Some(Arc::new(run));
            judged = a.run.clone();
            let t = a.time;
            a.seek(t);
            a.playing = a.scene.autoplay;
            // A companion run follows on the same channel (first recording only).
            if a.scene.companion.is_none() || !a.overrides.is_empty() || a.companion.is_some() {
                a.jobs = None;
            }
        }
        Some(Stage::Companion(result)) => {
            a.jobs = None;
            match result {
                Ok(run) => {
                    let run = Arc::new(run);
                    if let Some(main) = a.run.clone() {
                        a.charts = charts_with(&main, Some(&run), &a.scene, &a.timeline, &mut images);
                    }
                    a.companion = Some(run);
                }
                Err(e) if e != "cancelled" => a.error = Some(format!("companion run: {e}")),
                Err(_) => {}
            }
        }
        Some(Stage::Run(Err(e))) => {
            a.recording = false;
            a.jobs = None;
            if e != "cancelled" {
                a.error = Some(e);
            }
        }
        None => {}
    }
    // Installed once the builder on this sandbox has compiled the scene (the
    // builder received this frame is inserted only after this system).
    if !a.installed && !received {
        if let (Some(b), Some(sb)) = (builder.as_deref_mut(), a.sandbox.as_ref()) {
            if b.path() == sb.path && b.compile_settled() {
                a.installed = true;
                if let Some(e) = b.compile_error() {
                    a.error = Some(format!("Does not compile: {e}"));
                }
                scene.set_changed();
                changed = true;
            }
        }
    }
    if let (Some((id, parts)), Some(index)) = (parts, learn.index.as_mut()) {
        index.with_parts(&id, parts);
    }
    // Sketch questions: draw the reader's curve over the run once both exist.
    let pending: Vec<(sim_lesson::quiz::Quiz, String)> = learn.lesson.as_ref().zip(learn.slug()).map(|(lesson, slug)| {
        let run_key = learn.scene.as_ref().and_then(|a| a.run.as_ref().map(|r| r.key.clone()));
        lesson.quizzes().filter(|(_, q)| q.kind == sim_lesson::quiz::QuizKind::Sketch && learn.sketch_results.get(&q.id).map(|r| &r.2) != run_key.as_ref())
            .filter_map(|(_, q)| Some((q.clone(), learn.progress.quiz(slug, &q.id)?.prediction.clone()?))).collect()
    }).unwrap_or_default();
    for (q, prediction) in pending {
        let window = learn.sketch_window(&q);
        let Some(run) = learn.scene.as_ref().filter(|a| q.scene.as_ref() == Some(&a.id)).and_then(|a| a.run.clone()) else { continue };
        if let Some((image, gap)) = practice::sketch_result(&run, &q, &prediction, window) {
            learn.sketch_results.insert(q.id.clone(), (images.add(image), gap, run.key.clone()));
            learn.dirty = true;
        }
    }
    if changed {
        learn.dirty = true;
    }
    if let Some(run) = judged {
        learn.judge_task_run(&run);
    }
}

/// Operating-point images: y against x over the whole run.
fn phase_charts(run: &SceneRun, scene: &Scene, images: &mut Assets<Image>) -> Vec<PhaseChart> {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    scene
        .phase
        .iter()
        .filter_map(|p| {
            let (x, y) = (run.series(&p.x)?, run.series(&p.y)?);
            // Pair y with x at x's sample times (both recorded per step).
            let points: Vec<[f64; 2]> = x.times.iter().zip(&x.values).filter_map(|(t, xv)| {
                let i = y.times.partition_point(|u| u < t);
                y.values.get(i).filter(|_| y.times.get(i).is_some_and(|u| (u - t).abs() < 1e-9)).map(|yv| [*xv, *yv])
            }).collect();
            let (pixels, range, window) = crate::chart::rasterize_span(&[(&points, [120, 150, 190])], None);
            let (w, h) = crate::chart::RASTER;
            let image = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, pixels, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
            Some(PhaseChart {
                title: if p.title.is_empty() { format!("{} against {}", y.label, x.label) } else { p.title.clone() },
                x: (x.label.clone(), x.unit.clone()),
                y: (y.label.clone(), y.unit.clone()),
                ids: (x.observable.clone(), y.observable.clone()),
                image: images.add(image),
                range,
                window,
            })
        })
        .collect()
}

/// Chart images for the run's plotted observables.
fn charts(run: &SceneRun, scene: &Scene, timeline: &Timeline, images: &mut Assets<Image>) -> Vec<Chart> {
    charts_with(run, None, scene, timeline, images)
}

/// Charts with a companion run's curve drawn faintly behind each one.
fn charts_with(run: &SceneRun, companion: Option<&SceneRun>, scene: &Scene, timeline: &Timeline, images: &mut Assets<Image>) -> Vec<Chart> {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let keys = runtime::plot_keys(scene, timeline);
    let colors = [[77, 212, 191], [222, 148, 84], [140, 170, 255], [235, 110, 160]];
    keys.iter()
        .take(6)
        .enumerate()
        .filter_map(|(i, key)| {
            let s = run.series(key)?;
            let points: Vec<[f64; 2]> = s.times.iter().zip(&s.values).map(|(t, v)| [*t, *v]).collect();
            // The companion's curve first, in a faint purple, so this run's draws on top.
            let other: Vec<[f64; 2]> = companion.and_then(|c| c.series(key)).map(|c| c.times.iter().zip(&c.values).map(|(t, v)| [*t, *v]).collect()).unwrap_or_default();
            let (pixels, range, window) = crate::chart::rasterize_span(&[(&other, [120, 96, 170]), (&points, colors[i % colors.len()])], Some(crate::builder::HISTORY_SECONDS));
            let (w, h) = crate::chart::RASTER;
            let image = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, pixels, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
            Some(Chart { key: key.clone(), unit: s.unit.clone(), image: images.add(image), range, window })
        })
        .collect()
}

/// The timeline bar (a kit slider over the run's fraction): press or drag to scrub.
#[derive(Component)]
pub(crate) struct Timebar;
/// Input: pressing or dragging the timebar seeks the live scene (a
/// `SeekTo` every frame it is held; the press that starts a drag may count
/// a rewind). Nothing is sent while the scene is still recording.
fn seek(bars: Query<(&bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction), With<Timebar>>, learn: Res<Learn>, mut pressing: Local<bool>, mut out: MessageWriter<Act<actions::LessonCommand>>) {
    let mut pressed = false;
    for (value, pressed, interaction) in &bars {
        if !crate::ui_kit::slider_held(pressed, interaction) {
            continue;
        }
        pressed = true;
        let Some(a) = learn.scene.as_ref().filter(|a| a.run.is_some()) else { continue };
        let time = (value.0.clamp(0.0, 1.0) as f64) * a.duration();
        out.write(Act::ui(actions::LessonCommand::Ui(LessonAction::SeekTo { time: Some(time), rewind: !*pressing })));
    }
    *pressing = pressed;
}

/// Marks the node the live scene draws into.
#[derive(Component)]
pub(crate) struct SceneViewport(pub String);

fn viewport(learn: Res<Learn>, mut scene: ResMut<SpatialScene>, nodes: Query<(&SceneViewport, &ComputedNode, &UiGlobalTransform, Option<&bevy::ui::CalculatedClip>)>, window: Single<&Window>) {
    let want = if !learn.active {
        None
    } else {
        let active = learn.scene.as_ref().filter(|a| a.installed).map(|a| a.id.as_str());
        let found = active.and_then(|id| nodes.iter().find(|(v, ..)| v.0 == id));
        Some(match found {
            Some((_, node, transform, clip)) => {
                let center = transform.translation;
                let full = Rect::from_center_size(center, node.size());
                let window_rect = Rect::new(0., 0., window.physical_width() as f32, window.physical_height() as f32);
                let mut visible = full.intersect(window_rect);
                if let Some(c) = clip {
                    visible = visible.intersect(c.clip);
                }
                crate::LearnView { full, visible: if visible.is_empty() { Rect::default() } else { visible } }
            }
            None => crate::LearnView::default(),
        })
    };
    if scene.learn_view != want {
        scene.learn_view = want;
    }
}

/// Advance playback, show the recorded frame and apply presentation cues.
fn playback(time: Res<Time>, mut learn: ResMut<Learn>, mut scene: ResMut<SpatialScene>, mut orbit: Single<&mut Orbit>) {
    // A scene's display magnification applies only while it is on screen.
    let magnify = if learn.active { learn.scene.as_ref().map_or(1., |a| a.scene.magnify as f32) } else { 1. };
    if scene.motion_scale != magnify {
        scene.motion_scale = magnify;
    }
    if !learn.active {
        if scene.companion.is_some() {
            scene.companion = None;
        }
        return;
    }
    let hover = learn.hover_part.clone().or_else(|| learn.narration_part.clone());
    let picked = learn.picked.clone();
    let narrated = learn.narration.as_ref().filter(|n| n.playing).map(|n| n.view()).filter(|v| !v.is_default());
    // A met challenge is kept in progress, with the values that met it.
    if let Some((id, values)) = learn.scene.as_mut().and_then(|a| a.challenge_met.take().map(|v| (a.id.clone(), v))) {
        if let Some(slug) = learn.slug().map(str::to_string) {
            learn.progress.record(&slug, &format!("{id}:challenge"), true, &values, sim_lesson::progress::Mode::Learn, false, sim_lesson::progress::now());
            let path = learn.progress_path.clone();
            let _ = learn.progress.save(&path);
            learn.status = "Challenge met: saved with your values.".into();
        }
    }
    let Some(a) = learn.scene.as_mut() else { return };
    if !a.installed {
        return;
    }
    // Frame the camera once the scene is in place.
    // Not before the card has a real size: framing a half-laid-out view
    // (a sliver's aspect) puts the camera far too far away.
    let aspect = view_aspect(&scene);
    if a.framed && aspect.is_some_and(|x| (x / a.framed_aspect.max(1e-3) - 1.).abs() > 0.15) {
        a.framed = false;
    }
    if let Some(aspect) = aspect.filter(|_| !a.framed) {
        let spec = a.scene.camera.clone().unwrap_or_default();
        frame(&scene, &mut orbit, &spec);
        a.framed = true;
        a.framed_aspect = aspect;
        // Cues seen before the card was placed apply again, over this framing.
        a.camera_cue = None;
        a.zoom_cue = None;
    }
    if a.show_pending {
        a.show_pending = false;
        if let Some(layers) = &a.scene.show {
            scene.state.overlays = layers.iter().copied().collect();
        }
    }
    let Some(run) = a.run.clone() else { return };
    let duration = a.duration();
    if a.playing {
        // The plan's clock: holds and slow motion from the shared pacing rules.
        let before = a.wall;
        let end = a.plan.duration();
        let mut next = a.wall + time.delta_secs_f64() * a.user_speed;
        if let Some(p) = a.plan.pause_between(before, next) {
            next = p;
            a.playing = false;
        }
        if next >= end {
            next = end;
            a.playing = false;
        }
        if let Some(stop) = a.stop_at.filter(|s| a.plan.sim_at(next) >= *s) {
            next = a.plan.wall_at(stop.min(duration)).max(before);
            a.playing = false;
            a.stop_at = None;
        }
        a.wall = next;
        a.time = if next >= end { duration } else { a.plan.sim_at(next) };
    }
    let state = a.timeline.state_at(a.time);
    if let Some((at, camera)) = state.camera.as_ref().filter(|_| a.framed) {
        if a.camera_cue != Some(*at) {
            a.camera_cue = Some(*at);
            frame(&scene, &mut orbit, camera);
        }
    }
    // The companion run, posed at the same moment.
    let companion = match (&a.scene.companion, &a.companion) {
        (Some(spec), Some(run)) => Some(crate::view::CompanionView { label: spec.label.clone(), ghost: spec.mode == sim_lesson::CompanionMode::Ghost, frame: run.frame_interpolated(a.time.min(run.frames.last().map(|f| f.time).unwrap_or(0.))) }),
        _ => None,
    };
    if companion.as_ref().map(|c| (&c.label, c.ghost, c.frame.as_ref().map(|f| f.time))) != scene.companion.as_ref().map(|c| (&c.label, c.ghost, c.frame.as_ref().map(|f| f.time))) {
        scene.companion = companion;
    }
    // View directives: the narration's while it speaks, else the scene's.
    let mut view = narrated.unwrap_or_else(|| state.view.clone());
    // A script's highlight is emphasis you can see: the part gets an arrow
    // with its name, and the rest of the scene dims a little.
    let soft: Vec<String> = if hover.is_none() && picked.is_none() { state.highlight.clone() } else { Vec::new() };
    for p in &soft {
        if !view.pins.iter().any(|(q, _)| q == p) {
            let label = scene.description.components.get(p).map(|c| c.label.clone()).unwrap_or_else(|| p.rsplit('/').next().unwrap_or(p).to_string());
            view.pins.push((p.clone(), label));
        }
    }
    if scene.soft_focus != soft {
        scene.soft_focus = soft;
    }
    if let Some((at, sim_script::presentation::View::Zoom { focus, zoom, seconds })) = state.view.zoom.as_ref().filter(|_| a.framed) {
        if a.zoom_cue != Some(*at) {
            a.zoom_cue = Some(*at);
            let aspect = view_aspect(&scene).unwrap_or(1.6);
            let (yaw, pitch) = orbit.heading();
            let pose = crate::view::frame_pose(&scene, focus.as_deref(), *zoom as f32, yaw, pitch, aspect);
            orbit.glide_to(pose, *seconds as f32);
        }
    }
    if (view.orbit as f32 - orbit.spin).abs() > 1e-6 && (view.orbit != scene.directives.orbit) {
        orbit.spin = view.orbit as f32;
    }
    if scene.directives != view {
        scene.directives = view;
    }
    // Zoom to a clicked part; back to the scene's framing when cleared.
    if picked != a.zoomed_part {
        a.zoomed_part = picked.clone();
        match &picked {
            Some(p) => {
                let aspect = view_aspect(&scene).unwrap_or(1.6);
                let (yaw, pitch) = orbit.heading();
                let pose = crate::view::frame_pose(&scene, Some(p), 1.0, yaw, pitch, aspect);
                orbit.glide_to(pose, crate::view::GLIDE_S);
            }
            None => frame(&scene, &mut orbit, &a.scene.camera.clone().unwrap_or_default()),
        }
    }
    if let Some(frame) = run.frame_interpolated(a.time) {
        let changed = scene.live.snapshot.as_ref().is_none_or(|s| s.frame.as_ref().is_none_or(|f| f.time != frame.time || f.sequence != frame.sequence));
        if changed {
            let snapshot = sim_inspect::live::LiveSnapshot { version: 1, source_description_id: scene.description.id.clone(), description: None, status: None, frame: Some(frame), error: run.error.clone() };
            scene.live.snapshot = Some(Arc::new(snapshot));
        }
    }
    // Highlight: hovered link, else picked part, else the script's highlight.
    let wanted: Vec<String> = match (hover, picked) {
        (Some(h), _) => vec![h],
        (None, Some(p)) => vec![p],
        (None, None) => state.highlight.clone(),
    };
    if wanted != a.highlight {
        a.highlight = wanted.clone();
        let target = crate::builder::discussion::selection(&scene, &wanted);
        let _ = scene.set_selection(target);
    }
}

/// The live scene view's aspect ratio, once its card is laid out.
pub(crate) fn view_aspect(scene: &SpatialScene) -> Option<f32> {
    scene.learn_view.map(|v| v.full).filter(|r| r.width() >= 80. && r.height() >= 60.).map(|r| r.width() / r.height())
}

/// Frame a camera spec: an eased glide from wherever the view is now.
fn frame(scene: &SpatialScene, orbit: &mut Orbit, spec: &CameraSpec) {
    let (yaw, pitch) = spec.preset.map(|p| p.angles()).unwrap_or((0.35, 0.60));
    let aspect = view_aspect(&scene).unwrap_or(1.6);
    let pose = crate::view::frame_pose(scene, spec.focus.as_deref(), spec.zoom.unwrap_or(1.0), spec.yaw.unwrap_or(yaw), spec.pitch.unwrap_or(pitch), aspect);
    orbit.glide_to(pose, crate::view::GLIDE_S);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// The viewer's edits and notes go through the shared lesson command
    /// layer and note store; a note stays on its paragraph after edits above it.
    #[test]
    fn learn_edits_undo_and_notes_share_the_command_layers() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("learn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lesson_dir = dir.join("lessons/motor-torque-speed");
        std::fs::create_dir_all(&lesson_dir).unwrap();
        for f in ["lesson.md", "load-step.rhai", "motor.system.json"] {
            std::fs::copy(root.join("lessons/motor-torque-speed").join(f), lesson_dir.join(f)).unwrap();
        }
        // SAFETY: this test binary sets these once; nothing else reads them concurrently.
        unsafe {
            std::env::set_var("SIM_LESSON_CACHE", dir.join("cache"));
            std::env::set_var("SIM_LESSON_SANDBOX", dir.join("sandbox"));
        }
        let mut learn = Learn::new(dir.join("lessons"), root.join("library/systems"), sim_runtime::registry());
        assert_eq!(learn.entries.len(), 1);
        learn.open("motor-torque-speed").unwrap();
        let original = learn.lesson.clone().unwrap();
        // A note on the first paragraph.
        let b = original.blocks.iter().find(|b| matches!(b.kind, BlockKind::Markdown { .. })).unwrap().clone();
        let quote = learn.index.as_ref().unwrap().block_anchor(&b.id, &b.section).unwrap();
        let thread = Thread { id: "t".into(), title: "Question".into(), resolved: false, targets: vec![LessonAnchor::Text { quote }], comments: vec![Comment { id: "c".into(), author: "Reader".into(), body: "Why?".into(), created_at: sim_annotate::stamp(), edited_at: None, links: vec![] }], pin_m: None, view: None };
        let id = learn.note("New note", ThreadCommand::PutThread { thread }).unwrap();
        let start = Instant::now();
        loop {
            if let Some(r) = learn.notes.as_mut().unwrap().result(id) {
                learn.notes_doc = r.unwrap();
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(learn.notes_doc.revision, 1);
        // Insert a block at the top: the note moves with its paragraph.
        learn.edit("Add", Edit::InsertAfter { block: None, text: "A new opening paragraph.".into() }, None).unwrap();
        let moved = learn.threads()["t"].targets[0].block(learn.index.as_ref().unwrap()).map(String::from);
        let expected = learn.lesson.as_ref().unwrap().blocks.iter().find(|x| x.text(&learn.lesson.as_ref().unwrap().source) == b.text(&original.source)).map(|x| x.id.clone());
        assert_eq!(moved, expected);
        assert_ne!(moved.as_deref(), Some(b.id.as_str()), "block IDs shifted, the anchor followed the text");
        // Undo restores the file exactly.
        learn.undo(false).unwrap();
        assert_eq!(learn.lesson.as_ref().unwrap().source, original.source);
        drop(learn);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Mastery pacing: a wrong answer keeps the rest locked, the right one
    /// opens the next part; a prediction unlocks its scene; figures load.
    #[test]
    fn questions_gate_the_lesson_and_predictions_unlock_scenes() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("learn-practice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lesson_dir = dir.join("lessons/motor-torque-speed");
        std::fs::create_dir_all(&lesson_dir).unwrap();
        for f in ["lesson.md", "load-step.rhai", "motor.system.json", "motor-model.svg", "torque-speed.svg"] {
            std::fs::copy(root.join("lessons/motor-torque-speed").join(f), lesson_dir.join(f)).unwrap();
        }
        let mut learn = Learn::new(dir.join("lessons"), root.join("library/systems"), sim_runtime::registry());
        learn.progress_path = dir.join("progress.json");
        learn.progress = sim_lesson::progress::Progress::default();
        learn.open("motor-torque-speed").unwrap();
        assert!(learn.scene.is_none(), "the only scene waits for its prediction");
        assert_eq!(learn.scene_locked("load-step").as_deref(), Some("predict-doubled"));
        let blocks = learn.lesson.as_ref().unwrap().blocks.clone();
        // One idea at a time: each opening question opens the next part.
        assert_eq!(blocks[learn.gate().unwrap() - 1].id, "two-sides");
        learn.practice(LessonAction::QuizPick("two-sides".into(), 1)).unwrap();
        learn.practice(LessonAction::QuizCheck("two-sides".into())).unwrap();
        for (id, typed) in [("torque-from-current", "36 mN·m"), ("back-emf-at-speed", "3 V")] {
            assert_eq!(blocks[learn.gate().unwrap() - 1].id, id);
            learn.quiz_text.insert(id.into(), typed.into());
            learn.practice(LessonAction::QuizCheck(id.into())).unwrap();
            assert!(learn.quiz_verdict[id].correct, "{id}: {}", learn.quiz_verdict[id].feedback);
        }
        // The faded worked example: the blank steps, with units.
        assert_eq!(blocks[learn.gate().unwrap() - 1].id, "budget-steps");
        learn.step_text.insert(("budget-steps".into(), 1), "4.8 V".into());
        learn.step_text.insert(("budget-steps".into(), 2), "2400 mA".into());
        learn.practice(LessonAction::QuizCheck("budget-steps".into())).unwrap();
        assert!(learn.quiz_verdict["budget-steps"].correct, "{}", learn.quiz_verdict["budget-steps"].feedback);
        // A hint, then a sure answer: recorded with the hint and the confidence.
        assert_eq!(blocks[learn.gate().unwrap() - 1].id, "current-at-speed");
        learn.practice(LessonAction::HintMore("current-at-speed".into())).unwrap();
        learn.practice(LessonAction::Confidence("current-at-speed".into(), 2)).unwrap();
        learn.quiz_text.insert("current-at-speed".into(), "1.5 A".into());
        learn.practice(LessonAction::QuizCheck("current-at-speed".into())).unwrap();
        let r = learn.progress.quiz("motor-torque-speed", "current-at-speed").unwrap();
        assert_eq!((r.history[0].hints, r.history[0].confidence, r.box_), (1, Some(sim_lesson::progress::Confidence::Sure), 0), "a hinted answer starts in the first box");
        let first_gate = learn.gate().unwrap();
        assert_eq!(blocks[first_gate - 1].id, "stall-current");
        // A wrong answer keeps it locked and explains the misconception.
        learn.practice(LessonAction::QuizPick("stall-current".into(), 0)).unwrap();
        learn.practice(LessonAction::QuizCheck("stall-current".into())).unwrap();
        assert!(!learn.quiz_verdict["stall-current"].correct);
        assert!(learn.quiz_verdict["stall-current"].feedback.contains("no back-EMF"));
        assert_eq!(learn.gate(), Some(first_gate));
        learn.practice(LessonAction::QuizPick("stall-current".into(), 1)).unwrap();
        learn.practice(LessonAction::QuizCheck("stall-current".into())).unwrap();
        let second = learn.gate().unwrap();
        assert_eq!(blocks[second - 1].id, "no-load-speed");
        // A varied question: this attempt's supply voltage, k from the model (read off the UI thread).
        let mut images = Assets::<Image>::default();
        let start = std::time::Instant::now();
        while !learn.model.given.contains_key("no-load-speed") {
            learn.poll_model(&mut images);
            assert!(start.elapsed() < std::time::Duration::from_secs(60), "model values: {:?}", learn.model.errors);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let v = learn.progress.quiz("motor-torque-speed", "no-load-speed").unwrap().values["V"];
        let rpm = v / 0.012 * 60. / std::f64::consts::TAU;
        learn.quiz_text.insert("no-load-speed".into(), format!("{rpm:.0} rpm"));
        learn.practice(LessonAction::QuizCheck("no-load-speed".into())).unwrap();
        assert!(learn.quiz_verdict["no-load-speed"].correct, "{} V: {}", v, learn.quiz_verdict["no-load-speed"].feedback);
        assert!(learn.model.values.contains_key("param:motor.torque_constant"), "prose numbers resolved too");
        assert_eq!(blocks[learn.gate().unwrap() - 1].id, "line-midpoint");
        learn.practice(LessonAction::QuizPick("line-midpoint".into(), 0)).unwrap();
        learn.practice(LessonAction::QuizCheck("line-midpoint".into())).unwrap();
        let third = learn.gate().unwrap();
        assert_eq!(blocks[third - 1].id, "current-follows-load", "the prediction does not gate the text");
        // The prediction unlocks the scene and is saved.
        learn.quiz_text.insert("predict-doubled".into(), "500".into());
        learn.practice(LessonAction::QuizCheck("predict-doubled".into())).unwrap();
        assert!(learn.scene_locked("load-step").is_none());
        assert_eq!(learn.scene.as_ref().map(|a| a.id.as_str()), Some("load-step"));
        let saved = sim_lesson::progress::Progress::load(&dir.join("progress.json"));
        assert_eq!(saved.quiz("motor-torque-speed", "predict-doubled").and_then(|r| r.prediction.clone()).as_deref(), Some("500"));
        assert!(saved.passed("motor-torque-speed", "stall-current"));
        // Figures rasterize off the calling thread.
        let start = std::time::Instant::now();
        while learn.figures.values().any(|f| matches!(f.state, practice::FigureState::Loading(_))) {
            learn.poll_figures(&mut images);
            assert!(start.elapsed() < std::time::Duration::from_secs(20));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(learn.figures.len(), 2);
        assert!(learn.figures.values().all(|f| matches!(f.state, practice::FigureState::Ready { .. })));
        drop(learn);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Draw the whole lesson through Bevy with its cards in every state
    /// (unanswered, typing, answered, revealed, writing a reflection, all
    /// unlocked): spawning must never produce an invalid bundle.
    #[test]
    fn every_card_state_spawns_valid_ui() {
        use bevy::ecs::system::RunSystemOnce;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("learn-draw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("lessons")).unwrap();
        std::fs::copy(root.join("lessons/concepts.yaml"), dir.join("lessons/concepts.yaml")).unwrap();
        for slug in ["motor-torque-speed", "worm-self-locking", "motor-driver-board", "knee-servo-measured"] {
            let from = root.join("lessons").join(slug);
            let to = dir.join("lessons").join(slug);
            std::fs::create_dir_all(&to).unwrap();
            for e in std::fs::read_dir(&from).unwrap().flatten() {
                if e.path().is_file() && !e.file_name().to_string_lossy().contains("annotations") {
                    std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
                }
            }
            // Systems referenced outside the lesson folder.
            let text = std::fs::read_to_string(to.join("lesson.md")).unwrap().replace("../../examples/", &format!("{}/examples/", root.display()));
            std::fs::write(to.join("lesson.md"), text).unwrap();
        }
        let mut world = World::new();
        world.insert_resource(crate::tests::fixture());
        world.insert_resource(ButtonInput::<MouseButton>::default());
        world.insert_resource(crate::ui_kit::UiFonts { regular: Handle::default(), italic: Handle::default(), mono: Handle::default(), icons: Default::default(), medium: Handle::default(), semibold: Handle::default() });
        let mut learn = Learn::new(dir.join("lessons"), root.join("library/systems"), sim_runtime::registry());
        learn.progress_path = dir.join("progress.json");
        learn.progress = sim_lesson::progress::Progress::default();
        world.insert_resource(learn);
        let draw = |world: &mut World| {
            world.resource_mut::<Learn>().dirty = true;
            world.run_system_once(ui::rebuild).unwrap();
            world.flush();
        };
        world.resource_mut::<Learn>().refresh_catalog();
        for slug in ["motor-torque-speed", "worm-self-locking", "motor-driver-board", "knee-servo-measured"] {
            world.resource_mut::<Learn>().open(slug).unwrap();
            draw(&mut world);
            // Model values (numbers, equations, measured data, lab predictions) arrive, then draw.
            {
                let start = std::time::Instant::now();
                let mut images = Assets::<Image>::default();
                while world.resource::<Learn>().model.loading {
                    world.resource_mut::<Learn>().poll_model(&mut images);
                    assert!(start.elapsed() < std::time::Duration::from_secs(120), "{slug}: model");
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                assert!(world.resource::<Learn>().model.errors.is_empty(), "{slug}: {:?}", world.resource::<Learn>().model.errors);
            }
            draw(&mut world);
            let quizzes: Vec<sim_lesson::quiz::Quiz> = world.resource::<Learn>().lesson.as_ref().unwrap().quizzes().map(|(_, q)| q.clone()).collect();
            for q in &quizzes {
                // Typing (numeric and predict) or picking (choice), then a wrong and a right answer.
                {
                    let mut l = world.resource_mut::<Learn>();
                    if q.kind == sim_lesson::quiz::QuizKind::Steps {
                        l.practice(LessonAction::StepInput(q.id.clone(), q.steps.iter().position(|s| s.blank()).unwrap())).unwrap();
                        l.input.as_mut().unwrap().buffer = "1".into();
                    } else if q.options.is_empty() {
                        l.practice(LessonAction::QuizInput(q.id.clone())).unwrap();
                        l.input.as_mut().unwrap().buffer = "123 units".into();
                    } else {
                        l.practice(LessonAction::QuizPick(q.id.clone(), 0)).unwrap();
                    }
                }
                draw(&mut world);
                {
                    let mut l = world.resource_mut::<Learn>();
                    let _ = l.practice(LessonAction::QuizCheck(q.id.clone()));
                    let _ = l.practice(LessonAction::QuizCheck(q.id.clone()));
                }
                draw(&mut world);
                {
                    let mut l = world.resource_mut::<Learn>();
                    match q.options.iter().position(|o| o.correct) {
                        Some(i) => l.practice(LessonAction::QuizPick(q.id.clone(), i)).unwrap(),
                        None => {
                            l.quiz_text.insert(q.id.clone(), q.answer.unwrap_or(1.0).to_string());
                        }
                    }
                    if q.kind == sim_lesson::quiz::QuizKind::Predict {
                        let _ = l.practice(LessonAction::QuizCheck(q.id.clone()));
                    } else if !q.pretest {
                        let _ = l.practice(LessonAction::HintMore(q.id.clone()));
                        l.practice(LessonAction::QuizReveal(q.id.clone())).unwrap();
                    }
                }
                draw(&mut world);
            }
            let reflections: Vec<String> = world.resource::<Learn>().lesson.as_ref().unwrap().blocks.iter().filter_map(|b| match &b.kind { BlockKind::Reflect(r) => Some(r.id.clone()), _ => None }).collect();
            for r in reflections {
                {
                    let mut l = world.resource_mut::<Learn>();
                    l.practice(LessonAction::ReflectInput(r.clone())).unwrap();
                    l.input.as_mut().unwrap().buffer = "Because the back-EMF is zero at stall.".into();
                }
                draw(&mut world);
                world.resource_mut::<Learn>().practice(LessonAction::ReflectSave(r)).unwrap();
                draw(&mut world);
            }
            assert_eq!(world.resource::<Learn>().gate(), None, "{slug}: everything unlocked");
            // Tasks (started, with a hint) and labs (ticked, predicted) draw in every state.
            let (tasks, labs): (Vec<String>, Vec<String>) = {
                let l = world.resource::<Learn>();
                let lesson = l.lesson.as_ref().unwrap();
                (lesson.blocks.iter().filter_map(|b| match &b.kind { BlockKind::Task(t) => Some(t.id.clone()), _ => None }).collect(), lesson.blocks.iter().filter_map(|b| match &b.kind { BlockKind::Lab(x) => Some(x.id.clone()), _ => None }).collect())
            };
            for t in tasks {
                let mut l = world.resource_mut::<Learn>();
                l.start_task(&t).unwrap();
                l.tasks.entry(t.clone()).or_default().hints = 2;
                drop(l);
                draw(&mut world);
            }
            for x in labs {
                let mut l = world.resource_mut::<Learn>();
                for i in 0..4 {
                    let mut scene = crate::tests::fixture();
                    l.try_act(LessonAction::LabTick(x.clone(), i), &mut scene).unwrap();
                }
                l.labs.entry(x.clone()).or_default().prediction = "0.5 rad/s".into();
                l.labs.entry(x.clone()).or_default().result = Some(Ok(serde_json::json!({"steady_rad_s": 0.44, "stopped": null, "receipt": "r.json"})));
                drop(l);
                draw(&mut world);
            }
            // Edit mode and annotate mode draw too.
            world.resource_mut::<Learn>().mode = PageMode::Edit;
            draw(&mut world);
            world.resource_mut::<Learn>().mode = PageMode::Annotate;
            draw(&mut world);
            world.resource_mut::<Learn>().mode = PageMode::Read;
        }
        world.remove_resource::<Learn>();
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// A slider's track (a kit slider over the parameter's fraction of its
/// range; the reader drags along it).
#[derive(Component)]
pub(crate) struct SliderTrack(pub String);
#[derive(Component)]
pub(crate) struct SliderFill(pub String);
#[derive(Component)]
pub(crate) struct SliderValue(pub String);

impl ActiveScene {
    /// A slider's value now: while dragging, the drag; else the reader's
    /// override; else the compiled system's value.
    pub(crate) fn slider_value(&self, scene: &SpatialScene, parameter: &str) -> Option<f64> {
        if let Some((p, v)) = &self.slider_drag {
            if p == parameter {
                return Some(*v);
            }
        }
        if let Some(v) = self.overrides.get(parameter) {
            return Some(*v);
        }
        if let Some(v) = self.scene.set.get(parameter) {
            return Some(*v);
        }
        let (at, name, key) = sim_script::presentation::split_parameter(parameter).ok()?;
        let path = if at.is_empty() { name } else { format!("{at}/{name}") };
        scene.description.components.get(&path)?.parameters.get(&key).map(|p| p.value)
    }
}

/// Input: dragging a slider moves its previewed value (`slider_drag`, kept
/// here); letting go sends a `Slider` action, which sets it and re-records.
fn sliders(tracks: Query<(&bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction, &SliderTrack)>, mut learn: ResMut<Learn>, mut out: MessageWriter<Act<actions::LessonCommand>>) {
    if !learn.active {
        return;
    }
    let Some(a) = learn.scene.as_ref() else { return };
    let mut dragging = None;
    for (value, pressed, interaction, track) in &tracks {
        if !crate::ui_kit::slider_held(pressed, interaction) {
            continue;
        }
        let Some(spec) = a.scene.sliders.iter().find(|s| s.parameter == track.0) else { continue };
        // The kit slider's value is the pointer's fraction of the track, 0..=1.
        dragging = Some((track.0.clone(), spec.snap(spec.min + value.0.clamp(0., 1.) as f64 * (spec.max - spec.min))));
    }
    match (dragging, a.slider_drag.clone()) {
        (Some(d), previous) => {
            if previous.as_ref() != Some(&d) {
                learn.bypass_change_detection().scene.as_mut().unwrap().slider_drag = Some(d);
            }
        }
        (None, Some((parameter, value))) => {
            out.write(Act::ui(actions::LessonCommand::Ui(LessonAction::Slider { parameter, value })));
        }
        (None, None) => {}
    }
}

/// Slider fills and values follow the drag and the reader's overrides.
fn slider_live(learn: Res<Learn>, scene: Res<SpatialScene>, mut fills: Query<(&mut Node, &SliderFill)>, mut values: Query<(&mut Text, &SliderValue)>) {
    let Some(a) = learn.scene.as_ref() else { return };
    for (mut node, fill) in &mut fills {
        let Some(spec) = a.scene.sliders.iter().find(|s| s.parameter == fill.0) else { continue };
        let v = a.slider_value(&scene, &fill.0).unwrap_or(spec.min);
        let w = Val::Percent((((v - spec.min) / (spec.max - spec.min).max(1e-12)).clamp(0., 1.) * 100.) as f32);
        if node.width != w {
            node.width = w;
        }
    }
    for (mut text, value) in &mut values {
        let Some(spec) = a.scene.sliders.iter().find(|s| s.parameter == value.0) else { continue };
        let shown = a.slider_value(&scene, &value.0).map(|v| format!("{} {}", crate::builder::ui::num(v), spec.unit)).unwrap_or_default();
        if text.0 != shown {
            text.0 = shown;
        }
    }
}

/// A time chart that previews its moment on hover and lights with its part.
#[derive(Component)]
pub(crate) struct ChartHover(pub String, pub f64, pub f64);

/// Hovering a chart shows that moment in the scene (and the part it
/// measures); clicking keeps it. Hovering a part lights up its charts.
/// The hover preview is local (restored when the pointer leaves); a click
/// keeps the moment through the lesson handler (`KeepMoment`), once per
/// press, with the moment held when the press ends (a drag keeps where it ends).
#[allow(clippy::too_many_arguments)]
fn chart_hover(mut charts: Query<(&Interaction, &bevy::ui::RelativeCursorPosition, &ChartHover, &mut BorderColor)>, mut learn: ResMut<Learn>, pointed: Res<crate::view::PartHover>, mut preview: Local<Option<f64>>, mut lit: Local<Option<String>>, mut held: Local<Option<f64>>, mut out: MessageWriter<Act<actions::LessonCommand>>) {
    if !learn.active {
        *held = None;
        return;
    }
    let mut hovered = None;
    let mut clicked = false;
    for (interaction, cursor, chart, _) in &charts {
        if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            if let Some(p) = crate::ui_kit::surface_point(cursor) {
                hovered = Some((chart.0.clone(), chart.1 + p.x.clamp(0., 1.) as f64 * (chart.2 - chart.1)));
                clicked |= *interaction == Interaction::Pressed;
            }
        }
    }
    let part_of = |key: &str| key.split('.').next().unwrap_or(key).to_string();
    // The press ended: keep its last moment through the handler (once per press).
    if !clicked {
        if let Some(t) = held.take() {
            out.write(Act::ui(actions::LessonCommand::Ui(LessonAction::KeepMoment(t))));
        }
    }
    match (&hovered, learn.scene.as_ref().is_some_and(|a| a.run.is_some() && !a.playing)) {
        (Some((key, t)), true) => {
            let a = learn.bypass_change_detection().scene.as_mut().unwrap();
            if preview.is_none() {
                *preview = Some(a.time);
            }
            if (a.time - t).abs() > 1e-9 {
                a.seek(*t);
            }
            if clicked {
                // Kept while held: nothing to restore on leaving.
                *preview = None;
                *held = Some(*t);
            }
            let part = part_of(key);
            if learn.hover_part.as_deref() != Some(part.as_str()) {
                learn.hover_part = Some(part.clone());
                *lit = Some(part);
            }
        }
        _ => {
            if let Some(back) = preview.take() {
                if let Some(a) = learn.bypass_change_detection().scene.as_mut() {
                    a.seek(back);
                }
            }
            if lit.take().is_some() && hovered.is_none() {
                learn.hover_part = None;
            }
        }
    }
    // Charts of the hovered part (a link, a chart, the narration) get an outline.
    let part = learn.hover_part.clone().or_else(|| pointed.0.clone()).or_else(|| learn.narration_part.clone());
    for (_, _, chart, mut border) in &mut charts {
        let on = part.as_deref().is_some_and(|p| part_of(&chart.0) == p || chart.0.starts_with(&format!("{p}/")));
        let color = if on { crate::ui_kit::ACCENT } else { Color::NONE };
        if border.top != color {
            *border = BorderColor::all(color);
        }
    }
}

/// Times worth jumping to in a scene: captions, parameter changes and pauses.
pub(crate) fn event_times(a: &ActiveScene) -> Vec<f64> {
    use sim_script::presentation::Action;
    let d = a.duration();
    let mut times: Vec<f64> = a.timeline.cues.iter().filter(|c| matches!(c.action, Action::Caption { .. } | Action::Set { .. } | Action::Pause) && c.at_s > 1e-9 && c.at_s < d).map(|c| c.at_s).collect();
    times.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
    times
}
