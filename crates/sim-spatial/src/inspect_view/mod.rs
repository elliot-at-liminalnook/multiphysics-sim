//! Inspect mode's spatial assembly view: the [`SpatialScene`] resource, the
//! [`SpatialViewerPlugin`] that draws it, and the 3D scene the builder (Build)
//! and the lesson pages (Lessons) draw into. The scene and its parts
//! (`scene`), the camera's data for the shared camera module (`camera`) and
//! the inspection panels (`ui`) are children; the crate root re-exports the
//! names other modules use.
mod camera;
pub(crate) mod projection;
mod scene;
mod ui;

use crate::app::{self, ModeScope, SpatialScreen, ViewerMode, ViewerSet};
use crate::{animation, inspect, linked, notes, physics_view, ui_kit, view};
use bevy::prelude::*;
pub(crate) use camera::{spatial_rules, sync_camera};
use crate::camera::CameraSet;
use scene::{draw_guides, setup_scene};
pub(crate) use scene::{spawn_parts, update_parts};
use sim_inspect::selection::{SelectionDetails, SelectionTarget};
use sim_inspect::{
    InspectionError, SystemDescription,
    spatial::{SpatialCommand, SpatialDescription, SpatialShape, SpatialViewState},
};
use std::collections::BTreeMap;
use ui::{buttons, scroll_inspector, setup_ui, update_layout, update_ui};

const LEFT: f32 = 230.0;
const RIGHT: f32 = 320.0;
const TOP: f32 = 116.0;
const BOTTOM: f32 = 66.0;

#[derive(Resource)]
pub struct SpatialScene {
    pub description: SystemDescription,
    pub spatial: SpatialDescription,
    pub state: SpatialViewState,
    /// The selection the view shows: a display projection, written only by
    /// [`SpatialScene::set_selection`] (and cleared by `replace`). Never the
    /// owner: Inspect's selection is the shared `crate::selection::Selection`
    /// (projected by `projection::project_selection`), the builder's and the
    /// lesson page's are their own modes' projections.
    pub shown: SelectionTarget,
    pub(crate) details: SelectionDetails,
    pub animation: Option<sim_inspect::animation::AnimationDescription>,
    pub(crate) live: animation::LivePresentation,
    pub compact: bool,
    pub(crate) parts_visible: bool,
    pub(crate) annotations: Option<sim_inspect::annotations::native::Store>,
    pub(crate) note_navigation: u64,
    pub(crate) note_hover: SelectionTarget,
    pub(crate) note_pointer_hover: SelectionTarget,
    pub(crate) note_error: Option<String>,
    /// Components drawn translucent (outside the level being built).
    pub ghost: std::collections::BTreeSet<String>,
    /// Build mode replaces the parts list with the builder panel.
    pub builder_mode: bool,
    /// Height of the build-mode graph dock above the status bar (0 = hidden).
    pub builder_dock: f32,
    /// Width of the build-mode schematic pane beside the inspector.
    pub builder_side: f32,
    /// Learn mode: the 3D view is drawn inside a lesson's scene card.
    pub learn_view: Option<LearnView>,
    /// Display directives from lesson scripts and narration (spotlight,
    /// pins, inset, X-ray, explode), or set by the learner.
    pub directives: sim_script::presentation::ViewState,
    /// How far the exploded view has opened, 0–1 (eased).
    pub explode_t: f32,
    /// Sliding parts are drawn this many times further from their start
    /// (a lesson scene's `magnify`; display only).
    pub motion_scale: f32,
    /// Reader preference: no glides, orbiting, explode easing or motion blur.
    pub reduced_motion: bool,
    /// Parts a lesson script is pointing at: everything else dims a little.
    pub soft_focus: Vec<String>,
    /// A second run shown beside (split) or over (ghost) this one.
    pub companion: Option<view::CompanionView>,
}

/// Where the 3D view is embedded in a lesson page, in physical pixels: the
/// card's whole viewport and the part of it currently visible (scrolled).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LearnView {
    pub full: Rect,
    pub visible: Rect,
}

/// The catalog colour of one piece of a display model.
#[derive(Component)]
pub struct ModelColor(pub Color);

/// Parts, net hubs and reference images; respawned when the system changes.
#[derive(Component)]
pub struct SceneContent;
/// Root nodes of the inspection panels; rebuilt when the description changes.
#[derive(Component)]
pub struct UiRoot;

impl SpatialScene {
    /// Build mode: the system may be empty or incomplete, so an empty
    /// presentation is allowed here; everything else is validated as usual.
    pub fn for_builder(description: SystemDescription, spatial: SpatialDescription) -> Result<Self, InspectionError> {
        if !spatial.parts.is_empty() {
            spatial.validate(&description)?;
        }
        let mut scene = Self::unchecked(description, spatial);
        scene.builder_mode = true;
        Ok(scene)
    }
    /// Inspect `--compact` (and `--schematic`): the smaller inspector, with the parts list hidden.
    pub fn set_compact(&mut self, compact: bool) {
        self.compact = compact;
        if compact {
            self.parts_visible = false;
        }
    }
    /// Swap in a recompiled system, keeping display preferences.
    pub fn replace(&mut self, description: SystemDescription, spatial: SpatialDescription, animation: Option<sim_inspect::animation::AnimationDescription>) {
        self.description = description;
        self.spatial = spatial;
        self.animation = animation;
        self.state.hidden.clear();
        self.state.selected = None;
        self.shown = SelectionTarget::None;
        self.details = SelectionDetails::default();
    }
    fn unchecked(description: SystemDescription, spatial: SpatialDescription) -> Self {
        Self {
            description,
            spatial,
            state: SpatialViewState { overlays: sim_inspect::spatial::Overlay::defaults(), ..Default::default() },
            shown: SelectionTarget::None,
            details: SelectionDetails::default(),
            animation: None,
            live: animation::LivePresentation::default(),
            compact: false,
            parts_visible: true,
            annotations: None,
            note_navigation: 0,
            note_hover: SelectionTarget::None,
            note_pointer_hover: SelectionTarget::None,
            note_error: None,
            ghost: Default::default(),
            builder_mode: false,
            builder_dock: 0.,
            builder_side: 0.,
            learn_view: None,
            directives: Default::default(),
            explode_t: 0.,
            motion_scale: 1.,
            reduced_motion: false,
            soft_focus: Vec::new(),
            companion: None,
        }
    }
    pub fn new(
        description: SystemDescription,
        spatial: SpatialDescription,
    ) -> Result<Self, InspectionError> {
        spatial.validate(&description)?;
        Ok(Self::unchecked(description, spatial))
    }
    pub fn apply(&mut self, command: SpatialCommand) -> Result<(), InspectionError> {
        if matches!(command, SpatialCommand::HideSelected) {
            self.state.hidden.extend(
                self.spatial
                    .parts
                    .iter()
                    .filter(|p| self.details.components.contains(&p.component))
                    .map(|p| p.component.clone()),
            );
            return Ok(());
        }
        let select = match &command {
            SpatialCommand::Select { component } => {
                Some(SelectionTarget::component(component.clone()))
            }
            SpatialCommand::ClearSelection => Some(SelectionTarget::None),
            _ => None,
        };
        self.state.apply(&self.spatial, command)?;
        if let Some(target) = select {
            self.set_selection(target)?;
        }
        Ok(())
    }
    /// Show `target` (resolved against the description; an unknown id is
    /// refused and nothing changes): its details, the single selected
    /// component, revealed parts and, for ports and nets, the connections.
    pub fn set_selection(&mut self, target: SelectionTarget) -> Result<(), InspectionError> {
        let details = target.resolve(&self.description)?;
        self.state.selected = match &target {
            SelectionTarget::Components { ids } if ids.len() == 1 => ids.first().cloned(),
            _ => None,
        };
        for id in &details.components {
            self.state.hidden.remove(id);
        }
        if matches!(
            target,
            SelectionTarget::Ports { .. } | SelectionTarget::Nets { .. }
        ) {
            self.state.connections = true;
        }
        self.details = details;
        self.shown = target;
        Ok(())
    }
    pub(crate) fn left(&self) -> f32 {
        if self.builder_mode {
            return ui_kit::LEFT_WIDTH;
        }
        if self.parts_visible {
            if self.compact { 180. } else { LEFT }
        } else {
            0.
        }
    }
    pub(crate) fn right(&self) -> f32 {
        if self.builder_mode {
            return ui_kit::RIGHT_WIDTH + self.builder_side;
        }
        if self.compact { 260. } else { RIGHT }
    }
    pub(crate) fn top(&self) -> f32 {
        if self.builder_mode { ui_kit::TOPBAR } else { TOP }
    }
    /// The docks' bottom edge from the window's bottom: the status bar (and
    /// Build's graph dock) above the switcher strip (`SWITCHER_STRIP`), so
    /// the viewport and hit tests end where the docks do.
    pub(crate) fn bottom(&self) -> f32 {
        ui_kit::SWITCHER_STRIP + if self.builder_mode { ui_kit::STATUSBAR + self.builder_dock } else { BOTTOM }
    }
    pub(crate) fn representatives(&self) -> Vec<(String, String)> {
        let mut seen = std::collections::BTreeSet::new();
        self.spatial
            .parts
            .iter()
            .filter(|p| seen.insert(p.component.clone()))
            .map(|p| (p.component.clone(), p.label.clone()))
            .collect()
    }
    pub(crate) fn positions(&self) -> BTreeMap<String, Vec3> {
        let mut result = BTreeMap::new();
        for p in &self.spatial.parts {
            result.entry(p.component.clone()).or_insert_with(|| {
                Vec3::from_array(p.position)
                    + if self.state.exploded {
                        Vec3::from_array(p.exploded_offset)
                    } else {
                        Vec3::ZERO
                    }
            });
        }
        result
    }
    pub(crate) fn bounds(&self) -> (Vec3, f32) {
        self.bounds_of(None)
    }
    /// Centre and radius of the parts of one instance path (and everything
    /// inside it), or of every part.
    pub(crate) fn bounds_of(&self, prefix: Option<&str>) -> (Vec3, f32) {
        let inside = |c: &str| prefix.is_none_or(|p| p.is_empty() || c == p || c.starts_with(&format!("{p}/")));
        if !self.spatial.parts.iter().any(|p| inside(&p.component)) {
            if prefix.is_some() {
                return self.bounds_of(None);
            }
            return (Vec3::ZERO, 0.1);
        }
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for p in self.spatial.parts.iter().filter(|p| inside(&p.component)) {
            let center = Vec3::from_array(p.position)
                + if self.state.exploded {
                    Vec3::from_array(p.exploded_offset)
                } else {
                    Vec3::ZERO
                };
            let radius = match p.shape {
                SpatialShape::Box { size } => Vec3::from_array(size).length() * 0.5,
                SpatialShape::Cylinder { radius, length } => radius.hypot(length * 0.5),
                SpatialShape::Sphere { radius } => radius,
            };
            lo = lo.min(center - radius);
            hi = hi.max(center + radius);
        }
        // Pieces drawn inside a part (a link's arm, a gear train) can reach
        // well beyond its housing: frame them too.
        if let Some(a) = &self.animation {
            for b in &a.internals {
                let Some(p) = self.spatial.parts.iter().find(|p| p.id == b.part && inside(&p.component)) else { continue };
                let offset = if self.state.exploded { Vec3::from_array(p.exploded_offset) } else { Vec3::ZERO };
                let reach = match b.element {
                    sim_inspect::animation::InternalElement::Arm => b.length + b.radius,
                    _ => b.radius.hypot(b.length * 0.5),
                };
                let center = Vec3::from_array(b.center) + offset;
                lo = lo.min(center - reach);
                hi = hi.max(center + reach);
            }
        }
        ((lo + hi) * 0.5, (hi - lo).length() * 0.5)
    }
}

/// The spatial view's public ordering points (native-viewer.md "Public
/// system sets"), configured once in [`SpatialViewerPlugin`], all in
/// SimSync: the builder and the lesson pages that draw into this view order
/// against these, not against its functions.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum InspectViewSet {
    /// The notes' navigation (`notes::update`), first in the layout chain.
    Notes,
    /// The linked peer's selection exchange, after projection in Inspect.
    Link,
    /// The view's camera data goes to the shared camera (`sync_camera`),
    /// last in the layout chain, before `CameraSet::Viewport`.
    Camera,
    /// The parts follow the placed camera (`update_parts`, after `CameraSet::Place`).
    Parts,
}

/// [`InspectViewSet`] in the pipeline (SpatialViewerPlugin, and the
/// windowless schedule test).
pub(crate) fn configure_sets(app: &mut App) {
    app.configure_sets(Update, (InspectViewSet::Notes, InspectViewSet::Link, InspectViewSet::Camera, InspectViewSet::Parts).in_set(ViewerSet::SimSync));
}

/// The spatial assembly view: Inspect's whole screen, and the 3D scene the
/// builder (Build) and the lesson pages (Lessons) draw into. Its entities are
/// spawned on entering the Inspect or Builder scope (and despawned on leaving
/// it, `app::scope_new_entities`); its systems run while `SpatialScreen` is
/// active, in their original order.
pub struct SpatialViewerPlugin;
impl Plugin for SpatialViewerPlugin {
    fn build(&self, app: &mut App) {
        app::actions::register::<inspect::InspectAction>(app);
        configure_sets(app);
        // The notes panel's state and its composer and author fields.
        notes::build(app);
        app.init_resource::<physics_view::Labels>()
            .init_resource::<view::PartHover>()
            .add_systems(OnEnter(ModeScope::Inspect), (setup_scene, setup_ui))
            .add_systems(OnEnter(ModeScope::Builder), (setup_scene, setup_ui))
            // Buttons, keys, the notes panel and the overlay bar write the view's actions.
            .add_systems(Update, (inspect::input, notes::clicks, notes::compose, physics_view::overlay_clicks).chain().in_set(crate::app::InputSet::Window).run_if(in_state(SpatialScreen)))
            .add_systems(Update, inspect::apply.in_set(ViewerSet::Actions).run_if(in_state(SpatialScreen)))
            // Inspect's shared selection, shown (and re-checked after a
            // reload): after the notes' navigation, before the link's
            // exchange (which then sends the launch's `--select`, adopted
            // here, and never a reloaded document's dropped items; a peer's
            // change it applies it shows itself) and before the parts and
            // the inspector read what is shown.
            .add_systems(Update, projection::project_selection.after(InspectViewSet::Notes).before(InspectViewSet::Link).before(InspectViewSet::Parts).in_set(ViewerSet::SimSync).run_if(in_state(ViewerMode::Inspect)))
            .add_systems(
                Update,
                // The layout first: the camera's data (bounds, view area)
                // goes to the shared camera before it sets the viewport.
                (notes::update.in_set(InspectViewSet::Notes), buttons.in_set(ViewerSet::SimSync), linked::sync_link.in_set(InspectViewSet::Link), animation::sync_live.in_set(ViewerSet::SimSync), update_layout.in_set(ViewerSet::SimSync), sync_camera.in_set(InspectViewSet::Camera))
                    .chain()
                    .before(CameraSet::Viewport)
                    .run_if(in_state(SpatialScreen)),
            )
            .add_systems(
                Update,
                // Then what reads the placed camera.
                (scroll_inspector.in_set(ViewerSet::SimSync), update_parts.in_set(InspectViewSet::Parts), linked::update_nets.in_set(ViewerSet::SimSync), update_ui.in_set(ViewerSet::SimSync), draw_guides.in_set(ViewerSet::SimSync), notes::guides.in_set(ViewerSet::SimSync), animation::draw_markers.in_set(ViewerSet::SimSync))
                    .chain()
                    .after(CameraSet::Place)
                    .run_if(in_state(SpatialScreen)),
            )
            .add_systems(
                Update,
                (view::animate, physics_view::update_internals, physics_view::draw, view::draw_pins, view::draw_ghost, view::split, view::inset, physics_view::labels, physics_view::overlay_bar, inspect::publish)
                    .chain()
                    .in_set(ViewerSet::Present)
                    .run_if(in_state(SpatialScreen)),
            );
    }
}

/// The example assembly inspect mode shows when given no files.
pub fn default_inspect_paths() -> (std::path::PathBuf, std::path::PathBuf) {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/systems-viewer/spatial");
    (base.join("motor-thermal.description.json"), base.join("motor-thermal.spatial.json"))
}

/// Inspect mode's document when switching: a `*.description.json` and the
/// `*.spatial.json` beside it.
pub fn inspect_pair(description: &std::path::Path) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let name = description.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let stem = name.strip_suffix(".description.json").ok_or_else(|| format!("{}: inspect mode opens a *.description.json (with its *.spatial.json beside it)", description.display()))?;
    let spatial = description.with_file_name(format!("{stem}.spatial.json"));
    if !description.is_file() {
        return Err(format!("{}: no such file", description.display()));
    }
    if !spatial.is_file() {
        return Err(format!("{}: no {} beside it", description.display(), spatial.display()));
    }
    Ok((description.to_path_buf(), spatial))
}

/// Read and validate an assembly for inspect mode (the launch and a switch
/// share it). Errors name the file.
pub fn load_inspect(description: &std::path::Path, spatial: &std::path::Path) -> Result<SpatialScene, String> {
    let read = |p: &std::path::Path| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()));
    let parsed: SystemDescription = serde_json::from_slice(&read(description)?).map_err(|e| format!("{}: {e}", description.display()))?;
    let geometry: SpatialDescription = serde_json::from_slice(&read(spatial)?).map_err(|e| format!("{}: {e}", spatial.display()))?;
    SpatialScene::new(parsed, geometry).map_err(|e| format!("{}: {e}", spatial.display()))
}

#[derive(Component)]
pub(crate) struct Part {
    pub(crate) index: usize,
}
#[derive(Component)]
struct PartsPanel;
#[derive(Component)]
struct Inspector;
#[derive(Component)]
struct InspectorScroll;
#[derive(Component)]
struct Status;

#[cfg(test)]
mod tests;
