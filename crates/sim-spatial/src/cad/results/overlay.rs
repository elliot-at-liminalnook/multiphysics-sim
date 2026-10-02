//! The stress overlay (RoboCAD's `view.stress` / `print.overlay` toggle and
//! `viewport._stress_colors`), display only, and the results panel.
//!
//! - **One rule**: each drawn body whose node carries a link block with a
//!   `hotspot` (`GET /results/nodes`, RoboCAD's `Node.results`) gets
//!   per-vertex colours from `sim_domain_robot::stress_results::link_colours`,
//!   the function Robot mode colours its links with. Its scale is
//!   logarithmic over three decades (blue = 0.1 % of yield, red = yield);
//!   RoboCAD's own window maps linearly from blue 0 to red at yield. The
//!   shared rule wins (one rule for both modes); the panel and the state
//!   say so.
//! - **Frames**: hotspot cells are in the link frame (m) with its origin at
//!   the link's centre of mass; RoboCAD places them at `cells·1e3 + com·1e3`
//!   (world mm). Each mesh vertex `v` (mm, RoboCAD's frame) is therefore
//!   passed as `v·1e-3 − com`, `com` from the block (m), else the node's
//!   mass centroid (`GET /nodes/{id}`, mm → m), else (the node has no
//!   body, so no mass block) the origin, as RoboCAD falls back. When that
//!   read fails or the window is not connected the body is not coloured:
//!   its error names the node in the panel, and it is tried again once the
//!   connection's generation or state changes (a reconnect). Yield: the node material's yield strength, else the
//!   largest cell stress (at least 1 Pa; RoboCAD takes the largest sampled
//!   vertex stress, which is at most that).
//! - **Display only**: the colours are a vertex attribute on the drawn mesh
//!   asset (the geometry and RoboCAD are untouched), computed on a job
//!   keyed by (document generation, the body's mesh asset, the node's
//!   results; a failed one also by the connection), and removed when the overlay goes off. `mesh::highlight`
//!   draws a coloured body in a white material ([`StressPaint::painted`]).
//! - **Print study blocks** (section "print", `print.strength`/`print.plan`)
//!   go through the same rule: `print::overlay::inputs` makes one cell at
//!   the origin whose stress is the governing failure index (1 / safety
//!   factor) with yield 1, so the body is one colour, red at failure. The
//!   panel adds the print line with its staleness (`print::overlay`).
use super::{ROBOCAD_SCALE, controls_of, link_active, staleness};
use crate::app::ModeScope;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::document::CadDocument;
use crate::cad::mesh::CadBody;
use crate::cad::panel::CadButton;
use crate::jobs::{Job, Pool};
use crate::ui_kit::{BORDER, DANGER, Kit, LEFT_WIDTH, Look, STATUSBAR, SURFACE, UiFonts, above_strip, size, wrap};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;
use sim_domain_robot::stress_results::{Hotspot, SCALE, link_colours};
use std::collections::{HashMap, HashSet};

/// What a body's colours are computed from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Inputs {
    pub hotspot: Hotspot,
    /// The node material's yield strength (Pa), as RoboCAD reads it.
    pub yield_pa: Option<f64>,
    /// The link block's centre of mass (m), when the results carry it.
    pub com_m: Option<[f64; 3]>,
}

/// Node `id`'s inputs, when its results are a link block with a hotspot,
/// or a print study block with a safety factor (`print::overlay::inputs`).
pub(crate) fn inputs_of(doc: &CadDocument, id: &str) -> Option<Inputs> {
    let node = doc.robot.data.node_results(id)?;
    let block = &node.results;
    if block["section"].as_str() == Some(crate::cad::print::overlay::SECTION) {
        return crate::cad::print::overlay::inputs(node);
    }
    if block["section"].as_str() != Some("links") {
        return None;
    }
    let hotspot = Hotspot::from_block(&block["hotspot"])?;
    let com_m = block["com"].as_array().filter(|a| a.len() == 3).and_then(|a| Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?]));
    Some(Inputs { hotspot, yield_pa: node.yield_strength_pa, com_m })
}

/// The colours of RoboCAD mesh vertices `positions_mm` through the shared
/// rule (see the module doc for the frame and the yield).
pub(crate) fn cad_colours(inputs: &Inputs, com_m: [f64; 3], positions_mm: &[[f32; 3]]) -> Vec<[f32; 4]> {
    let yield_strength = inputs.yield_pa.filter(|y| y.is_finite() && *y > 0.0).unwrap_or_else(|| inputs.hotspot.peak_or_one());
    link_colours(&inputs.hotspot, yield_strength, positions_mm.iter().map(|p| [f64::from(p[0]) * 1e-3 - com_m[0], f64::from(p[1]) * 1e-3 - com_m[1], f64::from(p[2]) * 1e-3 - com_m[2]]))
}

/// One body's paint.
struct Paint {
    mesh: AssetId<Mesh>,
    inputs: Inputs,
    job: Option<Job<Vec<[f32; 4]>>>,
    /// The colours are on `mesh`.
    done: bool,
    error: Option<String>,
    /// (generation, connected) when the job started: a failed paint is
    /// tried again when this changes (a reconnect).
    tried: (u64, bool),
}

/// The overlay's paint per body (window only).
#[derive(Resource, Default)]
pub(crate) struct StressPaint {
    generation: u64,
    bodies: HashMap<String, Paint>,
    /// Bumped whenever a body starts or stops showing colours (`mesh::highlight` follows it).
    pub(crate) epoch: u64,
}
impl StressPaint {
    /// Body `id` shows stress colours now.
    pub(crate) fn painted(&self, id: &str) -> bool {
        self.bodies.get(id).is_some_and(|p| p.done)
    }
    fn counts(&self) -> (usize, usize, Vec<String>) {
        let painted = self.bodies.values().filter(|p| p.done).count();
        let pending = self.bodies.values().filter(|p| p.job.is_some()).count();
        let errors = self.bodies.values().filter_map(|p| p.error.clone()).collect();
        (painted, pending, errors)
    }
}

/// Removes our colours from `mesh` (if it still exists and has them).
fn strip(assets: &mut Assets<Mesh>, mesh: AssetId<Mesh>) {
    if let Some(mut m) = assets.get_mut(mesh)
        && matches!(m.try_contains_attribute(Mesh::ATTRIBUTE_COLOR), Ok(true))
    {
        let _ = m.try_remove_attribute(Mesh::ATTRIBUTE_COLOR);
    }
}

/// SimSync (after `mesh::sync`, before `mesh::highlight`): colours computed
/// for bodies that need them, put on their meshes when they land, and
/// removed from bodies that no longer should show them.
#[allow(clippy::type_complexity)]
pub(super) fn paint(
    doc: Option<Res<CadDocument>>,
    paint: Option<ResMut<StressPaint>>,
    mut assets: ResMut<Assets<Mesh>>,
    bodies: Query<(&CadBody, &Mesh3d)>,
    changed: Query<(), (With<CadBody>, Changed<Mesh3d>)>,
    mut last: Local<Option<(u64, u64, bool, bool)>>,
) {
    let (Some(doc), Some(mut paint)) = (doc, paint) else { return };
    let busy = paint.bodies.values().any(|p| p.job.is_some());
    let connection = (doc.generation, doc.connected());
    let key = (doc.generation, doc.revision, doc.results.overlay, connection.1);
    if !busy && *last == Some(key) && changed.is_empty() && paint.generation == doc.generation {
        return;
    }
    *last = Some(key);
    let paint = &mut *paint;
    if paint.generation != doc.generation {
        // Another document: its meshes are gone with it.
        paint.generation = doc.generation;
        paint.bodies.clear();
        paint.epoch += 1;
    }
    let mut seen: HashSet<&str> = HashSet::new();
    for (body, mesh) in &bodies {
        seen.insert(body.id.as_str());
        let id = mesh.0.id();
        let want = if doc.results.overlay { inputs_of(&doc, &body.id) } else { None };
        let Some(inputs) = want else {
            if let Some(old) = paint.bodies.remove(&body.id) {
                strip(&mut assets, old.mesh);
                if old.done {
                    paint.epoch += 1;
                }
            }
            continue;
        };
        if let Some(p) = paint.bodies.get_mut(&body.id)
            && p.mesh == id
            && p.inputs == inputs
            && (p.error.is_none() || p.tried == connection)
        {
            let landed = p.job.as_ref().and_then(Job::poll);
            if let Some(result) = landed {
                p.job = None;
                match result {
                    Ok(colours) => {
                        let fits = assets.get(id).and_then(|m| m.try_attribute_option(Mesh::ATTRIBUTE_POSITION).ok().flatten()).is_some_and(|a| a.len() == colours.len());
                        if fits && let Some(mut m) = assets.get_mut(id) {
                            p.done = m.try_insert_attribute(Mesh::ATTRIBUTE_COLOR, colours).is_ok();
                            p.error = None;
                        } else {
                            p.error = Some(format!("{}: the mesh changed while its colours were computed", body.id));
                        }
                        if p.done {
                            paint.epoch += 1;
                        }
                    }
                    // The job's errors name the node.
                    Err(e) => p.error = Some(e),
                }
            }
            continue;
        }
        // New, or its mesh or results changed: (re)computed off the UI thread.
        if let Some(old) = paint.bodies.remove(&body.id) {
            strip(&mut assets, old.mesh);
            if old.done {
                paint.epoch += 1;
            }
        }
        let Some(positions) = assets.get(id).and_then(|m| m.try_attribute_option(Mesh::ATTRIBUTE_POSITION).ok().flatten()).and_then(|a| a.as_float3()).map(<[[f32; 3]]>::to_vec) else { continue };
        let client = doc.client.clone().filter(|_| doc.connected());
        let (node, job_inputs) = (body.id.clone(), inputs.clone());
        // The centroid read is a request (Dedicated); the colouring alone is Compute.
        let pool = if inputs.com_m.is_some() { Pool::Compute } else { Pool::Dedicated };
        let job = Job::spawn(pool, doc.generation, "cad stress colours", move |ctx| {
            let com = match job_inputs.com_m {
                Some(c) => c,
                None => centroid_m(client.as_ref(), &node)?,
            };
            if ctx.cancelled() {
                return Err(format!("node {node}: superseded"));
            }
            Ok(cad_colours(&job_inputs, com, &positions))
        });
        paint.bodies.insert(body.id.clone(), Paint { mesh: id, inputs, job: Some(job), done: false, error: None, tried: connection });
    }
    // Bodies no longer drawn.
    let gone: Vec<String> = paint.bodies.keys().filter(|k| !seen.contains(k.as_str())).cloned().collect();
    for id in gone {
        if let Some(old) = paint.bodies.remove(&id) {
            strip(&mut assets, old.mesh);
            if old.done {
                paint.epoch += 1;
            }
        }
    }
}

/// Node `id`'s mass centroid (m) from `GET /nodes/{id}` (mm); the origin
/// when RoboCAD's answer has no mass centroid (a node without a body, as
/// RoboCAD falls back). Not connected, or the read failed: an error naming
/// the node (the body stays uncoloured; the paint is tried again on reconnect).
fn centroid_m(client: Option<&sim_runtime::cad_client::CadClient>, id: &str) -> Result<[f64; 3], String> {
    let client = client.ok_or_else(|| format!("node {id}'s centre of mass could not be read: not connected to RoboCAD; it is coloured once the window reconnects"))?;
    let detail = client.node(id).map_err(|e| format!("node {id}'s centre of mass could not be read from RoboCAD: {e}"))?;
    let centroid = detail.mass.map(|m| m.centroid).filter(|c| c.len() == 3);
    Ok(match centroid {
        Some(c) => [c[0].unwrap_or(0.0) * 1e-3, c[1].unwrap_or(0.0) * 1e-3, c[2].unwrap_or(0.0) * 1e-3],
        None => [0.0; 3],
    })
}

/// The results panel's root.
#[derive(Component)]
pub(super) struct ResultsPanelRoot;

/// What the panel shows, as a key (rebuilt when it changes; seconds tick once a second).
fn panel_key(doc: &CadDocument, paint: Option<&StressPaint>) -> Option<String> {
    let r = &doc.results;
    let running = r.exports.running.as_ref().map(|x| (x.request.label.clone(), x.started.elapsed().as_secs()));
    if !r.overlay && running.is_none() && !link_active(doc) {
        return None;
    }
    let loaded = doc.robot.data.results().map(|l| (l.path.clone(), l.loaded.clone(), l.stale));
    let print = if r.overlay { crate::cad::print::overlay::panel_line(doc) } else { None };
    Some(format!("{}{running:?}{:?}{:?}{loaded:?}{:?}{}{}{print:?}", r.overlay, r.exports.queued.as_ref().map(|q| &q.label), r.link, paint.map(StressPaint::counts), doc.edit.is_some(), doc.connected()))
}

/// Present: the floating results panel while the overlay is on, an export
/// runs or the live link is on: the overlay's legend and staleness, the
/// export's progress and Cancel, the link with "Show in Robot mode".
pub(super) fn panel(mut commands: Commands, doc: Option<Res<CadDocument>>, paint: Option<Res<StressPaint>>, fonts: Res<UiFonts>, roots: Query<Entity, With<ResultsPanelRoot>>, mut shown: Local<Option<String>>) {
    let want = doc.as_deref().and_then(|d| panel_key(d, paint.as_deref()));
    let present = roots.iter().next().is_some();
    if *shown == want && present == want.is_some() {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    *shown = want;
    let (Some(doc), true) = (doc.as_deref(), shown.is_some()) else { return };
    let k = Kit::new(&fonts);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(LEFT_WIDTH + 8.0),
                bottom: above_strip(STATUSBAR + 8.0),
                width: Val::Px(300.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(12.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(SURFACE),
            BorderColor::all(BORDER),
            FocusPolicy::Block,
            GlobalZIndex(30),
            AccessibleLabel::new("Results"),
            ResultsPanelRoot,
            DespawnOnExit(ModeScope::Cad),
        ))
        .with_children(|p| body(p, &k, doc, paint.as_deref()));
}

/// The controls (`results::controls_of`): (id, label, action, ready).
type Controls = [(String, String, crate::cad::actions::CadAction, Result<(), String>)];

/// The button of control `id`, enabled as the control is.
fn button(p: &mut ChildSpawnerCommands, k: &Kit, controls: &Controls, id: &str, label: &str, look: Look) {
    if let Some((_, _, action, ready)) = controls.iter().find(|c| c.0 == id) {
        p.spawn(k.button(label, CadButton(action.clone()), look, ready.is_ok()));
    }
}

fn body(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, paint: Option<&StressPaint>) {
    let controls = controls_of(doc);
    let controls = controls.as_slice();
    let r = &doc.results;
    if r.overlay {
        p.spawn(k.title("Stress overlay"));
        let status = staleness(doc);
        let loaded = doc.robot.data.results().and_then(|l| l.path.clone());
        p.spawn(k.caption(format!("Results: {status}{}", loaded.map_or_else(String::new, |l| format!(" · {l}")))));
        if let Some(line) = crate::cad::print::overlay::panel_line(doc) {
            p.spawn(k.caption(line));
            p.spawn(k.note(crate::cad::print::overlay::RULE));
        }
        p.spawn(k.note(SCALE));
        p.spawn(k.note(ROBOCAD_SCALE));
        if let Some((painted, pending, errors)) = paint.map(StressPaint::counts) {
            let line = match (painted, pending) {
                (0, 0) => "No body carries a stress hotspot or a print strength result.".to_string(),
                (n, 0) => format!("{n} bodies coloured."),
                (n, m) => format!("{n} bodies coloured, {m} being computed…"),
            };
            p.spawn(k.caption(line));
            for e in errors.iter().take(3) {
                p.spawn(k.text(e.clone(), size::SMALL, DANGER, 0));
            }
        }
        p.spawn(wrap()).with_children(|row| {
            button(row, k, controls, "cad:results:overlay", "Hide stress overlay", Look::Secondary);
            button(row, k, controls, "cad:results:load", "Load results…", Look::Ghost);
        });
    }
    if let Some(running) = &r.exports.running {
        p.spawn(k.title("Export"));
        p.spawn(k.caption(format!("exporting {} in the background… {} s", running.request.label, running.started.elapsed().as_secs())));
        p.spawn(k.note(running.request.path.display().to_string()));
        if let Some(q) = &r.exports.queued {
            p.spawn(k.note(format!("Next: {} (the latest save)", q.label)));
        }
        p.spawn(wrap()).with_children(|row| button(row, k, controls, "cad:results:export_cancel", "Cancel export", Look::Danger));
        p.spawn(k.note("Cancel writes nothing; RoboCAD still finishes deriving the model."));
    }
    if link_active(doc) {
        p.spawn(k.title("Live link"));
        let model = r.link.as_deref().map(super::model_path).map_or_else(String::new, |m| m.display().to_string());
        p.spawn(k.caption(format!("Every save re-exports {model}; Robot mode shows it.")));
        p.spawn(wrap()).with_children(|row| {
            button(row, k, controls, "cad:results:show_robot", "Show in Robot mode", Look::Primary);
            button(row, k, controls, "cad:results:link", "Stop live link", Look::Secondary);
        });
    }
}

/// CadPlugin: the paint resource and its system, and the panel.
pub(super) fn build(app: &mut App) {
    app.init_resource::<StressPaint>().add_systems(
        Update,
        (
            paint.after(crate::cad::CadSet::Mesh).before(crate::cad::CadSet::Highlight).in_set(ViewerSet::SimSync),
            panel.in_set(ViewerSet::Present),
        )
            .run_if(in_state(ViewerMode::Cad)),
    );
}
