//! CAD mode's display state (cad-views-export): one [`CadDisplay`]
//! resource holding RoboCAD's display mode, grid, build plate, view cube,
//! high contrast and the section tool. Display only: nothing here writes
//! geometry or sends an edit (the exact section is a read), and drawing is
//! in SimSync (material and mesh swaps on the drawn bodies) and Present
//! (lines, quads, lights, the toolbar and the view cube).
//!
//! - [`section`]: plane maths, clipping display triangles, the preview cache
//!   (clipped copies built on `Pool::Compute`) and the exact section job.
//! - `draw`: display modes (materials), display edges, the grid, the build
//!   plate, the section plane and outlines, render lights, high contrast.
//! - `ui`: the display toolbar and the view cube (kit widgets).
//! - `entry`: the toolbar's section offset field (the Section tool's Tab
//!   offset), and why R, Tab and the plane drag are not bound natively.
//!
//! **Decisions** (native-viewer.md "Shared camera and CAD views"):
//! - **Grid**: drawn as retained gizmo lines as RoboCAD's `_draw_grid` does
//!   (10 mm step, ±20 steps on the model's XY plane, every 5th line major,
//!   red X, green Y and blue Z axes), not with `bevy_dev_tools`'
//!   `InfiniteGrid`. That grid is infinite and fades with camera distance
//!   (RoboCAD's is a fixed ±200 mm square), marks every 10th line major
//!   (RoboCAD: every 5th), colours only X and Z axes, would need the
//!   `bevy_dev_tools` feature (bevy_diagnostic, bevy_state's dev tooling
//!   and an extra render pipeline in every mode) and draws in every camera
//!   that sees its entity; the section clip could not cut it either.
//! - **Section default plane**: RoboCAD's `SectionTool.activate`: XZ
//!   (`Plane.xz`) through the drawn bodies' bounds centre in Y (0 with
//!   nothing drawn), not the active plane.
//! - **High contrast**: the 3D view's background (RoboCAD's 0.98, 0.98,
//!   0.99), grid and edge colours; RoboCAD's stylesheet swap has no native
//!   counterpart yet (the kit's tokens are constants).
use crate::app::actions::{Call, Spec, spec};
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::cad::actions::{CAD, CadAction, Cx};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{SectionCurves, SectionQuery};
use std::sync::Arc;

mod draw;
pub(in crate::cad) mod entry;
pub(in crate::cad) mod section;
#[cfg(test)]
mod tests;
mod ui;

/// RoboCAD's display modes (`ui/viewport.py` `display_mode`; the
/// view-state schema's `display_mode`), in its `view.mode_next` order.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    Shaded,
    #[default]
    ShadedEdges,
    Wireframe,
    Xray,
    Matcap,
    Render,
}

impl DisplayMode {
    /// `Viewport.MODES`, in order.
    pub const ALL: [DisplayMode; 6] = [DisplayMode::Shaded, DisplayMode::ShadedEdges, DisplayMode::Wireframe, DisplayMode::Xray, DisplayMode::Matcap, DisplayMode::Render];
    /// RoboCAD's name (`shaded_edges`).
    pub fn name(self) -> &'static str {
        match self {
            DisplayMode::Shaded => "shaded",
            DisplayMode::ShadedEdges => "shaded_edges",
            DisplayMode::Wireframe => "wireframe",
            DisplayMode::Xray => "xray",
            DisplayMode::Matcap => "matcap",
            DisplayMode::Render => "render",
        }
    }
    /// As the toolbar shows it.
    pub fn label(self) -> &'static str {
        match self {
            DisplayMode::Shaded => "Shaded",
            DisplayMode::ShadedEdges => "Shaded + edges",
            DisplayMode::Wireframe => "Wireframe",
            DisplayMode::Xray => "X-ray",
            DisplayMode::Matcap => "Matcap",
            DisplayMode::Render => "Render",
        }
    }
    /// RoboCAD's `next_display_mode`: the next in `MODES`, wrapping.
    pub fn next(self) -> DisplayMode {
        let i = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
    /// Whether B-rep edges are drawn (RoboCAD: shaded_edges, wireframe, xray).
    pub fn edges(self) -> bool {
        matches!(self, DisplayMode::ShadedEdges | DisplayMode::Wireframe | DisplayMode::Xray)
    }
}

/// A section plane as RoboCAD writes it (`Plane.to_json`: model mm, unit
/// normal and x axis).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SectionPlane {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub x_axis: [f64; 3],
}

/// The section tool's state (the view-state schema's `section`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Section {
    pub enabled: bool,
    pub plane: Option<SectionPlane>,
}

/// What an exact section was asked for: the node, RoboCAD's revision, the
/// plane and its query form.
#[derive(Clone, Debug, PartialEq)]
pub struct ExactKey {
    pub node: String,
    pub revision: u64,
    pub plane: SectionPlane,
    pub query: SectionQuery,
}

/// The exact section (`GET /nodes/{id}/section`): the request and the last
/// answer with the key it was asked for. Drawn only while the answer is
/// for the request, RoboCAD's shown revision and the current plane
/// ([`ExactSection::drawn`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExactSection {
    pub request: Option<ExactKey>,
    pub result: Option<(ExactKey, Result<Arc<SectionCurves>, String>)>,
}

impl ExactSection {
    /// The answer to draw: for the request, at `revision`, with the section
    /// on and on the same plane. Anything older is never drawn.
    pub fn drawn(&self, section: &Section, revision: u64) -> Option<&Arc<SectionCurves>> {
        let (key, result) = self.result.as_ref()?;
        let current = self.request.as_ref() == Some(key) && key.revision == revision && section.enabled && section.plane == Some(key.plane);
        current.then_some(result.as_ref().ok()).flatten()
    }
    /// The request has no answer yet.
    pub fn pending(&self) -> bool {
        self.request.is_some() && self.result.as_ref().map(|(k, _)| k) != self.request.as_ref()
    }
}

/// The display state (display only): RoboCAD's defaults.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct CadDisplay {
    pub mode: DisplayMode,
    pub grid: bool,
    pub build_plate: bool,
    pub high_contrast: bool,
    pub view_cube: bool,
    /// The view-state schema's `comment_pins` (pins are drawn by cad-organize).
    pub comment_pins: bool,
    pub section: Section,
    /// The exact section asked for and its answer (`cad_section` exact).
    pub exact: ExactSection,
}
impl Default for CadDisplay {
    fn default() -> Self {
        Self { mode: DisplayMode::ShadedEdges, grid: true, build_plate: false, high_contrast: false, view_cube: true, comment_pins: true, section: Section::default(), exact: ExactSection::default() }
    }
}

/// RoboCAD's grid step (`Viewport.grid_step`, mm) and half extent in steps.
pub const GRID_STEP_MM: f64 = 10.0;
pub const GRID_STEPS: i32 = 20;
/// RoboCAD's build plate (`toggle_build_plate`: 220 × 220 mm).
pub const BUILD_PLATE_MM: [f64; 2] = [220.0, 220.0];

/// A display setting `cad_display` toggles.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplaySetting {
    Grid,
    BuildPlate,
    HighContrast,
    ViewCube,
    CommentPins,
}

impl DisplaySetting {
    pub const ALL: [DisplaySetting; 5] = [DisplaySetting::Grid, DisplaySetting::BuildPlate, DisplaySetting::HighContrast, DisplaySetting::ViewCube, DisplaySetting::CommentPins];
    pub fn name(self) -> &'static str {
        match self {
            DisplaySetting::Grid => "grid",
            DisplaySetting::BuildPlate => "build_plate",
            DisplaySetting::HighContrast => "high_contrast",
            DisplaySetting::ViewCube => "view_cube",
            DisplaySetting::CommentPins => "comment_pins",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            DisplaySetting::Grid => "Grid",
            DisplaySetting::BuildPlate => "Build plate",
            DisplaySetting::HighContrast => "High contrast",
            DisplaySetting::ViewCube => "View cube",
            DisplaySetting::CommentPins => "Comment pins",
        }
    }
    fn slot(self, d: &mut CadDisplay) -> &mut bool {
        match self {
            DisplaySetting::Grid => &mut d.grid,
            DisplaySetting::BuildPlate => &mut d.build_plate,
            DisplaySetting::HighContrast => &mut d.high_contrast,
            DisplaySetting::ViewCube => &mut d.view_cube,
            DisplaySetting::CommentPins => &mut d.comment_pins,
        }
    }
    /// The setting's current value.
    pub fn get(self, d: &CadDisplay) -> bool {
        match self {
            DisplaySetting::Grid => d.grid,
            DisplaySetting::BuildPlate => d.build_plate,
            DisplaySetting::HighContrast => d.high_contrast,
            DisplaySetting::ViewCube => d.view_cube,
            DisplaySetting::CommentPins => d.comment_pins,
        }
    }
}

/// `cad_display`'s arguments: each given setting is set; `next` cycles the
/// display mode (RoboCAD's `view.mode_next`, Z); `toggle` flips one setting.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct DisplayArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<DisplayMode>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub next: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_plate: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_contrast: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_cube: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_pins: Option<bool>,
    /// Flip this setting (RoboCAD's `toggle_grid`, `toggle_build_plate`,
    /// `toggle_high_contrast`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toggle: Option<DisplaySetting>,
}

/// A section axis: RoboCAD's `{axis, offset}` plane form (x → `Plane.yz`,
/// y → `Plane.xz`, z → `Plane.xy`, at `offset` mm along the axis).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SectionAxis {
    X,
    Y,
    Z,
}

impl SectionAxis {
    pub const ALL: [SectionAxis; 3] = [SectionAxis::X, SectionAxis::Y, SectionAxis::Z];
    pub fn name(self) -> &'static str {
        match self {
            SectionAxis::X => "x",
            SectionAxis::Y => "y",
            SectionAxis::Z => "z",
        }
    }
    pub fn index(self) -> usize {
        match self {
            SectionAxis::X => 0,
            SectionAxis::Y => 1,
            SectionAxis::Z => 2,
        }
    }
}

/// `cad_section`'s arguments. Nothing given toggles the section (RoboCAD's
/// `view.section`, Ctrl+Shift+X); a plane, axis, offset or rotate also
/// turns it on unless `enabled` says otherwise.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct SectionArgs {
    /// On, off; absent with nothing else toggles (RoboCAD's Ctrl+Shift+X).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane: Option<SectionPlane>,
    /// RoboCAD's `{axis, offset}` plane (offset in mm, 0 when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<SectionAxis>,
    /// With `axis`: the offset along it. Alone: move the current plane
    /// along its normal by this many mm (the Section tool's Tab offset).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
    /// Turn the plane 90° about Z (the Section tool's R).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rotate: bool,
    /// Read node `exact`'s exact B-rep section on the (resulting) plane
    /// (`GET /nodes/{id}/section`, on a job).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact: Option<String>,
}

/// What `cad_section` reads from the document: RoboCAD's revision, the
/// drawn bodies' bounds (mm) and the active plane node, if any.
#[derive(Clone, Debug, Default)]
pub struct SectionContext {
    pub revision: u64,
    pub bounds: Option<([f64; 3], [f64; 3])>,
    /// The active plane when it is a plane node with a frame: (id, plane).
    pub plane_node: Option<(String, SectionPlane)>,
    /// Nodes in the shown tree (None: no tree shown yet, any id passes).
    pub nodes: Option<Vec<String>>,
}

/// Apply `cad_display`'s arguments (all checked first).
pub fn apply_display(d: &mut CadDisplay, a: &DisplayArgs) -> Result<(), String> {
    let explicit = [(DisplaySetting::Grid, a.grid), (DisplaySetting::BuildPlate, a.build_plate), (DisplaySetting::HighContrast, a.high_contrast), (DisplaySetting::ViewCube, a.view_cube), (DisplaySetting::CommentPins, a.comment_pins)];
    if a.mode.is_none() && !a.next && a.toggle.is_none() && explicit.iter().all(|(_, v)| v.is_none()) {
        return Err("cad_display needs a setting: mode, next, toggle, grid, build_plate, high_contrast, view_cube or comment_pins".into());
    }
    if a.mode.is_some() && a.next {
        return Err("cad_display: give mode or next, not both".into());
    }
    if let Some(t) = a.toggle {
        if explicit.iter().any(|(s, v)| *s == t && v.is_some()) {
            return Err(format!("cad_display: toggle {0} and {0} together; give one", t.name()));
        }
    }
    if let Some(m) = a.mode {
        d.mode = m;
    }
    if a.next {
        d.mode = d.mode.next();
    }
    for (setting, value) in explicit {
        if let Some(v) = value {
            *setting.slot(d) = v;
        }
    }
    if let Some(t) = a.toggle {
        let slot = t.slot(d);
        *slot = !*slot;
    }
    Ok(())
}

/// Apply `cad_section`'s arguments (all checked first). Ok(true) when an
/// exact section was asked for.
pub fn apply_section(d: &mut CadDisplay, a: &SectionArgs, cx: &SectionContext) -> Result<bool, String> {
    let given = a.plane.is_some() || a.axis.is_some() || a.offset.is_some() || a.rotate;
    if a.plane.is_some() && a.axis.is_some() {
        return Err("cad_section: give plane or axis, not both".into());
    }
    if let Some(o) = a.offset.filter(|o| !o.is_finite()) {
        return Err(format!("cad_section: offset {o} is not a finite number of mm"));
    }
    if a.enabled.is_none() && !given && a.exact.is_none() {
        // RoboCAD's toggle_section: off when on, else the Section tool's activate.
        let on = !d.section.enabled;
        if on && d.section.plane.is_none() {
            d.section.plane = Some(default_plane(cx));
        }
        d.section.enabled = on;
        return Ok(false);
    }
    let mut plane = match (a.plane, a.axis) {
        (Some(p), _) => Some(p.validated()?),
        (None, Some(axis)) => Some(SectionPlane::on_axis(axis, a.offset.unwrap_or(0.0))),
        (None, None) => d.section.plane,
    };
    if a.axis.is_none() {
        if let Some(o) = a.offset {
            let p = plane.ok_or("cad_section: offset moves the current plane, and there is none; give plane or axis")?;
            plane = Some(p.moved(o));
        }
    }
    if a.rotate {
        let p = plane.ok_or("cad_section: rotate turns the current plane, and there is none; give plane or axis")?;
        plane = Some(p.rotated());
    }
    let enabled = a.enabled.unwrap_or(true);
    if enabled && plane.is_none() {
        plane = Some(default_plane(cx));
    }
    let request = match &a.exact {
        None => None,
        Some(node) => {
            if !enabled {
                return Err("cad_section: an exact section needs the section on".into());
            }
            if cx.nodes.as_ref().is_some_and(|n| !n.iter().any(|id| id == node)) {
                return Err(format!("cad_section: no node {node} in the shown tree"));
            }
            let p = plane.expect("an enabled section has a plane");
            let query = exact_query(&p, cx)?;
            Some(ExactKey { node: node.clone(), revision: cx.revision, plane: p, query })
        }
    };
    d.section = Section { enabled, plane };
    let asked = request.is_some();
    if let Some(key) = request {
        d.exact.request = Some(key);
    }
    Ok(asked)
}

/// The plane query RoboCAD's `/section` route accepts for `p`: a named
/// plane through the origin, else the active plane node when it is `p`.
pub fn exact_query(p: &SectionPlane, cx: &SectionContext) -> Result<SectionQuery, String> {
    if let Some(q) = SectionQuery::named(p.origin, p.normal, 1e-6) {
        return Ok(q);
    }
    if let Some((id, node)) = &cx.plane_node {
        if node.same_set(p) {
            return Ok(SectionQuery::Node(id.clone()));
        }
    }
    Err("RoboCAD's GET /nodes/{id}/section reads plane=xy|xz|yz (through the origin) or a plane node id (api.py passes the query string to ArgConverter.plane), and this section plane is neither: use axis x, y or z with offset 0, or make the active plane a plane node at it".into())
}

/// RoboCAD's `SectionTool.activate` default: `Plane.xz` through the drawn
/// bodies' bounds centre in Y (0 with nothing drawn).
pub fn default_plane(cx: &SectionContext) -> SectionPlane {
    let y = cx.bounds.map_or(0.0, |(lo, hi)| (lo[1] + hi[1]) / 2.0);
    SectionPlane::on_axis(SectionAxis::Y, y)
}

/// `cad_state`'s `display` key (the lead's `actions::state_json` calls it).
pub(in crate::cad) fn state_json(d: &CadDisplay) -> Value {
    let key = |k: &ExactKey| json!({"node": k.node, "revision": k.revision, "plane": k.plane, "query": k.query.as_str()});
    let result = d.exact.result.as_ref().map(|(k, r)| match r {
        Ok(s) => json!({"key": key(k), "ok": true, "polylines": s.polylines, "dropped": s.dropped}),
        Err(e) => json!({"key": key(k), "ok": false, "error": e}),
    });
    json!({
        "mode": d.mode,
        "modes": DisplayMode::ALL,
        "grid": d.grid,
        "grid_step_mm": GRID_STEP_MM,
        "build_plate": d.build_plate,
        "build_plate_mm": BUILD_PLATE_MM,
        "high_contrast": d.high_contrast,
        "view_cube": d.view_cube,
        "comment_pins": d.comment_pins,
        "section": {"enabled": d.section.enabled, "plane": d.section.plane, "exact": {"request": d.exact.request.as_ref().map(key), "pending": d.exact.pending(), "result": result}},
        "note": "display only: RoboCAD's document and its own window's view are unchanged",
    })
}

/// `CadDisplay` and `CadSection`, from any entry point.
pub(in crate::cad) fn handle(action: &CadAction, _call: &mut Call, cx: &mut Cx) -> Outcome {
    let context = context(cx);
    let Some(display) = cx.display.as_deref_mut() else {
        return Outcome::Done(Err("CAD mode's display state belongs to its 3D view, and this window has none".into()));
    };
    let result = match action {
        CadAction::CadDisplay(args) => apply_display(display, args).map(|()| state_json(display)),
        CadAction::CadSection(args) => apply_section(display, args, &context).map(|asked| {
            let mut state = state_json(display);
            if asked {
                state["message"] = json!("The exact section is read from RoboCAD on a job; cad_state.display.section.exact shows it (pending, then result).");
            }
            state
        }),
        _ => Err("not a display action".into()),
    };
    Outcome::Done(result)
}

/// What `cad_section` reads, from the handler's context.
fn context(cx: &Cx) -> SectionContext {
    let revision = cx.doc.doc_key.as_ref().map_or(0, |k| k.1);
    let bounds = cx.meshes.as_deref().and_then(section::model_bounds);
    let plane_node = match (&cx.plane.plane, cx.plane.frame()) {
        (Some(crate::cad::ActivePlane::Node { id, .. }), Ok(Some(f))) => Some((id.clone(), SectionPlane { origin: f.origin, normal: f.normal, x_axis: f.x_axis })),
        _ => None,
    };
    let nodes = cx.doc.doc.as_ref().map(|d| d.nodes.iter().map(|n| n.id.clone()).collect());
    SectionContext { revision, bounds, plane_node, nodes }
}

/// CadPlugin: this part's systems and resources (inserted on entering CAD
/// mode; `CadDisplay` is removed by `cad::clear`, the caches by OnExit here).
pub(in crate::cad) fn build(app: &mut App) {
    app.add_systems(OnEnter(ModeScope::Cad), |mut commands: Commands| {
        commands.insert_resource(CadDisplay::default());
        commands.insert_resource(section::Derived::default());
        commands.insert_resource(section::ExactJob::default());
        commands.insert_resource(draw::DisplayMaterials::default());
        commands.insert_resource(draw::DisplayEdges::default());
        commands.insert_resource(entry::SectionEntry::default());
    })
    .add_systems(OnExit(ModeScope::Cad), |mut commands: Commands| {
        commands.remove_resource::<section::Derived>();
        commands.remove_resource::<section::ExactJob>();
        commands.remove_resource::<draw::DisplayMaterials>();
        commands.remove_resource::<draw::DisplayEdges>();
        commands.remove_resource::<entry::SectionEntry>();
    })
    .add_systems(
        Update,
        (
            ui::cube_press.in_set(ViewerSet::Input),
            // As the saved views panel's fields: after the name field and the
            // inspector's editors (which reset `CadInputFocus`), before the
            // numeric bar's Tab, the chord gate and every CAD key reader, which
            // honour the focus set here.
            entry::input
                .after(crate::app::actions::serve)
                .after(crate::cad::panel::name_entry)
                .after(crate::cad::inspector::editor_entry)
                .before(crate::cad::numeric::entry)
                .before(crate::cad::keys::gate)
                .before(crate::cad::keys::keys)
                .in_set(ViewerSet::Input),
            section::exact_jobs.in_set(ViewerSet::JobResults),
            // After the bodies' meshes and materials are set for this frame (`mesh::sync`, `mesh::highlight`).
            (section::preview, draw::materials, draw::edges_sync).chain().after(crate::cad::mesh::highlight).in_set(ViewerSet::SimSync),
            (draw::lines, draw::quads, draw::lights, ui::toolbar).in_set(ViewerSet::Present),
        )
            .run_if(in_state(ViewerMode::Cad)),
    );
}

/// This part's REST commands (appended to `CadAction::commands`).
pub(in crate::cad) fn specs() -> Vec<Spec> {
    vec![
        spec("cad_display", CAD, json!({"mode": "wireframe"}), "CAD mode: the display state, display only (never a document edit; RoboCAD's own window is unchanged): mode (shaded | shaded_edges | wireframe | xray | matcap | render; RoboCAD's view.mode.*), next (RoboCAD's view.mode_next, Z: the next mode in that order, wrapping), grid (RoboCAD's 10 mm grid on XY, ±200 mm, view.grid), build_plate (RoboCAD's 220 × 220 mm plate with overhang shading, view.build_plate), high_contrast (the 3D view's light background, grid and edge colours, view.high_contrast), view_cube, comment_pins (true or false each), or toggle (grid | build_plate | high_contrast | view_cube | comment_pins: flip it). Settings given together apply together; mode with next, or toggle with the same setting, is refused. Answers the display state (also cad_state.display)."),
        spec("cad_section", CAD, json!({"axis": "z", "offset": 5.0}), "CAD mode: the section tool, display only (never a document edit). Nothing given toggles it (RoboCAD's view.section, Ctrl+Shift+X; turned on without a plane it starts on XZ through the drawn bodies' centre, as RoboCAD's Section tool). plane {origin, normal, x_axis} (mm, RoboCAD's model frame; unit, perpendicular axes, as a saved view's) or axis x | y | z with offset mm (RoboCAD's {axis, offset}: Plane.yz, xz or xy at that offset) sets the plane; offset alone moves the current plane along its normal (the Section tool's Tab; the display toolbar's offset field sends it); rotate turns it 90° about Z (the tool's R); enabled true or false. The preview cuts the drawn triangles (clipped copies built off the UI thread; the side the normal points to is removed, as RoboCAD's clip plane) and draws their cut outline. exact: a node id whose exact B-rep section is read from RoboCAD's GET /nodes/{id}/section on a job, cached by (node, RoboCAD revision, plane) and drawn only while all three are current; RoboCAD's route takes plane=xy|xz|yz through the origin or a plane node id, so an exact section is refused, naming why, on any other plane. Answers the display state (cad_state.display.section)."),
    ]
}

/// This part's `system_ui` controls: (id, label, action, ready).
pub(in crate::cad) fn controls(cx: &Cx) -> Vec<(String, String, CadAction, Result<(), String>)> {
    controls_of(cx.doc, cx.display.as_deref(), &context(cx))
}

/// The controls for `doc`, the display state (None: no 3D view) and what
/// `cad_section` reads (`context`): the toolbar's buttons write the same
/// actions.
pub(in crate::cad) fn controls_of(doc: &crate::cad::document::CadDocument, shown: Option<&CadDisplay>, context: &SectionContext) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let ready: Result<(), String> = if shown.is_some() { Ok(()) } else { Err("CAD mode's display state belongs to its 3D view, and this window has none".into()) };
    let display = |args: DisplayArgs| CadAction::CadDisplay(args);
    let mut out = Vec::new();
    for m in DisplayMode::ALL {
        out.push((format!("cad:display:mode_{}", m.name()), format!("Display: {}", m.label()), display(DisplayArgs { mode: Some(m), ..default() }), ready.clone()));
    }
    out.push(("cad:display:next".into(), "Next display mode".into(), display(DisplayArgs { next: true, ..default() }), ready.clone()));
    for s in DisplaySetting::ALL {
        let state = shown.map_or("", |d| if s.get(d) { " (on)" } else { " (off)" });
        out.push((format!("cad:display:{}", s.name()), format!("Toggle {}{state}", s.label().to_lowercase()), display(DisplayArgs { toggle: Some(s), ..default() }), ready.clone()));
    }
    let section = |args: SectionArgs| CadAction::CadSection(args);
    let on = shown.is_some_and(|d| d.section.enabled);
    out.push(("cad:section:toggle".into(), format!("Section analysis ({})", if on { "on" } else { "off" }), section(SectionArgs::default()), ready.clone()));
    let bounds = context.bounds;
    for axis in SectionAxis::ALL {
        // Through the drawn bodies' centre along the axis (0 with nothing drawn).
        let offset = bounds.map_or(0.0, |(lo, hi)| (lo[axis.index()] + hi[axis.index()]) / 2.0);
        out.push((format!("cad:section:{}", axis.name()), format!("Section across {} at {offset:.3} mm", axis.name().to_uppercase()), section(SectionArgs { axis: Some(axis), offset: Some(offset), ..default() }), ready.clone()));
    }
    let has_plane = shown.is_some_and(|d| d.section.plane.is_some());
    let need_plane = |r: &Result<(), String>| r.clone().and_then(|()| if has_plane { Ok(()) } else { Err("no section plane yet: turn the section on first".to_string()) });
    out.push(("cad:section:rotate".into(), "Rotate the section plane 90° about Z".into(), section(SectionArgs { rotate: true, ..default() }), need_plane(&ready)));
    let first = doc.selected_nodes().into_iter().next();
    let exact_ready = need_plane(&ready).and_then(|()| {
        let d = shown.expect("ready implies a display");
        if !d.section.enabled {
            return Err("the section is off".to_string());
        }
        if first.is_none() {
            return Err("select the node to section".to_string());
        }
        if !doc.connected() {
            return Err("not connected to RoboCAD".to_string());
        }
        exact_query(&d.section.plane.expect("checked"), context).map(|_| ())
    });
    let node = first.clone().unwrap_or_default();
    let label = match &first {
        Some(id) => format!("Exact section of {}", doc.node_name(id)),
        None => "Exact section of the selected node".into(),
    };
    out.push(("cad:section:exact".into(), label, section(SectionArgs { exact: Some(node), ..default() }), exact_ready));
    out
}
