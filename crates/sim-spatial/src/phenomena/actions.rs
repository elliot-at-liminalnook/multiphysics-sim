//! Phenomena mode's actions (native-viewer.md §3). Every intent is a
//! [`PhenomenaAction`]: keys (`keys`), the panel's buttons and knob slider
//! (`panel`), `system_ui` and REST all write `Act<PhenomenaAction>`, and
//! [`apply`] (ViewerSet::Actions) is the one handler. It validates against
//! the shown frame and sends the run thread a command; the thread applies it.
use super::ExhibitRef;
use super::gallery::{Control, Gallery, controls, rest_form, state_json};
use super::run::Op;
use crate::app::actions::{self, Act, Call, InFlight, Origin, PHENOMENA, Replies, Spec, spec};
use crate::app::ViewerMode;
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sim_api::Outcome;

/// Every intent of phenomena mode. REST commands keep their JSON shape
/// (`{"command": name, ...args}`).
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum PhenomenaAction {
    /// `state` (as every mode answers it): the same as `phenomena_state`.
    State,
    /// The gallery as shown: exhibits, current, knob, readouts, verdict,
    /// signal, time, speed, paused, error, frame generation.
    PhenomenaState,
    /// Open an exhibit by number or title fragment (digits 1–0, the list).
    PhenomenaSelect { exhibit: ExhibitRef },
    /// The next exhibit, wrapping (`]`, N, Tab).
    PhenomenaNext,
    /// The previous exhibit, wrapping (`[`, P, Shift+Tab).
    PhenomenaPrevious,
    /// Set the knob to `value`, or nudge it by `steps` knob steps (←/→ one,
    /// Shift five); exactly one. Clamped to the knob's range and rounded to
    /// its step, as sim-app did.
    PhenomenaKnob {
        #[serde(default)]
        value: Option<f64>,
        #[serde(default)]
        steps: Option<f64>,
    },
    /// Rebuild the exhibit's simulation at its current knob (R).
    PhenomenaReset,
    /// Pause or run (`paused`; absent toggles, as Space does).
    PhenomenaPause {
        #[serde(default)]
        paused: Option<bool>,
    },
    /// Set the speed to `speed`, or double (`steps` 1, ↑) or halve (`steps`
    /// −1, ↓) it; exactly one. Clamped to [1/64, 64].
    PhenomenaSpeed {
        #[serde(default)]
        speed: Option<f64>,
        #[serde(default)]
        steps: Option<i32>,
    },
    /// `system_ui` in phenomena mode: `{action: {operation: controls | activate, id?, ui_revision?}}`.
    SystemUi(Map<String, Value>),
}

/// How every changing command answers REST (appended to its description).
const ANSWER: &str = " Answers once the run thread has applied it, with phenomena_state as it is then (Pending until then; a cancel ends the wait, not the command, which was already sent). Refused, naming the reason, while the exhibits are still being built.";

impl actions::Action for PhenomenaAction {
    fn commands() -> Vec<Spec> {
        let changing = |text: &str| format!("{text}{ANSWER}");
        vec![
            spec("state", PHENOMENA, json!({}), "Phenomena mode: the same answer as phenomena_state, plus viewer_mode."),
            spec("phenomena_state", PHENOMENA, json!({}), "Phenomena mode: the gallery as this window shows it: ready (false while the exhibits are being built, with message), exhibits (number, title), current (1-based number, title, summary), knob (label, unit, min, max, step, value), readouts (label, value, unit), verdict, signal (label, value), chart (samples at 30 Hz of real time, the last 1800), time and time_unit (the exhibit's own clock), speed, paused, error (the exhibit's simulation error verbatim, or null; the run stops until reset or another exhibit), notice (an --exhibit no title matched, or null), status (the last refusal of a key or click, or null), generation (bumped by every select, next, previous, knob change and reset; older frames are never shown), requested_generation and applied (the sequence number of the last command the shown frame includes)."),
            spec("phenomena_select", PHENOMENA, json!({"exhibit": 1}), changing("Phenomena mode: open an exhibit by 1-based number or by title fragment (case-insensitive, the first title containing it), the same rule as --exhibit. The chart restarts; the error clears; the exhibit continues from where it was left (reset rebuilds it). Refused, naming the reason, for a number out of range or a fragment no title contains.")),
            spec("phenomena_next", PHENOMENA, json!({}), changing("Phenomena mode: the next exhibit (wraps), as the ] / N / Tab keys.")),
            spec("phenomena_previous", PHENOMENA, json!({}), changing("Phenomena mode: the previous exhibit (wraps), as the [ / P / Shift+Tab keys.")),
            spec("phenomena_knob", PHENOMENA, json!({"value": 1.0}), changing("Phenomena mode: set the exhibit's one knob to value, or nudge it by steps knob steps (the ←/→ keys send ±1, with Shift ±5); exactly one of the two, a finite number. Clamped to the knob's [min, max] and rounded to its step; the exhibit rebuilds its simulation at the new value and the chart restarts.")),
            spec("phenomena_reset", PHENOMENA, json!({}), changing("Phenomena mode: rebuild the exhibit's simulation at its current knob (the R key); the chart restarts and the error clears.")),
            spec("phenomena_pause", PHENOMENA, json!({}), changing("Phenomena mode: pause or run (paused: true | false; absent toggles, as the Space key).")),
            spec("phenomena_speed", PHENOMENA, json!({"steps": 1}), changing("Phenomena mode: the speed multiplier on the exhibit's own time scale: speed (absolute, a positive number), or steps (1 doubles, -1 halves, as the ↑/↓ keys); exactly one. Clamped to [1/64, 64]. Real time per tick is clamped to 0.05 s, so a slow frame never jumps the simulation.")),
            spec("system_ui", PHENOMENA, json!({"action": {"operation": "controls"}}), "Phenomena mode: its controls (phenomena:exhibit:<n> for each exhibit, phenomena:next, phenomena:previous, phenomena:reset, phenomena:pause, phenomena:speed_up, phenomena:speed_down, phenomena:knob_up, phenomena:knob_down), each with enabled and disabled_reason, then the mode switcher's mode:* controls; activate {id} writes the same action a click does (and answers as that action's command does). Ids are stable names: ui_revision is reported (the requested generation) but not checked."),
        ]
    }
    fn controls() -> &'static [&'static str] {
        &["phenomena:exhibit:<n>", "phenomena:next", "phenomena:previous", "phenomena:reset", "phenomena:pause", "phenomena:speed_up", "phenomena:speed_down", "phenomena:knob_up", "phenomena:knob_down"]
    }
}

/// Actions: phenomena mode's one apply system. A click's or key's refusal
/// is the header's status line (a success clears it); a REST caller gets
/// it, or waits for the run thread to apply its command.
pub(crate) fn apply(mut messages: ResMut<Messages<Act<PhenomenaAction>>>, mut in_flight: ResMut<InFlight<PhenomenaAction>>, mut replies: ResMut<Replies>, gallery: Option<ResMut<Gallery>>) {
    let Some(mut gallery) = gallery else {
        actions::apply(&mut messages, &mut in_flight, &mut replies, |_, _| Outcome::Done(Err("phenomena mode has no gallery open".into())));
        return;
    };
    actions::apply(&mut messages, &mut in_flight, &mut replies, |action, call| {
        let outcome = handle(action, call, &mut gallery);
        match call.origin {
            Origin::Rest(_) => outcome,
            origin => {
                if origin == Origin::Ui {
                    match &outcome {
                        Outcome::Done(Err(e)) => gallery.show(Some(Err(e.clone()))),
                        _ => gallery.show(None),
                    }
                }
                Outcome::Done(Ok(Value::Null))
            }
        }
    });
}

/// One action, from any entry point.
fn handle(action: &PhenomenaAction, call: &mut Call, g: &mut Gallery) -> Outcome {
    // A REST caller waiting for its command (also through system_ui activate).
    if let Some(seq) = call.continuation.get("seq").and_then(Value::as_u64) {
        return wait(g, call, seq);
    }
    let op = match action {
        PhenomenaAction::State | PhenomenaAction::PhenomenaState => return Outcome::Done(Ok(state_json(g))),
        PhenomenaAction::SystemUi(args) => return system_ui(g, call, args),
        other => match op_for(other, g) {
            Ok(op) => op,
            Err(e) => return Outcome::Done(Err(e)),
        },
    };
    match g.send(op) {
        Err(e) => Outcome::Done(Err(e)),
        Ok(seq) if call.rest() => {
            *call.continuation = json!({"seq": seq});
            Outcome::Pending
        }
        Ok(_) => Outcome::Done(Ok(Value::Null)),
    }
}

/// The run thread's command for a changing action, validated against the
/// shown frame (the catalogue resolves a selection; the thread applies a
/// knob nudge or speed step to its own current value).
fn op_for(action: &PhenomenaAction, g: &Gallery) -> Result<Op, String> {
    let frame = g.ready().ok_or_else(|| g.not_ready())?;
    Ok(match action {
        PhenomenaAction::PhenomenaSelect { exhibit } => Op::Select(exhibit.resolve(&frame.titles())?),
        PhenomenaAction::PhenomenaNext => Op::Step(1),
        PhenomenaAction::PhenomenaPrevious => Op::Step(-1),
        PhenomenaAction::PhenomenaKnob { value, steps } => match (*value, *steps) {
            (Some(v), None) if v.is_finite() => Op::Knob { value: Some(v), steps: None },
            (None, Some(n)) if n.is_finite() => Op::Knob { value: None, steps: Some(n) },
            (Some(_), None) | (None, Some(_)) => return Err("phenomena_knob: value and steps must be finite numbers".into()),
            _ => return Err("phenomena_knob takes value or steps, exactly one".into()),
        },
        PhenomenaAction::PhenomenaReset => Op::Reset,
        PhenomenaAction::PhenomenaPause { paused } => Op::Pause(*paused),
        PhenomenaAction::PhenomenaSpeed { speed, steps } => match (*speed, *steps) {
            (Some(s), None) if s.is_finite() && s > 0.0 => Op::Speed { speed: Some(s), steps: None },
            (Some(s), None) => return Err(format!("phenomena_speed: speed must be a positive number (got {s})")),
            (None, Some(n)) => Op::Speed { speed: None, steps: Some(n) },
            _ => return Err("phenomena_speed takes speed or steps, exactly one".into()),
        },
        PhenomenaAction::State | PhenomenaAction::PhenomenaState | PhenomenaAction::SystemUi(_) => return Err("not a changing action".into()),
    })
}

/// A REST caller's command: the state once the shown frame includes it.
fn wait(g: &Gallery, call: &Call, seq: u64) -> Outcome {
    if g.applied() >= seq {
        return Outcome::Done(Ok(state_json(g)));
    }
    if call.cancelled {
        return Outcome::Done(Err("cancelled waiting; the command was already sent to the run thread and still applies: see phenomena_state".into()));
    }
    if g.stopped() {
        return Outcome::Done(Err("the phenomena run thread stopped before applying this command".into()));
    }
    Outcome::Pending
}

/// `system_ui`: the controls, or one activated through this handler (the
/// same action a click writes).
fn system_ui(g: &mut Gallery, call: &mut Call, args: &Map<String, Value>) -> Outcome {
    let action = args.get("action").cloned().unwrap_or(Value::Null);
    match action["operation"].as_str() {
        Some("controls") => {
            let items: Vec<Value> = controls(g)
                .into_iter()
                .map(|c| json!({"id": c.id, "label": c.label, "enabled": c.ready.is_ok(), "disabled_reason": c.ready.err(), "action": rest_form(&c.action)}))
                .collect();
            Outcome::Done(Ok(json!({"ui_revision": g.requested, "ready": g.ready().is_some(), "controls": items, "state": state_json(g)})))
        }
        Some("activate") => {
            let Some(id) = action["id"].as_str() else { return Outcome::Done(Err("system_ui activate needs an id; request controls".into())) };
            match controls(g).into_iter().find(|c| c.id == id) {
                None => Outcome::Done(Err(format!("unknown control {id}; request controls"))),
                Some(Control { id, ready: Err(why), .. }) => Outcome::Done(Err(format!("{id} is disabled: {why}"))),
                Some(Control { action, ready: Ok(()), .. }) => handle(&action, call, g),
            }
        }
        _ => Outcome::Done(Err("system_ui in phenomena mode: operation controls, or activate with a control id (phenomena:* or mode:*)".into())),
    }
}

/// Present: `/v1/state` (with `viewer_mode`) and `/v1/phenomena_state` when
/// the shown frame, the requested generation or the status line changed, at
/// most every 100 ms.
pub(crate) fn publish(rest: Option<ResMut<crate::rest::Rest>>, gallery: Option<Res<Gallery>>, mut last: Local<Option<(u64, u64, u64)>>) {
    let (Some(mut rest), Some(g)) = (rest, gallery) else { return };
    if g.is_added() {
        *last = None;
    }
    let key = (g.requested, g.frame.as_ref().map_or(0, |f| f.tick), g.status_revision);
    if *last == Some(key) || !rest.0.snapshot_due() {
        return;
    }
    *last = Some(key);
    let state = state_json(&g);
    let mut shown = state.clone();
    shown["viewer_mode"] = json!(ViewerMode::Phenomena.name());
    rest.0.publish("phenomena_state", state);
    rest.0.publish("state", shown);
}
