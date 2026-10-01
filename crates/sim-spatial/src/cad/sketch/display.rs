//! The sketches' curves drawn (display only): RoboCAD's `_draw_sketches`
//! (ui/viewport.py:922-939) draws every visible sketch's curves, each
//! `sample(48)` (a slot's closed stadium outline) through the sketch's own
//! plane, orange (1.0, 0.65, 0.2) when the sketch is selected and blue
//! (0.35, 0.8, 1.0) otherwise, 2 px wide with the depth test off.
//!
//! Here: every `effective_visible` sketch node of the shown tree, from the
//! cache's last read (`CadSketches::sketch_last`, so an edit does not make
//! the sketch blink while it is refetched), as [`SketchGizmos`] lines (2 px,
//! drawn over the bodies). The world polylines are rebuilt only when the
//! cache's epoch, the shown sketches or the selection change; each frame
//! only maps them through the camera's model transform.
use super::CadSketches;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::document::CadDocument;
use crate::cad::selection::{CadItems, CadSelection};
use sim_runtime::cad_client::SelectionItem;
use crate::cad::view::CadView;
use bevy::prelude::*;

/// RoboCAD's sketch colours (viewport.py:936).
const SELECTED: Color = Color::srgb(1.0, 0.65, 0.2);
const NORMAL: Color = Color::srgb(0.35, 0.8, 1.0);
/// Samples per curve (viewport.py:932).
const SAMPLES: usize = 48;

/// The sketches' lines: 2 px (viewport.py:937) and over the bodies, as
/// RoboCAD draws them with the depth test off.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(in crate::cad) struct SketchGizmos;

pub(in crate::cad) fn build(app: &mut App) {
    app.insert_gizmo_config(SketchGizmos, GizmoConfig { depth_bias: -1.0, line: GizmoLineConfig { width: 2.0, ..default() }, ..default() })
        .add_systems(Update, draw.in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)));
}

/// What the lines were built from: the cache's epoch, and each shown
/// sketch with whether it is selected (walk order).
type Key = (u64, u64, Vec<(String, bool)>);

/// The last built lines (model mm), each with whether its sketch is selected.
#[derive(Default)]
pub(super) struct Drawn {
    key: Option<Key>,
    lines: Vec<(Vec<Vec3>, bool)>,
}

/// The shown sketches, in walk order, each with whether it is selected
/// (`selection`: the shared selection's CAD items).
pub(crate) fn shown(doc: &CadDocument, selection: &[SelectionItem]) -> Vec<(String, bool)> {
    let Some(state) = &doc.doc else { return Vec::new() };
    let selected = selection.nodes();
    state.nodes.iter().filter(|n| n.kind == "sketch" && n.effective_visible).map(|n| (n.id.clone(), selected.contains(&n.id))).collect()
}

/// Each shown sketch's curves as polylines through its own plane (mm,
/// RoboCAD's frame); a sketch not read yet, or whose plane is malformed, has none.
pub(crate) fn lines(sketches: &CadSketches, shown: &[(String, bool)]) -> Vec<(Vec<[f64; 3]>, bool)> {
    let mut out = Vec::new();
    for (id, selected) in shown {
        let Some(g) = sketches.sketch_last(id) else { continue };
        let Some(plane) = g.plane else { continue };
        for c in &g.curves {
            let pts: Vec<[f64; 3]> = c.sample(SAMPLES).into_iter().map(|p| plane.to_world(p[0], p[1], 0.0)).collect();
            if pts.len() >= 2 {
                out.push((pts, *selected));
            }
        }
    }
    out
}

/// Present: the sketches' curves (display only).
fn draw(doc: Option<Res<CadDocument>>, view: Option<Res<CadView>>, sketches: Option<Res<CadSketches>>, mut gizmos: Gizmos<SketchGizmos>, mut drawn: Local<Drawn>, selection: CadSelection) {
    let (Some(doc), Some(view), Some(sketches)) = (doc, view, sketches) else { return };
    if !view.valid {
        return;
    }
    let key: Key = (doc.generation, sketches.epoch, shown(&doc, &selection.items()));
    if drawn.key.as_ref() != Some(&key) {
        drawn.lines = lines(&sketches, &key.2).into_iter().map(|(l, s)| (l.into_iter().map(|p| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)).collect(), s)).collect();
        drawn.key = Some(key);
    }
    for (line, selected) in &drawn.lines {
        gizmos.linestrip(line.iter().map(|p| view.world_from_model.transform_point3(*p)), if *selected { SELECTED } else { NORMAL });
    }
}
