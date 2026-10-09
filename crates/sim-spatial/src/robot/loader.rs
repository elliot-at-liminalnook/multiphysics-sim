//! The robot loader: a `.simrobot.json` file or a preset scene's `robot`
//! through the shared `PhysicalModel` loader, plus triangulation and file notes.
use super::*;

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
    /// `--robot FILE` only (set by the reload worker, `robot::source::check`):
    /// the controller binding beside the file (`controller_binding::binding_path_for`):
    /// None when there is none, `Err("controller binding <path>: …")` when it
    /// did not load (the run then fails with it, never falling back to the hold
    /// run), else the drive session to run. Always None for presets.
    pub controlled: Option<Result<std::sync::Arc<crate::robot::run::ControlledRun>, String>>,
    /// `--robot FILE` only, without a controller binding: the robot project
    /// the file belongs to (`sim_runtime::robot_project::find_for_model`),
    /// whose system Robot mode runs; `Err` names a project whose system or
    /// model does not load (the run then fails with it). None otherwise.
    pub composed: Option<Result<std::sync::Arc<crate::robot::run::ComposedRun>, String>>,
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

/// The shared physical (v3+) loader plus triangulation, file notes and the
/// CAD link status (which hashes the CAD file); a planar (v2) file is refused
/// here by `PhysicalModel::parse` ([`load_file`] / [`load_file_bytes`] accept
/// both and are what `--robot FILE` uses). Errors name the path.
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

/// The `--robot FILE` loader (the first open and every reload, `robot_source`):
/// a JSON object whose version (`simrobot_version`, the shared rule) is below
/// `FIRST_PHYSICAL_VERSION` is a planar (v2) summary, read as sim-phenomena's
/// `CadModel` from the same bytes (`planar::load_bytes`); anything else
/// goes through [`load_bytes`] (`PhysicalModel`), so its errors are unchanged.
pub fn load_file_bytes(path: &Path, bytes: &[u8]) -> Result<FileModel, String> {
    if let Ok(raw @ Value::Object(_)) = serde_json::from_slice::<Value>(bytes) {
        if sim_domain_robot::model::simrobot_version(&raw) < sim_domain_robot::model::FIRST_PHYSICAL_VERSION {
            return planar::load_bytes(path, bytes, &raw).map(|p| FileModel::Planar(Box::new(p)));
        }
    }
    load_bytes(path, bytes).map(|l| FileModel::Physical(Box::new(l)))
}
/// [`load_file_bytes`] on the file at `path` (`--validate-only` for either version).
pub fn load_file(path: &Path) -> Result<FileModel, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    load_file_bytes(path, &bytes)
}

/// `robot_state.format.name` for a physical model (a v3+ file or a preset
/// scene's robot): "physical v{version}", the file's own version (3 or later).
pub fn physical_format_name(version: u32) -> String {
    format!("physical v{version}")
}
/// Shown for a planar file's pose (`robot_state.pose`).
pub const PLANAR_POSE: &str = "planar v2: outlines from the planar run thread's latest frame (CadRobot::outlines, see planar.frame); t = 0 of each generation is the CAD pose";

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
    Loaded { model, geometry, notes, cad_link, controlled: None, composed: None }
}

/// The robot project a physical `--robot FILE` (`path`) belongs to, read
/// for Robot mode: its system document and the jog plan over it (reads
/// files: call off the UI thread). None when the file is no project's model.
pub fn load_composed(path: &Path) -> Option<Result<std::sync::Arc<crate::robot::run::ComposedRun>, String>> {
    let project = sim_runtime::robot_project::find_for_model(path)?;
    let system = project.system();
    Some((|| {
        let document = sim_system::SystemStore::new(&system).load().map_err(|e| format!("{}: {e}", system.display()))?;
        let base = system.parent().unwrap_or(Path::new(".")).to_path_buf();
        let (robot_instance, jog) = sim_runtime::teleop::plan_for(&document, &base)?;
        Ok(std::sync::Arc::new(crate::robot::run::ComposedRun { project: project.path.clone(), system: system.clone(), base, document, robot_instance, jog, drive: Default::default() }))
    })().map_err(|e: String| format!("robot project {}: {e}", project.path.display())))
}

/// The controller binding beside a physical `--robot FILE` (`path`), loaded
/// for `model` (reads the binding, its drive profile and hashes the script:
/// call off the UI thread). None when no binding file exists.
pub fn load_controller(path: &Path, model: &PhysicalModel) -> Option<Result<std::sync::Arc<crate::robot::run::ControlledRun>, String>> {
    let binding = sim_runtime::controller_binding::binding_path_for(path);
    match std::fs::metadata(&binding) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => return Some(Err(format!("controller binding {}: {e}", binding.display()))),
        Ok(_) => {}
    }
    Some(
        sim_runtime::controller_binding::load(&binding, model)
            .map(|c| std::sync::Arc::new(crate::robot::run::ControlledRun::new(path, model.clone(), c)))
            .map_err(|e| format!("controller binding {}: {e}", binding.display())),
    )
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
