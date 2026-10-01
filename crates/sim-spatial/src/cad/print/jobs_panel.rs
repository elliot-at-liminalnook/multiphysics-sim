//! The Print jobs section of the right dock (`panel::Part::Print`):
//! RoboCAD's "Print jobs…" (ui/app.py:1265-1275) as a section beside the
//! Robot and Materials sections instead of a message box. It shows the
//! last eight of RoboCAD's print jobs as RoboCAD's lines ("kind id: state
//! n % message", or "No print jobs yet."), a "Cancel running jobs…" button
//! while one runs, RoboCAD's question "Cancel the running jobs?" with Yes
//! and No, and Close. Every button writes a `CadAction::CadPrint`
//! (`cad_print`), the same action REST and `system_ui` send. Opening it
//! starts a poll ([`super::jobs_tracker`] polls while it is open).
use super::jobs_tracker::{PrintJobTracker, UNREAD};
use super::{PrintArgs, PrintOp};
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::panel::CadButton;
use crate::ui_kit::{DANGER, Kit, Look, size, wrap};
use bevy::prelude::*;
use serde_json::{Value, json};

/// RoboCAD's text when no print job ran yet.
pub(super) const NO_JOBS: &str = "No print jobs yet.";
/// RoboCAD's question before cancelling.
pub(super) const QUESTION: &str = "Cancel the running jobs?";

fn act(op: PrintOp, open: Option<bool>, confirm: Option<bool>) -> CadAction {
    CadAction::CadPrint(PrintArgs { op, open, confirm, ..PrintArgs::default() })
}

/// `cad_print {op: jobs}`: the section shown (`Some(true)`), hidden
/// (`Some(false)`) or toggled (`None`); opening polls at once.
pub(super) fn show(doc: &mut CadDocument, open: Option<bool>) -> Result<Value, String> {
    let t = &mut doc.print.jobs;
    let now = open.unwrap_or(!t.open);
    t.open = now;
    if now {
        t.request_poll();
    } else {
        t.confirming = false;
    }
    let lines = t.lines();
    doc.touch();
    Ok(json!({"open": now, "jobs": lines, "message": if now { "Print jobs shown" } else { "Print jobs hidden" }}))
}

/// `cad_state.print.panel`.
pub(super) fn state_json(doc: &CadDocument) -> Value {
    let t = &doc.print.jobs;
    json!({"open": t.open, "confirming": t.confirming, "lines": t.lines(), "listed": t.listed, "running": t.running(None)})
}

/// `cad:print:jobs` (show or hide the section), `cad:print:cancel` (ready
/// while a job runs), and while the question is asked
/// `cad:print:cancel_yes` and `cad:print:cancel_no`: (id, label, action, ready).
pub(super) fn controls(doc: &CadDocument) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let t = &doc.print.jobs;
    let running = !t.running(None).is_empty();
    // As `jobs_tracker::cancel` refuses: nothing before a list was read.
    let none = || Err(if t.listed { "No print jobs are running" } else { UNREAD }.to_string());
    let mut out = vec![
        ("cad:print:jobs".to_string(), if t.open { "Hide print jobs" } else { "Show print jobs" }.to_string(), act(PrintOp::Jobs, Some(!t.open), None), Ok(())),
        ("cad:print:cancel".to_string(), "Cancel running jobs…".to_string(), act(PrintOp::Cancel, None, None), if running && t.listed { Ok(()) } else { none() }),
    ];
    if t.confirming {
        out.push(("cad:print:cancel_yes".to_string(), "Yes".to_string(), act(PrintOp::Cancel, None, Some(true)), if running && t.listed { Ok(()) } else { none() }));
        out.push(("cad:print:cancel_no".to_string(), "No".to_string(), act(PrintOp::Cancel, None, Some(false)), Ok(())));
    }
    out
}

/// The button of control `id`, enabled as the control is.
fn button(p: &mut ChildSpawnerCommands, k: &Kit, controls: &[(String, String, CadAction, Result<(), String>)], id: &str, label: &str, look: Look) {
    if let Some((_, _, action, ready)) = controls.iter().find(|c| c.0 == id) {
        p.spawn(k.button(label, CadButton(action.clone()), look, ready.is_ok()));
    }
}

/// The right dock's Print jobs section (`panel::Part::Print`); nothing while closed.
pub(in crate::cad) fn draw(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    let t: &PrintJobTracker = &doc.print.jobs;
    if !t.open {
        return;
    }
    let controls = controls(doc);
    p.spawn(k.title("Print jobs"));
    let lines = t.lines();
    if lines.is_empty() {
        p.spawn(k.caption(if t.listed { NO_JOBS } else { "Reading RoboCAD's print jobs…" }));
    }
    for line in lines {
        p.spawn(k.caption(line));
    }
    if let Some(e) = &t.error {
        p.spawn(k.text(format!("RoboCAD's print jobs could not be read: {e}"), size::SMALL, DANGER, 0));
    }
    if !doc.connected() {
        p.spawn(k.note("Not connected to RoboCAD: the list is as last read."));
    }
    let running = !t.running(None).is_empty();
    if t.confirming && running {
        p.spawn(k.caption(QUESTION));
        p.spawn(wrap()).with_children(|row| {
            button(row, k, &controls, "cad:print:cancel_yes", "Yes", Look::Danger);
            button(row, k, &controls, "cad:print:cancel_no", "No", Look::Secondary);
        });
    }
    p.spawn(wrap()).with_children(|row| {
        if running && !t.confirming {
            button(row, k, &controls, "cad:print:cancel", "Cancel running jobs…", Look::Secondary);
        }
        row.spawn(k.button("Close", CadButton(act(PrintOp::Jobs, Some(false), None)), Look::Ghost, true));
    });
}

/// What the section shows, as a comparable text (the jobs' states and
/// progress, the question, the error, the connection, open or closed).
pub(in crate::cad) fn key(doc: &CadDocument) -> String {
    let t = &doc.print.jobs;
    if !t.open {
        return "closed".to_string();
    }
    format!("{:?}", (t.lines(), t.confirming, t.listed, &t.error, t.running(None), doc.connected()))
}

/// CadPlugin: nothing beyond the dock's own refresh and buttons (`panel`).
pub(super) fn build(_app: &mut App) {}
