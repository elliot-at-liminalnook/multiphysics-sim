//! Robot mode (`--robot FILE`): a CAD-exported `.simrobot.json` opened
//! read-only. The shared `PhysicalModel` loader runs on a worker thread; each
//! link's collision geometry is drawn at the exported assembly pose until a
//! run starts. Run/Pause/Step/Reset drive the shared `PhysicalRobot` on the
//! run thread (`robot_run`); links follow its frames. Nothing is written.
//! A preset (`--robot-preset ID`, REST `robot_preset`) opens the same way:
//! its scene's `robot` goes through the same loader, and the run thread runs
//! the preset's shared EmbeddedEnvironment/EmbeddedSession (`robot_preset`).
use super::{ACCENT, INK, MUTED, PANEL};
use crate::builder::ui::UiFonts;
use bevy::{
    asset::RenderAssetUsages,
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    picking::mesh_picking::{MeshPickingCamera, MeshPickingSettings},
    prelude::*,
    render::{
        camera::Viewport,
        mesh::{Indices, PrimitiveTopology},
    },
    winit::{UpdateMode, WinitSettings},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_domain_robot::PhysicalModel;
use sim_domain_robot::cad_link::{self, CadLinkStatus};
use crate::robot_preset::{Preset, PresetRun};
use crate::robot_motion::{self, KEYS};
use crate::robot_recording;
use crate::robot_run::{JOG_LABEL, JOG_SEMANTICS, JOG_STEP_M, JOG_STEP_RAD, MotionRequest, ReplayPhase, RunAction, RunController};
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
    let name = path.to_string_lossy();
    // PhysicalModel::load is read + parse; parse the same bytes for the notes.
    let text = std::fs::read_to_string(path).map_err(|e| format!("{name}: {e}"))?;
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
    rx: Option<Mutex<mpsc::Receiver<Result<(Loaded, Option<PresetRun>), String>>>>,
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
}
impl RobotView {
    /// Starts the worker load; the window opens without waiting for it.
    pub fn open(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let worker = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(load(&worker).map(|l| (l, None)));
        });
        Self::new(path, rx, None)
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
            let _ = tx.send(load_preset(worker, &dir).map(|(l, r)| (l, Some(r))));
        });
        let mut view = Self::new(root.join(preset.scene.as_deref().unwrap_or_default()), rx, Some(preset));
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
    fn new(path: PathBuf, rx: mpsc::Receiver<Result<(Loaded, Option<PresetRun>), String>>, preset: Option<Preset>) -> Self {
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
            rx: Some(Mutex::new(rx)),
            run: None,
            run_message: None,
            pose_dirty: false,
            ui_revision: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_micros() as u64,
            panels_ready: false,
            graphs_visible: false,
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
        let preset = match (self.run.as_ref().and_then(|r| r.preset()), &self.preset) {
            (Some(p), _) => Some(p.state_json(self.run.as_ref().and_then(|r| r.frame()).and_then(|f| f.completed_steps))),
            (None, Some(p)) => Some(json!({"id": p.id, "label": p.label, "mode": p.mode, "scene": p.scene, "config": p.config, "task": p.task,
                "readiness": p.readiness(), "evidence": p.evidence(), "loaded": false})),
            (None, None) => None,
        };
        let stepped = self.run.as_ref().and_then(|r| r.frame()).is_some();
        let cad = self.cad_link.as_ref().map(|c| json!({"link": c, "rule": cad_link::RESOLUTION_RULE}));
        let jog = self.run.as_ref().filter(|r| r.preset().is_none()).map(|r| {
            let m = r.model();
            let joints: Vec<Value> = m.joints.iter().filter(|j| j.kind != "fixed" && !j.is_loop()).map(|j| r.jog_json(&j.name)).collect();
            let selected: Vec<String> = jog_joints(self).into_iter().map(|(j, _)| j).collect();
            json!({"label": JOG_LABEL, "semantics": JOG_SEMANTICS, "control_mode": m.control.mode, "trajectory_keyframes": m.control.trajectory.len(),
                "step_rad": JOG_STEP_RAD, "step_m": JOG_STEP_M, "selected_link_joints": selected, "joints": joints, "last_apply_error": r.jog_error()})
        });
        json!({"file": self.path, "workspace": crate::workspace::json(), "status": status, "error": error, "load_seconds": seconds,
            "link_count": m.map(|m| m.links.len()), "links": links, "selected": selected,
            "joints": joints, "motors": motors, "transmissions": m.map(|m| &m.transmissions), "battery": m.and_then(|m| m.battery.as_ref()),
            "actuator_profiles": profiles, "uncertainty": m.map(|_| &self.notes.uncertainty), "uncertainty_parsed": m.map(|m| &m.uncertainty), "identification": m.map(|m| &m.identification),
            "materials": m.map(|m| &m.materials), "source": m.map(|m| &m.source), "cad_link": cad,
            "provenance_rule": PROVENANCE_RULE, "unlabelled_values": UNLABELLED, "numbers": "JSON numbers as parsed by PhysicalModel (f64, shortest round-trip); SI units; a null in place of a number is non-finite",
            "section": self.section, "inspector_scroll": {"offset_px": self.scroll, "max_px": self.scroll_max},
            "pose": if stepped { SIMULATED_POSE } else { POSE }, "read_only": true, "stepped": stepped, "run": run, "jog": jog, "preset": preset, "motion": self.run.as_ref().map(|r| r.motion_json()), "recording": self.run.as_ref().filter(|r| r.preset().is_some()).map(|r| r.recording_json()),
            "recordings": self.run.as_ref().map(|r| r.recordings_json()), "replay": self.run.as_ref().map(|r| r.replay_json()),
            "graphs": self.run.as_ref().map_or_else(|| json!({"visible": self.graphs_visible, "charts": []}), |r| r.graphs_json(self.selected, self.graphs_visible)), "ui_revision": self.ui_revision, "controls_ready": self.panels_ready})
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
}
/// The absolute target a jog action asks for (file validation happens in `check_jog`).
fn jog_target(run: &RunController, joint: &str, delta: f64) -> Result<f64, String> {
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
        out.push(("graphs:toggle".into(), (if view.graphs_visible { "Hide graphs (G)" } else { "Show graphs (G)" }).into(), RobotAction::ToggleGraphs));
        for action in RunAction::ALL {
            out.push((format!("run:{}", action.name()), action.label().into(), RobotAction::Run { action }));
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
        }
    }
    out
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
        Request::Camera { focus, radius, yaw, pitch } => {
            if !focus.iter().chain([radius, yaw, pitch].iter()).all(|x| x.is_finite()) || radius <= 0. || pitch.abs() > 1.5 {
                return Err("finite camera required; radius > 0 and pitch within ±1.5 radians".into());
            }
            *orbit = RobotOrbit { focus: Vec3::from_array(focus), radius, yaw, pitch, home: false, ..*orbit };
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
        c("robot_state", json!({}), "Read-only robot mode: file, workspace (the resolved root: root, found_by override | env | opened_file | cwd, from, error, rule; also in GET /v1/capabilities), status (loading | loaded | error, with the error naming the path), link_count, links, the selected link (mass, com, inertia, material and its density or material_in_file=false, file_notes), joints, motors, transmissions, battery, actuator_profiles (with content hashes), uncertainty, identification, the source block verbatim and cad_link (current | stale | missing | no_recorded_hash | no_source_file | unreadable, with the resolution rule and paths tried). Numbers are full-precision JSON; provenance is null unless the file carries a typed label (provenance_rule). Also the inspector section and scroll, and run (null until loaded; phase idle with null time before any build; see robot_run). Nothing is written."),
        c("system_ui", json!({"action":{"operation":"controls"}}), "Discover the link list, inspector sections (section:link | joints | drives | source), inspector scrolling and view controls (controls) and activate one by id with the current ui_revision (activate), through the same handler as a click. Selection is shared by the list, the 3D view and robot_state."),
        c("robot_jog", json!({"joint":"left axle","target":0.5}), &format!("Servo-target jog of one joint by its file name: {JOG_LABEL}. Give target (absolute; rad, or m on a prismatic joint) or delta (from the current requested target). The same handler as the jog +/− buttons and system_ui jog:<joint>:+/- controls (±{JOG_STEP_RAD} rad, ±{JOG_STEP_M} m prismatic; listed for the joints touching the selected link). Errors name the joint: unknown joint, no servo target (passive joint, firmware none, fixed, or a trajectory-mode file), non-finite target, or a target outside the file's limits (with the limit; never clamped); after a failed run, Reset first. {JOG_SEMANTICS} robot_state.jog reports control mode, label and, per joint, the limit (or no limit in file), requested target, and the target and measured value from the latest accepted frame.")),
        c("robot_run", json!({"action":"start"}), "Run controls, the same handler as the Run/Pause/Step/Reset buttons and system_ui run:* controls. start runs the shared PhysicalRobot on the run thread (built from the loaded model with sim_runtime::registry() and BuildOptions::default()), paced at most to real time; pause stops it; step advances exactly one 0.02 s chunk and is refused while running; reset rebuilds at t = 0 (assembly pose), bumps the generation and leaves it paused (allowed after a failure). An unknown action is an error naming it and listing the valid ones. robot_state.run reports phase (idle | building | running | paused | failed), time, steps (chunks), chunk_s, measured rtf, generation, error and the latest accepted frame. Nothing is written."),
        c("robot_presets", json!({}), &format!("List the robot presets declared in {} (its paths resolved against the workspace root, reported in workspace): id, label, mode, scene/config/task paths, inputs_exist and missing, under_ignored_runs, runs_as (EmbeddedEnvironment with a task, else EmbeddedSession) and openable with the reason when not (the build itself is not attempted). Only mode `embedded` runs natively.", crate::robot_preset::PRESETS)),
        c("robot_preset", json!({"id":"robot-measured-400hz"}), "Open a declared embedded preset by id in this window (replacing the current robot and stopping its run thread). Unknown ids, other modes and missing inputs are refused naming the id. The scene, config and task are parsed exactly as declared on a worker thread; scene.robot is drawn and inspected through the same loader as --robot FILE. Run/Pause/Step/Reset (robot_run) then drive the shared EmbeddedEnvironment (task) or EmbeddedSession (no task) on the run thread with seed 0 (recorded), one action interval or clamp(report_every, 1, 40) nominal steps per chunk. robot_state.preset reports id, label, paths, readiness and evidence verbatim, seed, step_s, chunk and step counts; run.phase `ended` with run.end reports a reached horizon or a terminated/truncated episode. Servo-target jog is not offered for presets."),
        c("robot_input", json!({"channels":{"command.forward_speed":0.001}}), &format!("Motion request for a running robot preset: {}. Give channels (values by motion channel name; the other motion channels keep their requested values) or key (w | a | s | d latches that key's request until stop, another key or a channel request; stop sets every motion channel to 0). The same handler as physical W/A/S/D (press/release) and X (stop), the W/A/S/D/Stop buttons and system_ui motion:w|a|s|d|stop. Motion channels come from the session's policy_contract.step_reference.config, else the preset's presets.json motion_commands (as web/viewer/motion-commands.mjs); key vectors from motion_key_vectors, else the channel bounds. Refused naming the channel and its bounds: an unknown channel, a session input that is not a motion channel, a non-finite value, or a value or summed key vector outside the channel's bounds ({}). Also refused, naming the reason: --robot FILE, a preset with no motion config, no built session (Run or Step builds it), a failed or ended run, an exhausted heartbeat. A declared motion_heartbeat is {} robot_state.motion reports source, channels with bounds, requested and held values, active keys, heartbeat, last refusal and the label.", robot_motion::LABEL, robot_motion::CLAMP_RULE, robot_motion::HEARTBEAT_RULE)),
        c("robot_save_recording", json!({"note":"after motion:w"}), &format!("Save the loaded preset run's recording: the same handler as the Save recording button and system_ui recording:save. The run thread snapshots the shared recording (EmbeddedEnvironment::episode_recording() for a preset with a task, EmbeddedSession::recording() without, as the browser's Download) in any phase with a built session, running, paused, ended or failed; a writer thread writes it, so the response returns at once with recording.pending set and robot_state.recording.last_saved {{path, meta_path, kind, version, completed_steps, replayable, not_replayable_reason, failure, saved_utc, bytes}} (or recording.error) once written. Optional path (relative to the root or absolute) and note (kept in the sidecar). {} {} {} Refused, naming the reason: --robot FILE, no built session (Run or Step first), a save still being written, a path under examples/, cad/ or web/, a name not ending in .json or ending in .meta.json, and an existing file (reported in recording.error).", robot_recording::LOCATION_RULE, robot_recording::FILE_RULE, robot_recording::REPLAYABLE_RULE)),
        c("robot_replay", json!({"file":"20260930T060822.729Z.json"}), &format!("Replay a saved recording of the loaded preset: the same handler as the inspector Replay buttons and system_ui replay:<file>. Give file (a bare name listed in robot_state.recordings.files, in runs/robot-presets/<preset-id>/) or path (any readable recording .json, relative to the root or absolute; reading is not restricted). {{\"action\":\"cancel\"}} stops the replay between chunks (system_ui replay:cancel); {{\"action\":\"list\"}} lists the recordings again off the UI thread (system_ui replay:refresh). {} {} {} {} robot_state.replay reports path, phase (idle | replaying | cancelled | done | failed), completed/total with unit, completed_steps and recorded_completed_steps, verdict, error, measured, replaced and sidecar. Refused, naming the reason: --robot FILE, a replay already in progress, a running run (Pause first), a building session, a missing or non-.json file, a recording of the other kind, a runtime mismatch (the runtime's message) and a session identity mismatch (the viewer's labelled check).", robot_recording::REPLAY_RULE, robot_recording::VERDICT_RULE, robot_recording::IDENTITY_RULE, robot_recording::MEASURED_RULE)),
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
#[derive(Component)]
struct GraphDock;
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
}

/// The window: worker load, posed link meshes, link list, inspector and REST.
pub fn run_robot(view: RobotView, api: sim_api::Server) {
    App::new()
        .insert_resource(crate::rest::Rest(api, None))
        .insert_resource(view)
        .insert_resource(ClearColor(Color::srgb(0.10, 0.125, 0.155)))
        .insert_resource(AmbientLight { color: Color::srgb(0.85, 0.90, 1.0), brightness: 420.0, affects_lightmapped_meshes: true })
        .insert_resource(MeshPickingSettings { require_markers: true, ..default() })
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::reactive(std::time::Duration::from_secs_f64(1.0 / 60.0)),
            unfocused_mode: UpdateMode::reactive_low_power(std::time::Duration::from_millis(40)),
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Systems — Robot (file read-only)".into(),
                resolution: (1500.0_f32, 940.0_f32).into(),
                resize_constraints: bevy::window::WindowResizeConstraints { min_width: 980.0, min_height: 720.0, ..default() },
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, ((crate::builder::ui::load_fonts, setup).chain(), crate::rest::wake_on_request))
        .add_systems(Update, (receive, poll_rest, motion_keys, graph_key, buttons, apply_frames, scroll, orbit, viewport, highlight, panels, jog_panel, motion_panel, graph_dock, draw).chain())
        .run();
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>, view: Res<RobotView>, fonts: Res<UiFonts>) {
    let text = |value: &str, size: f32, color: Color| label(&fonts, value, size, color);
    commands.insert_resource(Materials {
        normal: materials.add(StandardMaterial { base_color: Color::srgb(0.62, 0.68, 0.76), perceptual_roughness: 0.7, metallic: 0.05, cull_mode: None, ..default() }),
        selected: materials.add(StandardMaterial { base_color: Color::srgb(0.98, 0.62, 0.22), emissive: LinearRgba::rgb(0.35, 0.16, 0.02), perceptual_roughness: 0.6, cull_mode: None, ..default() }),
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
    commands.spawn((DirectionalLight { illuminance: 9000.0, shadows_enabled: false, ..default() }, Transform::from_xyz(1.0, 2.0, 1.5).looking_at(Vec3::ZERO, Vec3::Y)));
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
            (Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() }, children![run_button(&fonts, RunAction::Start), run_button(&fonts, RunAction::Pause), run_button(&fonts, RunAction::Step), run_button(&fonts, RunAction::Reset), graphs_button(&fonts)]),
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
            // Motion request buttons for a preset (spawned by `motion_panel`).
            (Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() }, MotionRoot),
            // Servo-target jog rows for the selected link's joints (rebuilt by `jog_panel`).
            (Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() }, JogRoot),
            (
                Node { flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, overflow: Overflow::scroll_y(), ..default() },
                ScrollPosition::default(),
                InspectorScroll,
                children![(text("Select a link in the list or the 3D view.", 12.5, INK), Inspector)],
            )
        ],
    ));
    // Graph dock under the 3D view (filled by `graph_dock`).
    commands.spawn((
        Node { position_type: PositionType::Absolute, left: Val::Px(LEFT), right: Val::Px(RIGHT), bottom: Val::Px(0.0), height: Val::Px(DOCK), padding: UiRect::all(Val::Px(10.0)), column_gap: Val::Px(10.0), display: Display::None, border: UiRect::top(Val::Px(1.0)), ..default() },
        BackgroundColor(PANEL),
        BorderColor(Color::srgb(0.2, 0.24, 0.29)),
        GraphDock,
    ));
}

fn tab(fonts: &UiFonts, section: Section) -> impl Bundle {
    (
        Button,
        RobotAction::ShowSection { section },
        TabButton(section),
        Node { padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)), ..default() },
        BorderRadius::all(Val::Px(4.0)),
        BackgroundColor(Color::NONE),
        children![label(fonts, section.label(), 14.0, INK)],
    )
}

fn run_button(fonts: &UiFonts, action: RunAction) -> impl Bundle {
    (
        Button,
        RobotAction::Run { action },
        RunButton(action),
        Node { padding: UiRect::axes(Val::Px(12.0), Val::Px(3.0)), ..default() },
        BorderRadius::all(Val::Px(4.0)),
        BackgroundColor(Color::srgb(0.16, 0.20, 0.25)),
        children![label(fonts, action.label(), 13.0, INK)],
    )
}

/// The Graphs button: the same `RobotAction::ToggleGraphs` as key G and `system_ui` graphs:toggle.
fn graphs_button(fonts: &UiFonts) -> impl Bundle {
    (
        Button,
        RobotAction::ToggleGraphs,
        Node { padding: UiRect::axes(Val::Px(12.0), Val::Px(3.0)), margin: UiRect::left(Val::Px(8.0)), ..default() },
        BorderRadius::all(Val::Px(4.0)),
        BackgroundColor(Color::srgb(0.16, 0.20, 0.25)),
        children![label(fonts, "Graphs (G)", 13.0, INK)],
    )
}

fn label(fonts: &UiFonts, value: &str, size: f32, color: Color) -> (Text, TextFont, TextColor, TextLayout) {
    (Text::new(value), TextFont { font: fonts.regular.clone(), font_size: size, ..default() }, TextColor(color), TextLayout::new_with_linebreak(bevy::text::LineBreak::WordOrCharacter))
}

/// Takes the worker's result; spawns meshes and the link list on success.
fn receive(
    mut commands: Commands,
    old: Query<Entity, Or<(With<LinkMesh>, With<LinkRow>)>>,
    mut view: ResMut<RobotView>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<Materials>,
    root: Single<Entity, With<RobotRoot>>,
    list: Single<Entity, With<ListRoot>>,
    mut orbit: Single<&mut RobotOrbit>,
    mut redraw: EventWriter<bevy::window::RequestRedraw>,
    fonts: Res<UiFonts>,
) {
    let result = match view.rx.as_ref().map(|rx| rx.lock().unwrap_or_else(|p| p.into_inner()).try_recv()) {
        Some(Ok(result)) => result,
        Some(Err(mpsc::TryRecvError::Empty)) => {
            redraw.write(bevy::window::RequestRedraw);
            return;
        }
        Some(Err(mpsc::TryRecvError::Disconnected)) => Err(format!("{}: the loader stopped without a result", view.path.display())),
        None => return,
    };
    view.rx = None;
    // A reopened robot (REST robot_preset) replaces the previous meshes and rows.
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let started = match view.status {
        Status::Loading(t) => t,
        _ => std::time::Instant::now(),
    };
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
        orbit.focus = (lo + hi) / 2.0;
        orbit.extent = ((hi - lo).length() / 2.0).max(0.02);
    }
    orbit.home = true;
    view.triangles = loaded.geometry.iter().map(|g| g.as_ref().map_or(0, |g| g.triangles())).collect();
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
                    Node { padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)), flex_shrink: 0.0, ..default() },
                    BorderRadius::all(Val::Px(4.0)),
                    BackgroundColor(Color::NONE),
                    children![label(&fonts, &name, 13.0, INK)],
                ))
                .id()
        })
        .collect();
    commands.entity(*list).add_children(&rows);
    view.run = Some(match preset {
        Some(run) => RunController::spawn_preset(std::sync::Arc::new(run)),
        None => RunController::spawn(loaded.model.clone()),
    });
    view.model = Some(loaded.model);
    view.notes = loaded.notes;
    view.cad_link = Some(loaded.cad_link);
    view.status = Status::Loaded { seconds: started.elapsed().as_secs_f64() };
    view.ui_revision += 1;
    view.panels_ready = true;
}

fn pick_link(click: Trigger<Pointer<Click>>, links: Query<&LinkMesh>, mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>) {
    if click.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    if let Ok(link) = links.get(click.target()) {
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

fn poll_rest(mut commands: Commands, mut redraw: EventWriter<bevy::window::RequestRedraw>, mut rest: ResMut<crate::rest::Rest>, mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>) {
    let server = &mut rest.0;
    let mut shots = Vec::new();
    server.poll(|command, _, _| {
        if command.command == "screenshot" {
            let path = command.args.get("path").and_then(|p| p.as_str()).map(PathBuf::from);
            return sim_api::Outcome::Done(match path.filter(|p| p.extension().is_some_and(|e| e == "png")) {
                Some(p) => {
                    shots.push(p.clone());
                    Ok(json!({"path": p, "note": "saved once the next frame renders"}))
                }
                None => Err("screenshot needs {\"path\": \"…/file.png\"}".into()),
            });
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
fn apply_frames(mut view: ResMut<RobotView>, mut links: Query<(&LinkMesh, &mut Transform)>, mut redraw: EventWriter<bevy::window::RequestRedraw>) {
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
    let frame = view.run.as_ref().and_then(|r| r.frame());
    let Some(model) = view.model.as_ref() else { return };
    for (link, mut transform) in &mut links {
        let (p, q) = match frame.and_then(|f| f.poses.get(link.0)).and_then(|p| p.as_ref()) {
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
    mut motion: EventReader<MouseMotion>,
    mut wheel: EventReader<MouseWheel>,
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
        let want = if view.selected == Some(link.0) { &materials.selected } else { &materials.normal };
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
fn scroll(mut view: ResMut<RobotView>, mut wheel: EventReader<MouseWheel>, window: Single<&Window>, panel: Single<(&mut ScrollPosition, &ComputedNode), With<InspectorScroll>>) {
    let (mut position, node) = panel.into_inner();
    let delta = wheel.read().fold(0.0, |sum, e| sum + match e.unit {
        MouseScrollUnit::Line => e.y * 24.0,
        MouseScrollUnit::Pixel => e.y,
    });
    let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
    if delta != 0.0 && window.cursor_position().is_some_and(|p| p.x >= window.width() - RIGHT && p.y > TOP) {
        view.scroll_to = Some((position.offset_y - delta).clamp(0.0, max));
    }
    if let Some(y) = view.scroll_to.take() {
        position.offset_y = y.clamp(0.0, max);
    }
    if view.scroll != position.offset_y || view.scroll_max != max {
        view.scroll = position.offset_y;
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
) {
    let heading = match &view.preset {
        Some(p) => format!("Robot preset — {} ({})  ·  files read-only", p.label, p.id),
        None => format!("Robot — {}  ·  file read-only", view.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
    };
    if title.0 != heading {
        title.0 = heading;
    }
    let run_line = match &view.run {
        None => String::new(),
        Some(r) => {
            let time = r.frame().map_or("t —".to_string(), |f| format!("t {:.2} s · {} chunks", f.time, f.steps));
            let rtf = r.rtf().map_or(String::new(), |x| format!(" · RTF {x:.2}"));
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
            format!("{phase} · {time}{rtf} · gen {} · {} s chunks{replay}{error}{refused}", r.generation(), r.chunk_s())
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
            let pose = if view.run.as_ref().and_then(|r| r.frame()).is_some() { "simulated pose" } else { "exported assembly pose" };
            match view.preset.as_ref() {
                Some(p) => format!("{} links{missing} · loaded in {seconds:.2} s · {pose} · readiness: {}", m.links.len(), clip(p.readiness().unwrap_or("(none declared)"), 55)),
                None => format!("{} · {} links{missing} · loaded in {seconds:.2} s · {pose}", view.path.display(), m.links.len()),
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
            let button = |sign: f64, text: &str| (Button, JogButton, RobotAction::Jog { joint: joint.clone(), delta: sign * step }, Node { padding: UiRect::axes(Val::Px(9.0), Val::Px(1.0)), ..default() }, BorderRadius::all(Val::Px(4.0)), BackgroundColor(Color::srgb(0.16, 0.20, 0.25)), children![label(&fonts, text, 13.0, INK)]);
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
    let button = |commands: &mut Commands, action: RobotAction, text: &str| commands.spawn((Button, MotionButton, action, Node { padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)), ..default() }, BorderRadius::all(Val::Px(4.0)), BackgroundColor(Color::srgb(0.16, 0.20, 0.25)), children![label(&fonts, text, 12.0, INK)])).id();
    if view.preset.is_some() && !*shown {
        let header = commands.spawn(label(&fonts, &format!("Motion — {}", robot_motion::LABEL), 11.5, MUTED)).id();
        let row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() }).id();
        for (_, text, request) in motion_buttons() {
            let b = commands.spawn((Button, MotionButton, RobotAction::Motion { request }, Node { padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)), ..default() }, BorderRadius::all(Val::Px(4.0)), BackgroundColor(Color::srgb(0.16, 0.20, 0.25)), children![label(&fonts, text, 12.0, INK)])).id();
            commands.entity(row).add_child(b);
        }
        let line = commands.spawn((label(&fonts, "", 11.5, INK), MotionText)).id();
        // Save recording: the same RobotAction::SaveRecording as system_ui recording:save and REST robot_save_recording.
        let save_row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).id();
        let save = commands.spawn((Button, MotionButton, RobotAction::SaveRecording { path: None, note: None }, Node { padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)), ..default() }, BorderRadius::all(Val::Px(4.0)), BackgroundColor(Color::srgb(0.16, 0.20, 0.25)), children![label(&fonts, "Save recording", 12.0, INK)])).id();
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
    let mut t = format!("SOURCE BLOCK (verbatim)\nfile: {}\nexported: {}\ncad_sha256: {}\ncollision_ray_backend: {}\n", field("file"), field("exported"), field("cad_sha256"), field("collision_ray_backend"));
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

/// Floor grid and the selected link's centre of mass.
/// Key G: the same `RobotAction::ToggleGraphs` as the Graphs button and `system_ui` graphs:toggle.
fn graph_key(keys: Res<ButtonInput<KeyCode>>, mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>) {
    if keys.just_pressed(KeyCode::KeyG) {
        view.run_message = dispatch(&mut view, &mut orbit, RobotAction::ToggleGraphs).err();
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
    mut redraw: EventWriter<bevy::window::RequestRedraw>,
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
    let stamp = format!("{:?}|{}|{}|{}|{:?}|{:?}", view.selected, h.generation(), h.frames(), run.graphs_mode(), h.window(), run.frame().map(|f| (f.time, f.steps)));
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
    let num = |x: f64| if x == 0.0 || (x.abs() >= 1e-3 && x.abs() < 1e4) { format!("{x:.4}") } else { format!("{x:.3e}") };
    for (slot, c) in charts.iter().enumerate() {
        while handles.len() <= slot {
            handles.push(images.add(crate::chart::blank_image()));
        }
        let traces: Vec<(&[[f64; 2]], [u8; 3])> = c.traces.iter().enumerate().map(|(i, t)| (t.points.as_slice(), crate::chart::COLORS[i % crate::chart::COLORS.len()])).collect();
        let (pixels, range, window) = crate::chart::rasterize_span(&traces, Some(crate::robot_graphs::WINDOW_S));
        let drawable = c.traces.iter().map(|t| t.points.len()).sum::<usize>() >= 2;
        if let Some(image) = images.get_mut(&handles[slot]) {
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
            let plot = commands.spawn((Node { flex_grow: 1.0, min_height: Val::Px(60.0), border: UiRect::all(Val::Px(1.0)), ..default() }, BorderColor(Color::srgb(0.2, 0.24, 0.29)), ImageNode::new(handles[slot].clone()))).id();
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
                let swatch = commands.spawn((Node { width: Val::Px(9.0), height: Val::Px(9.0), flex_shrink: 0.0, ..default() }, BackgroundColor(color), BorderRadius::all(Val::Px(2.0)))).id();
                let line = commands.spawn(text(&format!("{}: {value}  ·  {source}", t.name), 10.5, color)).id();
                let row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(5.0), align_items: AlignItems::Center, ..default() }).add_children(&[swatch, line]).id();
                commands.entity(card).add_child(row);
            }
        }
        commands.entity(entity).add_child(card);
    }
}

fn draw(view: Res<RobotView>, mut gizmos: Gizmos) {
    let Some(model) = &view.model else { return };
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
