//! Lessons: pre-written, editable Markdown files that teach with live systems.
//!
//! A lesson is a folder `lessons/<slug>/` holding `lesson.md`, the Rhai
//! scripts it uses and optionally its own system files. `lesson.md` is
//! ordinary Markdown with YAML front matter and three fenced block kinds:
//!
//! ````text
//! ---
//! title: Why worm gears self-lock
//! order: 3
//! systems: { winch: ../../examples/systems-builder/worm-drive/winch.system.json }
//! ---
//! Watch [the worm](part:winch/gearbox/worm) when the power goes off.
//!
//! ```sim-scene
//! id: hold
//! system: winch
//! run: { duration_s: 2.0 }
//! script: power-off.rhai
//! plots: [drum.shaft.speed]
//! expect:
//!   - { observe: drum.shaft.angle, reduce: change, window: [1.4, 2.0], min: -0.001, max: 0.001 }
//! ```
//! ````
//!
//! - `sim-scene`: a system shown and animated from a recorded run.
//! - `sim-component`: a library card (notes, equations, trade-offs).
//! - `sim-compare`: a saved study (comparison or sweep) of a system.
//! - `part:<system>/<instance path>` links point at parts in a scene.
//!
//! This crate only parses and validates the file shape, with errors that name
//! `lesson.md:line`; resolving systems, running scenes and checking `expect`
//! claims is `sim_runtime::lesson`. Nothing here simulates.
use serde::{Deserialize, Serialize};
use sim_script::presentation::{Action, CameraSpec, Cue, Timeline};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub mod anchor;
pub mod blocks;
pub mod categories;
pub mod concepts;
pub mod density;
pub mod edit;
pub mod figure;
pub mod index;
pub mod progress;
pub mod quiz;
pub mod narration;
pub mod refs;
pub mod report;
pub mod units;

pub use anchor::{LessonAnchor, LessonIndex};
pub use sim_system::Reduce;

pub const SCHEMA: &str = "sim.lesson/1";
/// Most animation frames one scene may record.
pub const MAX_FRAMES: f64 = 20_000.0;

/// A located error: `path:line: message`.
#[derive(Debug, Clone, PartialEq)]
pub struct LessonError {
    pub path: PathBuf,
    pub line: usize,
    pub message: String,
}
impl std::fmt::Display for LessonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.path.display(), self.line, self.message)
    }
}
impl std::error::Error for LessonError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrontMatter {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    /// Position in the lesson list (lower first).
    #[serde(default)]
    pub order: Option<i64>,
    /// Slugs of lessons to read first.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Named system files, relative to the lesson folder.
    #[serde(default)]
    pub systems: BTreeMap<String, String>,
    #[serde(default)]
    pub authors: Vec<String>,
    /// Expected reading time.
    #[serde(default)]
    pub minutes: Option<u32>,
    /// The lesson's category (an ID in `categories.yaml`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Concepts this lesson teaches and needs (IDs in `concepts.yaml`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub teaches: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fidelity {
    /// The validated model at the system's recorded settings.
    #[default]
    Detailed,
    /// The system's realtime profile (reduced parts, coarser step).
    Realtime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slider {
    /// `instance/path.parameter`, like `set`.
    pub parameter: String,
    #[serde(default)]
    pub label: String,
    pub min: f64,
    pub max: f64,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub unit: String,
}
impl Slider {
    /// `value` snapped to the step and kept inside the range.
    pub fn snap(&self, value: f64) -> f64 {
        let v = value.clamp(self.min, self.max);
        match self.step.filter(|s| *s > 0.) {
            Some(s) => (self.min + ((v - self.min) / s).round() * s).clamp(self.min, self.max),
            None => v,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompanionMode {
    /// Side by side, on one clock.
    #[default]
    Split,
    /// Faint, over this run.
    Ghost,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Companion {
    pub label: String,
    /// Parameter overrides for the second run, like the scene's `set`.
    pub set: BTreeMap<String, f64>,
    #[serde(default)]
    pub mode: CompanionMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    /// What to achieve (Markdown).
    pub goal: String,
    /// All must hold on a run for the goal to be met.
    pub win: Vec<Expect>,
    #[serde(default)]
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhasePlot {
    pub x: String,
    pub y: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSpec {
    /// Simulated seconds recorded for playback and checks.
    #[serde(default = "default_duration")]
    pub duration_s: f64,
    /// Recorded animation frames per simulated second (fast electrical
    /// scenes use thousands, with a `speed` cue for slow motion).
    #[serde(default = "default_frame_rate")]
    pub frame_rate: f64,
}
fn default_duration() -> f64 {
    2.0
}
fn default_frame_rate() -> f64 {
    60.0
}
impl Default for RunSpec {
    fn default() -> Self {
        Self { duration_s: default_duration(), frame_rate: default_frame_rate() }
    }
}

/// A claim the lesson makes about a scene, checked against its recorded run.
/// The reduction is the same one saved studies use (`sim_system::Reduce`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Observable key (`drum.shaft.speed`) or ID.
    pub observe: String,
    pub reduce: Reduce,
    /// Window start and end in simulated seconds (whole run if absent).
    #[serde(default)]
    pub window: Option<[f64; 2]>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    /// What the claim means, shown next to the result.
    #[serde(default)]
    pub why: String,
}

/// YAML cue: `{ at: 1.2, caption: "…", set: { supply.voltage: 0 } }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CueSpec {
    pub at: f64,
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub set: BTreeMap<String, f64>,
    #[serde(default)]
    pub highlight: Option<Vec<String>>,
    #[serde(default)]
    pub camera: Option<CameraSpec>,
    #[serde(default)]
    pub plot: Option<Vec<String>>,
    #[serde(default)]
    pub pause: bool,
    #[serde(default)]
    pub speed: Option<f64>,
    /// Camera and emphasis directives, e.g. `[{view: orbit, rate: 0.2}]`.
    #[serde(default)]
    pub view: Vec<sim_script::presentation::View>,
}
impl CueSpec {
    fn cues(&self) -> Vec<Cue> {
        let mut out = Vec::new();
        let mut push = |action| out.push(Cue { at_s: self.at, action });
        for v in &self.view {
            push(Action::View { view: v.clone() });
        }
        if let Some(text) = &self.caption {
            push(Action::Caption { text: text.clone() });
        }
        for (parameter, value) in &self.set {
            push(Action::Set { parameter: parameter.clone(), value: *value });
        }
        if let Some(paths) = &self.highlight {
            push(Action::Highlight { paths: paths.clone() });
        }
        if let Some(camera) = &self.camera {
            push(Action::Camera { camera: camera.clone() });
        }
        if let Some(observables) = &self.plot {
            push(Action::Plot { observables: observables.clone() });
        }
        if let Some(factor) = self.speed {
            push(Action::Speed { factor });
        }
        if self.pause {
            push(Action::Pause);
        }
        out
    }
}

fn one() -> f64 {
    1.0
}
fn is_one(v: &f64) -> bool {
    *v == 1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub id: String,
    /// A name from the front matter `systems`, or a path relative to the lesson.
    pub system: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub caption: String,
    /// Instance path whose parts are shown in full (others ghosted); "" = all.
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub camera: Option<CameraSpec>,
    /// Parameter values applied before the run: `instance/path.parameter: value`.
    #[serde(default)]
    pub set: BTreeMap<String, f64>,
    #[serde(default)]
    pub run: RunSpec,
    /// Rhai presentation script, relative to the lesson folder.
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub cues: Vec<CueSpec>,
    /// Observables charted under the scene.
    #[serde(default)]
    pub plots: Vec<String>,
    /// Operating-point plots: `y` against `x` over the run, with a dot at the
    /// playhead (e.g. a motor's current against its speed traces its
    /// torque–speed line).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phase: Vec<PhasePlot>,
    /// Physics layers shown with this scene (power, forces, current, heat,
    /// trails); absent keeps the viewer's choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<Vec<sim_inspect::spatial::Overlay>>,
    /// Draw sliding parts' movement this many times larger (display only:
    /// values, plots and physics are unchanged; the view says so). For
    /// motions too small to see, such as a tenth of a millimetre of play.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub magnify: f64,
    /// Parameters the reader can change; releasing a slider re-records.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sliders: Vec<Slider>,
    /// Things to try in free exploration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
    /// A second run with other values, shown beside (`split`) or over (`ghost`) this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub companion: Option<Companion>,
    /// A goal the reader reaches by moving the sliders; checked on the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge: Option<Challenge>,
    #[serde(default)]
    pub expect: Vec<Expect>,
    /// Viewport height in logical pixels.
    #[serde(default)]
    pub height: Option<f32>,
    #[serde(default)]
    pub fidelity: Fidelity,
    /// Start playing when the scene becomes the active one.
    #[serde(default)]
    pub autoplay: bool,
    /// Observables recorded with the run but not charted: what equations and
    /// prose values read (filled in by the runtime, never authored).
    #[serde(skip)]
    pub record: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentCard {
    /// Registry component type, e.g. `rotational.worm_gear`.
    pub component: String,
    /// Which parts of the notes to show: summary, explanation, equations,
    /// tradeoffs, limits, parameters (default: summary, equations, tradeoffs).
    #[serde(default)]
    pub show: Vec<String>,
}
pub const CARD_SECTIONS: [&str; 6] = ["summary", "explanation", "equations", "tradeoffs", "limits", "parameters"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compare {
    pub id: String,
    pub system: String,
    /// Name of a saved study in the system file.
    pub study: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub caption: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    Heading { level: u8, text: String, slug: String },
    Markdown { text: String },
    Scene(Box<Scene>),
    Component(ComponentCard),
    Compare(Compare),
    Quiz(Box<quiz::Quiz>),
    Reflect(quiz::Reflect),
    Equation(Box<blocks::Equation>),
    Measured(Box<blocks::Measured>),
    Remedy(blocks::Remedy),
    Task(Box<blocks::Task>),
    Lab(blocks::Lab),
}

impl BlockKind {
    /// The `id` of an embedded block (text and headings have none).
    pub fn id(&self) -> Option<&str> {
        match self {
            BlockKind::Scene(s) => Some(&s.id),
            BlockKind::Compare(c) => Some(&c.id),
            BlockKind::Quiz(q) => Some(&q.id),
            BlockKind::Reflect(r) => Some(&r.id),
            BlockKind::Equation(e) => Some(&e.id),
            BlockKind::Measured(m) => Some(&m.id),
            BlockKind::Remedy(r) => Some(&r.id),
            BlockKind::Task(t) => Some(&t.id),
            BlockKind::Lab(l) => Some(&l.id),
            _ => None,
        }
    }
    /// Short kind name for listings.
    pub fn name(&self) -> &'static str {
        match self {
            BlockKind::Heading { .. } => "heading",
            BlockKind::Markdown { .. } => "text",
            BlockKind::Scene(_) => "scene",
            BlockKind::Component(_) => "component",
            BlockKind::Compare(_) => "compare",
            BlockKind::Quiz(_) => "quiz",
            BlockKind::Reflect(r) if r.kind == quiz::ReflectKind::Recall => "recall",
            BlockKind::Reflect(_) => "reflect",
            BlockKind::Equation(_) => "equation",
            BlockKind::Measured(_) => "measured",
            BlockKind::Remedy(_) => "remedy",
            BlockKind::Task(_) => "task",
            BlockKind::Lab(_) => "lab",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// Embeds: their `id`. Text blocks: `b<index>` (positional; use `hash`
    /// to detect that the block changed).
    pub id: String,
    /// 1-based first and last line in the file.
    pub line: usize,
    pub end_line: usize,
    /// Byte range in the source (the whole block, fences included).
    pub start: usize,
    pub end: usize,
    /// Content hash of `source[start..end]`.
    pub hash: String,
    /// Heading text of the section the block is in ("" before the first heading).
    pub section: String,
    pub kind: BlockKind,
}
impl Block {
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
    pub fn embed_id(&self) -> Option<&str> {
        self.kind.id()
    }
}

/// `part:<system>/<instance path>` found in Markdown text.
#[derive(Debug, Clone, PartialEq)]
pub struct PartLink {
    pub system: String,
    pub path: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Lesson {
    /// The `lesson.md` file.
    pub path: PathBuf,
    /// Folder name; the lesson's stable ID.
    pub slug: String,
    pub meta: FrontMatter,
    pub source: String,
    /// Byte offset where the body starts (after the front matter).
    pub body: usize,
    pub blocks: Vec<Block>,
}

pub fn hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex()[..16].to_string()
}

/// URL-style anchor for a heading.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

fn yaml_line(e: &serde_norway::Error) -> usize {
    e.location().map(|l| l.line()).unwrap_or(1)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl Lesson {
    pub fn load(path: &Path) -> Result<Self, LessonError> {
        let source = std::fs::read_to_string(path).map_err(|e| LessonError { path: path.into(), line: 0, message: e.to_string() })?;
        Self::parse(path, &source)
    }

    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }

    /// Parse and check the file's shape. Every error names the line.
    pub fn parse(path: &Path, source: &str) -> Result<Self, LessonError> {
        let err = |line: usize, message: String| LessonError { path: path.into(), line, message };
        if source.len() > 2_000_000 {
            return Err(err(1, "lesson file larger than 2 MB".into()));
        }
        // Line table: (1-based number, byte start, text without newline).
        let mut lines: Vec<Line> = Vec::new();
        let mut at = 0;
        for (i, l) in source.split_inclusive('\n').enumerate() {
            lines.push((i + 1, at, l.trim_end_matches(['\n', '\r'])));
            at += l.len();
        }
        let line_end = |i: usize| lines.get(i).map(|(_, s, _)| *s).unwrap_or(source.len());
        // Front matter.
        if lines.first().is_none_or(|(_, _, t)| t.trim_end() != "---") {
            return Err(err(1, "a lesson starts with YAML front matter between `---` lines (title is required)".into()));
        }
        let close = lines.iter().skip(1).position(|(_, _, t)| t.trim_end() == "---").map(|p| p + 1).ok_or_else(|| err(1, "front matter is not closed with `---`".into()))?;
        let yaml = &source[line_end(1)..line_end(close)];
        let meta: FrontMatter = serde_norway::from_str(if yaml.trim().is_empty() { "{}" } else { yaml }).map_err(|e| err(1 + yaml_line(&e), format!("front matter: {e}")))?;
        if meta.title.trim().is_empty() {
            return Err(err(2, "front matter: title is empty".into()));
        }
        for (name, file) in &meta.systems {
            if !valid_id(name) || file.trim().is_empty() {
                return Err(err(2, format!("front matter: system `{name}` needs a simple name and a file path")));
            }
        }
        let folder = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "lesson".into());
        let mut blocks: Vec<Block> = Vec::new();
        let mut section = String::new();
        let mut chunk: Option<(usize, usize)> = None; // (first line index, last line index)
        let mut ids = BTreeSet::new();
        let mut text_index = 0usize;
        let mut i = close + 1;
        while i < lines.len() {
            let (number, _, text) = lines[i];
            let trimmed = text.trim_start();
            let fence = ["```", "~~~"].into_iter().find(|f| trimmed.starts_with(f));
            if let Some(f) = fence {
                let marker: String = trimmed.chars().take_while(|c| *c == f.chars().next().unwrap()).collect();
                let info = trimmed[marker.len()..].trim();
                let closing = (i + 1..lines.len()).find(|&j| {
                    let t = lines[j].2.trim();
                    t.starts_with(&marker) && t.chars().all(|c| c == marker.chars().next().unwrap())
                });
                let Some(end) = closing else {
                    return Err(err(number, format!("code fence `{marker}` is never closed")));
                };
                if let Some(kind) = info.strip_prefix("sim-") {
                    flush(&mut chunk, &mut blocks, &lines, source, &section, &mut text_index);
                    let body = &source[line_end(i + 1)..lines[end].1];
                    let yaml = if body.trim().is_empty() { "{}" } else { body };
                    let at = |e: &serde_norway::Error| number + yaml_line(e);
                    let parsed = match kind {
                        "scene" => serde_norway::from_str::<Scene>(yaml).map(|s| BlockKind::Scene(Box::new(s))),
                        "component" => serde_norway::from_str::<ComponentCard>(yaml).map(BlockKind::Component),
                        "compare" => serde_norway::from_str::<Compare>(yaml).map(BlockKind::Compare),
                        "quiz" => serde_norway::from_str::<quiz::Quiz>(yaml).map(|q| BlockKind::Quiz(Box::new(q))),
                        "reflect" => serde_norway::from_str::<quiz::Reflect>(yaml).map(BlockKind::Reflect),
                        "recall" => serde_norway::from_str::<quiz::Reflect>(yaml).map(|r| BlockKind::Reflect(quiz::Reflect { kind: quiz::ReflectKind::Recall, ..r })),
                        "equation" => serde_norway::from_str::<blocks::Equation>(yaml).map(|e| BlockKind::Equation(Box::new(e))),
                        "measured" => serde_norway::from_str::<blocks::Measured>(yaml).map(|m| BlockKind::Measured(Box::new(m))),
                        "remedy" => serde_norway::from_str::<blocks::Remedy>(yaml).map(BlockKind::Remedy),
                        "task" => serde_norway::from_str::<blocks::Task>(yaml).map(|t| BlockKind::Task(Box::new(t))),
                        "lab" => serde_norway::from_str::<blocks::Lab>(yaml).map(BlockKind::Lab),
                        other => return Err(err(number, format!("unknown block `sim-{other}` (sim-scene, sim-component, sim-compare, sim-quiz, sim-reflect, sim-recall, sim-equation, sim-measured, sim-remedy, sim-task or sim-lab)"))),
                    }
                    .map_err(|e| err(at(&e), format!("sim-{kind}: {e}")))?;
                    check_embed(&parsed, &meta).map_err(|m| err(number, format!("sim-{kind}: {m}")))?;
                    if let Some(id) = parsed.id() {
                        if !ids.insert(id.to_string()) {
                            return Err(err(number, format!("sim-{kind}: id `{id}` is used twice")));
                        }
                    }
                    push_block(&mut blocks, &lines, source, i, end, parsed, &section, &mut text_index);
                } else {
                    // An ordinary code block belongs to the current text block.
                    let first = chunk.map(|c| c.0).unwrap_or(i);
                    chunk = Some((first, end));
                }
                i = end + 1;
                continue;
            }
            if trimmed.is_empty() {
                flush(&mut chunk, &mut blocks, &lines, source, &section, &mut text_index);
            } else if let Some(level) = heading_level(text) {
                flush(&mut chunk, &mut blocks, &lines, source, &section, &mut text_index);
                let title = text.trim_start_matches('#').trim().to_string();
                section = title.clone();
                push_block(&mut blocks, &lines, source, i, i, BlockKind::Heading { level, slug: slug(&title), text: title }, &section, &mut text_index);
            } else {
                chunk = Some((chunk.map(|c| c.0).unwrap_or(i), i));
            }
            i += 1;
        }
        flush(&mut chunk, &mut blocks, &lines, source, &section, &mut text_index);
        let lesson = Self { path: path.into(), slug: folder, meta, source: source.to_string(), body: line_end(close + 1), blocks };
        // Images are local files; predict questions come before their scene.
        for f in lesson.figures() {
            figure::validate_src(&f.src).map_err(|m| err(f.line, m))?;
        }
        for (i, b) in lesson.blocks.iter().enumerate() {
            if let BlockKind::Quiz(q) = &b.kind {
                if let Some(scene) = &q.scene {
                    match lesson.blocks.iter().position(|x| matches!(&x.kind, BlockKind::Scene(s) if &s.id == scene)) {
                        None => return Err(err(b.line, format!("sim-quiz {}: no scene `{scene}` in the lesson", q.id))),
                        Some(at) if at < i => return Err(err(b.line, format!("sim-quiz {}: a prediction must come before its scene `{scene}`", q.id))),
                        _ => {}
                    }
                }
            }
        }
        for link in lesson.part_links_checked()? {
            let _ = link;
        }
        lesson.check_references().map_err(|(line, m)| err(line, m))?;
        Ok(lesson)
    }

    /// Cross-block references: remedies, scenes named by equations and
    /// tasks, follow-up questions, lab predictions, inline `{{…}}` values.
    fn check_references(&self) -> Result<(), (usize, String)> {
        let scene = |id: &str| self.scene(id).is_some();
        let quiz = |id: &str| self.quiz(id).is_some();
        let remedy = |id: &str| self.blocks.iter().any(|b| matches!(&b.kind, BlockKind::Remedy(r) if r.id == id));
        for b in &self.blocks {
            let bad = match &b.kind {
                BlockKind::Quiz(q) => q.options.iter().filter_map(|o| o.remedy.as_ref()).find(|r| !remedy(r)).map(|r| format!("sim-quiz {}: no sim-remedy `{r}`", q.id)),
                BlockKind::Remedy(r) => r.scene.as_ref().filter(|s| !scene(s)).map(|s| format!("sim-remedy {}: no scene `{s}`", r.id)).or_else(|| r.then.as_ref().filter(|q| !quiz(q)).map(|q| format!("sim-remedy {}: no question `{q}`", r.id))),
                BlockKind::Equation(e) => e.scene.as_ref().filter(|s| !scene(s)).map(|s| format!("sim-equation {}: no scene `{s}`", e.id)),
                BlockKind::Task(t) => (!scene(&t.scene)).then(|| format!("sim-task {}: no scene `{}`", t.id, t.scene)),
                BlockKind::Lab(l) => l.compare.as_ref().filter(|c| self.measured(c).is_none()).map(|c| format!("sim-lab {}: no sim-measured `{c}` to compare with", l.id)),
                BlockKind::Markdown { text } => match refs::find(text) {
                    Err((at, m)) => return Err((b.line + text[..at].matches('\n').count(), m)),
                    Ok(found) => found.iter().find_map(|r| match &r.source {
                        refs::Source::Value { scene: s, .. } if !scene(s) => Some(format!("{{{{value …}}}}: no scene `{s}`")),
                        refs::Source::Data { block, .. } if self.measured(block).is_none() => Some(format!("{{{{data …}}}}: no sim-measured `{block}`")),
                        _ => None,
                    }),
                },
                _ => None,
            };
            if let Some(m) = bad {
                return Err((b.line, m));
            }
        }
        Ok(())
    }

    /// Every inline `{{…}}` value in the prose, with its line.
    pub fn inline_refs(&self) -> Vec<(usize, refs::InlineRef)> {
        self.blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Markdown { text } => Some((b, text)),
                _ => None,
            })
            .flat_map(|(b, text)| refs::find(text).unwrap_or_default().into_iter().map(move |r| (b.line + text[..r.start].matches('\n').count(), r)))
            .collect()
    }

    pub fn equations(&self) -> impl Iterator<Item = (&Block, &blocks::Equation)> {
        self.blocks.iter().filter_map(|b| match &b.kind {
            BlockKind::Equation(e) => Some((b, e.as_ref())),
            _ => None,
        })
    }
    pub fn remedy(&self, id: &str) -> Option<&blocks::Remedy> {
        self.blocks.iter().find_map(|b| match &b.kind {
            BlockKind::Remedy(r) if r.id == id => Some(r),
            _ => None,
        })
    }
    pub fn task(&self, id: &str) -> Option<&blocks::Task> {
        self.blocks.iter().find_map(|b| match &b.kind {
            BlockKind::Task(t) if t.id == id => Some(t.as_ref()),
            _ => None,
        })
    }
    pub fn measured(&self, id: &str) -> Option<&blocks::Measured> {
        self.blocks.iter().find_map(|b| match &b.kind {
            BlockKind::Measured(m) if m.id == id => Some(m.as_ref()),
            _ => None,
        })
    }
    pub fn lab(&self, id: &str) -> Option<&blocks::Lab> {
        self.blocks.iter().find_map(|b| match &b.kind {
            BlockKind::Lab(l) if l.id == id => Some(l),
            _ => None,
        })
    }
    /// Concepts a question practises: its own tags, else the lesson's.
    pub fn concepts_of(&self, q: &quiz::Quiz) -> Vec<String> {
        if q.concepts.is_empty() { self.meta.teaches.clone() } else { q.concepts.clone() }
    }

    pub fn block(&self, id: &str) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }
    pub fn scenes(&self) -> impl Iterator<Item = (&Block, &Scene)> {
        self.blocks.iter().filter_map(|b| match &b.kind {
            BlockKind::Scene(s) => Some((b, s.as_ref())),
            _ => None,
        })
    }
    pub fn quizzes(&self) -> impl Iterator<Item = (&Block, &quiz::Quiz)> {
        self.blocks.iter().filter_map(|b| match &b.kind {
            BlockKind::Quiz(q) => Some((b, q.as_ref())),
            _ => None,
        })
    }
    pub fn quiz(&self, id: &str) -> Option<&quiz::Quiz> {
        self.quizzes().find(|(_, q)| q.id == id).map(|(_, q)| q)
    }
    /// The predict question that must be answered before a scene plays.
    pub fn prediction_for(&self, scene: &str) -> Option<&quiz::Quiz> {
        self.quizzes().find(|(_, q)| q.kind.predicts() && q.scene.as_deref() == Some(scene)).map(|(_, q)| q)
    }
    /// Index of the first block hidden by an unanswered gating question
    /// (`passed(id)` says which questions and reflections are done).
    pub fn gate(&self, passed: impl Fn(&str) -> bool) -> Option<usize> {
        self.blocks.iter().position(|b| match &b.kind {
            BlockKind::Quiz(q) => q.gates() && !passed(&q.id),
            BlockKind::Reflect(r) => r.gate && !passed(&r.id),
            _ => false,
        }).map(|i| i + 1)
    }
    pub fn scene(&self, id: &str) -> Option<&Scene> {
        self.scenes().find(|(_, s)| s.id == id).map(|(_, s)| s)
    }
    pub fn compares(&self) -> impl Iterator<Item = (&Block, &Compare)> {
        self.blocks.iter().filter_map(|b| match &b.kind {
            BlockKind::Compare(c) => Some((b, c)),
            _ => None,
        })
    }
    pub fn headings(&self) -> impl Iterator<Item = (&Block, u8, &str, &str)> {
        self.blocks.iter().filter_map(|b| match &b.kind {
            BlockKind::Heading { level, text, slug } => Some((b, *level, text.as_str(), slug.as_str())),
            _ => None,
        })
    }

    /// Absolute path of a system named in the front matter (or given as a
    /// path relative to the lesson).
    pub fn system_path(&self, system: &str) -> PathBuf {
        let file = self.meta.systems.get(system).map(String::as_str).unwrap_or(system);
        self.dir().join(file)
    }

    /// Every `part:` link in text blocks, resolved to a system name.
    pub fn part_links(&self) -> Vec<PartLink> {
        self.part_links_checked().unwrap_or_default()
    }
    fn part_links_checked(&self) -> Result<Vec<PartLink>, LessonError> {
        let mut out = Vec::new();
        for b in &self.blocks {
            let BlockKind::Markdown { text } = &b.kind else { continue };
            for (offset, _) in text.match_indices("](part:") {
                let rest = &text[offset + 7..];
                let target = rest.split(|c| c == ')' || c == ' ').next().unwrap_or("");
                let line = b.line + text[..offset].matches('\n').count();
                out.push(self.resolve_part(target).map_err(|m| LessonError { path: self.path.clone(), line, message: m })?.with_line(line));
            }
        }
        Ok(out)
    }
    /// `winch/gearbox/worm` → (winch, gearbox/worm). With a single system the
    /// system name may be left out.
    pub fn resolve_part(&self, target: &str) -> Result<PartLink, String> {
        let target = target.strip_prefix("part:").unwrap_or(target);
        let (first, rest) = target.split_once('/').unwrap_or((target, ""));
        if self.meta.systems.contains_key(first) && !rest.is_empty() {
            return Ok(PartLink { system: first.into(), path: rest.into(), line: 0 });
        }
        if self.meta.systems.len() == 1 && !target.is_empty() {
            let system = self.meta.systems.keys().next().unwrap().clone();
            return Ok(PartLink { system, path: target.into(), line: 0 });
        }
        Err(format!("part link `part:{target}` must start with a system named in the front matter ({})", self.meta.systems.keys().cloned().collect::<Vec<_>>().join(", ")))
    }

    /// The scene's full timeline: its script (if any) merged with YAML cues.
    pub fn timeline(&self, scene: &Scene) -> Result<Timeline, String> {
        let mut timeline = Timeline::new(scene.cues.iter().flat_map(CueSpec::cues).collect())?;
        if let Some(script) = &scene.script {
            let path = self.dir().join(script);
            let source = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            timeline = timeline.merged(sim_script::presentation::evaluate(&path.display().to_string(), &source)?)?;
        }
        Ok(timeline)
    }

    /// Normalized plain text of every prose block, with each block's
    /// character range: what text annotations anchor to.
    pub fn text_index(&self) -> LessonIndex {
        LessonIndex::build(self)
    }
}

impl PartLink {
    fn with_line(mut self, line: usize) -> Self {
        self.line = line;
        self
    }
}

type Line<'a> = (usize, usize, &'a str);

#[allow(clippy::too_many_arguments)]
fn push_block(blocks: &mut Vec<Block>, lines: &[Line], source: &str, first: usize, last: usize, kind: BlockKind, section: &str, text_index: &mut usize) {
    let start = lines[first].1;
    let end = lines[last].1 + lines[last].2.len();
    let id = match kind.id() {
        Some(id) => id.to_string(),
        None => {
            *text_index += 1;
            format!("b{}", *text_index)
        }
    };
    blocks.push(Block { id, line: lines[first].0, end_line: lines[last].0, start, end, hash: hash(&source[start..end]), section: section.to_string(), kind });
}

fn flush(chunk: &mut Option<(usize, usize)>, blocks: &mut Vec<Block>, lines: &[Line], source: &str, section: &str, text_index: &mut usize) {
    if let Some((first, last)) = chunk.take() {
        let text = source[lines[first].1..lines[last].1 + lines[last].2.len()].to_string();
        push_block(blocks, lines, source, first, last, BlockKind::Markdown { text }, section, text_index);
    }
}

fn heading_level(text: &str) -> Option<u8> {
    let hashes = text.chars().take_while(|c| *c == '#').count();
    ((1..=6).contains(&hashes) && text[hashes..].starts_with(' ')).then_some(hashes as u8)
}

fn check_embed(kind: &BlockKind, meta: &FrontMatter) -> Result<(), String> {
    let system_known = |s: &str| meta.systems.contains_key(s) || s.ends_with(".system.json");
    match kind {
        BlockKind::Scene(s) => {
            if !valid_id(&s.id) {
                return Err(format!("id `{}` must be 1–64 letters, digits, - or _", s.id));
            }
            if !system_known(&s.system) {
                return Err(format!("system `{}` is not named in the front matter", s.system));
            }
            if !(s.run.duration_s.is_finite() && s.run.duration_s > 0.0 && s.run.duration_s <= 600.0) {
                return Err("run.duration_s must be in (0, 600] seconds".into());
            }
            if !(s.run.frame_rate.is_finite() && s.run.frame_rate >= 1.0) || s.run.frame_rate * s.run.duration_s > MAX_FRAMES {
                return Err(format!("run.frame_rate must be ≥ 1 frame per simulated second, with at most {MAX_FRAMES} frames in the run (fast systems: raise the rate and slow playback with a speed cue)"));
            }
            if s.height.is_some_and(|h| !(120.0..=1200.0).contains(&h)) {
                return Err("height must be 120–1200 pixels".into());
            }
            if !(s.magnify.is_finite() && (1.0..=100_000.0).contains(&s.magnify)) {
                return Err("magnify must be 1–100000 (display-only enlargement of sliding motion)".into());
            }
            if let Some(c) = &s.camera {
                c.validate()?;
            }
            for key in s.set.keys() {
                sim_script::presentation::split_parameter(key)?;
            }
            for (i, c) in s.cues.iter().enumerate() {
                if !(c.at.is_finite() && c.at >= 0.0) {
                    return Err(format!("cue {} time must be ≥ 0", i + 1));
                }
                for key in c.set.keys() {
                    sim_script::presentation::split_parameter(key)?;
                }
            }
            for e in &s.expect {
                if e.min.is_none() && e.max.is_none() {
                    return Err(format!("expect on {}: give min, max or both", e.observe));
                }
                if e.window.is_some_and(|[a, b]| !(a.is_finite() && b.is_finite() && a < b)) {
                    return Err(format!("expect on {}: window must be [start, end] with start < end", e.observe));
                }
            }
            Ok(())
        }
        BlockKind::Compare(c) => {
            if !valid_id(&c.id) {
                return Err(format!("id `{}` must be 1–64 letters, digits, - or _", c.id));
            }
            if !system_known(&c.system) {
                return Err(format!("system `{}` is not named in the front matter", c.system));
            }
            if c.study.trim().is_empty() {
                return Err("study is empty".into());
            }
            Ok(())
        }
        BlockKind::Quiz(q) => {
            if !valid_id(&q.id) {
                return Err(format!("id `{}` must be 1–64 letters, digits, - or _", q.id));
            }
            q.validate()
        }
        BlockKind::Reflect(r) => {
            if !valid_id(&r.id) {
                return Err(format!("id `{}` must be 1–64 letters, digits, - or _", r.id));
            }
            if r.prompt.trim().is_empty() {
                return Err("prompt is empty".into());
            }
            Ok(())
        }
        BlockKind::Equation(e) => {
            if !valid_id(&e.id) {
                return Err(format!("id `{}` must be 1–64 letters, digits, - or _", e.id));
            }
            blocks::validate_equation(e)
        }
        BlockKind::Measured(m) => {
            if !valid_id(&m.id) {
                return Err(format!("id `{}` must be 1–64 letters, digits, - or _", m.id));
            }
            if !system_known(&m.system) {
                return Err(format!("system `{}` is not named in the front matter", m.system));
            }
            blocks::validate_measured(m)
        }
        BlockKind::Remedy(r) => {
            if !valid_id(&r.id) {
                return Err(format!("id `{}` must be 1–64 letters, digits, - or _", r.id));
            }
            if r.misconception.trim().is_empty() || r.body.trim().is_empty() {
                return Err("name the misconception and give a body".into());
            }
            Ok(())
        }
        BlockKind::Task(t) => {
            if !valid_id(&t.id) {
                return Err(format!("id `{}` must be 1–64 letters, digits, - or _", t.id));
            }
            blocks::validate_task(t)
        }
        BlockKind::Lab(l) => {
            if !valid_id(&l.id) {
                return Err(format!("id `{}` must be 1–64 letters, digits, - or _", l.id));
            }
            blocks::validate_lab(l)
        }
        BlockKind::Component(card) => {
            if !card.component.contains('.') {
                return Err(format!("component `{}` must be a registry type such as rotational.worm_gear", card.component));
            }
            if let Some(bad) = card.show.iter().find(|s| !CARD_SECTIONS.contains(&s.as_str())) {
                return Err(format!("show: unknown section `{bad}` ({})", CARD_SECTIONS.join(", ")));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const GOOD: &str = "---\ntitle: Worms\nsystems: { winch: winch.system.json }\n---\n# Intro\n\nA worm [drive](part:winch/gearbox/worm)\nholds.\n\n```rust\nlet x = 1;\n\nlet y = 2;\n```\n\n```sim-scene\nid: hold\nsystem: winch\nset: { supply.voltage: 6 }\nexpect:\n  - { observe: drum.shaft.speed, reduce: mean, min: 0 }\n```\n\n## After\n\n```sim-component\ncomponent: rotational.worm_gear\n```\n";
    #[test]
    fn parses_blocks_sections_and_links() {
        let l = Lesson::parse(Path::new("lessons/worms/lesson.md"), GOOD).unwrap();
        assert_eq!(l.slug, "worms");
        let kinds: Vec<&str> = l.blocks.iter().map(|b| match b.kind { BlockKind::Heading { .. } => "h", BlockKind::Markdown { .. } => "p", BlockKind::Scene(_) => "scene", BlockKind::Component(_) => "card", BlockKind::Compare(_) => "cmp", _ => "q" }).collect();
        assert_eq!(kinds, vec!["h", "p", "p", "scene", "h", "card"]);
        // The code fence with a blank line inside stays one block.
        assert!(matches!(&l.blocks[2].kind, BlockKind::Markdown { text } if text.contains("let y")));
        assert_eq!(l.blocks[3].id, "hold");
        assert_eq!(l.blocks[3].section, "Intro");
        assert_eq!(l.part_links(), vec![PartLink { system: "winch".into(), path: "gearbox/worm".into(), line: 7 }]);
        assert_eq!(l.block("b2").unwrap().text(&l.source).lines().count(), 2);
    }
    #[test]
    fn errors_name_the_line() {
        let bad = GOOD.replace("reduce: mean", "reduce: average");
        let e = Lesson::parse(Path::new("l/lesson.md"), &bad).unwrap_err();
        assert_eq!(e.line, 21, "{e}");
        let e = Lesson::parse(Path::new("l/lesson.md"), &GOOD.replace("sim-component", "sim-widget")).unwrap_err();
        assert!(e.to_string().starts_with("l/lesson.md:26: unknown block `sim-widget`"), "{e}");
        // One system: the name may be left out (the path is then checked by
        // `sim-lesson check` against the system). Two systems: it must be given.
        let l = Lesson::parse(Path::new("l/lesson.md"), &GOOD.replace("part:winch/gearbox", "part:crane/gearbox")).unwrap();
        assert_eq!(l.part_links()[0].path, "crane/gearbox/worm");
        let two = GOOD.replace("systems: { winch: winch.system.json }", "systems: { winch: winch.system.json, crane: crane.system.json }").replace("part:winch/gearbox", "part:hoist/gearbox");
        let e = Lesson::parse(Path::new("l/lesson.md"), &two).unwrap_err();
        assert_eq!(e.line, 7);
        let e = Lesson::parse(Path::new("l/lesson.md"), &GOOD.replace("system: winch\nset", "system: crane\nset")).unwrap_err();
        assert!(e.message.contains("not named in the front matter"));
        assert!(Lesson::parse(Path::new("l/lesson.md"), "# no front matter").is_err());
    }

    #[test]
    fn sliders_snap_to_their_steps_and_range() {
        let s = Slider { parameter: "supply.voltage".into(), label: String::new(), min: 6., max: 14., step: Some(0.5), unit: "V".into() };
        assert_eq!(s.snap(9.3), 9.5);
        assert_eq!(s.snap(20.), 14.);
        assert_eq!(s.snap(-1.), 6.);
        let free = Slider { step: None, ..s };
        assert_eq!(free.snap(9.3), 9.3);
    }
}
