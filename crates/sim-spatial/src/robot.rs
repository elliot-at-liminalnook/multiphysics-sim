//! Robot mode (`--robot FILE`): a CAD-exported `.simrobot.json` opened
//! read-only. The shared `PhysicalModel` loader runs on a worker thread; each
//! link's collision geometry is drawn at the exported assembly pose until a
//! run starts. Run/Pause/Step/Reset drive the shared `PhysicalRobot` on the
//! run thread (`robot_run`); links follow its frames. Nothing is written.
//! A preset (`--robot-preset ID`, REST `robot_preset`) opens the same way:
//! its scene's `robot` goes through the same loader, and the run thread runs
//! the preset's shared EmbeddedEnvironment/EmbeddedSession (`robot_preset`).
//! A FILE is watched and reloaded on change or Reload (`robot_source`); a
//! reload replaces the model and starts a fresh run context.
use super::{ACCENT, INK, MUTED, PANEL};
use crate::builder::ui::UiFonts;
use bevy::{
    asset::RenderAssetUsages,
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    picking::mesh_picking::{MeshPickingCamera, MeshPickingSettings},
    prelude::*,
    camera::Viewport,
    render::{
        mesh::{Indices, PrimitiveTopology},
    },
    winit::{UpdateMode, WinitSettings},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_domain_robot::PhysicalModel;
use sim_domain_robot::cad_link::{self, CadLinkStatus};
use crate::robot_preset::{Preset, PresetRun, RecordedRun};
use crate::robot_motion::{self, KEYS};
use crate::robot_recording;
use crate::robot_source::{self, SourceWatch, Trigger as ReloadTrigger};
use crate::robot_stress::{self, StressOverlay};
use crate::robot_gait::{self, GaitAction, GaitSource};
use crate::robot_playback::{self, RecordedAction};
use crate::robot_run::{self, JOG_LABEL, JOG_SEMANTICS, JOG_STEP_M, JOG_STEP_RAD, MotionRequest, OverlayFlags, ReplayPhase, RunAction, RunController, SPEED_SCALES, SpeedRequest};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};

const LEFT: f32 = 280.0;
const RIGHT: f32 = 390.0;
const TOP: f32 = 64.0;
/// Height of the graph dock above the bottom edge, when shown.
const DOCK: f32 = 262.0;
/// Exported link frames: origin at the stored `com`, axes aligned with the
/// model frame (the solver's zero-angle pose, `articulated.rs` `com0`).
pub const POSE: &str = "exported assembly pose: link frames at the stored com, axes aligned with the model frame (Z up); no joint motion, not stepped";
/// Which values carry a measured/derived/estimated label, stated in the UI and REST.
pub const PROVENANCE_RULE: &str = "typed provenance labels are shown only where the file carries one: joint physics.drive_backlash.provenance and actuator profile parameters. Free text the file carries (link mass_sources and member_names, joint physics.source, motor notes) is shown verbatim as the file's text, never mapped to a label. Every other value has no per-value provenance in the export; see the source block's notes.";
/// Shown once a run frame (built or stepped) is displayed.
pub const SIMULATED_POSE: &str = "simulated pose from the run thread's latest frame (see run.poses); the exported assembly pose is t = 0 of each generation";
/// Shown while a recorded preset's capture poses the links.
pub const RECORDED_POSE: &str = "recorded pose: a frame of the preset's capture (recorded physics, played back, not simulated here), mapped to the scene's links by name; see preset.frame_count and run.poses";
/// Shown while a gait preview poses the links (robot_gait).
pub const GAIT_POSE: &str = "gait preview pose from the shared KinematicMirror at the sampled gait time (see gait_preview): kinematic preview (geometry only, suspended) — not a physics result; Stop shows the run's frame again";
const UNLABELLED: &str = "no per-value provenance in export (see Source → notes)";

/// One link's display triangles in its own frame (flat-shaded).
pub struct LinkGeometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
}
impl LinkGeometry {
    pub fn triangles(&self) -> usize {
        self.positions.len() / 3
    }
}
/// A robot file as loaded on the worker thread.
pub struct Loaded {
    pub model: PhysicalModel,
    /// Per link, `None` when it has no collision geometry to draw.
    pub geometry: Vec<Option<LinkGeometry>>,
    pub notes: FileNotes,
    /// The export's `source` block compared with the CAD file on disk.
    pub cad_link: CadLinkStatus,
}
/// Free text the file carries that `PhysicalModel` does not keep, read from
/// the same bytes. Shown verbatim; never mapped to a provenance label.
#[derive(Default)]
pub struct FileNotes {
    /// Per link: `{"member_names": …, "mass_sources": …}` as stored (null when absent).
    pub links: Vec<Value>,
    /// Per motor: its `notes` as stored (null when absent).
    pub motors: Vec<Value>,
    /// The `uncertainty` block as stored; the parsed struct fills absent fields with 0.
    pub uncertainty: Value,
}

/// The shared loader plus triangulation, file notes and the CAD link status
/// (which hashes the CAD file); called on the worker thread (and by
/// `--validate-only`). Errors name the path.
pub fn load(path: &Path) -> Result<Loaded, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    load_bytes(path, &bytes)
}

/// [`load`] on bytes already read from `path` (the reload worker hashes the
/// same bytes it parses, `robot_source`).
pub fn load_bytes(path: &Path, bytes: &[u8]) -> Result<Loaded, String> {
    let name = path.to_string_lossy();
    // PhysicalModel::load is read + parse; parse the same bytes for the notes.
    let text = std::str::from_utf8(bytes).map_err(|e| format!("{name}: {e}"))?;
    let model = PhysicalModel::parse(&text).map_err(|e| format!("{name}: {e}"))?;
    let raw: Value = serde_json::from_str(&text).map_err(|e| format!("{name}: {e}"))?;
    Ok(loaded(model, &raw, path))
}

/// The loader's second half, shared by a `.simrobot.json` file and a preset
/// scene's `robot` (`raw` is that robot's JSON as stored; `path` is the file
/// the CAD link resolves against).
pub fn loaded(model: PhysicalModel, raw: &Value, path: &Path) -> Loaded {
    let entry = |key: &str, i: usize, field: &str| raw[key].get(i).and_then(|v| v.get(field)).cloned().unwrap_or(Value::Null);
    let notes = FileNotes {
        links: (0..model.links.len()).map(|i| json!({"member_names": entry("links", i, "member_names"), "mass_sources": entry("links", i, "mass_sources")})).collect(),
        motors: (0..model.motors.len()).map(|i| entry("motors", i, "notes")).collect(),
        uncertainty: raw.get("uncertainty").cloned().unwrap_or(Value::Null),
    };
    let cad_link = cad_link::status(path, &model.source);
    let geometry = model
        .links
        .iter()
        .map(|l| {
            let mut g = LinkGeometry { positions: Vec::new(), normals: Vec::new() };
            for tri in l.collision.display_triangles() {
                let n = sim_domain_robot::model::triangle_normal(tri);
                for p in tri {
                    g.positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
                    g.normals.push([n[0] as f32, n[1] as f32, n[2] as f32]);
                }
            }
            (!g.positions.is_empty()).then_some(g)
        })
        .collect();
    Loaded { model, geometry, notes, cad_link }
}

/// A preset opened on the loader thread: its parsed inputs (`robot_preset`)
/// and its scene's `robot` through the same loader as a file.
pub fn load_preset(preset: Preset, root: &Path) -> Result<(Loaded, PresetRun), String> {
    let (run, robot) = PresetRun::load(preset, root)?;
    let loaded = loaded(run.scene.robot.clone(), &robot, &run.scene_path);
    Ok((loaded, run))
}
/// A recorded preset opened on the loader thread: its scene's `robot` through
/// the same loader, and the capture read and mapped by `RecordedRun::load`.
pub fn load_recorded(preset: Preset, root: &Path) -> Result<(Loaded, RecordedRun), String> {
    let (run, robot) = RecordedRun::load(preset, root)?;
    let loaded = loaded(run.scene.robot.clone(), &robot, &run.scene_path);
    Ok((loaded, run))
}

/// What the loader thread opened besides the model: nothing (`--robot FILE`), an embedded preset or a recorded one.
pub enum Opened {
    Preset(PresetRun),
    Recorded(RecordedRun),
}

enum Status {
    Loading(std::time::Instant),
    Loaded { seconds: f64 },
    Error(String),
}

#[derive(Resource)]
pub struct RobotView {
    pub path: PathBuf,
    status: Status,
    model: Option<PhysicalModel>,
    triangles: Vec<usize>,
    notes: FileNotes,
    cad_link: Option<CadLinkStatus>,
    /// The one link selection shared by the list, the 3D view and REST.
    pub selected: Option<usize>,
    section: Section,
    /// Inspector scroll offset and its maximum, in logical pixels (as laid out).
    scroll: f32,
    scroll_max: f32,
    scroll_to: Option<f32>,
    rx: Option<Mutex<mpsc::Receiver<Result<(Loaded, Option<Opened>), String>>>>,
    /// The preset being opened or run (None for `--robot FILE`).
    preset: Option<Preset>,
    /// The preset list and the root its paths resolve against.
    /// The preset list (explicit --robot-presets, else `<root>/web/viewer/presets.json`).
    presets: Result<PathBuf, String>,
    /// The launch's workspace root (`crate::workspace`); preset inputs and recordings resolve against it.
    root: Result<PathBuf, String>,
    /// The run thread, spawned idle once the model has loaded.
    run: Option<RunController>,
    /// The last refused run control from a click (REST gets the error directly).
    run_message: Option<String>,
    /// LinkMesh transforms need re-applying (new frame, or reset to the assembly pose).
    pose_dirty: bool,
    ui_revision: u64,
    panels_ready: bool,
    /// The graph dock (system_ui graphs:toggle, key G, the Graphs button).
    graphs_visible: bool,
    /// `--robot FILE`: the file's hash, watch and reload state (None for a preset).
    source: Option<SourceWatch>,
    /// The last reload's result line (reason, run reset, selection), shown in the header.
    notice: Option<String>,
    /// `--robot FILE`: the read-only `.simresult.json` stress overlay (`robot_stress`).
    stress: StressOverlay,
}
impl RobotView {
    /// Starts the worker load; the window opens without waiting for it.
    /// The first load goes through the same worker check as a reload (`robot_source`).
    pub fn open(path: PathBuf) -> Self {
        let mut view = Self::new(path.clone(), None, None);
        view.source = Some(SourceWatch::open(path));
        view
    }
    /// Opens preset `id` from `presets` (its paths resolved against the
    /// workspace root, `crate::workspace`): refused now, naming the id, when it
    /// is unknown, not embedded or missing inputs, or when no root was found;
    /// its files are parsed on a worker thread.
    pub fn open_preset(presets: &Path, id: &str) -> Result<Self, String> {
        let root = crate::workspace::root().map_err(|e| format!("robot preset `{id}` resolves its inputs against the workspace root: {e}"))?.to_path_buf();
        let preset = crate::robot_preset::select(presets, &root, id)?;
        let (tx, rx) = mpsc::channel();
        let (worker, dir) = (preset.clone(), root.clone());
        std::thread::spawn(move || {
            let opened = if worker.is_recorded() {
                load_recorded(worker, &dir).map(|(l, r)| (l, Some(Opened::Recorded(r))))
            } else {
                load_preset(worker, &dir).map(|(l, r)| (l, Some(Opened::Preset(r))))
            };
            let _ = tx.send(opened);
        });
        let mut view = Self::new(root.join(preset.scene.as_deref().unwrap_or_default()), Some(rx), Some(preset));
        view.presets = Ok(presets.to_path_buf());
        Ok(view)
    }
    /// The preset list REST robot_presets/robot_preset read (None: the
    /// default `<root>/web/viewer/presets.json`).
    pub fn with_presets(mut self, presets: Option<PathBuf>) -> Self {
        if let Some(p) = presets {
            self.presets = Ok(p);
        }
        self
    }
    fn new(path: PathBuf, rx: Option<mpsc::Receiver<Result<(Loaded, Option<Opened>), String>>>, preset: Option<Preset>) -> Self {
        Self {
            preset,
            presets: crate::robot_preset::default_file(),
            root: crate::workspace::root().map(Path::to_path_buf),
            path,
            status: Status::Loading(std::time::Instant::now()),
            model: None,
            triangles: Vec::new(),
            notes: FileNotes::default(),
            cad_link: None,
            selected: None,
            section: Section::Link,
            scroll: 0.0,
            scroll_max: 0.0,
            scroll_to: None,
            rx: rx.map(Mutex::new),
            run: None,
            run_message: None,
            pose_dirty: false,
            ui_revision: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_micros() as u64,
            panels_ready: false,
            graphs_visible: false,
            source: None,
            notice: None,
            stress: StressOverlay::default(),
        }
    }
    fn link_name(&self, i: usize) -> Option<&str> {
        self.model.as_ref()?.links.get(i).map(|l| l.name.as_str())
    }
    pub fn state_json(&self) -> Value {
        let (status, error, seconds) = match &self.status {
            Status::Loading(_) => ("loading", None, None),
            Status::Loaded { seconds } => ("loaded", None, Some(*seconds)),
            Status::Error(e) => ("error", Some(e.clone()), None),
        };
        let links: Vec<Value> = self
            .model
            .iter()
            .flat_map(|m| m.links.iter().enumerate())
            .map(|(i, l)| json!({"index": i, "name": l.name, "has_mesh": self.triangles.get(i).is_some_and(|t| *t > 0), "triangles": self.triangles.get(i).copied().unwrap_or(0)}))
            .collect();
        let selected = self.selected.and_then(|i| {
            let m = self.model.as_ref()?;
            let l = m.links.get(i)?;
            let material = m.materials.get(&l.material);
            let joints: Vec<usize> = touching(m, &l.name).map(|(j, _)| j).collect();
            Some(json!({"index": i, "name": l.name, "mass": l.mass, "com": l.com, "inertia": l.inertia, "material": l.material,
                "material_in_file": material.is_some(), "density": material.map(|x| x.density), "material_entry": material,
                "ground": l.ground, "members": l.members, "triangles": self.triangles.get(i).copied().unwrap_or(0), "joints": joints,
                "provenance": Value::Null, "file_notes": self.notes.links.get(i)}))
        });
        let m = self.model.as_ref();
        let joints: Vec<Value> = m.iter().flat_map(|m| m.joints.iter().enumerate()).map(|(i, j)| joint_json(i, j)).collect();
        let motors: Vec<Value> = m.iter().flat_map(|m| m.motors.iter().enumerate()).map(|(i, x)| json!({"motor": x, "provenance": Value::Null, "file_notes": {"notes": self.notes.motors.get(i)}})).collect();
        let profiles = m.and_then(|m| m.actuator_profiles.as_ref()).map(|p| {
            let hashes: serde_json::Map<String, Value> = p.families.iter().map(|(k, f)| (k.clone(), json!(f.content_hash()))).collect();
            json!({"content_hashes": hashes, "profiles": p, "provenance": "per parameter, typed in the file (measured | derived | estimated)"})
        });
        let names: Vec<String> = m.iter().flat_map(|m| m.links.iter().map(|l| l.name.clone())).collect();
        let run = self.run.as_ref().map(|r| r.state_json(&names));
        // The preset block: the parsed run once loaded, else the declared entry while loading.
        let recorded = self.run.as_ref().and_then(|r| r.recorded());
        let preset = match (self.run.as_ref().and_then(|r| r.preset()), &self.preset) {
            _ if recorded.is_some() => recorded.map(|r| r.state_json(names.len())),
            (Some(p), _) => Some(p.state_json(self.run.as_ref().and_then(|r| r.frame()).and_then(|f| f.completed_steps))),
            (None, Some(p)) => Some(json!({"id": p.id, "label": p.label, "mode": p.mode, "scene": p.scene, "config": p.config, "task": p.task,
                "readiness": p.readiness(), "evidence": p.evidence(), "loaded": false})),
            (None, None) => None,
        };
        let stepped = self.run.as_ref().and_then(|r| r.frame()).is_some();
        let previewing = self.run.as_ref().and_then(|r| r.gait_preview()).and_then(|g| g.poses()).is_some();
        let cad = self.cad_link.as_ref().map(|c| json!({"link": c, "rule": cad_link::RESOLUTION_RULE}));
        let jog = self.run.as_ref().filter(|r| r.preset().is_none() && r.recorded().is_none()).map(|r| {
            let m = r.model();
            let joints: Vec<Value> = m.joints.iter().filter(|j| j.kind != "fixed" && !j.is_loop()).map(|j| r.jog_json(&j.name)).collect();
            let selected: Vec<String> = jog_joints(self).into_iter().map(|(j, _)| j).collect();
            json!({"label": JOG_LABEL, "semantics": JOG_SEMANTICS, "control_mode": m.control.mode, "trajectory_keyframes": m.control.trajectory.len(),
                "step_rad": JOG_STEP_RAD, "step_m": JOG_STEP_M, "selected_link_joints": selected, "joints": joints, "last_apply_error": r.jog_error()})
        });
        let mut out = json!({"file": self.path, "workspace": crate::workspace::json(), "status": status, "error": error, "load_seconds": seconds,
            "link_count": m.map(|m| m.links.len()), "links": links, "selected": selected,
            "joints": joints, "motors": motors, "transmissions": m.map(|m| &m.transmissions), "battery": m.and_then(|m| m.battery.as_ref()),
            "actuator_profiles": profiles, "uncertainty": m.map(|_| &self.notes.uncertainty), "uncertainty_parsed": m.map(|m| &m.uncertainty), "identification": m.map(|m| &m.identification),
            "materials": m.map(|m| &m.materials), "source": m.map(|m| &m.source), "cad_link": cad,
            "source_file": self.source.as_ref().map_or_else(|| json!({"watching": false, "reason": "a preset is not watched (--robot FILE only)"}), |s| s.json(true)), "notice": self.notice,
            "provenance_rule": PROVENANCE_RULE, "unlabelled_values": UNLABELLED, "numbers": "JSON numbers as parsed by PhysicalModel (f64, shortest round-trip); SI units; a null in place of a number is non-finite",
            "section": self.section, "inspector_scroll": {"offset_px": self.scroll, "max_px": self.scroll_max},
            "pose": if recorded.is_some() { RECORDED_POSE } else if previewing { GAIT_POSE } else if stepped { SIMULATED_POSE } else { POSE }, "read_only": true, "stepped": stepped, "run": run, "jog": jog, "preset": preset, "motion": self.run.as_ref().map(|r| r.motion_json()), "recording": self.run.as_ref().filter(|r| r.preset().is_some()).map(|r| r.recording_json()),
            "recordings": self.run.as_ref().map(|r| r.recordings_json()), "replay": self.run.as_ref().map(|r| r.replay_json()), "gait_preview": self.run.as_ref().map(|r| r.gait_json()),
            "graphs": self.run.as_ref().map_or_else(|| json!({"visible": self.graphs_visible, "charts": []}), |r| r.graphs_json(self.selected, self.graphs_visible)), "ui_revision": self.ui_revision, "controls_ready": self.panels_ready});
        // Run-thread overlays (robot_overlay); null until loaded.
        out["overlays"] = self.run.as_ref().map_or(Value::Null, RunController::overlays_json);
        // The recorded timeline (robot_recorded); absent unless a recorded preset is loaded.
        if let Some(r) = self.run.as_ref().and_then(RunController::recorded_json) {
            out["recorded"] = r;
        }
        if let Some(o) = out["overlays"].as_object_mut() {
            let stress = match &self.source {
                Some(_) => self.stress.json(self.model.as_ref()),
                None => json!({"available": false, "reason": STRESS_PRESET}),
            };
            o.insert("stress".into(), stress);
        }
        out
    }
}

/// Joints whose parent or child is the named link.
fn touching<'a>(m: &'a PhysicalModel, link: &'a str) -> impl Iterator<Item = (usize, &'a sim_domain_robot::model::Joint)> + 'a {
    m.joints.iter().enumerate().filter(move |(_, j)| j.child == link || j.parent.as_deref() == Some(link))
}
fn joint_json(i: usize, j: &sim_domain_robot::model::Joint) -> Value {
    json!({"index": i, "name": j.name, "id": j.id, "type": j.kind, "parent": j.parent, "child": j.child, "origin": j.origin, "axis": j.axis,
        "limits": j.limits, "home": j.home, "motor": j.motor, "physics": j.physics, "fastened": j.fastened,
        "typed_provenance": {"drive_backlash": j.physics.drive_backlash.as_ref().map(|b| b.provenance)},
        "provenance": Value::Null, "file_notes": {"physics.source": j.physics.source}})
}

/// Inspector sections, switched by tab click or `system_ui`.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Link,
    Joints,
    Drives,
    Source,
}
impl Section {
    const ALL: [Section; 4] = [Section::Link, Section::Joints, Section::Drives, Section::Source];
    fn label(self) -> &'static str {
        match self {
            Section::Link => "Link",
            Section::Joints => "Joints",
            Section::Drives => "Drives",
            Section::Source => "Source",
        }
    }
}

/// The handlers behind a click and `system_ui` activation.
#[derive(Component, Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RobotAction {
    SelectLink { index: usize, name: String },
    ClearSelection,
    ShowSection { section: Section },
    /// Scroll the inspector by logical pixels (positive is down).
    ScrollInspector { delta: f32 },
    Fit,
    /// Run/Pause/Step/Reset on the run thread.
    Run { action: RunAction },
    /// Servo-target jog by a step from the current requested target (the +/− buttons, `system_ui` jog:*).
    Jog { joint: String, delta: f64 },
    /// Servo-target jog to an absolute target (REST `robot_jog`).
    JogTo { joint: String, target: f64 },
    /// A motion request through the preset's Rust controller (physical keys,
    /// the W/A/S/D/Stop buttons, `system_ui` motion:*, REST `robot_input`).
    Motion { request: MotionRequest },
    /// Save the preset run's shared recording (the Save recording button,
    /// `system_ui` recording:save, REST `robot_save_recording`).
    SaveRecording { path: Option<String>, note: Option<String> },
    /// Replay a saved recording through the shared prepare_replay (the inspector
    /// Replay buttons, `system_ui` replay:<file>, REST `robot_replay`).
    Replay { file: Option<String>, path: Option<String> },
    /// Cancel the replay between chunks (Cancel button, `system_ui` replay:cancel, REST `robot_replay {action: cancel}`).
    CancelReplay,
    /// List the preset's saved recordings again (off the UI thread).
    RefreshRecordings,
    /// Show or hide the graph dock (the Graphs button, key G, `system_ui` graphs:toggle).
    ToggleGraphs,
    /// The kinematic gait preview: open, play, pause, seek, speed, stop, list (REST `robot_gait`).
    Gait { action: GaitAction },
    /// Re-read `--robot FILE` on a worker (`robot_source`): the watch (a changed
    /// stat), the Reload button, `system_ui` robot:reload and REST `robot_reload`.
    Reload { trigger: ReloadTrigger },
    /// Set the overlays (`--robot FILE`); a None flag keeps its value. Keys C/J/F/H, the
    /// inspector overlay buttons and `system_ui` overlay:* send the flipped flag; REST `robot_overlay` sends values.
    /// contacts, joints and deflections are run-thread overlays; stress colours the link meshes from the results file.
    Overlay { contacts: Option<bool>, joints: Option<bool>, deflections: Option<bool>, stress: Option<bool> },
    /// The run speed scale (robot_run::PACING; pacing only): keys =/+ and −, the header
    /// −/×scale/+ buttons, `system_ui` run:speed_* and REST `robot_speed`.
    Speed { speed: SpeedRequest },
    /// The recorded-preset timeline (robot_playback): play, pause, seek, step, speed, start —
    /// the inspector Recorded buttons, `system_ui` recorded:* and REST `robot_recorded`.
    Recorded { action: RecordedAction },
}
/// The overlays: (system_ui id suffix, label, key). H (hotspots) for stress: S is the WASD jog key.
const OVERLAYS: [(&str, &str, KeyCode); 4] =
    [("contacts", "Contacts", KeyCode::KeyC), ("joints", "Joint frames", KeyCode::KeyJ), ("deflections", "Deflections", KeyCode::KeyF), ("stress", "Stress", KeyCode::KeyH)];
const STRESS_PRESET: &str = "the stress overlay is not available for presets: it reads the .simresult.json beside a --robot FILE model (sim_runtime::physical::results_path); a preset scene has no results file";
fn overlay_on(view: &RobotView, kind: &str) -> bool {
    let flags = view.run.as_ref().map_or_else(OverlayFlags::default, RunController::overlays);
    match kind {
        "contacts" => flags.contacts,
        "joints" => flags.joints,
        "stress" => view.stress.enabled,
        _ => flags.deflections,
    }
}
/// The action that flips one overlay from its current requested value.
fn overlay_toggle(view: &RobotView, kind: &str) -> RobotAction {
    let flip = Some(!overlay_on(view, kind));
    match kind {
        "contacts" => RobotAction::Overlay { contacts: flip, joints: None, deflections: None, stress: None },
        "joints" => RobotAction::Overlay { contacts: None, joints: flip, deflections: None, stress: None },
        "stress" => RobotAction::Overlay { contacts: None, joints: None, deflections: None, stress: flip },
        _ => RobotAction::Overlay { contacts: None, joints: None, deflections: flip, stress: None },
    }
}
/// Why the stress overlay cannot be set now.
fn check_stress(view: &RobotView) -> Result<(), String> {
    view.source.as_ref().ok_or(STRESS_PRESET)?;
    view.model.as_ref().ok_or("the robot has not loaded")?;
    Ok(())
}
/// The absolute target a jog action asks for (file validation happens in `check_jog`).
fn jog_target(run: &RunController, joint: &str, delta: f64) -> Result<f64, String> {
    if let Some(r) = run.recorded() {
        // Named before the joint lookup, as check_jog would.
        return Err(r.refusal(&format!("servo-target jog of `{joint}`")));
    }
    let servo = crate::robot_run::servo(run.model(), joint)?;
    Ok(run.requested_target(&servo) + delta)
}
/// Jog controls follow the one link selection: the non-fixed joints touching
/// the selected link, the same joints the Joints section lists. A joint links
/// two bodies, so selecting either one reaches it, and no second (joint)
/// selection state is needed. Joints without a servo target stay listed,
/// disabled with the reason.
fn jog_joints(view: &RobotView) -> Vec<(String, f64)> {
    if view.preset.is_some() {
        // A preset's joints are driven by its declared controller: no servo-target jog.
        return Vec::new();
    }
    let (Some(m), Some(i)) = (view.model.as_ref(), view.selected) else { return Vec::new() };
    let Some(l) = m.links.get(i) else { return Vec::new() };
    touching(m, &l.name).filter(|(_, j)| j.kind != "fixed" && !j.is_loop()).map(|(_, j)| (j.name.clone(), if j.kind == "prismatic" { JOG_STEP_M } else { JOG_STEP_RAD })).collect()
}
/// Why a control is unavailable now (`Ok` when enabled).
fn check(view: &RobotView, action: &RobotAction) -> Result<(), String> {
    match action {
        RobotAction::Run { action } => view.run.as_ref().ok_or("the robot has not loaded")?.check(*action),
        RobotAction::Jog { joint, delta } => {
            let run = view.run.as_ref().ok_or("the robot has not loaded")?;
            run.check_jog(joint, jog_target(run, joint, *delta)?).map(|_| ())
        }
        RobotAction::JogTo { joint, target } => view.run.as_ref().ok_or("the robot has not loaded")?.check_jog(joint, *target).map(|_| ()),
        RobotAction::Motion { request } => view.run.as_ref().ok_or("the robot has not loaded")?.check_motion_request(request),
        RobotAction::SaveRecording { .. } => view.run.as_ref().ok_or("the robot has not loaded")?.check_save().map(|_| ()),
        RobotAction::Replay { .. } => view.run.as_ref().ok_or("the robot has not loaded")?.check_replay().map(|_| ()),
        RobotAction::CancelReplay => view.run.as_ref().ok_or("the robot has not loaded")?.check_cancel(),
        RobotAction::Gait { action } => view.run.as_ref().ok_or("the robot has not loaded")?.check_gait(action),
        RobotAction::Speed { speed } => view.run.as_ref().ok_or("the robot has not loaded")?.check_speed(*speed).map(|_| ()),
        RobotAction::Recorded { action } => view.run.as_ref().ok_or("the robot has not loaded")?.check_recorded(action),
        RobotAction::Reload { .. } => view.source.as_ref().ok_or("a preset is not reloaded; reload is for --robot FILE")?.check_reload(),
        RobotAction::Overlay { contacts, joints, deflections, stress } => {
            if contacts.is_some() || joints.is_some() || deflections.is_some() {
                view.run.as_ref().ok_or("the robot has not loaded")?.check_overlays()?;
            }
            if stress.is_some() {
                check_stress(view)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
fn dispatch(view: &mut RobotView, orbit: &mut RobotOrbit, action: RobotAction) -> Result<(), String> {
    if let RobotAction::Motion { request } = action {
        // Validated (and a refusal recorded) inside the one motion handler.
        return view.run.as_mut().ok_or("the robot has not loaded")?.motion(request);
    }
    if let RobotAction::SaveRecording { path, note } = action {
        // Validated (and a refusal recorded) inside the one save handler.
        return view.run.as_mut().ok_or("the robot has not loaded")?.save_recording(path.as_deref(), note.as_deref()).map(|_| ());
    }
    check(view, &action)?;
    match action {
        RobotAction::Motion { .. } | RobotAction::SaveRecording { .. } => unreachable!("handled above"),
        RobotAction::Replay { file, path } => {
            view.run.as_mut().ok_or("the robot has not loaded")?.replay(file.as_deref(), path.as_deref())?;
        }
        RobotAction::CancelReplay => view.run.as_mut().ok_or("the robot has not loaded")?.cancel_replay()?,
        RobotAction::RefreshRecordings => view.run.as_mut().ok_or("the robot has not loaded")?.refresh_recordings(),
        RobotAction::Run { action } => {
            view.run.as_mut().ok_or("the robot has not loaded")?.act(action)?;
            if action == RunAction::Reset {
                // Old-generation frames are stale: show the assembly pose until the rebuild publishes t = 0.
                view.pose_dirty = true;
            }
        }
        RobotAction::Jog { joint, delta } => {
            let run = view.run.as_mut().ok_or("the robot has not loaded")?;
            let target = jog_target(run, &joint, delta)?;
            run.jog(&joint, target)?;
        }
        RobotAction::JogTo { joint, target } => view.run.as_mut().ok_or("the robot has not loaded")?.jog(&joint, target)?,
        RobotAction::SelectLink { index, .. } => {
            view.selected = Some(index);
            view.scroll_to = Some(0.0);
        }
        RobotAction::ClearSelection => view.selected = None,
        RobotAction::ShowSection { section } => {
            view.section = section;
            view.scroll_to = Some(0.0);
        }
        RobotAction::ScrollInspector { delta } => view.scroll_to = Some((view.scroll + delta).clamp(0.0, view.scroll_max)),
        RobotAction::Fit => orbit.home = true,
        RobotAction::ToggleGraphs => view.graphs_visible = !view.graphs_visible,
        RobotAction::Speed { speed } => view.run.as_mut().ok_or("the robot has not loaded")?.speed(speed)?,
        RobotAction::Recorded { action } => view.run.as_mut().ok_or("the robot has not loaded")?.recorded_act(action)?,
        // Checked above; applied in `receive` when the worker finishes.
        RobotAction::Reload { trigger } => view.source.as_mut().ok_or("a preset is not reloaded; reload is for --robot FILE")?.start(trigger)?,
        RobotAction::Overlay { contacts, joints, deflections, stress } => {
            if contacts.is_some() || joints.is_some() || deflections.is_some() {
                let run = view.run.as_mut().ok_or("the robot has not loaded")?;
                let f = run.overlays();
                run.set_overlays(OverlayFlags { contacts: contacts.unwrap_or(f.contacts), joints: joints.unwrap_or(f.joints), deflections: deflections.unwrap_or(f.deflections) })?;
            }
            if let Some(on) = stress {
                if on && !view.stress.enabled {
                    // Re-read the results file on a worker: one written since the model loaded is picked up.
                    let path = view.path.clone();
                    view.stress.refresh(&path);
                }
                view.stress.enabled = on;
                view.stress.revision += 1;
            }
        }
        RobotAction::Gait { action } => {
            let stop = action == GaitAction::Stop;
            view.run.as_mut().ok_or("the robot has not loaded")?.gait(action)?;
            if stop {
                // The live frame (or the assembly pose) shows again at once.
                view.pose_dirty = true;
            }
        }
    }
    Ok(())
}
fn controls(view: &RobotView) -> Vec<(String, String, RobotAction)> {
    let mut out: Vec<(String, String, RobotAction)> = view
        .model
        .iter()
        .flat_map(|m| m.links.iter().enumerate())
        .map(|(index, l)| (format!("link:{index}"), l.name.clone(), RobotAction::SelectLink { index, name: l.name.clone() }))
        .collect();
    if view.panels_ready {
        out.push(("clear_selection".into(), "Clear selection".into(), RobotAction::ClearSelection));
        for section in Section::ALL {
            out.push((format!("section:{}", section.label().to_lowercase()), section.label().into(), RobotAction::ShowSection { section }));
        }
        out.push(("inspector:scroll_down".into(), "Scroll inspector down".into(), RobotAction::ScrollInspector { delta: 400.0 }));
        out.push(("inspector:scroll_up".into(), "Scroll inspector up".into(), RobotAction::ScrollInspector { delta: -400.0 }));
        out.push(("fit".into(), "Fit".into(), RobotAction::Fit));
        if view.source.is_some() {
            out.push(("robot:reload".into(), "Reload file".into(), RobotAction::Reload { trigger: ReloadTrigger::Manual }));
        }
        out.push(("graphs:toggle".into(), (if view.graphs_visible { "Hide graphs (G)" } else { "Show graphs (G)" }).into(), RobotAction::ToggleGraphs));
        for (kind, name, key) in OVERLAYS {
            let key = format!("{key:?}").trim_start_matches("Key").to_string();
            out.push((format!("overlay:{kind}"), format!("{} {name} overlay ({key})", if overlay_on(view, kind) { "Hide" } else { "Show" }), overlay_toggle(view, kind)));
        }
        for action in RunAction::ALL {
            out.push((format!("run:{}", action.name()), action.label().into(), RobotAction::Run { action }));
        }
        out.push(("run:speed_down".into(), "Run speed down (−)".into(), RobotAction::Speed { speed: SpeedRequest::Down }));
        out.push(("run:speed_up".into(), "Run speed up (=/+)".into(), RobotAction::Speed { speed: SpeedRequest::Up }));
        for scale in SPEED_SCALES {
            out.push((format!("run:speed:{scale}"), format!("Run speed ×{scale}"), RobotAction::Speed { speed: SpeedRequest::Set { scale } }));
        }
        if view.run.as_ref().is_some_and(|r| r.playback().is_some()) {
            for (id, label, action) in recorded_controls() {
                out.push((id, label, RobotAction::Recorded { action }));
            }
        }
        for (joint, step) in jog_joints(view) {
            let unit = if step == JOG_STEP_M { "m" } else { "rad" };
            out.push((format!("jog:{joint}:-"), format!("Jog {joint} servo target −{step} {unit}"), RobotAction::Jog { joint: joint.clone(), delta: -step }));
            out.push((format!("jog:{joint}:+"), format!("Jog {joint} servo target +{step} {unit}"), RobotAction::Jog { joint, delta: step }));
        }
        if view.preset.is_some() {
            for (id, label, request) in motion_buttons() {
                out.push((id.into(), label.into(), RobotAction::Motion { request }));
            }
            out.push(("recording:save".into(), "Save recording".into(), RobotAction::SaveRecording { path: None, note: None }));
            if let Some(r) = view.run.as_ref() {
                for l in r.recordings() {
                    out.push((format!("replay:{}", l.file), format!("Replay {}", l.file), RobotAction::Replay { file: Some(l.file.clone()), path: None }));
                }
            }
            out.push(("replay:cancel".into(), "Cancel replay".into(), RobotAction::CancelReplay));
            out.push(("replay:refresh".into(), "Refresh recordings".into(), RobotAction::RefreshRecordings));
            for (id, label, action) in gait_controls(view) {
                out.push((id, label, RobotAction::Gait { action }));
            }
        }
    }
    out
}
/// The gait preview controls (system_ui id, label, action), all through `RobotAction::Gait`
/// as the inspector's Gait preview buttons and REST `robot_gait`: one open per offered
/// tracked report, then the transport. Seek steps are relative to the latest pose's gait
/// time, resolved when listed or clicked.
fn gait_controls(view: &RobotView) -> Vec<(String, String, GaitAction)> {
    let Some(g) = view.run.as_ref().and_then(|r| r.gait_preview()) else { return Vec::new() };
    let mut out: Vec<(String, String, GaitAction)> = g.reports().iter().map(|r| (format!("gait:open:{}", r.name), format!("Open gait {}", r.name), GaitAction::Open { source: GaitSource::Report(r.name.clone()) })).collect();
    out.push(("gait:play".into(), "Play gait preview".into(), GaitAction::Play));
    out.push(("gait:pause".into(), "Pause gait preview".into(), GaitAction::Pause));
    out.push(("gait:stop".into(), "Stop gait preview (live frame again)".into(), GaitAction::Stop));
    for (step, id, label) in GAIT_SEEK {
        out.push((format!("gait:seek:{id}"), label.into(), gait_seek(view, step)));
    }
    for scale in GAIT_SCALES {
        out.push((format!("gait:speed:{scale}"), format!("Gait speed ×{scale}"), GaitAction::Speed { scale }));
    }
    out.push(("gait:list".into(), "List gait reports again".into(), GaitAction::List));
    out
}
/// The recorded timeline controls (system_ui id, label, action), all through
/// `RobotAction::Recorded` as the inspector Recorded buttons and REST `robot_recorded`.
fn recorded_controls() -> Vec<(String, String, RecordedAction)> {
    let mut out: Vec<(String, String, RecordedAction)> = RECORDED_TRANSPORT.iter().map(|(id, label, a)| (format!("recorded:{id}"), label.to_string(), *a)).collect();
    for scale in SPEED_SCALES {
        out.push((format!("recorded:speed:{scale}"), format!("Recorded playback speed ×{scale}"), RecordedAction::Speed { scale }));
    }
    out
}
/// The transport buttons: (system_ui id suffix, label, action).
const RECORDED_TRANSPORT: [(&str, &str, RecordedAction); 5] = [
    ("start", "Start", RecordedAction::Start),
    ("step-", "Step −1", RecordedAction::Step { delta: -1 }),
    ("play", "Play", RecordedAction::Play),
    ("pause", "Pause", RecordedAction::Pause),
    ("step+", "Step +1", RecordedAction::Step { delta: 1 }),
];
/// Seek controls: (step in twelfths of the period, id suffix, label); step 0 seeks to t = 0.
const GAIT_SEEK: [(i8, &str, &str); 3] = [(0, "0", "Seek gait to t = 0"), (-1, "-", "Seek gait −period/12"), (1, "+", "Seek gait +period/12")];
/// Speed-scale buttons, all within the shared Clock's (0, 1].
const GAIT_SCALES: [f64; 3] = [0.25, 0.5, 1.0];
/// A seek by `step` twelfths of the period from the latest pose's gait time (wrapped into one period).
fn gait_seek(view: &RobotView, step: i8) -> GaitAction {
    let g = view.run.as_ref().and_then(|r| r.gait_preview());
    let (t, period) = g.and_then(|g| Some((g.sample().map_or(0.0, |s| s.gait_time_s), g.loaded()?.period_s))).unwrap_or((0.0, 0.0));
    let t = if step == 0 || period <= 0.0 { 0.0 } else { (t + f64::from(step) * period / 12.0).rem_euclid(period) };
    GaitAction::Seek { t }
}
/// The motion controls (system_ui id, label, request): each key latches its request; Stop zeros.
fn motion_buttons() -> [(&'static str, &'static str, MotionRequest); 5] {
    [
        ("motion:w", "W · Forward", MotionRequest::Key('w')),
        ("motion:a", "A · Left", MotionRequest::Key('a')),
        ("motion:s", "S · Back", MotionRequest::Key('s')),
        ("motion:d", "D · Right", MotionRequest::Key('d')),
        ("motion:stop", "Stop (X)", MotionRequest::Stop),
    ]
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum UiRequest {
    Controls,
    Activate { id: String, ui_revision: u64 },
}
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    RobotState,
    SystemUi { action: UiRequest },
    Camera { focus: [f32; 3], radius: f32, yaw: f32, pitch: f32 },
    Fit,
    RobotRun { action: String },
    RobotJog { joint: String, target: Option<f64>, delta: Option<f64> },
    RobotPresets,
    RobotPreset { id: String },
    RobotInput { channels: Option<std::collections::BTreeMap<String, f64>>, key: Option<String> },
    RobotSaveRecording { path: Option<String>, note: Option<String> },
    RobotReplay { file: Option<String>, path: Option<String>, action: Option<String> },
    RobotGait { action: Option<String>, report: Option<String>, path: Option<String>, t: Option<f64>, scale: Option<f64> },
    RobotReload,
    RobotOverlay { contacts: Option<bool>, joints: Option<bool>, deflections: Option<bool>, stress: Option<bool> },
    RobotSpeed { action: Option<String>, scale: Option<f64> },
    RobotRecorded { action: String, t: Option<f64>, scale: Option<f64>, delta: Option<i64> },
}
fn execute(view: &mut RobotView, orbit: &mut RobotOrbit, command: &sim_api::Command) -> sim_api::Result {
    match sim_api::decode::<Request>(command)? {
        Request::RobotState => {}
        Request::RobotPresets => {
            let (file, root) = (view.presets.clone()?, view.root.clone()?);
            let presets = crate::robot_preset::list(&file)?;
            let rows: Vec<Value> = presets.iter().map(|p| p.discovery(&root)).collect();
            return Ok(json!({"presets_file": file, "root": root, "workspace": crate::workspace::json(), "count": rows.len(), "presets": rows,
                "current": view.preset.as_ref().map(|p| &p.id)}));
        }
        Request::RobotPreset { id } => {
            // Refused here (naming the id) before anything is replaced; the old run thread stops when its controller drops.
            let mut next = RobotView::open_preset(&view.presets.clone()?, &id)?;
            next.ui_revision = view.ui_revision + 1;
            *view = next;
        }
        Request::SystemUi { action: UiRequest::Controls } => {
            let items: Vec<Value> = controls(view).into_iter().map(|(id, label, action)| json!({"id": id, "label": label, "enabled": check(view, &action).is_ok(), "disabled_reason": check(view, &action).err(), "action": action})).collect();
            return Ok(json!({"ui_revision": view.ui_revision, "ready": view.panels_ready, "controls": items, "state": view.state_json()}));
        }
        Request::SystemUi { action: UiRequest::Activate { id, ui_revision } } => {
            if !view.panels_ready || ui_revision != view.ui_revision {
                return Err("UI changed; request controls again before activating".into());
            }
            let (_, _, action) = controls(view).into_iter().find(|(i, _, _)| *i == id).ok_or("unknown control; request controls")?;
            dispatch(view, orbit, action)?;
        }
        Request::RobotRun { action } => {
            let action = RunAction::parse(&action)?;
            dispatch(view, orbit, RobotAction::Run { action })?;
        }
        Request::RobotJog { joint, target, delta } => {
            let action = match (target, delta) {
                (Some(target), None) => RobotAction::JogTo { joint, target },
                (None, Some(delta)) => RobotAction::Jog { joint, delta },
                _ => return Err("robot_jog needs exactly one of target (absolute, rad or m) or delta".into()),
            };
            dispatch(view, orbit, action)?;
        }
        Request::RobotInput { channels, key } => {
            let request = match (channels, key.as_deref()) {
                (Some(map), None) => MotionRequest::Channels(map),
                (None, Some("stop")) => MotionRequest::Stop,
                (None, Some(k)) => match k.chars().collect::<Vec<_>>()[..] {
                    [c] if KEYS.contains(&c) => MotionRequest::Key(c),
                    _ => return Err(format!("robot_input key `{k}` is not one of w, a, s, d, stop")),
                },
                _ => return Err("robot_input needs exactly one of channels ({\"<motion channel>\": value, …}) or key (w | a | s | d | stop)".into()),
            };
            dispatch(view, orbit, RobotAction::Motion { request })?;
        }
        Request::RobotSaveRecording { path, note } => dispatch(view, orbit, RobotAction::SaveRecording { path, note })?,
        Request::RobotReplay { file, path, action } => {
            let action = match (action.as_deref(), file.is_some() || path.is_some()) {
                (None | Some("start"), _) => RobotAction::Replay { file, path },
                (Some("cancel"), false) => RobotAction::CancelReplay,
                (Some("list"), false) => RobotAction::RefreshRecordings,
                (Some(a @ ("cancel" | "list")), true) => return Err(format!("robot_replay action `{a}` takes no file or path")),
                (Some(a), _) => return Err(format!("unknown robot_replay action `{a}`; valid actions: start (default, with file or path), cancel, list")),
            };
            dispatch(view, orbit, action)?;
        }
        Request::RobotGait { action, report, path, t, scale } => {
            let action = match (action.as_deref(), report, path, t, scale) {
                (None | Some("open"), Some(r), None, None, None) => GaitAction::Open { source: GaitSource::Report(r) },
                (None | Some("open"), None, Some(p), None, None) => GaitAction::Open { source: GaitSource::Path(p) },
                (Some("seek"), None, None, Some(t), None) => GaitAction::Seek { t },
                (Some("speed"), None, None, None, Some(scale)) => GaitAction::Speed { scale },
                (Some("play"), None, None, None, None) => GaitAction::Play,
                (Some("pause"), None, None, None, None) => GaitAction::Pause,
                (Some("stop"), None, None, None, None) => GaitAction::Stop,
                (Some("list"), None, None, None, None) => GaitAction::List,
                (a, ..) => return Err(format!("robot_gait {}: give exactly one of report or path (open, the default), or action seek with t, speed with scale, or play | pause | stop | list alone", a.map_or("open".into(), |a| format!("action `{a}`")))),
            };
            dispatch(view, orbit, RobotAction::Gait { action })?;
        }
        Request::Camera { focus, radius, yaw, pitch } => {
            if !focus.iter().chain([radius, yaw, pitch].iter()).all(|x| x.is_finite()) || radius <= 0. || pitch.abs() > 1.5 {
                return Err("finite camera required; radius > 0 and pitch within ±1.5 radians".into());
            }
            *orbit = RobotOrbit { focus: Vec3::from_array(focus), radius, yaw, pitch, home: false, ..*orbit };
        }
        Request::RobotReload => dispatch(view, orbit, RobotAction::Reload { trigger: ReloadTrigger::Manual })?,
        Request::RobotOverlay { contacts: None, joints: None, deflections: None, stress: None } => return Err("robot_overlay needs at least one of contacts, joints, deflections, stress (true | false)".into()),
        Request::RobotOverlay { contacts, joints, deflections, stress } => dispatch(view, orbit, RobotAction::Overlay { contacts, joints, deflections, stress })?,
        Request::RobotSpeed { action, scale } => {
            let speed = match (action.as_deref(), scale) {
                (None | Some("set"), Some(scale)) => SpeedRequest::Set { scale },
                (Some("up"), None) => SpeedRequest::Up,
                (Some("down"), None) => SpeedRequest::Down,
                (a, _) => return Err(format!("robot_speed {}: give scale (one of {}) or action up | down alone", a.map_or("without scale".into(), |a| format!("action `{a}`")), SPEED_SCALES.map(|s| s.to_string()).join(", "))),
            };
            dispatch(view, orbit, RobotAction::Speed { speed })?;
        }
        Request::RobotRecorded { action: name, t, scale, delta } => {
            let action = match (name.as_str(), t, scale, delta) {
                ("play", None, None, None) => RecordedAction::Play,
                ("pause", None, None, None) => RecordedAction::Pause,
                ("start", None, None, None) => RecordedAction::Start,
                ("seek", Some(t), None, None) => RecordedAction::Seek { t },
                ("step", None, None, Some(delta)) => RecordedAction::Step { delta },
                ("speed", None, Some(scale), None) => RecordedAction::Speed { scale },
                (a, ..) => return Err(format!("robot_recorded action `{a}`: give play | pause | start alone, seek with t (s), step with delta (+1 | -1) or speed with scale (one of {})", SPEED_SCALES.map(|s| s.to_string()).join(", "))),
            };
            dispatch(view, orbit, RobotAction::Recorded { action })?;
        }
        Request::Fit => orbit.home = true,
    }
    Ok(view.state_json())
}

/// The loopback REST server for robot mode.
pub fn server(port: u16) -> std::io::Result<sim_api::Server> {
    let server = sim_api::Server::bind(port, "robot", capabilities())?;
    server.describe("workspace", crate::workspace::json());
    Ok(server)
}
fn capabilities() -> Vec<Value> {
    use sim_api::capability as c;
    vec![
        c("robot_state", json!({}), "Read-only robot mode: file, workspace (the resolved root: root, found_by override | env | opened_file | cwd, from, error, rule; also in GET /v1/capabilities), status (loading | loaded | error, with the error naming the path), link_count, links, the selected link (mass, com, inertia, material and its density or material_in_file=false, file_notes), joints, motors, transmissions, battery, actuator_profiles (with content hashes), uncertainty, identification, the source block verbatim (the export's own `source`), source_file (--robot FILE's path, sha256, loaded_at, reload_count, watching, last_reload, run_reset; see robot_reload; watching=false for a preset), notice (the last reload's result), and cad_link (current | stale | missing | no_recorded_hash | no_source_file | unreadable, with the resolution rule and paths tried). Numbers are full-precision JSON; provenance is null unless the file carries a typed label (provenance_rule). Also the inspector section and scroll, run (null until loaded; phase idle with null time before any build; see robot_run), and overlays (--robot FILE run-thread contacts, joint frames and deflections with flags, counts, samples and scales, and overlays.stress for the read-only .simresult.json; available=false with the reason for presets; see robot_overlay). Nothing is written."),
        c("system_ui", json!({"action":{"operation":"controls"}}), "Discover the link list, inspector sections (section:link | joints | drives | source), inspector scrolling and view controls, the run controls (run:start | pause | step | reset, and run:speed_down | run:speed_up | run:speed:<scale>: the same RobotAction::Speed as robot_speed), robot:reload and overlay:contacts | overlay:joints | overlay:deflections | overlay:stress for --robot FILE (the same RobotAction::Reload as robot_reload and RobotAction::Overlay as robot_overlay; overlay controls are listed for presets but disabled with the reason), and for a preset the run, motion, recording, replay and gait preview controls (gait:open:<report>, gait:play, gait:pause, gait:stop, gait:seek:0 | - | + (±period/12 from the latest pose), gait:speed:0.25 | 0.5 | 1, gait:list: the same RobotAction::Gait as REST robot_gait), and for a recorded preset the timeline controls recorded:start | recorded:step- | recorded:play | recorded:pause | recorded:step+ | recorded:speed:<scale> (the same RobotAction::Recorded as REST robot_recorded) (controls) and activate one by id with the current ui_revision (activate), through the same handler as a click. Selection is shared by the list, the 3D view and robot_state."),
        c("robot_jog", json!({"joint":"left axle","target":0.5}), &format!("Servo-target jog of one joint by its file name: {JOG_LABEL}. Give target (absolute; rad, or m on a prismatic joint) or delta (from the current requested target). The same handler as the jog +/− buttons and system_ui jog:<joint>:+/- controls (±{JOG_STEP_RAD} rad, ±{JOG_STEP_M} m prismatic; listed for the joints touching the selected link). Errors name the joint: unknown joint, no servo target (passive joint, firmware none, fixed, or a trajectory-mode file), non-finite target, or a target outside the file's limits (with the limit; never clamped); after a failed run, Reset first. {JOG_SEMANTICS} robot_state.jog reports control mode, label and, per joint, the limit (or no limit in file), requested target, and the target and measured value from the latest accepted frame.")),
        c("robot_run", json!({"action":"start"}), "Run controls, the same handler as the Run/Pause/Step/Reset buttons and system_ui run:* controls. start runs the shared PhysicalRobot on the run thread (built from the loaded model with sim_runtime::registry() and BuildOptions::default()), paced at most to speed_scale × real time (robot_speed; default ×1); pause stops it; step advances exactly one 0.02 s chunk and is refused while running; reset rebuilds at t = 0 (assembly pose), bumps the generation and leaves it paused (allowed after a failure). An unknown action is an error naming it and listing the valid ones. robot_state.run reports phase (idle | building | running | paused | failed), time, steps (chunks), chunk_s, measured rtf, speed_scale (requested) and compute_limited, generation, error and the latest accepted frame. Nothing is written."),
        c("robot_speed", json!({"scale":4}), &format!("Set the run speed scale (× real time) for --robot FILE runs, preset runs and replays alike: scale one of {} (powers of two, as sim-app's cad scene; anything else is refused naming the allowed values, never clamped), or action up | down alone (refused at ×8 / ×0.125, naming the limit). The same RobotAction::Speed as keys =/+ and − (numpad too), the header −/×scale/+ buttons (×scale resets to ×1) and system_ui run:speed_up | run:speed_down | run:speed:<scale>. Allowed in every phase; it applies from the next chunk (or the next Run) and survives Reset and a file reload. Pacing only: dt, chunk_s and the physics step are unchanged. {} robot_state.run reports speed_scale (requested), rtf (achieved) and compute_limited: {}.", SPEED_SCALES.map(|s| s.to_string()).join(", "), robot_run::PACING, robot_run::COMPUTE_LIMITED_RULE)),
        c("robot_recorded", json!({"action":"seek","t":0.8}), &format!("Recorded-preset timeline ({}): action play | pause | start alone, seek with t (recorded time, s), step with delta (+1 | -1) or speed with scale. The same RobotAction::Recorded as the inspector Recorded buttons (Start, Step −1, Play, Pause, Step +1) and system_ui recorded:start | recorded:step- | recorded:play | recorded:pause | recorded:step+ | recorded:speed:<scale>. One playback worker (not the UI thread) owns the pre-mapped frames, runs the clock and publishes generation-stamped frames; the lookup is {}. {} {} {} {} speed accepts exactly one of {} (the run speed scales, never clamped) and is the same scale as robot_speed and the header −/×/+ buttons, which drive this playback for a recorded preset. Refused naming the reason: a non-recorded view (an embedded preset or --robot FILE), a non-finite or out-of-range t, a step at either end or not ±1, play while playing, pause while not playing. The response returns at once; robot_state.recorded reports label, preset, scene/capture paths, frame_count, first/last_time_s, duration_s, time_s, frame_index, frame_time_s, speed, phase (playing | paused | ended), generation, frame_generation and pending (a command not yet applied by the worker), description, readiness and evidence verbatim from presets.json, meta (the capture's source fidelity/cad_sha256/file/cad_revision, completed, error, simulated_s, stepping_wall_s, step_s, completed_steps, requested_steps as read; absent ones null and listed in absent), recorded_rate, unmatched_capture_links and the rules. Nothing is simulated.", crate::robot_preset::RECORDED_LABEL, sim_runtime::embedded_capture::LOOKUP_RULE, robot_playback::CLOCK_RULE, robot_playback::SEEK_RULE, robot_playback::STEP_RULE, robot_playback::START_RULE, SPEED_SCALES.map(|s| s.to_string()).join(", "))),
        c("robot_presets", json!({}), &format!("List the robot presets declared in {} (its paths resolved against the workspace root, reported in workspace): id, label, mode, scene/config/task paths, inputs_exist and missing, under_ignored_runs, capture, runs_as (EmbeddedEnvironment with a task, else EmbeddedSession; recorded playback for mode `recorded`) and openable with the reason when not (the build itself is not attempted): {}. Mode `embedded` runs natively; mode `recorded` plays back its capture (no physics).", crate::robot_preset::PRESETS, crate::robot_preset::OPENABLE_RULE)),
        c("robot_preset", json!({"id":"robot-measured-400hz"}), "Open a declared embedded or recorded preset by id. A recorded preset (mode `recorded`, a scene and a capture, e.g. robot-lift-5mm) reads its scene and its *-execution.json capture on the loader thread with the shared sim_runtime::embedded_capture reader, maps every frame to the scene's links by name and shows frame 0; robot_state.preset reports frame_count, duration_s, the capture's metadata as written (absent fields null), unmatched_capture_links and load_seconds; no physics is built, and run, jog, motion, save recording, replay, gait preview and overlays are refused naming the preset. An embedded preset opens in this window (replacing the current robot and stopping its run thread). Unknown ids, other modes and missing inputs are refused naming the id. The scene, config and task are parsed exactly as declared on a worker thread; scene.robot is drawn and inspected through the same loader as --robot FILE. Run/Pause/Step/Reset (robot_run) then drive the shared EmbeddedEnvironment (task) or EmbeddedSession (no task) on the run thread with seed 0 (recorded), one action interval or clamp(report_every, 1, 40) nominal steps per chunk. robot_state.preset reports id, label, paths, readiness and evidence verbatim, seed, step_s, chunk and step counts; run.phase `ended` with run.end reports a reached horizon or a terminated/truncated episode. Servo-target jog is not offered for presets."),
        c("robot_input", json!({"channels":{"command.forward_speed":0.001}}), &format!("Motion request for a running robot preset: {}. Give channels (values by motion channel name; the other motion channels keep their requested values) or key (w | a | s | d latches that key's request until stop, another key or a channel request; stop sets every motion channel to 0). The same handler as physical W/A/S/D (press/release) and X (stop), the W/A/S/D/Stop buttons and system_ui motion:w|a|s|d|stop. Motion channels come from the session's policy_contract.step_reference.config, else the preset's presets.json motion_commands (as web/viewer/motion-commands.mjs); key vectors from motion_key_vectors, else the channel bounds. Refused naming the channel and its bounds: an unknown channel, a session input that is not a motion channel, a non-finite value, or a value or summed key vector outside the channel's bounds ({}). Also refused, naming the reason: --robot FILE, a preset with no motion config, no built session (Run or Step builds it), a failed or ended run, an exhausted heartbeat. A declared motion_heartbeat is {} robot_state.motion reports source, channels with bounds, requested and held values, active keys, heartbeat, last refusal and the label.", robot_motion::LABEL, robot_motion::CLAMP_RULE, robot_motion::HEARTBEAT_RULE)),
        c("robot_save_recording", json!({"note":"after motion:w"}), &format!("Save the loaded preset run's recording: the same handler as the Save recording button and system_ui recording:save. The run thread snapshots the shared recording (EmbeddedEnvironment::episode_recording() for a preset with a task, EmbeddedSession::recording() without, as the browser's Download) in any phase with a built session, running, paused, ended or failed; a writer thread writes it, so the response returns at once with recording.pending set and robot_state.recording.last_saved {{path, meta_path, kind, version, completed_steps, replayable, not_replayable_reason, failure, saved_utc, bytes}} (or recording.error) once written. Optional path (relative to the root or absolute) and note (kept in the sidecar). {} {} {} Refused, naming the reason: --robot FILE, no built session (Run or Step first), a save still being written, a path under examples/, cad/ or web/, a name not ending in .json or ending in .meta.json, and an existing file (reported in recording.error).", robot_recording::LOCATION_RULE, robot_recording::FILE_RULE, robot_recording::REPLAYABLE_RULE)),
        c("robot_gait", json!({"report":"6216-Bayesian-009-472d11d4"}), &format!("Kinematic gait preview on the loaded preset: {}. Open a gait with report (a name in robot_state.gait_preview.reports: {}) or path (a compiled.json, relative to the workspace root or absolute); then {{\"action\":\"play\"}}, pause, stop, list, {{\"action\":\"seek\",\"t\":0.5}} (gait time, s) or {{\"action\":\"speed\",\"scale\":0.5}} (0 < scale <= 1). One worker reads the gait with sim_runtime::gait_playback::compiled_with_governor (governor from detailed.spec.json, else spec-identity.json, else none) and Gait::from_compiled, samples it ({}) and poses the scene with the shared KinematicMirror at lift {} m (web/viewer/calibration-mirror.mjs). Refused naming the reason: --robot FILE (no scene), a missing file (named), an unknown report, a gait joint that is not a coordinate of the preset's scene (named), a mirror that cannot serve the scene, a running physics run or a replay in progress; Run, Step and Replay are refused while a gait is loaded. Load errors after the command returns land in robot_state.gait_preview.error, with any previous preview kept. robot_state.gait_preview reports label, phase (idle | loading | playing | paused | failed), generation and frame_generation, report, compiled, governor_source, period_s, nominal_speed_m_s, report_speed_m_s, status and fidelity (the report's, verbatim), gait_time_s, speed_scale, desired_rad and commanded_rad by joint, drives, lift_m, authored_limit_violations and solve_ms. Nothing is simulated, written or sent to hardware.", robot_gait::LABEL, robot_gait::LISTING_RULE, robot_gait::SAMPLING_RULE, robot_gait::LIFT_M)),
        c("robot_replay", json!({"file":"20260930T060822.729Z.json"}), &format!("Replay a saved recording of the loaded preset: the same handler as the inspector Replay buttons and system_ui replay:<file>. Give file (a bare name listed in robot_state.recordings.files, in runs/robot-presets/<preset-id>/) or path (any readable recording .json, relative to the root or absolute; reading is not restricted). {{\"action\":\"cancel\"}} stops the replay between chunks (system_ui replay:cancel); {{\"action\":\"list\"}} lists the recordings again off the UI thread (system_ui replay:refresh). {} {} {} {} robot_state.replay reports path, phase (idle | replaying | cancelled | done | failed), completed/total with unit, completed_steps and recorded_completed_steps, verdict, error, measured, replaced and sidecar. Refused, naming the reason: --robot FILE, a replay already in progress, a running run (Pause first), a building session, a missing or non-.json file, a recording of the other kind, a runtime mismatch (the runtime's message) and a session identity mismatch (the viewer's labelled check).", robot_recording::REPLAY_RULE, robot_recording::VERDICT_RULE, robot_recording::IDENTITY_RULE, robot_recording::MEASURED_RULE)),
        c("robot_reload", json!({}), &format!("Re-read the opened --robot FILE now: the same RobotAction::Reload as the watch, the header Reload button and system_ui robot:reload. {} Returns at once; the result lands in robot_state.source_file {{path, sha256 (of the displayed model's bytes), loaded_at (UTC), reload_count (successful reloads), unchanged_checks, watching, in_flight, last_reload {{trigger watch | manual, outcome loaded | unchanged | failed, error naming the path, at (UTC)}}, run_reset, showing_last_good, failing_error}} and robot_state.notice. A loaded reload replaces the model, meshes, link list, notes and cad_link, keeps the selected link by name (else clears it with a note), discards any run or jog and spawns a fresh idle run thread whose generation is the old one + 1 (robot_state.run.generation; graphs clear by the generation rule). Refused naming the reason: a preset, a load or reload already in flight. (robot_state.source is still the file's own export source block.)", robot_source::RULE)),
        c("robot_overlay", json!({"contacts":true,"joints":true,"deflections":false,"stress":true}), &format!("Show or hide the --robot FILE overlays. stress (key H, system_ui overlay:stress; default off, as sim-app's cad scene) colours the link meshes per vertex from the model's read-only .simresult.json through sim_domain_robot::stress_results ({}); robot_state.overlays.stress reports enabled, painting, path, mtime_unix_s and mtime_utc, status (current | stale | no recorded hash | no results file | invalid results file), recorded_physical_hash, model_physical_hash, peak_stress_pa per link, hotspot_links, error, absent and paint_seconds. {} Run-thread overlays: contacts (spheres at PhysicalRobot::contacts points with force lines at {} m/N; red on the ground, orange against another link), joints (PhysicalRobot::joint_frames: a white sphere and each axis drawn ±{} m in yellow, cyan, magenta) and deflections (PhysicalRobot::deflections: flexible-link boundary displacement lines magnified ×{}). Give any subset; the others keep their values. The same RobotAction::Overlay as keys C / J / F / H, the inspector overlay buttons and system_ui overlay:contacts | overlay:joints | overlay:deflections | overlay:stress. Run-thread defaults: all on, as sim-app's cad scene draws them. {} Drawn in the model frame through RobotRoot's transform (the link meshes' parent), only from the latest accepted frame of the current generation. robot_state.overlays reports flags, frame_flags, frame_generation, frame_time, contacts {{count, sample: first {} of link, other (link name | ground), point, force, penetration}}, joints {{count, sample}}, deflections {{count, max_displacement_m}} and scales. Refused for presets (their session frames publish no contacts, joint frames or deflections, and a preset has no results file).", sim_domain_robot::stress_results::SCALE, robot_stress::RULE, robot_run::FORCE_SCALE_M_PER_N, robot_run::JOINT_AXIS_HALF_M, robot_run::DEFLECTION_MAGNIFICATION, robot_run::OVERLAY_COST_RULE, robot_run::OVERLAY_SAMPLE)),
        c("screenshot", json!({"path":"/tmp/view.png"}), "Save the window exactly as drawn to a PNG after the next frame"),
        c("camera", json!({"focus":[0,0,0],"radius":0.5,"yaw":0.7,"pitch":0.4}), "Absolute orbit in the display frame (Y up); SI metres and radians"),
        c("fit", json!({}), "Fit the robot"),
    ]
}

#[derive(Component, Clone, Copy)]
struct RobotOrbit {
    focus: Vec3,
    radius: f32,
    yaw: f32,
    pitch: f32,
    home: bool,
    extent: f32,
}
#[derive(Component)]
struct RobotRoot;
#[derive(Component)]
struct LinkMesh(usize);
#[derive(Component)]
struct LinkRow(usize);
#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct TitleText;
#[derive(Component)]
struct Inspector;
#[derive(Component)]
struct ListRoot;
#[derive(Component)]
struct InspectorScroll;
#[derive(Component)]
struct TabButton(Section);
#[derive(Component)]
struct RunButton(RunAction);
#[derive(Component)]
struct RunText;
#[derive(Component)]
struct JogRoot;
#[derive(Component)]
struct JogText(String);
#[derive(Component)]
struct JogButton;
#[derive(Component)]
struct MotionRoot;
#[derive(Component)]
struct MotionText;
#[derive(Component)]
struct MotionButton;
#[derive(Component)]
struct RecordingText;
/// The inspector's Recorded block (a recorded preset only; filled by `recorded_panel`).
#[derive(Component)]
struct RecordedRoot;
#[derive(Component)]
struct RecordedText;
#[derive(Component)]
struct RecordedButton;
/// The inspector's Gait preview block (a preset only; filled by `gait_panel`).
#[derive(Component)]
struct GaitRoot;
#[derive(Component)]
struct GaitText;
#[derive(Component)]
struct GaitError;
/// The tracked report buttons, rebuilt when the listing changes.
#[derive(Component)]
struct GaitList;
#[derive(Component)]
struct GaitButton;
/// A relative seek button: its action is re-resolved from the latest pose each frame.
#[derive(Component)]
struct GaitSeekButton(i8);
#[derive(Component)]
struct GraphDock;
/// The inspector's overlay block (`--robot FILE`): toggle buttons and the frame's counts.
#[derive(Component)]
struct OverlayRoot;
#[derive(Component)]
struct OverlayButton(&'static str);
#[derive(Component)]
struct OverlayLabel(&'static str);
#[derive(Component)]
struct OverlayText;
/// The stress overlay's label: results path, mtime, status, peaks and scale.
#[derive(Component)]
struct StressText;
#[derive(Component)]
struct ReloadButton;
#[derive(Component)]
struct SpeedButton;
/// A speed button's label; true on the middle (×scale) one.
#[derive(Component)]
struct SpeedLabel(bool);
#[derive(Component)]
struct ReplayText;
/// The Replay buttons (one per recent saved recording), rebuilt when the list changes.
#[derive(Component)]
struct ReplayList;
/// Replay buttons shown in the inspector (the rest are in system_ui and REST).
const REPLAY_BUTTONS: usize = 5;
#[derive(Resource)]
struct Materials {
    normal: Handle<StandardMaterial>,
    selected: Handle<StandardMaterial>,
    /// White bases for per-vertex stress colours (selection keeps its emissive tint).
    stress: Handle<StandardMaterial>,
    stress_selected: Handle<StandardMaterial>,
}
/// `Materials::normal`'s base colour: the vertex colour of a link without hotspot cells while stress is shown.
const LINK_COLOUR: Color = Color::srgb(0.62, 0.68, 0.76);

/// The window: worker load, posed link meshes, link list, inspector and REST.
pub fn run_robot(view: RobotView, api: sim_api::Server) {
    App::new()
        .insert_resource(crate::rest::Rest(api, None))
        .insert_resource(view)
        .insert_resource(ClearColor(Color::srgb(0.10, 0.125, 0.155)))
        .insert_resource(GlobalAmbientLight { color: Color::srgb(0.85, 0.90, 1.0), brightness: 420.0, affects_lightmapped_meshes: true })
        .insert_resource(MeshPickingSettings { require_markers: true, ..default() })
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::reactive(std::time::Duration::from_secs_f64(1.0 / 60.0)),
            unfocused_mode: UpdateMode::reactive_low_power(std::time::Duration::from_millis(40)),
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Systems — Robot (file read-only)".into(),
                resolution: (1500_u32, 940_u32).into(),
                resize_constraints: bevy::window::WindowResizeConstraints { min_width: 980.0, min_height: 720.0, ..default() },
                ..default()
            }),
            ..default()
        }))
        .insert_gizmo_config(OverlayGizmos, overlay_gizmo_config())
        .add_systems(Startup, ((crate::builder::ui::load_fonts, setup).chain(), crate::rest::wake_on_request))
        .add_systems(PostUpdate, crate::view::clamp_scroll_positions.after(bevy::ui::UiSystems::Layout))
        .init_resource::<crate::rest::Occlusion>()
        .add_systems(PreUpdate, crate::rest::track_occlusion)
        .add_systems(Update, ((watch, receive, poll_rest, motion_keys, graph_key, overlay_keys, speed_keys, buttons, stress_paint, apply_frames, scroll, orbit, viewport, highlight).chain(), (panels, speed_panel, overlay_panel, stress_panel, jog_panel, motion_panel, recorded_panel, gait_panel, graph_dock, draw).chain()).chain())
        .run();
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>, view: Res<RobotView>, fonts: Res<UiFonts>) {
    let text = |value: &str, size: f32, color: Color| label(&fonts, value, size, color);
    commands.insert_resource(Materials {
        normal: materials.add(StandardMaterial { base_color: LINK_COLOUR, perceptual_roughness: 0.7, metallic: 0.05, cull_mode: None, ..default() }),
        selected: materials.add(StandardMaterial { base_color: Color::srgb(0.98, 0.62, 0.22), emissive: LinearRgba::rgb(0.35, 0.16, 0.02), perceptual_roughness: 0.6, cull_mode: None, ..default() }),
        stress: materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.7, metallic: 0.05, cull_mode: None, ..default() }),
        stress_selected: materials.add(StandardMaterial { base_color: Color::WHITE, emissive: LinearRgba::rgb(0.35, 0.16, 0.02), perceptual_roughness: 0.6, cull_mode: None, ..default() }),
    });
    commands.spawn((
        Camera3d::default(),
        MeshPickingCamera,
        Tonemapping::None,
        Transform::from_xyz(0.5, 0.6, 0.8).looking_at(Vec3::ZERO, Vec3::Y),
        RobotOrbit { focus: Vec3::ZERO, radius: 1.0, yaw: 0.7, pitch: 0.45, home: false, extent: 0.3 },
    ));
    // UI over the whole window; the 3D camera only draws the middle viewport.
    commands.spawn((Camera2d, Camera { order: 3, clear_color: ClearColorConfig::None, ..default() }, IsDefaultUiCamera));
    commands.spawn((DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: false, ..default() }, Transform::from_xyz(1.0, 2.0, 1.5).looking_at(Vec3::ZERO, Vec3::Y)));
    // Z-up model frame shown in Bevy's Y-up frame (as sim-app does).
    commands.spawn((Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), Visibility::default(), RobotRoot));
    let file = view.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    commands.spawn((
        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: Val::Px(0.0), height: Val::Px(TOP), padding: UiRect::axes(Val::Px(18.0), Val::Px(8.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), ..default() },
        BackgroundColor(PANEL),
        children![(text(&format!("Robot — {file}  ·  file read-only"), 18.0, INK), TitleText), (text("Loading…", 13.0, MUTED), StatusText)],
    ));
    // Run controls: the same handler as system_ui run:* and REST robot_run.
    commands.spawn((
        Node { position_type: PositionType::Absolute, right: Val::Px(18.0), top: Val::Px(6.0), flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, row_gap: Val::Px(3.0), ..default() },
        children![
            (Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() }, children![reload_button(&fonts, view.source.is_some()), run_button(&fonts, RunAction::Start), run_button(&fonts, RunAction::Pause), run_button(&fonts, RunAction::Step), run_button(&fonts, RunAction::Reset), speed_button(&fonts, SpeedRequest::Down), speed_button(&fonts, SpeedRequest::Set { scale: 1.0 }), speed_button(&fonts, SpeedRequest::Up), graphs_button(&fonts)]),
            (text("", 12.0, MUTED), RunText),
        ],
    ));
    commands.spawn((
        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(TOP), bottom: Val::Px(0.0), width: Val::Px(LEFT), padding: UiRect::all(Val::Px(14.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), overflow: Overflow::clip_y(), ..default() },
        BackgroundColor(PANEL),
        ListRoot,
        children![text("Links", 15.0, INK)],
    ));
    commands.spawn((
        Node { position_type: PositionType::Absolute, right: Val::Px(0.0), top: Val::Px(TOP), bottom: Val::Px(0.0), width: Val::Px(RIGHT), padding: UiRect::all(Val::Px(16.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), ..default() },
        BackgroundColor(PANEL),
        children![
            (Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), flex_shrink: 0.0, ..default() }, children![tab(&fonts, Section::Link), tab(&fonts, Section::Joints), tab(&fonts, Section::Drives), tab(&fonts, Section::Source)]),
            // Run-thread overlay toggles and counts (`overlay_panel`).
            (
                Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() },
                OverlayRoot,
                children![
                    (
                        Node { flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(4.0), row_gap: Val::Px(3.0), ..default() },
                        children![overlay_button(&fonts, 0), overlay_button(&fonts, 1), overlay_button(&fonts, 2), overlay_button(&fonts, 3)]
                    ),
                    (text("", 11.5, MUTED), OverlayText),
                    (text("", 11.5, MUTED), StressText),
                ],
            ),
            // Motion request buttons for a preset (spawned by `motion_panel`).
            (Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() }, MotionRoot),
            // The recorded timeline for a recorded preset (spawned by `recorded_panel`).
            (Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() }, RecordedRoot),
            // Servo-target jog rows for the selected link's joints (rebuilt by `jog_panel`).
            (Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() }, JogRoot),
            (
                Node { flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, overflow: Overflow::scroll_y(), ..default() },
                ScrollPosition::default(),
                InspectorScroll,
                children![(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() }, GaitRoot), (text("Select a link in the list or the 3D view.", 12.5, INK), Inspector)],
            )
        ],
    ));
    // Graph dock under the 3D view (filled by `graph_dock`).
    commands.spawn((
        Node { position_type: PositionType::Absolute, left: Val::Px(LEFT), right: Val::Px(RIGHT), bottom: Val::Px(0.0), height: Val::Px(DOCK), padding: UiRect::all(Val::Px(10.0)), column_gap: Val::Px(10.0), display: Display::None, border: UiRect::top(Val::Px(1.0)), ..default() },
        BackgroundColor(PANEL),
        BorderColor::all(Color::srgb(0.2, 0.24, 0.29)),
        GraphDock,
    ));
}

fn tab(fonts: &UiFonts, section: Section) -> impl Bundle {
    (
        Button,
        RobotAction::ShowSection { section },
        TabButton(section),
        Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)), ..default() },
        BackgroundColor(Color::NONE),
        children![label(fonts, section.label(), 14.0, INK)],
    )
}

fn run_button(fonts: &UiFonts, action: RunAction) -> impl Bundle {
    (
        Button,
        RobotAction::Run { action },
        RunButton(action),
        Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(12.0), Val::Px(3.0)), ..default() },
        BackgroundColor(Color::srgb(0.16, 0.20, 0.25)),
        children![label(fonts, action.label(), 13.0, INK)],
    )
}

/// A run speed button: the same `RobotAction::Speed` as keys =/+ and −, `system_ui`
/// run:speed_* and REST robot_speed. The middle one shows the requested ×scale
/// (updated by `speed_panel`) and resets to ×1.
fn speed_button(fonts: &UiFonts, speed: SpeedRequest) -> impl Bundle {
    let name = match speed {
        SpeedRequest::Down => "−",
        SpeedRequest::Up => "+",
        SpeedRequest::Set { .. } => "×1",
    };
    (
        Button,
        RobotAction::Speed { speed },
        SpeedButton,
        Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)), ..default() },
        BackgroundColor(Color::srgb(0.16, 0.20, 0.25)),
        children![(label(fonts, name, 13.0, INK), SpeedLabel(matches!(speed, SpeedRequest::Set { .. })))],
    )
}

/// The Reload button (FILE mode): the same `RobotAction::Reload` as the watch,
/// `system_ui` robot:reload and REST robot_reload.
fn reload_button(fonts: &UiFonts, shown: bool) -> impl Bundle {
    (
        Button,
        RobotAction::Reload { trigger: ReloadTrigger::Manual },
        ReloadButton,
        Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(12.0), Val::Px(3.0)), margin: UiRect::right(Val::Px(8.0)), display: if shown { Display::Flex } else { Display::None }, ..default() },
        BackgroundColor(Color::srgb(0.16, 0.20, 0.25)),
        children![label(fonts, "Reload file", 13.0, INK)],
    )
}

/// An overlay toggle: the same `RobotAction::Overlay` as its key and `system_ui` overlay:*
/// (the flipped value is re-resolved each frame by `overlay_panel`).
fn overlay_button(fonts: &UiFonts, i: usize) -> impl Bundle {
    let (kind, name, _) = OVERLAYS[i];
    (
        Button,
        RobotAction::Overlay { contacts: None, joints: None, deflections: None, stress: None },
        OverlayButton(kind),
        Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)), ..default() },
        BackgroundColor(Color::srgb(0.16, 0.20, 0.25)),
        children![(label(fonts, name, 12.0, INK), OverlayLabel(kind))],
    )
}

/// The Graphs button: the same `RobotAction::ToggleGraphs` as key G and `system_ui` graphs:toggle.
fn graphs_button(fonts: &UiFonts) -> impl Bundle {
    (
        Button,
        RobotAction::ToggleGraphs,
        Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(12.0), Val::Px(3.0)), margin: UiRect::left(Val::Px(8.0)), ..default() },
        BackgroundColor(Color::srgb(0.16, 0.20, 0.25)),
        children![label(fonts, "Graphs (G)", 13.0, INK)],
    )
}

fn label(fonts: &UiFonts, value: &str, size: f32, color: Color) -> (Text, TextFont, TextColor, TextLayout) {
    (Text::new(value), TextFont { font: fonts.regular.clone().into(), font_size: FontSize::Px(size), ..default() }, TextColor(color), TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter))
}

/// UI thread, FILE mode: stats the opened file every `robot_source::POLL`;
/// a changed stat dispatches the one Reload handler (trigger watch).
fn watch(mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>) {
    let idle = view.rx.is_none();
    let due = match view.source.as_mut() {
        Some(s) if idle => s.poll(std::time::Instant::now()),
        _ => false,
    };
    if due {
        // A refusal (a check already in flight) is retried by the next poll.
        let _ = dispatch(&mut view, &mut orbit, RobotAction::Reload { trigger: ReloadTrigger::Watch });
    }
}

/// Takes the worker's result (a preset open, or a FILE open/reload from
/// `robot_source`); spawns meshes and the link list on a model to apply.
/// A failed or unchanged reload returns before anything is despawned.
fn receive(
    mut commands: Commands,
    old: Query<Entity, Or<(With<LinkMesh>, With<LinkRow>)>>,
    mut view: ResMut<RobotView>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<Materials>,
    root: Single<Entity, With<RobotRoot>>,
    list: Single<Entity, With<ListRoot>>,
    mut orbit: Single<&mut RobotOrbit>,
    mut redraw: MessageWriter<bevy::window::RequestRedraw>,
    fonts: Res<UiFonts>,
) {
    let started = match view.status {
        Status::Loading(t) => t,
        _ => std::time::Instant::now(),
    };
    // `reload`: the trigger and the worker's seconds when a FILE reload replaces a displayed model.
    let (result, reload) = if let Some(rx) = view.rx.as_ref() {
        let result = match rx.lock().unwrap_or_else(|p| p.into_inner()).try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => {
                redraw.write(bevy::window::RequestRedraw);
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => Err(format!("{}: the loader stopped without a result", view.path.display())),
        };
        view.rx = None;
        (result, None)
    } else {
        let Some(source) = view.source.as_mut() else { return };
        let Some((trigger, mut checked)) = source.take() else {
            if source.busy().is_some() {
                redraw.write(bevy::window::RequestRedraw);
            }
            return;
        };
        let seconds = checked.seconds;
        let results = checked.results.take();
        let was_failing = source.failing.is_some();
        let settled = source.settle(trigger, checked, robot_source::now_utc());
        if let Some(r) = results {
            // Read by the same worker; its status is always judged against the displayed model.
            view.stress.set(r);
        }
        let Some(source) = view.source.as_mut() else { return };
        let Some(loaded) = settled else {
            // Unchanged, or failed: the displayed model, meshes, run and selection stay.
            let failing = source.failing.clone();
            match (failing, view.model.is_some()) {
                (Some(e), false) => view.status = Status::Error(e),
                (Some(_), true) => view.notice = Some(format!("{} reload failed: showing the last good model (see header)", if trigger == ReloadTrigger::Watch { "watched" } else { "manual" })),
                (None, _) if trigger == ReloadTrigger::Manual => view.notice = Some("manual reload: file unchanged (same sha256); nothing replaced".into()),
                (None, _) if was_failing => view.notice = Some("the file on disk matches the displayed model again (same sha256); nothing replaced".into()),
                // A watch that finds identical bytes (a touch, an atomic same-content rewrite) stays quiet.
                (None, _) => {}
            }
            return;
        };
        // The first successful load (open, or a watch after a failed open) is not a reload.
        let reload = view.model.is_some().then_some((trigger, seconds));
        (Ok((*loaded, None)), reload)
    };
    // A reopened robot (REST robot_preset) or a reloaded file replaces the previous meshes and rows.
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let (loaded, preset) = match result {
        Ok(l) => l,
        Err(e) => {
            view.status = Status::Error(e);
            return;
        }
    };
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    let to_display = |p: Vec3| Vec3::new(p.x, p.z, -p.y);
    for (i, (link, geometry)) in loaded.model.links.iter().zip(&loaded.geometry).enumerate() {
        let com = Vec3::new(link.com[0] as f32, link.com[1] as f32, link.com[2] as f32);
        let Some(g) = geometry else { continue };
        for p in &g.positions {
            let w = to_display(com + Vec3::from_array(*p));
            lo = lo.min(w);
            hi = hi.max(w);
        }
        let count = g.positions.len() as u32;
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, g.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, g.normals.clone());
        mesh.insert_indices(Indices::U32((0..count).collect()));
        let entity = commands
            .spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(materials.normal.clone()), Transform::from_translation(com), Visibility::default(), LinkMesh(i), Pickable::default()))
            .observe(pick_link)
            .id();
        commands.entity(*root).add_child(entity);
    }
    if lo.x.is_finite() {
        orbit.extent = ((hi - lo).length() / 2.0).max(0.02);
        if reload.is_none() {
            orbit.focus = (lo + hi) / 2.0;
        }
    }
    // A reload keeps the user's camera.
    orbit.home = reload.is_none();
    view.triangles = loaded.geometry.iter().map(|g| g.as_ref().map_or(0, |g| g.triangles())).collect();
    // New meshes are painted (or not) for the stress overlay by `stress_paint`.
    view.stress.revision += 1;
    let rows: Vec<Entity> = loaded
        .model
        .links
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let name = if view.triangles[i] > 0 { l.name.clone() } else { format!("{}  (no collision geometry)", l.name) };
            commands
                .spawn((
                    Button,
                    RobotAction::SelectLink { index: i, name: l.name.clone() },
                    LinkRow(i),
                    Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)), flex_shrink: 0.0, ..default() },
                    BackgroundColor(Color::NONE),
                    children![label(&fonts, &name, 13.0, INK)],
                ))
                .id()
        })
        .collect();
    commands.entity(*list).add_children(&rows);
    // A file's previous run context is discarded (its thread stops) and the
    // fresh one continues its generation; a preset opens a new view.
    let previous = view.run.take();
    let (run, run_reset) = match preset {
        Some(Opened::Preset(run)) => (RunController::spawn_preset(std::sync::Arc::new(run)), false),
        Some(Opened::Recorded(run)) => (RunController::spawn_recorded(std::sync::Arc::new(run)), false),
        None => RunController::replace(previous, loaded.model.clone()),
    };
    let generation = run.generation();
    let kept = view.selected.and_then(|i| view.link_name(i)).map(str::to_string);
    view.run = Some(run);
    view.selected = kept.as_ref().and_then(|n| loaded.model.links.iter().position(|l| &l.name == n));
    view.model = Some(loaded.model);
    view.notes = loaded.notes;
    view.cad_link = Some(loaded.cad_link);
    view.pose_dirty = true;
    view.run_message = None;
    view.status = Status::Loaded { seconds: match reload {
        Some((_, s)) => s,
        None => started.elapsed().as_secs_f64(),
    } };
    if let Some((trigger, _)) = reload {
        let reason = if trigger == ReloadTrigger::Watch { "file changed on disk" } else { "manual reload" };
        let run = if run_reset { "run reset" } else { "no run to reset" };
        let selection = match (&kept, view.selected) {
            (Some(n), Some(_)) => format!("; selection kept: {n}"),
            (Some(n), None) => format!("; selection cleared: link `{n}` is not in the new file"),
            (None, _) => String::new(),
        };
        view.notice = Some(format!("reloaded: {reason}; {run}; generation {generation}{selection}"));
        if let Some(s) = view.source.as_mut() {
            s.run_reset = Some(run_reset);
        }
    }
    view.ui_revision += 1;
    view.panels_ready = true;
}

fn pick_link(click: On<Pointer<Click>>, links: Query<&LinkMesh>, mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>) {
    if click.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    if let Ok(link) = links.get(click.entity) {
        let name = view.link_name(link.0).unwrap_or_default().to_string();
        let _ = dispatch(&mut view, &mut orbit, RobotAction::SelectLink { index: link.0, name });
    }
}

fn buttons(clicks: Query<(&Interaction, &RobotAction), Changed<Interaction>>, mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>) {
    for (interaction, action) in &clicks {
        if *interaction == Interaction::Pressed {
            view.run_message = dispatch(&mut view, &mut orbit, action.clone()).err();
        }
    }
}

fn poll_rest(mut commands: Commands, mut redraw: MessageWriter<bevy::window::RequestRedraw>, mut rest: ResMut<crate::rest::Rest>, mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>, occlusion: Res<crate::rest::Occlusion>) {
    let server = &mut rest.0;
    let mut shots = Vec::new();
    server.poll(|command, _, _| {
        if command.command == "screenshot" {
            return sim_api::Outcome::Done(crate::rest::screenshot_path(&command.args, occlusion.0).map(|p| {
                shots.push(p.clone());
                json!({"path": p, "note": "saved once the next frame renders"})
            }));
        }
        sim_api::Outcome::Done(execute(&mut view, &mut orbit, command))
    });
    if server.snapshot_due() {
        server.publish("robot_state", view.state_json());
    }
    if server.busy() || !shots.is_empty() {
        redraw.write(bevy::window::RequestRedraw);
    }
    for path in shots {
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
}

/// Takes the run thread's latest frame (stale generations are discarded in
/// `RunController::poll`) and poses the link meshes from it; with no frame of
/// the current generation the static assembly pose is shown.
fn apply_frames(mut view: ResMut<RobotView>, mut links: Query<(&LinkMesh, &mut Transform)>, mut redraw: MessageWriter<bevy::window::RequestRedraw>) {
    let Some(run) = view.run.as_mut() else { return };
    let changed = run.poll();
    let active = run.active();
    if active {
        redraw.write(bevy::window::RequestRedraw);
    }
    if !changed && !view.pose_dirty {
        return;
    }
    view.pose_dirty = false;
    // A loaded gait preview's pose, else the run's latest accepted frame.
    let poses = view.run.as_ref().and_then(|r| r.display_poses());
    let Some(model) = view.model.as_ref() else { return };
    for (link, mut transform) in &mut links {
        let (p, q) = match poses.and_then(|f| f.get(link.0)).and_then(|p| p.as_ref()) {
            Some((p, q)) => (Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32), Quat::from_xyzw(q.x as f32, q.y as f32, q.z as f32, q.w as f32)),
            None => {
                let c = model.links[link.0].com;
                (Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32), Quat::IDENTITY)
            }
        };
        if transform.translation != p || transform.rotation != q {
            transform.translation = p;
            transform.rotation = q;
        }
    }
}

fn orbit(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    window: Single<&Window>,
    camera: Single<(&mut Transform, &mut RobotOrbit)>,
    view: Res<RobotView>,
) {
    let drag = motion.read().fold(Vec2::ZERO, |sum, e| sum + e.delta);
    let zoom = wheel.read().fold(0.0, |sum, e| sum + match e.unit {
        MouseScrollUnit::Line => e.y,
        MouseScrollUnit::Pixel => e.y * 0.02,
    });
    let (mut transform, mut orbit) = camera.into_inner();
    if orbit.home {
        orbit.radius = orbit.extent * 3.2;
        orbit.home = false;
    }
    let dock = if view.graphs_visible { DOCK } else { 0.0 };
    let in_scene = window.cursor_position().is_some_and(|p| p.x > LEFT && p.x < window.width() - RIGHT && p.y > TOP && p.y < window.height() - dock);
    if in_scene {
        let pan = buttons.pressed(MouseButton::Middle) || (buttons.pressed(MouseButton::Right) && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)));
        if pan {
            let shift = (transform.right() * -drag.x + transform.up() * drag.y) * orbit.radius * 0.0015;
            orbit.focus += shift;
        } else if buttons.pressed(MouseButton::Right) {
            orbit.yaw -= drag.x * 0.007;
            orbit.pitch = (orbit.pitch + drag.y * 0.007).clamp(-1.4, 1.4);
        }
        orbit.radius = (orbit.radius * (-zoom * 0.12).exp()).clamp(orbit.extent * 0.3, orbit.extent * 20.0);
    }
    let horizontal = orbit.pitch.cos() * orbit.radius;
    let eye = orbit.focus + Vec3::new(orbit.yaw.sin() * horizontal, orbit.pitch.sin() * orbit.radius, orbit.yaw.cos() * horizontal);
    let target = Transform::from_translation(eye).looking_at(orbit.focus, Vec3::Y);
    if *transform != target {
        *transform = target;
    }
}

fn viewport(window: Single<&Window>, view: Res<RobotView>, mut camera: Single<&mut Camera, With<RobotOrbit>>) {
    let scale = window.scale_factor();
    let width = (window.width() - LEFT - RIGHT).max(1.0);
    let height = (window.height() - TOP - if view.graphs_visible { DOCK } else { 0.0 }).max(1.0);
    let viewport = Viewport { physical_position: UVec2::new((LEFT * scale) as u32, (TOP * scale) as u32), physical_size: UVec2::new((width * scale) as u32, (height * scale) as u32), ..default() };
    if camera.viewport.as_ref().is_none_or(|old| old.physical_size != viewport.physical_size || old.physical_position != viewport.physical_position) {
        camera.viewport = Some(viewport);
    }
}

/// The one selection, shown in 3D and in the list.
fn highlight(
    view: Res<RobotView>,
    materials: Res<Materials>,
    mut meshes: Query<(&LinkMesh, &mut MeshMaterial3d<StandardMaterial>)>,
    mut rows: Query<(&LinkRow, &Interaction, &mut BackgroundColor), (Without<TabButton>, Without<RunButton>)>,
    mut tabs: Query<(&TabButton, &Interaction, &mut BackgroundColor), (Without<LinkRow>, Without<RunButton>)>,
    mut runs: Query<(&RunButton, &Interaction, &mut BackgroundColor), (Without<LinkRow>, Without<TabButton>)>,
) {
    for (button, interaction, mut background) in &mut runs {
        let enabled = view.run.as_ref().is_some_and(|r| r.check(button.0).is_ok());
        let color = match (enabled, interaction) {
            (false, _) => Color::srgba(0.16, 0.20, 0.25, 0.35),
            (true, Interaction::Hovered | Interaction::Pressed) => ACCENT.with_alpha(0.45),
            (true, _) => Color::srgb(0.16, 0.20, 0.25),
        };
        if background.0 != color {
            background.0 = color;
        }
    }
    for (tab, interaction, mut background) in &mut tabs {
        let color = if view.section == tab.0 {
            ACCENT.with_alpha(0.28)
        } else if *interaction == Interaction::Hovered {
            Color::srgb(0.13, 0.17, 0.21)
        } else {
            Color::NONE
        };
        if background.0 != color {
            background.0 = color;
        }
    }
    for (link, mut material) in &mut meshes {
        let want = match (view.selected == Some(link.0), view.stress.painting()) {
            (true, false) => &materials.selected,
            (false, false) => &materials.normal,
            (true, true) => &materials.stress_selected,
            (false, true) => &materials.stress,
        };
        if material.0 != *want {
            material.0 = want.clone();
        }
    }
    for (row, interaction, mut background) in &mut rows {
        let color = if view.selected == Some(row.0) {
            ACCENT.with_alpha(0.28)
        } else if *interaction == Interaction::Hovered {
            Color::srgb(0.13, 0.17, 0.21)
        } else {
            Color::NONE
        };
        if background.0 != color {
            background.0 = color;
        }
    }
}

/// Wheel over the inspector, or a requested offset (reset on selection and
/// section changes); reports the laid-out offset and its maximum back to REST.
fn scroll(mut view: ResMut<RobotView>, mut wheel: MessageReader<MouseWheel>, window: Single<&Window>, panel: Single<(&mut ScrollPosition, &ComputedNode), With<InspectorScroll>>) {
    let (mut position, node) = panel.into_inner();
    let delta = wheel.read().fold(0.0, |sum, e| sum + match e.unit {
        MouseScrollUnit::Line => e.y * 24.0,
        MouseScrollUnit::Pixel => e.y,
    });
    let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
    if delta != 0.0 && window.cursor_position().is_some_and(|p| p.x >= window.width() - RIGHT && p.y > TOP) {
        view.scroll_to = Some((position.y - delta).clamp(0.0, max));
    }
    if let Some(y) = view.scroll_to.take() {
        position.y = y.clamp(0.0, max);
    }
    if view.scroll != position.y || view.scroll_max != max {
        view.scroll = position.y;
        view.scroll_max = max;
    }
}

/// Status line and the sectioned inspector.
fn panels(
    view: Res<RobotView>,
    mut title: Single<&mut Text, (With<TitleText>, Without<StatusText>, Without<Inspector>, Without<RunText>)>,
    mut status: Single<&mut Text, (With<StatusText>, Without<Inspector>, Without<RunText>, Without<TitleText>)>,
    mut inspector: Single<&mut Text, (With<Inspector>, Without<StatusText>, Without<RunText>, Without<TitleText>)>,
    mut run_text: Single<&mut Text, (With<RunText>, Without<StatusText>, Without<Inspector>, Without<TitleText>)>,
    mut reload: Query<&mut Node, With<ReloadButton>>,
) {
    // A REST robot_preset replaces a FILE view: presets are not reloaded.
    let display = if view.source.is_some() { Display::Flex } else { Display::None };
    for mut node in &mut reload {
        if node.display != display {
            node.display = display;
        }
    }
    let heading = match &view.preset {
        Some(p) if p.is_recorded() => format!("Robot preset — {} ({})  ·  {}", p.label, p.id, crate::robot_preset::RECORDED_LABEL),
        Some(p) => format!("Robot preset — {} ({})  ·  files read-only", p.label, p.id),
        None => format!("Robot — {}  ·  file read-only", view.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
    };
    if title.0 != heading {
        title.0 = heading;
    }
    let run_line = match &view.run {
        None => String::new(),
        Some(r) if r.recorded().is_some() => {
            let refused = view.run_message.as_ref().map_or(String::new(), |m| format!(" · refused: {}", clip(m, 60)));
            r.playback().map_or(String::new(), |p| format!("{}{refused}", recorded_line(p)))
        }
        Some(r) => {
            let time = r.frame().map_or("t —".to_string(), |f| format!("t {:.2} s · {} chunks", f.time, f.steps));
            let rtf = r.rtf().map_or(String::new(), |x| format!(" · RTF {x:.2}"));
            // The requested scale while running or paused, and whether compute kept it from being reached.
            let speed = match r.phase() {
                robot_run::Phase::Running | robot_run::Phase::Paused => format!(" · ×{}{}", r.speed_scale(), if r.compute_limited() == Some(true) { " (compute-limited)" } else { "" }),
                _ => String::new(),
            };
            // The header has one line beside the subtitle: long messages are cut here and shown in full in the inspector.
            let error = r.error().map_or(String::new(), |e| format!(" · {} (full error in the inspector)", clip(e, 40)));
            let ended = r.end().and_then(|e| e["message"].as_str()).map_or(String::new(), |m| format!(" · {} (see the inspector)", clip(m, 40)));
            let error = format!("{error}{ended}");
            let refused = view.run_message.as_ref().map_or(String::new(), |m| format!(" · refused: {}", clip(m, 60)));
            let phase = serde_json::to_value(r.phase()).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let replay = r.replay_state();
            let replay = match replay.phase {
                ReplayPhase::Idle => String::new(),
                phase => format!(" · replay {} {}/{}", format!("{phase:?}").to_lowercase(), replay.completed, replay.total.map_or("?".into(), |t| t.to_string())),
            };
            format!("{phase} · {time}{rtf}{speed} · gen {} · {} s chunks{replay}{error}{refused}", r.generation(), r.chunk_s())
        }
    };
    if run_text.0 != run_line {
        run_text.0 = run_line;
    }
    let line = match (&view.status, &view.model) {
        (Status::Loading(t), _) => format!("Loading {} on a worker thread… {:.1} s", view.path.display(), t.elapsed().as_secs_f64()),
        (Status::Error(e), _) => format!("Could not open the robot: {e}"),
        (Status::Loaded { seconds }, Some(m)) => {
            let without = view.triangles.iter().filter(|t| **t == 0).count();
            let missing = if without > 0 { format!(" · {without} without collision geometry (listed, not drawn)") } else { String::new() };
            // Same pose-state rule as robot_state.pose (GAIT_POSE, SIMULATED_POSE, POSE), short form.
            let previewing = view.run.as_ref().and_then(|r| r.gait_preview()).and_then(|g| g.poses()).is_some();
            let pose = if view.run.as_ref().is_some_and(|r| r.recorded().is_some()) { "recorded pose (not simulated here)" } else if previewing { "kinematic gait preview pose (not physics)" } else if view.run.as_ref().and_then(|r| r.frame()).is_some() { "simulated pose" } else { "exported assembly pose" };
            match view.preset.as_ref() {
                Some(p) => format!("{} links{missing} · loaded in {seconds:.2} s · {pose} · readiness: {}", m.links.len(), clip(p.readiness().unwrap_or("(none declared)"), 55)),
                None => match view.source.as_ref().filter(|s| s.failing.is_some()) {
                    // The header says the displayed model is the last good one; the full error is in Source.
                    Some(s) => format!("SHOWING LAST GOOD MODEL (loaded {}) · reload failed: {} (full error in Source)", s.loaded_at.as_deref().unwrap_or("—"), clip(s.failing.as_deref().unwrap_or_default(), 70)),
                    None => {
                        let notice = view.notice.as_ref().map_or(String::new(), |n| format!("{} · ", clip(n, 90)));
                        format!("{notice}{} · {} links{missing} · loaded in {seconds:.2} s · {pose}", view.path.display(), m.links.len())
                    }
                },
            }
        }
        (Status::Loaded { .. }, None) => String::new(),
    };
    if status.0 != line {
        status.0 = line;
    }
    let body = match &view.model {
        None => String::new(),
        Some(m) => match view.section {
            Section::Link => link_text(&view, m),
            Section::Joints => joints_text(&view, m),
            Section::Drives => drives_text(&view, m),
            Section::Source => source_text(&view, m),
        },
    };
    let failure = view.run.as_ref().and_then(|r| r.error().map(|e| format!("RUN FAILED: {e}\n\n")));
    let ended = view.run.as_ref().and_then(|r| r.end()).map(|e| format!("RUN ENDED ({}): {}\n\n", e["kind"].as_str().unwrap_or(""), e["message"].as_str().unwrap_or("")));
    let failure = Some(format!("{}{}{}", failure.unwrap_or_default(), ended.unwrap_or_default(), preset_text(&view)));
    let refused = view.run_message.as_ref().map(|m| format!("Refused: {m}\n\n"));
    let body = format!("{}{}{body}", failure.unwrap_or_default(), refused.unwrap_or_default());
    if inspector.0 != body {
        inspector.0 = body;
    }
}

/// The preset's identity and readiness (verbatim) atop every inspector
/// section; its evidence text is added on the Source section.
fn preset_text(view: &RobotView) -> String {
    let Some(p) = &view.preset else { return String::new() };
    let mut t = format!("PRESET {} ({})\nreadiness (verbatim): {}\n", p.label, p.id, p.readiness().unwrap_or("(none declared)"));
    if let Some(rec) = view.run.as_ref().and_then(|r| r.recorded()) {
        let c = &rec.capture;
        let src = c.meta.source.as_ref();
        let opt = |x: Option<String>| x.unwrap_or_else(|| "(absent in file)".into());
        t += &format!("{}\n", crate::robot_preset::RECORDED_LABEL.to_uppercase());
        if let Some(f) = view.run.as_ref().and_then(|r| r.frame()) {
            t += &format!("frame {} of {} · t {} s · recorded {} s\n", f.steps, c.frames.len(), f.time, c.duration_s());
        }
        t += &format!("fidelity (file): {}\ncad_sha256 (file): {}\ncompleted (file): {} · simulated {} s in {} s stepping wall (rate {})\n",
            opt(src.and_then(|s| s.fidelity.clone())), opt(src.and_then(|s| s.cad_sha256.clone())), opt(c.meta.completed.map(|x| x.to_string())),
            opt(c.meta.simulated_s.map(|x| x.to_string())), opt(c.meta.stepping_wall_s.map(|x| format!("{x:.2}"))), opt(c.meta.recorded_rate().map(|x| format!("{x:.4}"))));
        if !rec.unmatched.is_empty() {
            t += &format!("capture links matching no scene link: {}\n", rec.unmatched.join(", "));
        }
        for (k, path) in p.paths() {
            t += &format!("{k}: {path}\n");
        }
        t += &format!("description (verbatim): {}\n", p.entry.get("description").and_then(|d| d.as_str()).unwrap_or("(none declared)"));
        t += &format!("evidence (verbatim): {}\n", p.evidence().unwrap_or("(none declared)"));
        t += "Unavailable for a recorded preset (nothing is simulated here): run, jog, motion, save recording, replay, gait preview and overlays; each is refused naming the preset.\n\n";
        return t;
    }
    if let Some(run) = view.run.as_ref().and_then(|r| r.preset()) {
        let f = view.run.as_ref().and_then(|r| r.frame());
        t += &format!("{} · seed {} · step {} s · chunk {} steps ({} s) · {} / {} steps\n",
            run.kind(), run.seed, run.config.step_s, run.chunk_steps(), run.chunk_s(), f.and_then(|f| f.completed_steps).map_or("—".into(), |n| n.to_string()), run.config.steps);
        if let Some(f) = f.filter(|f| !f.unmatched.is_empty()) {
            t += &format!("frame links matching no loaded link: {}\n", f.unmatched.join(", "));
        }
    }
    let motion = view.run.as_ref().map(motion_text).unwrap_or_default();
    for (k, path) in p.paths() {
        t += &format!("{k}: {path}\n");
    }
    if view.section == Section::Source {
        t += &format!("evidence (verbatim): {}\n", p.evidence().unwrap_or("(none declared)"));
    }
    t.push('\n');
    t.push_str(&motion);
    if let Some(r) = view.run.as_ref() {
        t += "RECORDING — the shared recording JSON, as the browser's Download, plus a .meta.json sidecar\n";
        if let Some(s) = r.saved() {
            t += &format!("last saved: {}\n  sidecar {}\n  {} v{} · {} steps · {}\n", s.path.display(), s.meta_path.display(), s.kind, s.version, s.completed_steps,
                s.not_replayable_reason.as_deref().map_or("replayable".to_string(), |why| format!("not replayable: {why}")));
        }
        if let Some(e) = r.save_error() {
            t += &format!("last save error: {e}\n");
        }
        t += &format!("{}\n\n", robot_recording::LOCATION_RULE);
        let s = r.replay_state();
        t += "REPLAY — the shared prepare_replay on the run thread; the verdict is the runtime's\n";
        t += &format!("{} saved recording(s) for this preset\n", r.recordings().len());
        if s.phase != ReplayPhase::Idle {
            t += &format!("{}\n", replay_line(r));
            if let Some(v) = &s.verdict {
                t += &format!("full verdict: {v}\n");
            }
            if let Some(e) = &s.error {
                t += &format!("full error: {e}\n");
            }
        }
        t += &format!("{}\n\n", robot_recording::VERDICT_RULE);
    }
    t
}

/// The recorded timeline's status line (header and inspector Recorded section).
fn recorded_line(p: &robot_playback::RecordedPlayback) -> String {
    let tl = p.timeline();
    let f = p.frame();
    let phase = json!(tl.phase);
    format!("recorded · {} · t {:.3} s · frame {} / {} (t {:.3} s) · ×{} · gen {}", phase.as_str().unwrap_or(""), tl.t, f.steps, tl.times.len(), f.time, tl.speed, f.generation)
}

/// `s` cut to at most `n` characters, marked with an ellipsis when cut.
fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

/// Jog rows for the joints touching the selected link: the joint's servo
/// state and −/+ buttons (the same `RobotAction::Jog` as `system_ui` jog:*).
fn jog_panel(
    mut commands: Commands,
    view: Res<RobotView>,
    fonts: Res<UiFonts>,
    root: Single<Entity, With<JogRoot>>,
    mut shown: Local<Option<Vec<String>>>,
    mut texts: Query<(&JogText, &mut Text)>,
    mut buttons: Query<(&RobotAction, &Interaction, &mut BackgroundColor), With<JogButton>>,
) {
    let joints = jog_joints(&view);
    let names: Vec<String> = joints.iter().map(|(j, _)| j.clone()).collect();
    if shown.as_ref() != Some(&names) {
        commands.entity(*root).despawn_related::<Children>();
        let header = if joints.is_empty() { String::new() } else { format!("Jog — {JOG_LABEL}") };
        let mut rows = vec![commands.spawn(label(&fonts, &header, 11.5, MUTED)).id()];
        for (joint, step) in &joints {
            let button = |sign: f64, text: &str| (Button, JogButton, RobotAction::Jog { joint: joint.clone(), delta: sign * step }, Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(9.0), Val::Px(1.0)), ..default() }, BackgroundColor(Color::srgb(0.16, 0.20, 0.25)), children![label(&fonts, text, 13.0, INK)]);
            rows.push(
                commands
                    .spawn((
                        Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), align_items: AlignItems::Center, ..default() },
                        children![button(-1.0, "−"), button(1.0, "+"), (label(&fonts, joint, 11.5, INK), JogText(joint.clone()))],
                    ))
                    .id(),
            );
        }
        commands.entity(*root).add_children(&rows);
        *shown = Some(names);
    }
    if let Some(r) = &view.run {
        for (joint, mut text) in &mut texts {
            let line = format!("{} · {}", joint.0, jog_line(r, &joint.0));
            if text.0 != line {
                text.0 = line;
            }
        }
    }
    for (action, interaction, mut background) in &mut buttons {
        let enabled = check(&view, action).is_ok();
        let color = match (enabled, interaction) {
            (false, _) => Color::srgba(0.16, 0.20, 0.25, 0.35),
            (true, Interaction::Hovered | Interaction::Pressed) => ACCENT.with_alpha(0.45),
            (true, _) => Color::srgb(0.16, 0.20, 0.25),
        };
        if background.0 != color {
            background.0 = color;
        }
    }
}

/// Physical W/A/S/D (press/release) and X (Stop) while a built preset has a
/// motion config: the same `RobotAction::Motion` handler as the buttons,
/// `system_ui` motion:* and REST `robot_input`. Robot mode's camera is
/// mouse-only, so these keys take no camera action. Bevy releases every key
/// when the window loses keyboard focus, which requests zero, as the browser's blur.
fn motion_keys(keys: Res<ButtonInput<KeyCode>>, mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>) {
    const MAP: [(KeyCode, char); 4] = [(KeyCode::KeyW, 'w'), (KeyCode::KeyA, 'a'), (KeyCode::KeyS, 's'), (KeyCode::KeyD, 'd')];
    let Some(run) = view.run.as_ref().filter(|r| r.motion_keys_active()) else { return };
    let request = if keys.just_pressed(KeyCode::KeyX) {
        Some(MotionRequest::Stop)
    } else if MAP.iter().any(|(c, _)| keys.just_pressed(*c) || keys.just_released(*c)) {
        let held: Vec<char> = MAP.iter().filter(|(c, _)| keys.pressed(*c)).map(|(_, k)| *k).collect();
        // A release with only a latched (system_ui/REST) key active leaves that key's request alone.
        let pressed = MAP.iter().any(|(c, _)| keys.just_pressed(*c));
        (pressed || run.keys_physical()).then_some(MotionRequest::HeldKeys(held))
    } else {
        None
    };
    if let Some(request) = request {
        view.run_message = dispatch(&mut view, &mut orbit, RobotAction::Motion { request }).err();
    }
}

/// W/A/S/D/Stop buttons and the requested values for a preset (the same
/// `RobotAction::Motion` as `system_ui` motion:*), enabled per the handler's check.
fn motion_panel(
    mut commands: Commands,
    view: Res<RobotView>,
    fonts: Res<UiFonts>,
    root: Single<Entity, With<MotionRoot>>,
    mut shown: Local<bool>,
    mut listed: Local<Option<Vec<String>>>,
    replay_list: Query<Entity, With<ReplayList>>,
    mut text: Query<(&mut Text, Has<RecordingText>, Has<ReplayText>), Or<(With<MotionText>, With<RecordingText>, With<ReplayText>)>>,
    mut buttons: Query<(&RobotAction, &Interaction, &mut BackgroundColor), With<MotionButton>>,
) {
    let button = |commands: &mut Commands, action: RobotAction, text: &str| commands.spawn((Button, MotionButton, action, Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)), ..default() }, BackgroundColor(Color::srgb(0.16, 0.20, 0.25)), children![label(&fonts, text, 12.0, INK)])).id();
    if view.preset.as_ref().is_some_and(|p| !p.is_recorded()) && !*shown {
        let header = commands.spawn(label(&fonts, &format!("Motion — {}", robot_motion::LABEL), 11.5, MUTED)).id();
        let row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() }).id();
        for (_, text, request) in motion_buttons() {
            let b = commands.spawn((Button, MotionButton, RobotAction::Motion { request }, Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)), ..default() }, BackgroundColor(Color::srgb(0.16, 0.20, 0.25)), children![label(&fonts, text, 12.0, INK)])).id();
            commands.entity(row).add_child(b);
        }
        let line = commands.spawn((label(&fonts, "", 11.5, INK), MotionText)).id();
        // Save recording: the same RobotAction::SaveRecording as system_ui recording:save and REST robot_save_recording.
        let save_row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).id();
        let save = commands.spawn((Button, MotionButton, RobotAction::SaveRecording { path: None, note: None }, Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)), ..default() }, BackgroundColor(Color::srgb(0.16, 0.20, 0.25)), children![label(&fonts, "Save recording", 12.0, INK)])).id();
        let saved = commands.spawn((label(&fonts, "", 11.5, INK), RecordingText, Node { flex_shrink: 1.0, ..default() })).id();
        commands.entity(save_row).add_children(&[save, saved]);
        // Replay: the same RobotAction::Replay / CancelReplay as system_ui replay:<file> / replay:cancel and REST robot_replay.
        let replay_header = commands.spawn(label(&fonts, "Replay — re-executed through the shared prepare_replay on the run thread", 11.5, MUTED)).id();
        let list = commands.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), ..default() }, ReplayList)).id();
        let replay_row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).id();
        let cancel = button(&mut commands, RobotAction::CancelReplay, "Cancel replay");
        let refresh = button(&mut commands, RobotAction::RefreshRecordings, "Refresh list");
        let replay_line = commands.spawn((label(&fonts, "", 11.5, INK), ReplayText, Node { flex_shrink: 1.0, ..default() })).id();
        commands.entity(replay_row).add_children(&[cancel, refresh]);
        commands.entity(*root).add_children(&[header, row, line, save_row, replay_header, list, replay_row, replay_line]);
        *shown = true;
    }
    if let (Some(r), Ok(list)) = (view.run.as_ref(), replay_list.single()) {
        // The most recent recordings first; rebuilt only when the listed files change.
        let files: Vec<String> = r.recordings().iter().rev().take(REPLAY_BUTTONS).map(|l| l.file.clone()).collect();
        if listed.as_ref() != Some(&files) {
            commands.entity(list).despawn_related::<Children>();
            let mut rows = Vec::new();
            for l in r.recordings().iter().rev().take(REPLAY_BUTTONS) {
                let summary = l.meta.as_ref().map_or("no sidecar".to_string(), |m| format!("{} steps{}{}", m["completed_steps"], if m["replayable"] == false { " · diagnostic" } else { "" }, m["note"].as_str().map_or(String::new(), |n| format!(" · {}", clip(n, 30)))));
                let row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).id();
                let b = button(&mut commands, RobotAction::Replay { file: Some(l.file.clone()), path: None }, "Replay");
                let t = commands.spawn(label(&fonts, &format!("{} · {summary}", l.file), 11.0, INK)).id();
                commands.entity(row).add_children(&[b, t]);
                rows.push(row);
            }
            let more = r.recordings().len().saturating_sub(REPLAY_BUTTONS);
            let note = if r.recordings().is_empty() { "no saved recordings for this preset yet".to_string() } else if more > 0 { format!("{more} older in robot_state.recordings (system_ui replay:<file>, REST robot_replay)") } else { String::new() };
            if !note.is_empty() {
                rows.push(commands.spawn(label(&fonts, &note, 11.0, MUTED)).id());
            }
            commands.entity(list).add_children(&rows);
            *listed = Some(files);
        }
    }
    if let Some(r) = view.run.as_ref() {
        for (mut t, recording, replay) in &mut text {
            let line = if replay { replay_line(r) } else if recording { recording_line(r) } else { motion_line(r) };
            if t.0 != line {
                t.0 = line;
            }
        }
    }
    for (action, interaction, mut background) in &mut buttons {
        let enabled = check(&view, action).is_ok();
        let color = match (enabled, interaction) {
            (false, _) => Color::srgba(0.16, 0.20, 0.25, 0.35),
            (true, Interaction::Hovered | Interaction::Pressed) => ACCENT.with_alpha(0.45),
            (true, _) => Color::srgb(0.16, 0.20, 0.25),
        };
        if background.0 != color {
            background.0 = color;
        }
    }
}

/// The inspector's Recorded block for a recorded preset: the label, the
/// transport (Start, Step −1, Play, Pause, Step +1) and the timeline line. Each
/// button is the same `RobotAction::Recorded` as `system_ui` recorded:* and REST
/// `robot_recorded`, enabled per the handler's check. Speed is the header's
/// −/×/+ (the same scale); seek to a time is REST/system_ui only (no slider idiom here).
fn recorded_panel(
    mut commands: Commands,
    view: Res<RobotView>,
    fonts: Res<UiFonts>,
    root: Single<Entity, With<RecordedRoot>>,
    mut shown: Local<bool>,
    mut text: Query<&mut Text, With<RecordedText>>,
    mut buttons: Query<(&RobotAction, &Interaction, &mut BackgroundColor), With<RecordedButton>>,
) {
    let Some(p) = view.run.as_ref().and_then(|r| r.playback()) else { return };
    if !*shown {
        let header = commands.spawn(label(&fonts, &format!("Recorded — {} · speed: header −/×/+ · seek: REST robot_recorded", crate::robot_preset::RECORDED_LABEL), 11.5, MUTED)).id();
        let row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), align_items: AlignItems::Center, flex_wrap: FlexWrap::Wrap, ..default() }).id();
        for (_, name, action) in RECORDED_TRANSPORT {
            let b = commands.spawn((Button, RecordedButton, RobotAction::Recorded { action }, Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)), ..default() }, BackgroundColor(Color::srgb(0.16, 0.20, 0.25)), children![label(&fonts, name, 12.0, INK)])).id();
            commands.entity(row).add_child(b);
        }
        let line = commands.spawn((label(&fonts, "", 11.5, INK), RecordedText)).id();
        commands.entity(*root).add_children(&[header, row, line]);
        *shown = true;
    }
    let want = recorded_line(p);
    for mut t in &mut text {
        if t.0 != want {
            t.0 = want.clone();
        }
    }
    for (action, interaction, mut background) in &mut buttons {
        let enabled = check(&view, action).is_ok();
        let color = match (enabled, interaction) {
            (false, _) => Color::srgba(0.16, 0.20, 0.25, 0.35),
            (true, Interaction::Hovered | Interaction::Pressed) => ACCENT.with_alpha(0.45),
            (true, _) => Color::srgb(0.16, 0.20, 0.25),
        };
        if background.0 != color {
            background.0 = color;
        }
    }
}

/// The inspector's Gait preview block for a preset: the tracked reports (name,
/// report speed, status verbatim), the transport, and the truthful labels. Every
/// button is the same `RobotAction::Gait` as `system_ui` gait:* and REST
/// `robot_gait`, enabled per the handler's check. `--robot FILE` has no scene, so
/// nothing is shown there. Robot mode has no text-field idiom, so an explicit
/// compiled.json path is REST-only (`robot_gait {path}`), as a replay path is.
fn gait_panel(
    mut commands: Commands,
    view: Res<RobotView>,
    fonts: Res<UiFonts>,
    root: Single<Entity, With<GaitRoot>>,
    mut shown: Local<bool>,
    mut listed: Local<Option<Vec<String>>>,
    list: Query<Entity, With<GaitList>>,
    mut text: Query<(&mut Text, Has<GaitError>), Or<(With<GaitText>, With<GaitError>)>>,
    mut buttons: Query<(&mut RobotAction, &Interaction, &mut BackgroundColor, Option<&GaitSeekButton>), With<GaitButton>>,
) {
    let Some(g) = view.run.as_ref().and_then(|r| r.gait_preview()) else { return };
    let button = |commands: &mut Commands, action: GaitAction, text: &str| commands.spawn((Button, GaitButton, RobotAction::Gait { action }, Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(7.0), Val::Px(2.0)), ..default() }, BackgroundColor(Color::srgb(0.16, 0.20, 0.25)), children![label(&fonts, text, 12.0, INK)])).id();
    if !*shown {
        let header = commands.spawn(label(&fonts, &format!("Gait preview — {}", robot_gait::LABEL), 11.5, MUTED)).id();
        let row = |commands: &mut Commands| commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), align_items: AlignItems::Center, flex_wrap: FlexWrap::Wrap, ..default() }).id();
        let transport = row(&mut commands);
        let play = button(&mut commands, GaitAction::Play, "Play");
        let pause = button(&mut commands, GaitAction::Pause, "Pause");
        let stop = button(&mut commands, GaitAction::Stop, "Stop");
        let mut seek = Vec::new();
        for (step, _, _) in GAIT_SEEK {
            let b = button(&mut commands, GaitAction::Seek { t: 0.0 }, match step { 0 => "t = 0", -1 => "−P/12", _ => "+P/12" });
            commands.entity(b).insert(GaitSeekButton(step));
            seek.push(b);
        }
        commands.entity(transport).add_children(&[play, pause, stop]).add_children(&seek);
        let speed = row(&mut commands);
        let speed_label = commands.spawn(label(&fonts, "speed ×", 11.5, MUTED)).id();
        commands.entity(speed).add_child(speed_label);
        for scale in GAIT_SCALES {
            let b = button(&mut commands, GaitAction::Speed { scale }, &format!("{scale}"));
            commands.entity(speed).add_child(b);
        }
        let status = commands.spawn((label(&fonts, "", 11.5, INK), GaitText)).id();
        let error = commands.spawn((label(&fonts, "", 11.5, crate::builder::ui::DANGER), GaitError)).id();
        let list_header = commands.spawn(label(&fonts, "Tracked gait reports (report speed · status, verbatim) — click to open. A compiled.json path: REST robot_gait {path} (no path field here).", 11.0, MUTED)).id();
        let reports = commands.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), ..default() }, GaitList)).id();
        commands.entity(*root).add_children(&[header, status, error, transport, speed, list_header, reports]);
        *shown = true;
    }
    if let Ok(list) = list.single() {
        // Rebuilt only when the offered reports change.
        let names: Vec<String> = g.reports().iter().map(|r| r.name.clone()).collect();
        if listed.as_ref() != Some(&names) {
            commands.entity(list).despawn_related::<Children>();
            let mut rows = Vec::new();
            for r in g.reports() {
                let row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).id();
                let b = button(&mut commands, GaitAction::Open { source: GaitSource::Report(r.name.clone()) }, &r.name);
                let speed = r.speed_m_s.map_or("speed —".to_string(), |v| format!("{v:.3} m/s"));
                let t = commands.spawn(label(&fonts, &format!("{speed} · {}", r.status), 11.0, INK)).id();
                commands.entity(row).add_children(&[b, t]);
                rows.push(row);
            }
            let note = match g.list_error() {
                Some(e) => format!("listing failed: {e}"),
                None if names.is_empty() => "no tracked gait report with an existing compiled gait (robot_state.gait_preview.reports_skipped says why)".into(),
                None => String::new(),
            };
            if !note.is_empty() {
                rows.push(commands.spawn(label(&fonts, &note, 11.0, MUTED)).id());
            }
            commands.entity(list).add_children(&rows);
            *listed = Some(names);
        }
    }
    let r = view.run.as_ref().expect("gait preview implies a run");
    for (mut t, error) in &mut text {
        let line = if error { g.error().map_or(String::new(), |e| format!("error: {e}")) } else { gait_line(r, g) };
        if t.0 != line {
            t.0 = line;
        }
    }
    for (mut action, interaction, mut background, step) in &mut buttons {
        if let Some(step) = step {
            let next = RobotAction::Gait { action: gait_seek(&view, step.0) };
            if *action != next {
                *action = next;
            }
        }
        let enabled = check(&view, &action).is_ok();
        let color = match (enabled, interaction) {
            (false, _) => Color::srgba(0.16, 0.20, 0.25, 0.35),
            (true, Interaction::Hovered | Interaction::Pressed) => ACCENT.with_alpha(0.45),
            (true, _) => Color::srgb(0.16, 0.20, 0.25),
        };
        if background.0 != color {
            background.0 = color;
        }
    }
}

/// The Gait preview status lines (rounded, so they only change with the pose).
fn gait_line(r: &RunController, g: &robot_gait::GaitPreview) -> String {
    let phase = json!(g.phase());
    let phase = phase.as_str().unwrap_or("");
    let blocked = r.check_gait(&GaitAction::Play).err().filter(|e| e.starts_with("a physics") || e.starts_with("a replay")).map_or(String::new(), |e| format!("\nunavailable: {e}"));
    let Some(l) = g.loaded() else {
        return format!("phase {phase} · no gait open · lift {} m (browser calibration-mirror){blocked}", robot_gait::LIFT_M);
    };
    let source = json!(l.governor_source);
    let (status, fidelity) = l.report.as_ref().map_or(("(opened by path: no report)", "(opened by path: no report)"), |x| (x.status.as_str(), x.fidelity.as_str()));
    let mut t = format!(
        "{} · governor {} · period {:.3} s\nphase {phase} · gait time {} · scale ×{}\nstatus (verbatim): {status}\nfidelity (verbatim): {fidelity}\nlift {} m (browser calibration-mirror)",
        l.report.as_ref().map_or_else(|| l.compiled.display().to_string(), |x| x.name.clone()),
        source.as_str().unwrap_or(""),
        l.period_s,
        g.sample().map_or("—".into(), |x| format!("{:.2} s", x.gait_time_s)),
        g.speed_scale(),
        robot_gait::LIFT_M,
    );
    match g.sample() {
        Some(x) => {
            t += &format!("\nauthored-limit violations: {}", if x.authored_limit_violations.is_empty() { "none".to_string() } else { x.authored_limit_violations.join(", ") });
            let q = if x.drives == "commanded" { &x.commanded } else { &x.desired };
            let joints: Vec<String> = l.joints.iter().zip(q).map(|(j, v)| format!("{j} {v:+.3}")).collect();
            t += &format!("\ndrives {} (rad): {}", x.drives, joints.join(" · "));
        }
        None => t += "\nauthored-limit violations: — (no pose yet)",
    }
    t + blocked.as_str()
}

/// One line under the motion buttons: the requested values, or why motion is unavailable.
fn motion_line(r: &RunController) -> String {
    let m = r.motion_json();
    if m["available"] != true {
        return format!("unavailable: {}", clip(m["unavailable_reason"].as_str().unwrap_or(""), 120));
    }
    let values: Vec<String> = m["channels"].as_array().into_iter().flatten().map(|c| {
        let short = c["name"].as_str().unwrap_or("").trim_start_matches("command.");
        format!("{short} {}", c["requested"].as_f64().or(c["held"].as_f64()).map_or("—".into(), |x| format!("{x}")))
    }).collect();
    let keys = m["active_keys"].as_array().filter(|k| !k.is_empty()).map_or("none".into(), |k| k.iter().filter_map(|k| k.as_str()).collect::<Vec<_>>().join("+"));
    let refused = m["last_refusal"].as_str().map_or(String::new(), |e| format!("\nrefused: {}", clip(e, 110)));
    format!("requested {} · keys {keys}{refused}", values.join(" · "))
}

/// The line beside Save recording: pending, the last pair written (path, steps, kind, replayable) or the last error.
fn recording_line(r: &RunController) -> String {
    let path = |p: &std::path::Path| p.strip_prefix(r.preset().map(|p| p.root.as_path()).unwrap_or(std::path::Path::new(""))).unwrap_or(p).display().to_string();
    if let Some(p) = r.save_pending() {
        return format!("saving {}…", path(p));
    }
    let mut t = match r.saved() {
        Some(s) => format!("saved {} · {} steps · {}{}", path(&s.path), s.completed_steps, s.kind, if s.replayable { String::new() } else { " · diagnostic (not replayable)".into() }),
        None => "no recording saved yet".into(),
    };
    if let Some(e) = r.save_error() {
        t += &format!("\nsave refused/failed: {}", clip(e, 140));
    }
    t
}

/// The line under the replay controls: phase, progress, verdict and error, and the measured difference labelled as such.
fn replay_line(r: &RunController) -> String {
    let s = r.replay_state();
    let mut t = match s.phase {
        ReplayPhase::Idle => "no replay".to_string(),
        phase => format!("{} {} · {}", format!("{phase:?}").to_lowercase(), s.path.as_ref().and_then(|p| p.file_name()).map_or(String::new(), |n| n.to_string_lossy().into_owned()), format_args!("{}/{} {}", s.completed, s.total.map_or("?".into(), |n| n.to_string()), s.unit.unwrap_or(""))),
    };
    if let Some(v) = &s.verdict {
        t += &format!("\nverdict: {}", clip(v, 150));
    }
    if let Some(e) = &s.error {
        t += &format!("\nerror: {}", clip(e, 150));
    }
    if let Some(m) = &s.measured {
        t += &format!("\nmeasured difference, not a pass criterion: max |Δp| {:.3e} m ({}); {} {:.3e} m", m["max_position_diff_m"].as_f64().unwrap_or(f64::NAN), m["max_link"].as_str().unwrap_or(""), m["first_link"].as_str().unwrap_or(""), m["first_link_position_diff_m"].as_f64().unwrap_or(f64::NAN));
    }
    t
}

/// The motion block of the inspector: source, channels with bounds, requested
/// and held values, keys, heartbeat and the last refusal (robot_state.motion).
fn motion_text(r: &RunController) -> String {
    let m = r.motion_json();
    let mut t = format!("MOTION — {}\n", robot_motion::LABEL);
    if m["available"] != true {
        t += &format!("unavailable: {}\n", m["unavailable_reason"].as_str().unwrap_or(""));
    }
    if let Some(source) = m["source"].as_str() {
        t += &format!("channels from: {source}\n");
    }
    for c in m["channels"].as_array().into_iter().flatten() {
        let v = |k: &str| c[k].as_f64().map_or("—".into(), |x| format!("{x}"));
        t += &format!("• {} [{}, {}] {} — requested {} · held {}\n", c["name"].as_str().unwrap_or(""), v("lower"), v("upper"), c["unit"].as_str().unwrap_or(""), v("requested"), v("held"));
    }
    if let Some(rule) = m["config"]["key_rule"].as_str() {
        let keys = m["active_keys"].as_array().filter(|k| !k.is_empty()).map_or("none".into(), |k| k.iter().filter_map(|k| k.as_str()).collect::<Vec<_>>().join("+"));
        t += &format!("keys: {keys}{} · {rule}\n", if m["keys_physical"] == true { " (held)" } else if keys != "none" { " (latched)" } else { "" });
    }
    if let Some(h) = m["heartbeat"].as_object() {
        t += &format!("heartbeat {}: {} of {} (one per action packet)\n", h["channel"].as_str().unwrap_or(""), h["value"].as_f64().map_or("—".into(), |x| format!("{x}")), h["upper"]);
    }
    for (k, name) in [("last_refusal", "last refusal"), ("last_apply_error", "not applied")] {
        if let Some(e) = m[k].as_str() {
            t += &format!("{name}: {e}\n");
        }
    }
    t.push_str(&format!("{}\n{}\n\n", robot_motion::KEY_SEMANTICS, robot_motion::CLAMP_RULE));
    t
}

/// A typed provenance label spelled as the file stores it.
fn provenance_label(p: &impl Serialize) -> String {
    serde_json::to_value(p).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}
fn v3(v: &[f64; 3]) -> String {
    format!("[{:?}, {:?}, {:?}]", v[0], v[1], v[2])
}
/// A stored JSON note as text: strings verbatim, anything else as JSON.
fn verbatim(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_string)
}
fn or_none(s: &str) -> &str {
    if s.is_empty() { "(empty in file)" } else { s }
}

/// Selected link: stored mass properties, material and the file's own text.
fn link_text(view: &RobotView, m: &PhysicalModel) -> String {
    let Some((i, l)) = view.selected.and_then(|i| Some((i, m.links.get(i)?))) else {
        return format!("Select a link in the list or the 3D view.\n\n{} links · {} joints · {} motors\n\nValues are shown exactly as stored (SI units, full precision).", m.links.len(), m.joints.len(), m.motors.len());
    };
    let mut t = format!("{}   (link {} of {})\n\n", l.name, i + 1, m.links.len());
    t += &format!("mass: {:?} kg\ncom: {} m (model frame, Z up)\ninertia about com (kg·m², model axes):\n", l.mass, v3(&l.com));
    for row in &l.inertia {
        t += &format!("  {}\n", v3(row));
    }
    t += &match (l.material.as_str(), m.materials.get(&l.material)) {
        ("", _) => "material: none recorded\n".to_string(),
        (name, Some(mat)) => format!("material: {name} — density {:?} kg/m³ (file's materials map)\n", mat.density),
        (name, None) => format!("material: {name} — not in the file's materials map; no density shown\n"),
    };
    t += &format!("ground: {}\n", if l.ground { "yes" } else { "no" });
    t += &match view.triangles.get(i).copied().unwrap_or(0) {
        0 => "collision: no geometry (not drawn)\n".to_string(),
        n => format!("collision: {n} display triangles\n"),
    };
    t += &format!("\nprovenance: {UNLABELLED}\n\nFILE'S OWN TEXT (verbatim; not a provenance label)\n");
    let notes = view.notes.links.get(i).cloned().unwrap_or(Value::Null);
    let names = notes.get("member_names").and_then(|n| n.as_array());
    let sources = notes.get("mass_sources").and_then(|s| s.as_object());
    if l.members.is_empty() && sources.is_none_or(|s| s.is_empty()) {
        t += "no members or mass_sources in file\n";
    }
    for (k, id) in l.members.iter().enumerate() {
        let name = names.and_then(|n| n.get(k)).map_or("(no member_name)".to_string(), verbatim);
        let source = sources.and_then(|s| s.get(id)).map_or("(no mass_source)".to_string(), |s| format!("\"{}\"", verbatim(s)));
        t += &format!("• {name} [{id}]\n   mass_source: {source}\n");
    }
    for (id, source) in sources.into_iter().flatten().filter(|(id, _)| !l.members.contains(id)) {
        t += &format!("• [{id}] (not a listed member)\n   mass_source: \"{}\"\n", verbatim(source));
    }
    let joints = touching(m, &l.name).count();
    t += &format!("\n{joints} joint(s) touch this link — see Joints.");
    t
}

/// Joints touching the selected link (all joints when none is selected).
fn joints_text(view: &RobotView, m: &PhysicalModel) -> String {
    let selected = view.selected.and_then(|i| m.links.get(i));
    let joints: Vec<_> = match selected {
        Some(l) => touching(m, &l.name).collect(),
        None => m.joints.iter().enumerate().collect(),
    };
    let mut t = match selected {
        Some(l) => format!("Joints touching {} ({} of {})\n", l.name, joints.len(), m.joints.len()),
        None => format!("All {} joints (select a link to filter)\n", m.joints.len()),
    };
    t += &format!("SI units, as stored. Values without a label: {UNLABELLED}.\n");
    if let Some(r) = &view.run {
        t += &format!("\nJOG — {JOG_LABEL}\ncontrol mode (file): {}\n{}\n", m.control.mode, JOG_NOTE);
        if let Some(e) = r.jog_error() {
            t += &format!("last jog not applied: {e}\n");
        }
    }
    for (_, j) in joints {
        let p = &j.physics;
        let f = &p.friction;
        t += &format!("\n{} — {}\n  {} → {}\n  axis {} · origin {} m\n", j.name, j.kind, j.parent.as_deref().unwrap_or("(world)"), j.child, v3(&j.axis), v3(&j.origin));
        t += &match j.limits {
            Some([lo, hi]) => format!("  limits [{lo:?}, {hi:?}] · home {:?}\n", j.home),
            None => format!("  limits: none stored · home {:?}\n", j.home),
        };
        t += &format!("  friction: coulomb {:?}, viscous {:?}, stribeck {:?}, stribeck_speed {:?}, static_ratio {:?}\n", f.coulomb, f.viscous, f.stribeck, f.stribeck_speed, f.static_ratio);
        t += &format!("  clearance {:?} m · backlash {:?} rad · wobble {:?} · damping_ratio {:?}\n", p.clearance, p.backlash, p.wobble, p.damping_ratio);
        t += &match &p.drive_backlash {
            Some(b) => format!(
                "  drive_backlash: width {} rad, uncertainty {} rad\n    provenance: {} (typed label in file)\n    reference: \"{}\"\n",
                b.width_rad.map_or("none".into(), |w| format!("{w:?}")),
                b.uncertainty_rad.map_or("none".into(), |w| format!("{w:?}")),
                provenance_label(&b.provenance),
                b.reference
            ),
            None => "  drive_backlash: none stored\n".to_string(),
        };
        t += &format!("  physics.source (file's text): \"{}\"\n  motor: {}\n", or_none(&p.source), j.motor.as_deref().unwrap_or("none"));
        if let Some(r) = &view.run {
            t += &format!("  servo: {}\n", jog_line(r, &j.name));
        }
    }
    t
}

const JOG_NOTE: &str = "Jogging while paused sets the target; it takes effect when running or stepping. Before the first build it is queued and applied after the build. Reset returns every target to the file's control targets. Nothing is written.";

/// A joint's servo state from the latest accepted frame, or why it has none.
fn jog_line(r: &RunController, joint: &str) -> String {
    match crate::robot_run::servo(r.model(), joint) {
        Err(e) => format!("none — {e}"),
        Ok(s) => {
            let latest = r.frame().and_then(|f| f.servo(joint));
            let now = latest.map_or("target — · measured — (no frame yet)".to_string(), |(t, a)| format!("target {t:.3} · measured {a:.3} {}", s.unit));
            let requested = r.requested_target(&s);
            let pending = if latest.is_none_or(|(t, _)| t != requested) { format!(" · requested {requested:.3}") } else { String::new() };
            format!("{now}{pending} · {}", s.limit_text())
        }
    }
}

/// Motors, transmissions, battery and actuator profiles.
fn drives_text(view: &RobotView, m: &PhysicalModel) -> String {
    let mut t = format!("SI units, as stored. Motor, transmission and battery values: {UNLABELLED}.\n\nMOTORS ({})\n", m.motors.len());
    for (i, x) in m.motors.iter().enumerate() {
        let (e, g, fw) = (&x.electrical, &x.gearbox, &x.firmware);
        t += &format!("• {} — spec {} · joint {} · gear_ratio {:?}\n", x.name, or_none(&x.spec), x.joint.as_deref().unwrap_or("none"), x.gear_ratio);
        t += &format!("   R {:?} Ω · L {:?} H · kt {:?} · ke {:?} · supply {:?} V · limit {:?} A\n", e.resistance, e.inductance, e.torque_constant, e.back_emf_constant, e.supply_voltage, e.current_limit);
        t += &format!("   gearbox ratio {:?} · efficiency {:?} · backlash {:?} rad · max torque {:?} · max speed {:?}\n", g.ratio, g.efficiency, g.backlash_rad, g.max_output_torque, g.max_output_speed);
        t += &format!("   firmware {} · {:?} Hz · latency {:?} s · kp {:?} ki {:?} kd {:?}\n", fw.kind, fw.loop_rate_hz, fw.latency_s, fw.kp, fw.ki, fw.kd);
        if let Some(n) = view.notes.motors.get(i).filter(|n| !n.is_null()) {
            t += &format!("   notes (file's text): \"{}\"\n", verbatim(n));
        }
    }
    t += &format!("\nTRANSMISSIONS ({})\n", m.transmissions.len());
    for x in &m.transmissions {
        t += &format!("• {}: {} = {:?} × {}\n", x.name, x.driver_joint, x.ratio, x.driven_joint);
    }
    t += &match &m.battery {
        Some(b) => format!("\nBATTERY\n  cells {:?} · nominal {:?} V · R {:?} Ω · {:?} Ah · soc {:?} · cutoff {:?} V\n", b.cells, b.nominal_voltage, b.internal_resistance, b.capacity_ah, b.initial_soc, b.cutoff_voltage),
        None => "\nBATTERY: none in file\n".to_string(),
    };
    match &m.actuator_profiles {
        None => t += "\nACTUATOR PROFILES: none in file\n",
        Some(p) => {
            t += &format!("\nACTUATOR PROFILES (v{}) — {} bindings\n", p.version, p.bindings.len());
            for (key, fam) in &p.families {
                t += &format!("• {key} v{} — content hash {}\n  \"{}\"\n", fam.version, fam.content_hash(), fam.description);
                let bound = p.bindings.values().filter(|b| &b.family == key).count();
                t += &format!("  bound to {bound} motor(s); provenance per parameter (typed label in file):\n");
                for (group, params) in [("motor", &fam.motor), ("driver", &fam.driver)] {
                    for (name, x) in params {
                        let u = x.uncertainty.map_or("unknown".into(), |u| format!("{u:?}"));
                        t += &format!("   {group}.{name} = {:?} {} — {}, ± {u}\n", x.value, x.unit, provenance_label(&x.provenance));
                    }
                }
                for l in &fam.limitations {
                    t += &format!("  limitation (file's text): \"{l}\"\n");
                }
            }
        }
    }
    t
}

/// The source block verbatim, the CAD link status, uncertainty and identification.
fn source_text(view: &RobotView, m: &PhysicalModel) -> String {
    let src = &m.source;
    let field = |k: &str| src.get(k).map_or("not recorded".to_string(), verbatim);
    let mut t = view.source.as_ref().map(file_watch_text).unwrap_or_default();
    t += &format!("SOURCE BLOCK (verbatim)\nfile: {}\nexported: {}\ncad_sha256: {}\ncollision_ray_backend: {}\n", field("file"), field("exported"), field("cad_sha256"), field("collision_ray_backend"));
    t += "\nNOTES — benchmark_assumptions (verbatim)\n";
    match src.get("benchmark_assumptions") {
        None => t += "none recorded\n",
        Some(Value::Object(map)) => map.iter().for_each(|(k, v)| t += &format!("• {k}: {}\n", verbatim(v))),
        Some(Value::Array(items)) => items.iter().for_each(|v| t += &format!("• {}\n", verbatim(v))),
        Some(v) => t += &format!("{}\n", verbatim(v)),
    }
    let shown = ["file", "exported", "cad_sha256", "collision_ray_backend", "benchmark_assumptions"];
    for (k, v) in src.as_object().into_iter().flatten().filter(|(k, _)| !shown.contains(&k.as_str())) {
        t += &format!("{k}: {}\n", verbatim(v));
    }
    t += "\nCAD LINK\n";
    t += &match &view.cad_link {
        None => "not computed".to_string(),
        Some(CadLinkStatus::Current { path, sha256, .. }) => format!("current — {} matches the recorded sha256\n  {sha256}", path.display()),
        Some(CadLinkStatus::Stale { path, recorded_sha256, on_disk_sha256, .. }) => format!("stale — {} changed since export\n  recorded {recorded_sha256}\n  on disk  {on_disk_sha256}", path.display()),
        Some(CadLinkStatus::Missing { file, .. }) => format!("missing — no file found for \"{file}\""),
        Some(CadLinkStatus::NoRecordedHash { path, on_disk_sha256, .. }) => format!("no recorded hash — {} exists, but the export has no cad_sha256, so it cannot be compared\n  on disk {on_disk_sha256}", path.display()),
        Some(CadLinkStatus::NoSourceFile) => "no source file recorded in the export".to_string(),
        Some(CadLinkStatus::Unreadable { path, error, .. }) => format!("unreadable — {}: {error}", path.display()),
    };
    let tried = match &view.cad_link {
        Some(CadLinkStatus::Current { tried, .. } | CadLinkStatus::Stale { tried, .. } | CadLinkStatus::Missing { tried, .. } | CadLinkStatus::NoRecordedHash { tried, .. } | CadLinkStatus::Unreadable { tried, .. }) => tried.as_slice(),
        _ => &[],
    };
    if !tried.is_empty() {
        t += "\n  tried:";
        for p in tried {
            t += &format!("\n   {}", p.display());
        }
    }
    t += &format!("\n  rule: {}\n", cad_link::RESOLUTION_RULE);
    t += "\nUNCERTAINTY (as stored)\n";
    match &view.notes.uncertainty {
        Value::Object(map) => map.iter().for_each(|(k, v)| t += &format!("  {k}: {v}\n")),
        Value::Null => t += "none in file\n",
        v => t += &format!("  {v}\n"),
    }
    t += &format!("\nIDENTIFICATION ({})\n", m.identification.len());
    if m.identification.is_empty() {
        t += "none in file\n";
    }
    for (k, x) in &m.identification {
        t += &format!("• {k}: rms {:?} rad · fitted {} · log {}\n   {}\n", x.rms_error_rad, or_none(&x.fitted_at), or_none(&x.source_log), serde_json::to_string(x).unwrap_or_default());
    }
    t += &format!("\nPROVENANCE RULE\n{PROVENANCE_RULE}\n");
    t
}

/// The opened file's identity and last reload (robot_state.source_file).
fn file_watch_text(s: &SourceWatch) -> String {
    let mut t = format!("FILE (watched every {:.1} s; Reload re-reads it)\n{}\nsha256 {}\nloaded at {} · {} reload(s)\n", robot_source::POLL.as_secs_f64(), s.path.display(), s.hash.as_deref().unwrap_or("—"), s.loaded_at.as_deref().unwrap_or("—"), s.reload_count);
    if let Some(r) = &s.last {
        t += &format!("last reload: {:?} · {} at {}\n", r.trigger, r.outcome, r.at).to_lowercase();
    }
    if let Some(e) = &s.failing {
        t += &format!("SHOWING THE LAST GOOD MODEL — the file on disk does not load:\n{e}\n");
    }
    t.push('\n');
    t
}

/// Floor grid and the selected link's centre of mass.
/// Key G: the same `RobotAction::ToggleGraphs` as the Graphs button and `system_ui` graphs:toggle.
fn graph_key(keys: Res<ButtonInput<KeyCode>>, mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>) {
    if keys.just_pressed(KeyCode::KeyG) {
        view.run_message = dispatch(&mut view, &mut orbit, RobotAction::ToggleGraphs).err();
    }
}

/// Keys =/+ and − (main row and numpad): the same `RobotAction::Speed` as the header
/// −/+ buttons, `system_ui` run:speed_* and REST robot_speed. A refusal at ×8 / ×0.125 shows in the header.
fn speed_keys(keys: Res<ButtonInput<KeyCode>>, mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>) {
    let speed = if keys.any_just_pressed([KeyCode::Equal, KeyCode::NumpadAdd]) {
        SpeedRequest::Up
    } else if keys.any_just_pressed([KeyCode::Minus, KeyCode::NumpadSubtract]) {
        SpeedRequest::Down
    } else {
        return;
    };
    view.run_message = dispatch(&mut view, &mut orbit, RobotAction::Speed { speed }).err();
}

/// The speed buttons: dimmed when refused (at a limit, or no robot), the middle one shows ×scale.
fn speed_panel(view: Res<RobotView>, mut buttons: Query<(&RobotAction, &Interaction, &mut BackgroundColor), With<SpeedButton>>, mut labels: Query<(&SpeedLabel, &mut Text)>) {
    for (action, interaction, mut background) in &mut buttons {
        let enabled = check(&view, action).is_ok();
        let color = match (enabled, interaction) {
            (false, _) => Color::srgba(0.16, 0.20, 0.25, 0.35),
            (true, Interaction::Hovered | Interaction::Pressed) => ACCENT.with_alpha(0.45),
            (true, _) => Color::srgb(0.16, 0.20, 0.25),
        };
        if background.0 != color {
            background.0 = color;
        }
    }
    let scale = view.run.as_ref().map_or(1.0, RunController::speed_scale);
    for (label, mut text) in &mut labels {
        if label.0 {
            let want = format!("×{scale}");
            if text.0 != want {
                text.0 = want;
            }
        }
    }
}

/// Keys C / J / F / H: the same `RobotAction::Overlay` as the inspector buttons and `system_ui` overlay:*.
fn overlay_keys(keys: Res<ButtonInput<KeyCode>>, mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>) {
    for (kind, _, key) in OVERLAYS {
        if keys.just_pressed(key) {
            let action = overlay_toggle(&view, kind);
            view.run_message = dispatch(&mut view, &mut orbit, action).err();
        }
    }
}

/// The overlay block: each button carries the flip of its current flag and
/// shows on/off; the line under it gives the accepted frame's counts and the scales.
fn overlay_panel(
    view: Res<RobotView>,
    mut buttons: Query<(&OverlayButton, &mut RobotAction, &mut BackgroundColor, &mut Node)>,
    mut labels: Query<(&OverlayLabel, &mut Text), Without<OverlayText>>,
    mut line: Single<&mut Text, With<OverlayText>>,
) {
    let run = view.run.as_ref();
    let available = run.is_some_and(|r| r.check_overlays().is_ok());
    for (b, mut action, mut color, mut node) in &mut buttons {
        let next = overlay_toggle(&view, b.0);
        if *action != next {
            *action = next;
        }
        let on = overlay_on(&view, b.0);
        let c = if on { Color::srgb(0.18, 0.36, 0.30) } else { Color::srgb(0.16, 0.20, 0.25) };
        if color.0 != c {
            color.0 = c;
        }
        let enabled = if b.0 == "stress" { check_stress(&view).is_ok() } else { available };
        let display = if enabled { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for (l, mut text) in &mut labels {
        let (_, name, key) = OVERLAYS.iter().find(|o| o.0 == l.0).copied().unwrap_or(OVERLAYS[0]);
        let t = format!("{name} {} ({})", if overlay_on(&view, l.0) { "on" } else { "off" }, format!("{key:?}").trim_start_matches("Key"));
        if text.0 != t {
            text.0 = t;
        }
    }
    let t = match run {
        None => String::new(),
        Some(r) => match r.check_overlays() {
            Err(_) => "Overlays (contacts, joint frames, deflections): not available for presets".into(),
            Ok(()) => match r.frame() {
                None => format!("Overlays: no run frame yet (Run or Step) · force {} m/N · deflection ×{}", robot_run::FORCE_SCALE_M_PER_N, robot_run::DEFLECTION_MAGNIFICATION),
                Some(f) => {
                    let o = &f.overlays;
                    let count = |n: Option<usize>| n.map_or("—".into(), |n| n.to_string());
                    let max = o.deflections.as_ref().map(|d| d.iter().map(|d| d.displacement.iter().map(|x| x * x).sum::<f64>().sqrt()).fold(0.0, f64::max));
                    format!("Overlays · gen {} t {:.2} s: {} contacts · {} joint frames · {} deflection points{} · force {} m/N · deflection ×{}",
                        f.generation, f.time, count(o.contacts.as_ref().map(Vec::len)), count(o.joints.as_ref().map(Vec::len)), count(o.deflections.as_ref().map(Vec::len)),
                        max.filter(|m| *m > 0.0).map_or(String::new(), |m| format!(" (max {:.3} mm)", m * 1e3)), robot_run::FORCE_SCALE_M_PER_N, robot_run::DEFLECTION_MAGNIFICATION)
                }
            },
        },
    };
    if line.0 != t {
        line.0 = t;
    }
}

/// The stress overlay's paint: when the overlay revision changed (toggle, new
/// results, new meshes), each link mesh gets per-vertex colours through the
/// shared rule (`robot_stress`, from already-parsed results; bounded by
/// vertices × ≤200 hotspot cells per link, timed in `paint_seconds`) or
/// loses them. Links without cells take the normal link colour.
fn stress_paint(mut view: ResMut<RobotView>, links: Query<(&LinkMesh, &Mesh3d)>, mut meshes: ResMut<Assets<Mesh>>, mut redraw: MessageWriter<bevy::window::RequestRedraw>) {
    view.stress.take();
    if view.stress.busy() {
        // Keep polling the results-only read in the reactive window.
        redraw.write(bevy::window::RequestRedraw);
    }
    if view.stress.painted.is_some_and(|(r, _)| r == view.stress.revision) {
        return;
    }
    let started = std::time::Instant::now();
    let paint = view.stress.painting();
    let plain = LINK_COLOUR.to_linear().to_f32_array();
    for (link, mesh) in &links {
        let Some(mut mesh) = meshes.get_mut(&mesh.0) else { continue };
        if !paint {
            mesh.remove_attribute(Mesh::ATTRIBUTE_COLOR);
            continue;
        }
        let Some(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|a| a.as_float3()).map(<[[f32; 3]]>::to_vec) else { continue };
        let colours = match (view.stress.results.as_ref(), view.model.as_ref()) {
            (Some(r), Some(m)) => r.colours(m, link.0, &positions),
            _ => None,
        };
        let colours = colours.unwrap_or_else(|| vec![plain; positions.len()]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    }
    let revision = view.stress.revision;
    view.stress.painted = Some((revision, started.elapsed().as_secs_f64()));
}

/// A stress peak for the label: three significant figures in Pa, kPa or MPa,
/// chosen after rounding so 999.96 kPa reads "1.00 MPa". Display only; the
/// stored `peak_stress_pa` is never rewritten. Rounding keeps the label steady.
fn stress_label(pa: f64) -> String {
    if !pa.is_finite() {
        return "—".into();
    }
    if pa == 0.0 {
        return "0 Pa".into();
    }
    let sig3 = |v: f64| match v.abs().log10().floor() as i32 - 2 {
        k if k >= 0 => (v / 10f64.powi(k)).round() * 10f64.powi(k),
        k => (v * 10f64.powi(-k)).round() / 10f64.powi(-k),
    };
    let r = sig3(pa);
    let (v, unit) = match r.abs() {
        a if a >= 1e6 => (r / 1e6, "MPa"),
        a if a >= 1e3 => (r / 1e3, "kPa"),
        _ => (r, "Pa"),
    };
    let decimals = (2 - v.abs().log10().floor() as i32).max(0) as usize;
    format!("{v:.decimals$} {unit}")
}

/// The stress label under the overlay buttons (`--robot FILE`): path, mtime,
/// status against the loaded model, peak per link and the colour scale.
fn stress_panel(view: Res<RobotView>, mut line: Single<&mut Text, With<StressText>>) {
    let t = match (&view.source, view.model.as_ref()) {
        (None, _) if view.run.is_some() => "Stress: not available for presets (no .simresult.json)".to_string(),
        (None, _) | (_, None) => String::new(),
        (Some(_), Some(m)) if view.stress.enabled => match view.stress.results.as_ref() {
            None => "Stress: reading results…".into(),
            Some(r) => {
                let path = r.path.display();
                match &r.contents {
                    robot_stress::Contents::Missing => format!("Stress: no results file ({path}); run `sim-cad run <model>` to write one"),
                    robot_stress::Contents::Invalid(e) => format!("Stress: results file not usable: {e}"),
                    robot_stress::Contents::Parsed(v) => {
                        let mtime = r.mtime_unix_s.map_or("mtime unknown".into(), |t| format!("mtime {}", robot_recording::iso((t * 1e3) as u128)));
                        let peaks: Vec<String> = sim_domain_robot::stress_results::peaks(v).into_iter().map(|(k, p)| format!("{k} {}", p.map_or("—".into(), stress_label))).collect();
                        format!("Stress · {} · {path} · {mtime}\npeak: {}\n{}", r.status(m), peaks.join(" · "), sim_domain_robot::stress_results::SCALE)
                    }
                }
            }
        },
        (Some(_), Some(_)) => "Stress off (H): colours links from the model's .simresult.json".into(),
    };
    if line.0 != t {
        line.0 = t;
    }
}

/// The graph dock: the fixed chart set (`robot_graphs::charts`) drawn with the
/// shared `crate::chart` raster, redrawn at most ten times a second and only
/// when the sampled history, selection, mode or visibility changed.
fn graph_dock(
    mut commands: Commands,
    view: Res<RobotView>,
    fonts: Res<UiFonts>,
    time: Res<Time>,
    mut images: ResMut<Assets<Image>>,
    dock: Single<(Entity, &mut Node), With<GraphDock>>,
    mut handles: Local<Vec<Handle<Image>>>,
    mut drawn: Local<Option<(String, f64)>>,
    mut redraw: MessageWriter<bevy::window::RequestRedraw>,
) {
    let (entity, mut node) = dock.into_inner();
    let display = if view.graphs_visible { Display::Flex } else { Display::None };
    if node.display != display {
        node.display = display;
    }
    let Some(run) = view.run.as_ref().filter(|_| view.graphs_visible) else {
        *drawn = None;
        return;
    };
    let h = run.graphs();
    let gait = run.gait_preview().is_some_and(|g| g.loaded().is_some());
    let stamp = format!("{:?}|{}|{}|{}|{:?}|{:?}|{gait}", view.selected, h.generation(), h.frames(), run.graphs_mode(), h.window(), run.frame().map(|f| (f.time, f.steps)));
    let now = time.elapsed_secs_f64();
    match drawn.as_ref() {
        Some((s, _)) if *s == stamp => return,
        // Bounded refresh while frames stream in: come back once the interval has passed.
        Some((_, at)) if now - at < 0.1 && run.active() => {
            redraw.write(bevy::window::RequestRedraw);
            return;
        }
        _ => {}
    }
    *drawn = Some((stamp, now));
    let charts = run.graph_charts(view.selected);
    let mode = run.graphs_mode();
    commands.entity(entity).despawn_related::<Children>();
    let text = |value: &str, size: f32, color: Color| label(&fonts, value, size, color);
    if gait {
        // The charts are physics frames only; preview samples are never plotted as traces.
        let caption = commands.spawn((Node { width: Val::Px(150.0), flex_shrink: 0.0, ..default() }, children![text("Gait preview is kinematic and not charted: these charts are the physics run's frames only.", 11.0, crate::builder::ui::WARN)])).id();
        commands.entity(entity).add_child(caption);
    }
    let num = |x: f64| if x == 0.0 || (x.abs() >= 1e-3 && x.abs() < 1e4) { format!("{x:.4}") } else { format!("{x:.3e}") };
    for (slot, c) in charts.iter().enumerate() {
        while handles.len() <= slot {
            handles.push(images.add(crate::chart::blank_image()));
        }
        let traces: Vec<(&[[f64; 2]], [u8; 3])> = c.traces.iter().enumerate().map(|(i, t)| (t.points.as_slice(), crate::chart::COLORS[i % crate::chart::COLORS.len()])).collect();
        let (pixels, range, window) = crate::chart::rasterize_span(&traces, Some(crate::robot_graphs::WINDOW_S));
        let drawable = c.traces.iter().map(|t| t.points.len()).sum::<usize>() >= 2;
        if let Some(mut image) = images.get_mut(&handles[slot]) {
            image.data = Some(pixels);
        }
        let units: std::collections::BTreeSet<&str> = c.traces.iter().map(|t| t.unit.as_str()).collect();
        let unit = if units.len() == 1 { units.into_iter().next().unwrap_or_default().to_string() } else { String::new() };
        let card = commands.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), row_gap: Val::Px(3.0), ..default() }).id();
        let head = commands.spawn(Node { flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(6.0), flex_shrink: 0.0, ..default() }).id();
        let title = commands.spawn(text(&c.title, 11.5, INK)).id();
        let badge = commands.spawn(text(&format!("{} · gen {}", mode.to_uppercase(), h.generation()), 11.0, if mode == "replay" { Color::srgb(0.98, 0.62, 0.22) } else { ACCENT })).id();
        commands.entity(head).add_children(&[title, badge]);
        commands.entity(card).add_child(head);
        if let Some(why) = &c.absent_reason {
            let t = commands.spawn(text(why, 11.0, MUTED)).id();
            commands.entity(card).add_child(t);
        }
        if !c.traces.is_empty() {
            let plot = commands.spawn((Node { flex_grow: 1.0, min_height: Val::Px(60.0), border: UiRect::all(Val::Px(1.0)), ..default() }, BorderColor::all(Color::srgb(0.2, 0.24, 0.29)), ImageNode::new(handles[slot].clone()))).id();
            if drawable {
                let with_unit = |v: f64| if unit.is_empty() { num(v) } else { format!("{} {unit}", num(v)) };
                let top = commands.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(4.0), top: Val::Px(2.0), ..default() }, children![text(&with_unit(range.1), 10.0, MUTED)])).id();
                let bottom = commands.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(4.0), bottom: Val::Px(2.0), ..default() }, children![text(&with_unit(range.0), 10.0, MUTED)])).id();
                let x = commands.spawn((Node { position_type: PositionType::Absolute, right: Val::Px(4.0), bottom: Val::Px(2.0), ..default() }, children![text(&format!("{:.2} – {:.2} s sim time", window.0, window.1), 10.0, MUTED)])).id();
                commands.entity(plot).add_children(&[top, bottom, x]);
            }
            commands.entity(card).add_child(plot);
            for (i, t) in c.traces.iter().enumerate() {
                let [r, g, b] = crate::chart::COLORS[i % crate::chart::COLORS.len()];
                let value = match (t.points.last(), &t.absent_reason) {
                    (Some(p), _) => format!("{} {}", num(p[1]), t.unit),
                    (None, Some(why)) => why.clone(),
                    (None, None) => "–".into(),
                };
                let source = if t.source.starts_with("request") { "request (held input in frame)" } else if t.source.starts_with(crate::robot_graphs::WORLD_FRAME) { crate::robot_graphs::WORLD_FRAME } else { t.source.split(" (").next().unwrap_or(&t.source) };
                let color = Color::srgb_u8(r, g, b);
                let swatch = commands.spawn((Node { border_radius: BorderRadius::all(Val::Px(2.0)), width: Val::Px(9.0), height: Val::Px(9.0), flex_shrink: 0.0, ..default() }, BackgroundColor(color))).id();
                let line = commands.spawn(text(&format!("{}: {value}  ·  {source}", t.name), 10.5, color)).id();
                let row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(5.0), align_items: AlignItems::Center, ..default() }).add_children(&[swatch, line]).id();
                commands.entity(card).add_child(row);
            }
        }
        commands.entity(entity).add_child(card);
    }
}

/// The run-thread overlays' gizmo group: drawn over the link meshes (contacts sit under the
/// wheels and joint axes inside the links, so depth-tested lines were hidden), with wider lines.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct OverlayGizmos;

fn overlay_gizmo_config() -> GizmoConfig {
    GizmoConfig { depth_bias: -1.0, line: GizmoLineConfig { width: 3.0, ..default() }, ..default() }
}

fn draw(view: Res<RobotView>, root: Single<&GlobalTransform, With<RobotRoot>>, mut gizmos: Gizmos, mut overlay: Gizmos<OverlayGizmos>) {
    let Some(model) = &view.model else { return };
    // Run-thread overlays (`--robot FILE`), only from an accepted frame of the current
    // generation, mapped model → display through RobotRoot like the link meshes.
    if let Some(run) = view.run.as_ref() {
        let flags = run.overlays();
        if let Some(f) = run.frame().filter(|f| robot_run::accept(run.generation(), f)) {
            let affine = root.affine();
            let point = |p: &[f64; 3]| affine.transform_point3(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32));
            let vector = |v: &[f64; 3], k: f64| affine.transform_vector3(Vec3::new((v[0] * k) as f32, (v[1] * k) as f32, (v[2] * k) as f32));
            if let Some(joints) = f.overlays.joints.as_ref().filter(|_| flags.joints) {
                const AXES: [Color; 3] = [Color::srgb(1.0, 0.95, 0.3), Color::srgb(0.3, 1.0, 0.95), Color::srgb(1.0, 0.5, 1.0)];
                for j in joints {
                    let p = point(&j.point);
                    for (k, a) in j.axes.iter().enumerate() {
                        let d = vector(a, robot_run::JOINT_AXIS_HALF_M);
                        overlay.line(p - d, p + d, AXES[k % 3]);
                    }
                    overlay.sphere(Isometry3d::from_translation(p), 0.003, Color::WHITE);
                }
            }
            if let Some(contacts) = f.overlays.contacts.as_ref().filter(|_| flags.contacts) {
                for c in contacts {
                    let p = point(&c.point);
                    let color = if c.other == "ground" { Color::srgb(1.0, 0.2, 0.2) } else { Color::srgb(1.0, 0.4, 0.2) };
                    overlay.sphere(Isometry3d::from_translation(p), 0.003, color);
                    overlay.line(p, p + vector(&c.force, robot_run::FORCE_SCALE_M_PER_N), color);
                }
            }
            if let Some(deflections) = f.overlays.deflections.as_ref().filter(|_| flags.deflections) {
                for d in deflections {
                    let p = point(&d.point);
                    overlay.line(p, p + vector(&d.displacement, robot_run::DEFLECTION_MAGNIFICATION), Color::srgb(0.6, 1.0, 0.6));
                }
            }
        }
    }
    let floor = model.world.floor_z as f32;
    gizmos.grid(Isometry3d::new(Vec3::new(0.0, floor, 0.0), Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), UVec2::splat(20), Vec2::splat(0.05), Color::srgba(0.45, 0.50, 0.58, 0.35));
    if let Some(l) = view.selected.and_then(|i| model.links.get(i)) {
        let com = Vec3::new(l.com[0] as f32, l.com[2] as f32, -l.com[1] as f32);
        gizmos.sphere(Isometry3d::from_translation(com), 0.006, ACCENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stress_label_keeps_three_significant_figures_across_units() {
        assert_eq!(stress_label(0.0), "0 Pa");
        assert_eq!(stress_label(12_345.0), "12.3 kPa");
        assert_eq!(stress_label(4_560.0), "4.56 kPa");
        assert_eq!(stress_label(999_960.0), "1.00 MPa");
        assert_eq!(stress_label(23_456_789.0), "23.5 MPa");
        assert_eq!(stress_label(250e6), "250 MPa");
        assert_eq!(stress_label(7.25), "7.25 Pa");
        assert_eq!(stress_label(f64::NAN), "—");
    }
    #[test]
    fn robot_mode_loads_wheeled_baseline_and_names_bad_paths() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let loaded = load(&root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap();
        assert_eq!(loaded.model.links.len(), 4);
        assert_eq!(loaded.geometry.len(), 4);
        assert!(loaded.geometry.iter().all(|g| g.as_ref().is_some_and(|g| g.triangles() > 0)));
        let missing = root.join("examples/wheeled-robot/baseline/no-such.simrobot.json");
        let err = load(&missing).err().unwrap();
        assert!(err.contains(&*missing.to_string_lossy()), "{err}");
    }
}
