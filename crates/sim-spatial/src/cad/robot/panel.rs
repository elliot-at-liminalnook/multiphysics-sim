//! RoboCAD's Robot dock (ui/widgets.py:1234-1366) as a section of CAD
//! mode's right dock: the summary line ("n bodies, n joints, n DoF, n
//! motors, n sensors, n cables. Ground: …. Power: …."), the Links /
//! Joints / Motors / Sensors & cables tree with each row's detail and
//! margins, the issues (or "✓ robot is valid") and RoboCAD's panel buttons
//! in its order.
//!
//! - **Data only from RoboCAD.** Every value is `doc.robot.data`'s (`GET
//!   /robot`, `/results/nodes`, `/sensors`, `/cables`, `/battery`) or the
//!   shown tree's (names, materials, which nodes are bodies), with the
//!   revision it was read at; a read that failed says so instead of a value.
//!   The texts follow `RobotPanel.refresh` (margins as `margin_text`: "+.2f"
//!   for ratios, "+.0f°C" for temperatures). RoboCAD's row and issue glyphs
//!   (⚙ ⚡ ◎ 〜 ▣ ⛔ ⚠) are not in the interface font, so rows sit under
//!   their branch heading without one and issues read "Error:" / "Warning:".
//! - **One view for everything.** [`view`] builds what the panel shows;
//!   [`draw`] spawns it, [`key`] is its text (the part is rebuilt only when
//!   it changes), [`controls`] lists its buttons and rows for `system_ui`
//!   and [`state_json`] reports it, so none of them can drift apart.
//! - **A row press** is `CadSelect {ids: [id]}`, the model tree's own path
//!   into the shared selection (`cad:robot:row:<id>`), with `picked_at` the
//!   revision the description was read at. Two presses on the
//!   same joint row within [`DOUBLE_CLICK`] also write `CadInvoke
//!   ops.set_joint` (the Edit joint form for the selected joint), as
//!   RoboCAD's `_edit` runs `robot_edit_joint`.
//! - **Buttons** write `CadInvoke` with RoboCAD's command id for the
//!   catalogue's robot dialogs (enabled as the command surfaces enable them:
//!   `surfaces::registry::ready`), and `robot::command_action(id)` for
//!   "Export sim…" (`sim.export`, what RoboCAD's button runs), "Stress
//!   overlay", "Load results…" and "Apply identification…"; a button whose
//!   action is not available is not shown.
use super::{RobotArgs, RobotOp};
use crate::app::actions::{Act, Call};
use crate::app::{ViewerMode};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::CadDocument;
use crate::cad::panel::{CadButton, material};
use crate::cad::surfaces::registry;
use crate::ui_kit::{ACCENT, ACCENT_BG, DANGER, Kit, Look, OK, SUBTLE, TEXT, Tint, VALUE, WARN, size, wrap};
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{CableMeta, Margins, NodeDetail, RobotSummary, SelectionItem, SensorMeta};
use std::collections::HashSet;
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;

/// Two presses on the same joint row within this open the Edit joint form.
pub(crate) const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// The catalogue form a joint row's double-click opens (part D's "Edit joint").
const EDIT_JOINT: &str = "ops.set_joint";

/// RoboCAD's panel buttons that are catalogue dialogs, in its order (widgets.py:1255-1266).
const CATALOGUE: [(&str, &str); 10] = [
    ("Add joint…", "robot.add_joint"),
    ("Add motor…", "robot.add_motor"),
    ("Joint from selection…", "robot.joint_dialog"),
    ("Infer joints", "robot.infer"),
    ("Assign motor…", "robot.assign_motor"),
    ("Fix together", "robot.fixed"),
    ("Toggle ground", "robot.ground"),
    ("Add sensor…", "robot.add_sensor"),
    ("Add cable…", "robot.add_cable"),
    ("Battery / control…", "robot.power"),
];

/// The rest of RoboCAD's panel buttons (widgets.py:1267-1268): actions of `robot::command_action`.
const OTHERS: [(&str, &str); 4] = [("Export sim…", "sim.export"), ("Stress overlay", "view.stress"), ("Load results…", "robot.load_results"), ("Apply identification…", "robot.apply_identification")];

/// The Robot dock's display state (on the document; reset with it).
#[derive(Debug)]
pub struct PanelState {
    /// The section's content is shown (RoboCAD's Robot dock is shown by default).
    pub open: bool,
}
impl Default for PanelState {
    fn default() -> Self {
        PanelState { open: true }
    }
}

/// A tree row: a press selects the node; a joint row's double-click edits it.
#[derive(Component, Debug)]
pub(crate) struct RobotRow {
    id: String,
    joint: bool,
}

/// How a line of text is coloured.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tone {
    Quiet,
    Good,
    Warn,
    Bad,
}

#[derive(Clone, Debug, PartialEq)]
struct Line {
    text: String,
    tone: Tone,
}
impl Line {
    fn new(text: impl Into<String>, tone: Tone) -> Self {
        Line { text: text.into(), tone }
    }
}

/// One row of the tree: RoboCAD's "Item", "Detail" and "Margin" columns.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    id: String,
    /// link | joint | motor | sensor | cable.
    kind: &'static str,
    name: String,
    detail: String,
    margin: String,
    selected: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Branch {
    title: &'static str,
    rows: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq)]
struct PanelButton {
    /// RoboCAD's command id.
    id: &'static str,
    label: &'static str,
    action: CadAction,
    ready: Result<(), String>,
}

/// What the panel shows now.
#[derive(Clone, Debug, PartialEq)]
struct View {
    open: bool,
    /// Which revision the description was read at, or why there is none.
    read: Line,
    summary: Option<String>,
    /// Failed reads and the validation scope.
    notes: Vec<Line>,
    branches: Vec<Branch>,
    issues: Vec<Line>,
    buttons: Vec<PanelButton>,
}

/// Python's `f"{v:g}"` for ordinary magnitudes (six significant digits, no trailing zeros).
fn g(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    let digits = if v == 0.0 { 0 } else { (5 - v.abs().log10().floor() as i32).clamp(0, 15) as usize };
    let s = format!("{v:.digits$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    if s == "-0" { "0".into() } else { s }
}

/// RoboCAD's `margin_text`: yield, bearing, screw and stall as "+.2f",
/// Tg and mount Tg as "+.0f°C", two spaces apart; empty without results.
fn margin_text(m: Option<&Margins>) -> String {
    let Some(m) = m else { return String::new() };
    let all = [(m.yield_margin, "yield", false), (m.bearing_margin, "bearing", false), (m.screw_shear_margin, "screw", false), (m.stall_margin, "stall", false), (m.tg_margin_c, "Tg", true), (m.mount_tg_margin_c, "mount Tg", true)];
    all.iter()
        .filter_map(|(v, label, temperature)| v.map(|v| if *temperature { format!("{label} {v:+.0}°C") } else { format!("{label} {v:+.2}") }))
        .collect::<Vec<_>>()
        .join("  ")
}

/// Node `id`'s name in the shown tree, if the tree has it (RoboCAD's `… in doc.nodes`).
fn name_in(doc: &CadDocument, id: Option<&str>) -> Option<String> {
    let id = id?;
    doc.doc.as_ref()?.nodes.iter().find(|n| n.id == id).map(|n| n.name.clone())
}

/// The shown tree's selection rule: a row is lit when any item of its node is selected.
fn selected(selection: &[SelectionItem], id: &str) -> bool {
    selection.iter().any(|s| s.0 == id)
}

/// A row press: the model tree's `CadSelect` (the node as a body item),
/// with `picked_at` set to the revision the description the row was drawn
/// from was read at (the cad-print batch has row presses carry it). It
/// changes no selection behaviour: `selection::select` uses `picked_at`
/// only for non-body items and for an Alt-menu choice (no `ids`), and a
/// row's item is a body item named in `ids`, so it is selected exactly as
/// an unstamped press would be. The stamp only records, on the action,
/// which revision the row came from; nothing stores or checks it. A node
/// that has left the shown tree is refused by name there (`validate`).
fn select_action(doc: &CadDocument, id: &str) -> CadAction {
    CadAction::CadSelect { ids: vec![id.to_string()], items: Vec::new(), extend: false, toggle: false, picked_at: doc.robot.data.read_at() }
}

/// Which revision the description describes, or why there is none.
fn read_line(doc: &CadDocument) -> Line {
    let data = &doc.robot.data;
    let shown = doc.shown_revision();
    match data.read_at() {
        Some(at) if data.current(doc) => Line::new(format!("RoboCAD's robot description at revision {at}"), Tone::Quiet),
        Some(at) if data.reading() => Line::new(format!("At revision {at}; reading revision {shown}…"), Tone::Quiet),
        Some(at) => Line::new(format!("At revision {at}; the document shows revision {shown}"), Tone::Warn),
        None if data.reading() => Line::new("Reading RoboCAD's robot description…", Tone::Quiet),
        None if !doc.connected() => Line::new(format!("No robot description: no CAD document is open ({})", doc.connection_line().0), Tone::Warn),
        None => Line::new("Waiting for RoboCAD's document…", Tone::Quiet),
    }
}

/// RoboCAD's summary line, and a note per read beside it that failed.
fn summary_line(doc: &CadDocument, s: &RobotSummary, notes: &mut Vec<Line>) -> String {
    let Some(b) = doc.robot.data.bundle.as_ref() else { return String::new() };
    let mut count = |what: &str, r: &Result<Vec<NodeDetail>, String>| match r {
        Ok(v) => v.len().to_string(),
        Err(e) => {
            notes.push(Line::new(format!("{what} could not be read: {e}"), Tone::Warn));
            "?".to_string()
        }
    };
    let sensors = count("Sensors", &b.sensors);
    let cables = count("Cables", &b.cables);
    let ground: Vec<String> = s.ground.iter().filter_map(|g| name_in(doc, Some(g.as_str()))).collect();
    let ground = if ground.is_empty() { "none (heaviest root body is used)".to_string() } else { ground.join(", ") };
    let power = match &b.battery {
        Ok(Some(bat)) => format!("{} V {}", g(bat.nominal_voltage), bat.chemistry),
        Ok(None) => "no battery (motor supply voltage)".to_string(),
        Err(e) => {
            notes.push(Line::new(format!("The battery could not be read: {e}"), Tone::Warn));
            "unknown".to_string()
        }
    };
    let results = match &b.results {
        Ok(r) => r.path.as_deref().map_or_else(String::new, |path| {
            let file = std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |f| f.to_string_lossy().into_owned());
            let stale = if r.stale == Some(true) { " (stale: the document changed since they were loaded)" } else { "" };
            format!("  Results: {file}{stale}")
        }),
        Err(e) => {
            notes.push(Line::new(format!("Results and margins could not be read: {e}"), Tone::Warn));
            String::new()
        }
    };
    let mobility = s.dof.map_or_else(|| "closed-loop mobility requires constraint analysis".to_string(), |d| format!("{d} DoF"));
    format!("{} bodies, {} joints, {mobility}, {} motors, {sensors} sensors, {cables} cables. Ground: {ground}. Power: {power}.{results}", s.links, s.joints.len(), s.motors.len())
}

/// The Links branch: the shown tree's bodies and sheets, motors excepted
/// (RoboCAD's `doc.bodies()` without `robot.kind == "motor"`).
fn links(doc: &CadDocument, s: &RobotSummary, selection: &[SelectionItem]) -> Vec<Row> {
    let Some(state) = doc.doc.as_ref() else { return Vec::new() };
    let motors: HashSet<&str> = s.motors.iter().map(|m| m.id.as_str()).collect();
    let materials: Vec<(String, String)> = state.materials.iter().filter_map(material).collect();
    let data = &doc.robot.data;
    state
        .nodes
        .iter()
        .filter(|n| matches!(n.kind.as_str(), "body" | "sheet") && !motors.contains(n.id.as_str()))
        .map(|n| {
            let mat = n.material.as_ref().and_then(|m| materials.iter().find(|(id, _)| id == m)).map_or("—", |(_, name)| name.as_str());
            let r = data.node_results(&n.id).map(|r| &r.results).filter(|r| r.get("section").and_then(Value::as_str) == Some("links"));
            let num = |key: &str| r.and_then(|r| r.get(key)).and_then(Value::as_f64);
            let mut detail = mat.to_string();
            if let Some(p) = num("peak_stress_pa") {
                detail += &format!(", peak {:.1} MPa", p / 1e6);
            }
            if let Some(t) = num("peak_temperature_c") {
                detail += &format!(", {t:.0} °C");
            }
            Row { id: n.id.clone(), kind: "link", name: n.name.clone(), detail, margin: margin_text(data.margins(&n.id)), selected: selected(selection, &n.id) }
        })
        .collect()
}

/// Everything the panel shows (`open` only decides whether it is drawn).
fn view(doc: &CadDocument, selection: &[SelectionItem]) -> View {
    let data = &doc.robot.data;
    let mut v = View { open: doc.robot.panel.open, read: read_line(doc), summary: None, notes: Vec::new(), branches: Vec::new(), issues: Vec::new(), buttons: buttons(doc, selection) };
    if let Some(e) = data.summary_error() {
        v.notes.push(Line::new(format!("The robot description could not be read: {e}"), Tone::Bad));
    }
    let Some(s) = data.summary() else { return v };
    v.summary = Some(summary_line(doc, s, &mut v.notes));
    if !s.validation_scope.is_empty() {
        v.notes.push(Line::new(format!("Checks: {}", s.validation_scope), Tone::Quiet));
    }
    v.branches.push(Branch { title: "Links", rows: links(doc, s, selection) });
    let joints = s
        .joints
        .iter()
        .map(|j| {
            let mut lim = String::new();
            if j.lower.is_some() || j.upper.is_some() {
                let f = |v: Option<f64>| match v {
                    None => "…".to_string(),
                    Some(v) if j.kind != "prismatic" => format!("{:.0}°", v.to_degrees()),
                    Some(v) => format!("{} mm", g(v)),
                };
                lim = format!("  [{}, {}]", f(j.lower), f(j.upper));
            }
            let parent = j.parent_name.as_deref().filter(|p| !p.is_empty()).unwrap_or("world");
            let child = j.child_name.as_deref().unwrap_or("?");
            let motor = j.motor_name.as_deref().map_or_else(String::new, |m| format!("  motor {m}"));
            let detail = format!("{}: {parent} → {child}{lim}{motor}", j.kind);
            Row { id: j.id.clone(), kind: "joint", name: j.name.clone(), detail, margin: margin_text(data.margins(&j.id)), selected: selected(selection, &j.id) }
        })
        .collect();
    v.branches.push(Branch { title: "Joints", rows: joints });
    let motors = s
        .motors
        .iter()
        .map(|m| {
            let on = name_in(doc, m.mounted_on.as_deref()).unwrap_or_else(|| "loose".to_string());
            let drives = name_in(doc, m.drives.as_deref()).unwrap_or_else(|| "no joint".to_string());
            let spec = m.spec_name.as_deref().or(m.spec.as_deref()).unwrap_or("?");
            Row { id: m.id.clone(), kind: "motor", name: m.name.clone(), detail: format!("{spec}: on {on}, drives {drives}"), margin: margin_text(data.margins(&m.id)), selected: selected(selection, &m.id) }
        })
        .collect();
    v.branches.push(Branch { title: "Motors", rows: motors });
    let named = |id: &str| name_in(doc, Some(id)).unwrap_or_else(|| "?".to_string());
    let mut extra = Vec::new();
    for n in data.sensors() {
        let detail = SensorMeta::of(n).map_or_else(|| "?".to_string(), |r| format!("{} on {}{}", r.kind, named(&r.body), r.joint_name.as_deref().map(|j| format!(", reads {j}")).unwrap_or_default()));
        extra.push(Row { id: n.summary.id.clone(), kind: "sensor", name: n.summary.name.clone(), detail, margin: String::new(), selected: selected(selection, &n.summary.id) });
    }
    for n in data.cables() {
        let detail = CableMeta::of(n).map_or_else(|| "?".to_string(), |r| format!("{} → {}", named(&r.from_body), named(&r.to_body)));
        extra.push(Row { id: n.summary.id.clone(), kind: "cable", name: n.summary.name.clone(), detail, margin: String::new(), selected: selected(selection, &n.summary.id) });
    }
    if !extra.is_empty() {
        v.branches.push(Branch { title: "Sensors & cables", rows: extra });
    }
    for i in &s.issues {
        let error = i.severity == "error";
        v.issues.push(Line::new(format!("{}: {}", if error { "Error" } else { "Warning" }, i.message), if error { Tone::Bad } else { Tone::Warn }));
    }
    if s.issues.is_empty() && !s.joints.is_empty() {
        v.issues.push(Line::new("✓ robot is valid", Tone::Good));
    }
    v
}

/// RoboCAD's panel buttons with their actions and whether they can run now.
fn buttons(doc: &CadDocument, selection: &[SelectionItem]) -> Vec<PanelButton> {
    let mut out: Vec<PanelButton> = CATALOGUE
        .iter()
        .map(|&(label, id)| {
            let ready = registry::command(id).map_or_else(|| Err(format!("{id} is not in RoboCAD's command list")), |cmd| registry::ready(cmd, doc, selection, &[]));
            PanelButton { id, label, action: CadAction::CadInvoke { id: id.to_string() }, ready }
        })
        .collect();
    for &(label, id) in &OTHERS {
        let Some(action) = super::command_action(id) else { continue };
        let ready = match id {
            // Display only.
            "view.stress" => Ok(()),
            // Writes a file, not the document.
            "sim.export" if doc.client.is_some() && doc.connected() => Ok(()),
            "sim.export" => Err(crate::cad::results::SIM_EXPORT_UNPORTED.to_string()),
            // Loading results and identification change RoboCAD's document.
            _ => doc.edit_refusal().map_or(Ok(()), Err),
        };
        out.push(PanelButton { id, label, action, ready });
    }
    out
}

/// `CadRobot {op: panel}`: show (`open: true`), hide or toggle the section's content.
pub(in crate::cad) fn handle(args: &RobotArgs, _call: &mut Call, cx: &mut Cx) -> Outcome {
    let open = args.open.unwrap_or(!cx.doc.robot.panel.open);
    cx.doc.robot.panel.open = open;
    cx.doc.touch();
    let message = if open { "The Robot panel is shown." } else { "The Robot panel is hidden." };
    Outcome::Done(Ok(json!({"open": open, "message": message})))
}

/// The part's text: the panel is rebuilt only when it changes.
pub(crate) fn key(doc: &CadDocument, selection: &[SelectionItem]) -> String {
    format!("{:?}", view(doc, selection))
}

fn colour(tone: Tone) -> Color {
    match tone {
        Tone::Quiet => SUBTLE,
        Tone::Good => OK,
        Tone::Warn => WARN,
        Tone::Bad => DANGER,
    }
}

/// The Robot section of the right dock.
pub(crate) fn draw(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, selection: &[SelectionItem]) {
    let v = view(doc, selection);
    p.spawn(k.section("Robot"));
    p.spawn(wrap()).with_children(|w| {
        w.spawn(k.chip(if v.open { "Hide" } else { "Show" }, CadButton(RobotArgs::of(RobotOp::Panel, Some(!v.open))), v.open, true));
        w.spawn(k.chip("Refresh", CadButton(RobotArgs::of(RobotOp::Refresh, None)), false, true));
    });
    if !v.open {
        return;
    }
    p.spawn(k.text(v.read.text.clone(), size::CAPTION, colour(v.read.tone), 0));
    if let Some(s) = &v.summary {
        p.spawn(k.text(s.clone(), size::SMALL, TEXT, 0));
    }
    for n in &v.notes {
        p.spawn(k.text(n.text.clone(), size::CAPTION, colour(n.tone), 0));
    }
    for b in &v.branches {
        p.spawn((k.text(b.title, size::CAPTION, SUBTLE, 2), Node { margin: UiRect::top(Val::Px(6.0)), ..default() }));
        if b.rows.is_empty() {
            p.spawn(k.note("none"));
        }
        for r in &b.rows {
            row(p, k, doc, r);
        }
    }
    if !v.issues.is_empty() {
        p.spawn((k.text("Issues", size::CAPTION, SUBTLE, 2), Node { margin: UiRect::top(Val::Px(6.0)), ..default() }));
        for i in &v.issues {
            p.spawn(k.text(i.text.clone(), size::SMALL, colour(i.tone), 0));
        }
    }
    p.spawn(Node { margin: UiRect::top(Val::Px(8.0)), ..wrap() }).with_children(|w| {
        for b in &v.buttons {
            w.spawn(k.button(b.label, CadButton(b.action.clone()), Look::Secondary, b.ready.is_ok()));
        }
    });
}

/// One tree row: the name, then RoboCAD's Detail and Margin columns as lines under it.
fn row(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, r: &Row) {
    let mut label = format!("{}, {}", r.name, r.kind);
    if !r.detail.is_empty() {
        label += &format!(", {}", r.detail);
    }
    if !r.margin.is_empty() {
        label += &format!(", margin {}", r.margin);
    }
    if r.selected {
        label += ", selected";
    }
    p.spawn((
        Button,
        crate::ui_kit::activation::Ordinary,
        CadButton(select_action(doc, &r.id)),
        RobotRow { id: r.id.clone(), joint: r.kind == "joint" },
        Tint::selectable(r.selected),
        AccessibleLabel::new(label),
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(1.0),
            padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
            border: UiRect::left(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BorderColor::all(if r.selected { ACCENT } else { Color::NONE }),
        BackgroundColor(if r.selected { ACCENT_BG } else { Color::NONE }),
    ))
    .with_children(|row| {
        row.spawn(k.text(r.name.clone(), size::BODY, TEXT, 1));
        if !r.detail.is_empty() {
            row.spawn(k.text(r.detail.clone(), size::DETAIL, SUBTLE, 0));
        }
        if !r.margin.is_empty() {
            row.spawn(k.mono(r.margin.clone(), size::DETAIL, VALUE));
        }
    });
}

/// `cad_state.robot.panel`.
pub(crate) fn state_json(doc: &CadDocument) -> Value {
    let v = view(doc, &[]);
    fn lines(l: &[Line]) -> Vec<String> {
        l.iter().map(|l| l.text.clone()).collect()
    }
    let mut branches = Vec::new();
    for b in &v.branches {
        let rows: Vec<Value> = b.rows.iter().map(|r| json!({"id": r.id, "kind": r.kind, "name": r.name, "detail": r.detail, "margin": r.margin})).collect();
        branches.push(json!({"title": b.title, "rows": rows}));
    }
    let buttons: Vec<Value> = v.buttons.iter().map(|b| json!({"id": b.id, "label": b.label, "enabled": b.ready.is_ok(), "disabled_reason": b.ready.as_ref().err()})).collect();
    json!({"open": v.open, "read": v.read.text, "summary": v.summary, "notes": lines(&v.notes), "branches": branches, "issues": lines(&v.issues), "buttons": buttons})
}

/// The panel's `system_ui` controls: `cad:robot:panel`, `cad:robot:refresh`,
/// and while it is shown `cad:robot:row:<node id>` per row and
/// `cad:robot:button:<RoboCAD command id>` per button.
pub(crate) fn controls(doc: &CadDocument, selection: &[SelectionItem]) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let v = view(doc, selection);
    let mut out = vec![
        ("cad:robot:panel".to_string(), if v.open { "Hide the Robot panel" } else { "Show the Robot panel" }.to_string(), RobotArgs::of(RobotOp::Panel, Some(!v.open)), Ok(())),
        ("cad:robot:refresh".to_string(), "Refresh the robot description".to_string(), RobotArgs::of(RobotOp::Refresh, None), Ok(())),
    ];
    if !v.open {
        return out;
    }
    for b in &v.branches {
        for r in &b.rows {
            out.push((format!("cad:robot:row:{}", r.id), format!("{}: {}", b.title, r.name), select_action(doc, &r.id), Ok(())));
        }
    }
    for b in v.buttons {
        out.push((format!("cad:robot:button:{}", b.id), b.label.to_string(), b.action, b.ready));
    }
    out
}

/// CadPlugin: the joint rows' double-click.
pub(crate) fn build(app: &mut App) {
    app.add_systems(Update, double_click.in_set(crate::app::InputSet::Window).run_if(in_state(ViewerMode::Cad)));
}

/// A row press at `now`: the Edit joint form when it is the second press on
/// the same joint row within [`DOUBLE_CLICK`] (the first press's `CadSelect`,
/// written by the row's `CadButton`, selected it).
fn press(last: &mut Option<(String, Instant)>, id: &str, joint: bool, now: Instant) -> Option<CadAction> {
    let double = joint && last.as_ref().is_some_and(|(prev, at)| prev == id && now.saturating_duration_since(*at) <= DOUBLE_CLICK);
    *last = if double { None } else { Some((id.to_string(), now)) };
    double.then(|| CadAction::CadInvoke { id: EDIT_JOINT.to_string() })
}

/// Input: a joint row's double-click writes `CadInvoke ops.set_joint`. The
/// last press is kept by node id (the row is respawned when the first
/// press changes the selection).
fn double_click(rows: Query<(&Interaction, &RobotRow), (Changed<Interaction>, With<Button>)>, mut last: Local<Option<(String, Instant)>>, mut out: MessageWriter<Act<CadAction>>) {
    for (interaction, row) in &rows {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if let Some(action) = press(&mut last, &row.id, row.joint, Instant::now()) {
            out.write(Act::ui(action));
        }
    }
}
