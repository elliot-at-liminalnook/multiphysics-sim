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
//!   scene parts (`sim_lesson::LessonAnchor`), in `lesson.md.annotations.json`,
//!   edited through the annotations service (`threads`, `crate::annotations`).
use crate::app::actions::Act;
use crate::builder::Builder;
use crate::{Orbit, SpatialScene};
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
mod controls;
mod editing;
pub(crate) mod extras;
mod lab;
pub(crate) mod frames;
pub(crate) mod guide;
mod handler;
pub(crate) mod narrate;
mod opening;
pub(crate) mod practice;
pub(crate) mod actions;
mod scene_view;
mod selection;
mod threads;
mod ui;
mod watch;

use controls::{ChartHover, SliderFill, SliderTrack, SliderValue, chart_hover, slider_live, sliders};
pub use opening::open_lessons;
use scene_view::{SceneViewport, Timebar, event_times, frame, playback, seek, view_aspect, viewport};
use watch::{apply_settings, poll};

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
    LabStop,
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
        if !learn { self.stop_labs(); }
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
        actions::add_fields(app);
        app.add_systems(Update, lab::window_loss.in_set(crate::app::InputSet::Window));
        // Keys, buttons, the timebars and slider releases write lesson actions;
        // the one handler applies them and REST's (in build and lessons:
        // without a lesson, REST is told so).
        app.add_systems(Update, (actions::keys, actions::buttons, seek, narrate::seek, sliders).chain().in_set(crate::app::InputSet::Window).run_if(open()))
            .add_systems(Update, actions::apply.in_set(crate::app::ViewerSet::Actions).run_if(in_state(crate::app::ModeScope::Builder)));
        app.add_systems(
            Update,
            (poll, practice::sketch_input, slider_live, chart_hover, ui::rebuild, ui::scroll, viewport, narrate::tick, playback, ui::live_text, narrate::live, narrate::overlay, practice::sketch_dots)
                .chain()
                // The card's place (`viewport` writes `learn_view`) and the
                // cues' glides reach the camera before it is placed.
                .before(crate::inspect_view::InspectViewSet::Camera)
                .in_set(crate::app::ViewerSet::SimSync)
                .run_if(open()),
        );
        app.add_systems(Update, (extras::live_equations, extras::track_blocks, apply_settings, frames::step).after(ui::rebuild).in_set(crate::app::ViewerSet::SimSync).run_if(open()));
        // A lesson draft's kit field follows the draft (after this frame's actions opened or closed it).
        app.add_systems(Update, actions::sync_field.in_set(crate::app::ViewerSet::SimSync).run_if(open()));
        // The page's pick follows the shared selection (Lessons only: in Build
        // the builder shows it).
        app.add_systems(Update, selection::follow.before(playback).in_set(crate::app::ViewerSet::SimSync).run_if(in_state(crate::app::ViewerMode::Lessons).and_then(resource_exists::<Learn>)));
    }
}

#[cfg(test)]
mod tests;
