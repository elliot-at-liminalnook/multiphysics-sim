//! Read-only analysis overlays (cad-modify): RoboCAD's control points
//! (`tool.control_points`), curvature comb (`inspect.curvature`) and
//! continuity check (`inspect.continuity`), read through RoboCAD's routes
//! on a job and drawn as display-only lines. Nothing is written.
//!
//! - **One read at a time** ([`start`]): a `Pool::Dedicated` job (network
//!   I/O, the jobs module's pool rule) stamped with the document's
//!   generation, refused by name, with nothing sent, when not connected,
//!   while another read runs, while the shown document is behind RoboCAD's,
//!   or when the shown revision is not the one the picks were made at.
//! - **Results** ([`receive`], `ViewerSet::JobResults`, after
//!   `sync::receive`): a copy goes to `OpsState::clipboard` with the
//!   revision it was read at ("Copied N item(s) with placement", as
//!   RoboCAD's `copy_with_placement`); an analysis replaces the overlay
//!   (RoboCAD's `viewport.temp_shapes`) and sets the status line. A result
//!   of an older document generation is dropped silently; an analysis read
//!   whose revision is no longer the shown one is dropped with a status
//!   saying so. The overlay is cleared when the shown revision (or the
//!   document) changes, as RoboCAD's temporary shapes are cleared on refresh.
//! - **Drawing** ([`draw`], `ViewerSet::Present`) on the tools' gizmo group
//!   (`transform::ToolGizmos`: over the bodies, as RoboCAD draws its
//!   temporary shapes), model mm mapped through `CadView::world_from_model`;
//!   RoboCAD's colours (`ui/app.py` `control_points`, `curvature_comb`,
//!   `continuity`).
use super::document::CadDocument;
use super::transform::{ToolGizmos, marker};
use super::view::CadView;
use crate::app::{ViewerMode, ViewerSet};
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_runtime::cad_client::{ControlPoints, Continuity, CurvatureComb};
use std::collections::BTreeMap;

/// Control point markers (app.py:981 `("point", (p, (1.0, 0.5, 0.9), 8.0))`).
pub const POINT_COLOUR: Color = Color::srgb(1.0, 0.5, 0.9);
/// Control point marker size, pixels (app.py:981).
pub const POINT_PIXELS: f32 = 8.0;
/// The rows of control points (app.py:981 `("poly", (row, (0.9, 0.4, 0.8)))`).
pub const ROW_COLOUR: Color = Color::srgb(0.9, 0.4, 0.8);
/// Curvature comb teeth (app.py:1283 `("line", (a, c, (0.9, 0.5, 1.0)))`).
pub const COMB_COLOUR: Color = Color::srgb(0.9, 0.5, 1.0);
/// Continuity grades (app.py:1293).
pub const G0_COLOUR: Color = Color::srgb(1.0, 0.3, 0.3);
pub const G1_COLOUR: Color = Color::srgb(1.0, 0.8, 0.3);
pub const G2_COLOUR: Color = Color::srgb(0.3, 0.9, 0.4);
pub const BOUNDARY_COLOUR: Color = Color::srgb(0.5, 0.5, 0.5);

/// The analysis shown now and the read in flight (`CadDocument::ops.analysis`).
#[derive(Default)]
pub struct Analysis {
    /// The read in flight.
    pending: Option<Pending>,
    /// The overlay drawn now.
    shown: Option<Overlay>,
}

/// A read in flight: what it asks, the shown revision its picks were made
/// at, and the job.
struct Pending {
    read: Read,
    revision: u64,
    job: Job<Landed>,
}

/// What a read brought back.
enum Landed {
    Copied(Value),
    ControlPoints(ControlPoints),
    CurvatureComb(CurvatureComb),
    Continuity(Continuity),
}

/// One drawn analysis (RoboCAD's `temp_shapes`), in model mm.
#[derive(Clone, Debug, PartialEq)]
pub struct Overlay {
    /// `control_points` | `curvature_comb` | `continuity` ([`Read::name`]).
    pub kind: &'static str,
    pub node: String,
    /// The face (control points).
    pub face: Option<i64>,
    /// The shown revision and document generation it was read at.
    pub revision: u64,
    pub generation: u64,
    /// Polylines with their colours.
    pub lines: Vec<(Vec<Vec3>, Color)>,
    /// Point markers: where, colour, size in pixels.
    pub markers: Vec<(Vec3, Color, f32)>,
    /// The continuity counts per grade (`G0`, `G1`, `G2`, `boundary`).
    pub counts: Option<BTreeMap<String, u64>>,
}

/// What a read asks RoboCAD for (read-only; built by `ops::args`).
#[derive(Clone, Debug, PartialEq)]
pub enum Read {
    /// RoboCAD's "Copy with Placement": the nodes' clipboard JSON (B-rep and
    /// placement), `POST /clipboard/copy`.
    Copy { ids: Vec<String> },
    /// A face's control points (`kernel.control_points`),
    /// `GET /nodes/{id}/control_points?face=i`.
    ControlPoints { node: String, face: i64 },
    /// A curve or sketch's curvature comb (`analysis.curvature_comb`),
    /// `GET /nodes/{id}/curvature_comb`.
    CurvatureComb { node: String },
    /// A body's edge continuity (`analysis.continuity_report`),
    /// `GET /nodes/{id}/continuity`.
    Continuity { node: String },
}

impl Read {
    /// The read's name (`state_json`'s `kind`, the job's name).
    pub fn name(&self) -> &'static str {
        match self {
            Read::Copy { .. } => "copy",
            Read::ControlPoints { .. } => "control_points",
            Read::CurvatureComb { .. } => "curvature_comb",
            Read::Continuity { .. } => "continuity",
        }
    }
    /// RoboCAD's label for it.
    pub fn label(&self) -> &'static str {
        match self {
            Read::Copy { .. } => "Copy with Placement",
            Read::ControlPoints { .. } => "Control points",
            Read::CurvatureComb { .. } => "Curvature comb",
            Read::Continuity { .. } => "Continuity check",
        }
    }
    fn json(&self) -> Value {
        match self {
            Read::Copy { ids } => json!({"ids": ids}),
            Read::ControlPoints { node, face } => json!({"node": node, "face": face}),
            Read::CurvatureComb { node } | Read::Continuity { node } => json!({"node": node}),
        }
    }
}

/// Start `read` on a job (refused by name while one runs or when not
/// connected); the result lands in `CadDocument::ops` (the clipboard or
/// the overlay) and the status line. `revision`: the shown revision the
/// picks were made at. Answers `{"reading", "read", "revision"}`.
pub(super) fn start(doc: &mut CadDocument, read: Read, revision: u64) -> Result<Value, String> {
    let label = read.label();
    let client = doc.client.clone().filter(|_| doc.connected()).ok_or_else(|| format!("{label}: not connected to RoboCAD: {}", doc.connection_line().0))?;
    if let Some(p) = &doc.ops.analysis.pending {
        return Err(format!("{label}: {} is still being read; nothing was sent", p.read.label()));
    }
    if let Some(stale) = &doc.stale {
        return Err(format!("{label}: the shown document is behind RoboCAD's ({stale}); nothing was sent"));
    }
    let shown = doc.shown_revision();
    if revision != shown {
        return Err(format!("{label}: the document changed since the selection was made (revision {revision}, now {shown}); nothing was sent: select again"));
    }
    let ask = read.clone();
    let job = Job::spawn(Pool::Dedicated, doc.generation, format!("cad-{}", read.name()), move |_| {
        let landed = match ask {
            Read::Copy { ids } => client.copy_nodes(&ids).map(Landed::Copied),
            Read::ControlPoints { node, face } => client.control_points(&node, face).map(Landed::ControlPoints),
            Read::CurvatureComb { node } => client.curvature_comb(&node).map(Landed::CurvatureComb),
            Read::Continuity { node } => client.continuity(&node).map(Landed::Continuity),
        };
        landed.map_err(|e| e.to_string())
    });
    let answer = json!({"reading": read.name(), "read": read.json(), "revision": revision});
    doc.ops.analysis.pending = Some(Pending { read, revision, job });
    doc.touch();
    Ok(answer)
}

/// The overlays' systems (JobResults receive, Present draw).
pub(super) fn build(app: &mut App) {
    app.add_systems(
        Update,
        (receive.in_set(ViewerSet::JobResults).after(super::sync::receive), draw.in_set(ViewerSet::Present)).run_if(in_state(ViewerMode::Cad)),
    );
}

/// The analysis part of `cad_state`: the overlay shown (kind, node, face,
/// revision, continuity counts) and the read in flight.
pub(super) fn state_json(a: &Analysis) -> Value {
    let shown = a.shown.as_ref();
    json!({
        "kind": shown.map(|s| s.kind),
        "node": shown.map(|s| s.node.clone()),
        "face": shown.and_then(|s| s.face),
        "revision": shown.map(|s| s.revision),
        "counts": shown.and_then(|s| s.counts.clone()),
        "lines": shown.map_or(0, |s| s.lines.len()),
        "markers": shown.map_or(0, |s| s.markers.len()),
        "reading": a.pending.as_ref().map(|p| p.read.name()),
    })
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// RoboCAD's colour for a continuity grade (app.py:1293).
pub fn grade_colour(grade: &str) -> Color {
    match grade {
        "G0" => G0_COLOUR,
        "G1" => G1_COLOUR,
        "G2" => G2_COLOUR,
        _ => BOUNDARY_COLOUR,
    }
}

/// Python's `str(dict)` of the counts, as RoboCAD's status line prints it
/// (`f"Continuity: {counts}"`): `{'G0': 12, 'G1': 0, 'G2': 0, 'boundary': 0}`.
pub fn counts_text(counts: &BTreeMap<String, u64>) -> String {
    let items: Vec<String> = counts.iter().map(|(k, v)| format!("'{k}': {v}")).collect();
    format!("{{{}}}", items.join(", "))
}

/// The overlay and status line of a landed analysis (None for a copy).
fn overlay(landed: Landed, read: &Read, revision: u64, generation: u64) -> Result<(Overlay, String), Value> {
    let node = match read {
        Read::Copy { .. } => String::new(),
        Read::ControlPoints { node, .. } | Read::CurvatureComb { node } | Read::Continuity { node } => node.clone(),
    };
    let mut o = Overlay { kind: read.name(), node, face: None, revision, generation, lines: Vec::new(), markers: Vec::new(), counts: None };
    let status = match landed {
        Landed::Copied(clip) => return Err(clip),
        Landed::ControlPoints(cp) => {
            o.face = Some(cp.face);
            for row in &cp.rows {
                o.markers.extend(row.iter().map(|p| (v3(*p), POINT_COLOUR, POINT_PIXELS)));
                o.lines.push((row.iter().map(|p| v3(*p)).collect(), ROW_COLOUR));
            }
            let n: usize = cp.rows.iter().map(Vec::len).sum();
            format!("{n} control points (edit via Ops.set_control_points; proportional falloff in scripting)")
        }
        Landed::CurvatureComb(comb) => {
            o.lines = comb.lines.iter().map(|[a, b]| (vec![v3(*a), v3(*b)], COMB_COLOUR)).collect();
            format!("Curvature comb: {} line(s)", comb.lines.len())
        }
        Landed::Continuity(c) => {
            o.lines = c.edges.iter().map(|e| (e.points.iter().map(|p| v3(*p)).collect(), grade_colour(&e.continuity))).collect();
            let text = format!("Continuity: {}", counts_text(&c.counts));
            o.counts = Some(c.counts);
            text
        }
    };
    Ok((o, status))
}

/// JobResults (after `sync::receive`, so the shown revision is this
/// frame's): the read that came back, and the overlay dropped once the
/// shown document moved on.
fn receive(doc: Option<ResMut<CadDocument>>, redraw: Option<MessageWriter<bevy::window::RequestRedraw>>) {
    let Some(mut doc) = doc else { return };
    let outdated = doc.ops.analysis.shown.as_ref().is_some_and(|s| s.generation != doc.generation || s.revision != doc.shown_revision());
    let result = doc.ops.analysis.pending.as_ref().and_then(|p| p.job.poll());
    if outdated {
        doc.ops.analysis.shown = None;
        doc.touch();
    }
    let Some(result) = result else {
        if doc.ops.analysis.pending.is_some() {
            // Keep frames coming while the read is out (an unfocused window
            // otherwise steps only on its low-power timer).
            if let Some(mut redraw) = redraw {
                redraw.write(bevy::window::RequestRedraw);
            }
        }
        return;
    };
    let Some(pending) = doc.ops.analysis.pending.take() else { return };
    let doc = &mut *doc;
    doc.touch();
    if pending.job.generation() != doc.generation {
        // The document was replaced or reconnected: not this document's answer.
        return;
    }
    let label = pending.read.label();
    let landed = match result {
        Ok(landed) => landed,
        Err(e) => return doc.show(Err(format!("{label}: {e}"))),
    };
    match overlay(landed, &pending.read, pending.revision, pending.job.generation()) {
        Err(clip) => {
            let n = clip.get("items").and_then(Value::as_array).map_or(0, Vec::len);
            doc.ops.clipboard = Some((pending.revision, clip));
            doc.show(Ok(format!("Copied {n} item(s) with placement")));
        }
        Ok((overlay, status)) => {
            let now = doc.shown_revision();
            if pending.revision != now {
                return doc.show(Err(format!("{label}: read at revision {}, but the document is now at revision {now}; the result was dropped: run it again", pending.revision)));
            }
            doc.ops.analysis.shown = Some(overlay);
            doc.show(Ok(status));
        }
    }
}

/// Present: the overlay, over the bodies.
fn draw(doc: Option<Res<CadDocument>>, view: Option<Res<CadView>>, mut gizmos: Gizmos<ToolGizmos>) {
    let (Some(doc), Some(view)) = (doc, view) else { return };
    let Some(shown) = &doc.ops.analysis.shown else { return };
    if !view.valid || shown.generation != doc.generation || shown.revision != doc.shown_revision() {
        return;
    }
    for (points, colour) in &shown.lines {
        gizmos.linestrip(points.iter().map(|p| view.world_from_model.transform_point3(*p)), *colour);
    }
    for (p, colour, pixels) in &shown.markers {
        marker(&mut gizmos, &view, *p, *pixels, *colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_runtime::cad_client::EdgeContinuity;

    #[test]
    fn counts_print_as_robocads_status() {
        let counts: BTreeMap<String, u64> = [("boundary", 1), ("G2", 0), ("G0", 12), ("G1", 3)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        assert_eq!(counts_text(&counts), "{'G0': 12, 'G1': 3, 'G2': 0, 'boundary': 1}");
    }

    #[test]
    fn overlays_use_robocads_shapes_and_colours() {
        let read = Read::ControlPoints { node: "n1".into(), face: 2 };
        let cp = ControlPoints { node: "n1".into(), face: 2, rows: vec![vec![[0.0; 3], [0.0, 10.0, 0.0]], vec![[20.0, 0.0, 0.0], [20.0, 10.0, 0.0]]] };
        let (o, status) = overlay(Landed::ControlPoints(cp), &read, 7, 3).unwrap();
        assert_eq!((o.kind, o.node.as_str(), o.face, o.revision, o.generation), ("control_points", "n1", Some(2), 7, 3));
        assert_eq!(o.markers.len(), 4);
        assert!(o.markers.iter().all(|m| m.1 == POINT_COLOUR && m.2 == 8.0));
        assert_eq!(o.lines, vec![(vec![Vec3::ZERO, Vec3::new(0.0, 10.0, 0.0)], ROW_COLOUR), (vec![Vec3::new(20.0, 0.0, 0.0), Vec3::new(20.0, 10.0, 0.0)], ROW_COLOUR)]);
        assert_eq!(status, "4 control points (edit via Ops.set_control_points; proportional falloff in scripting)");

        let comb = CurvatureComb { node: "c".into(), lines: vec![[[5.0, 0.0, 0.0], [4.0, 0.0, 0.0]]] };
        let (o, status) = overlay(Landed::CurvatureComb(comb), &Read::CurvatureComb { node: "c".into() }, 1, 1).unwrap();
        assert_eq!(o.lines, vec![(vec![Vec3::new(5.0, 0.0, 0.0), Vec3::new(4.0, 0.0, 0.0)], COMB_COLOUR)]);
        assert_eq!(status, "Curvature comb: 1 line(s)");

        let counts: BTreeMap<String, u64> = [("G0", 1), ("G1", 0), ("G2", 0), ("boundary", 1)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let c = Continuity {
            node: "b".into(),
            edges: vec![
                EdgeContinuity { index: 0, continuity: "G0".into(), points: vec![[0.0; 3], [1.0, 0.0, 0.0]] },
                EdgeContinuity { index: 1, continuity: "boundary".into(), points: vec![[0.0; 3], [0.0, 1.0, 0.0]] },
            ],
            counts: counts.clone(),
        };
        let (o, status) = overlay(Landed::Continuity(c), &Read::Continuity { node: "b".into() }, 1, 1).unwrap();
        assert_eq!(o.lines.iter().map(|l| l.1).collect::<Vec<_>>(), vec![G0_COLOUR, BOUNDARY_COLOUR]);
        assert_eq!(o.counts, Some(counts));
        assert_eq!(status, "Continuity: {'G0': 1, 'G1': 0, 'G2': 0, 'boundary': 1}");

        let clip = json!({"robocad_clipboard": true, "items": []});
        assert_eq!(overlay(Landed::Copied(clip.clone()), &Read::Copy { ids: vec![] }, 1, 1).unwrap_err(), clip);
    }

    #[test]
    fn state_json_names_the_overlay_and_the_read() {
        let mut a = Analysis::default();
        assert_eq!(state_json(&a)["kind"], Value::Null);
        a.shown = Some(Overlay { kind: "continuity", node: "b".into(), face: None, revision: 4, generation: 1, lines: vec![], markers: vec![], counts: Some(BTreeMap::from([("G0".to_string(), 2)])) });
        let s = state_json(&a);
        assert_eq!((s["kind"].clone(), s["node"].clone(), s["revision"].clone(), s["counts"].clone()), (json!("continuity"), json!("b"), json!(4), json!({"G0": 2})));
        assert_eq!(s["reading"], Value::Null);
    }
}
