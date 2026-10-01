//! The UI side of the run thread: [`Gallery`] (the mode's resource) sends
//! commands, keeps the generation it asked for and takes only frames at or
//! after it (`RunThread::latest`), so a frame from before a switch is never
//! shown. Also the mode's controls (`system_ui`, the panel's buttons) and
//! `phenomena_state`.
use super::actions::PhenomenaAction;
use super::run::{self, Command, Frame, MAX_SPEED, MIN_SPEED, Op};
use super::ExhibitRef;
use crate::jobs::RunThread;
use bevy::prelude::*;
use serde_json::{Value, json};

/// Phenomena mode's state: the run thread and what the window shows.
#[derive(Resource)]
pub(crate) struct Gallery {
    run: RunThread<Command, Frame>,
    /// The generation the UI asked for last (bumped by every select, next,
    /// previous, knob change and reset); older frames are dropped.
    pub(crate) requested: u64,
    /// The last command's sequence number.
    seq: u64,
    /// The latest frame at `requested` or later (None: none yet).
    pub(crate) frame: Option<Frame>,
    /// The status line: a click's or key's refusal, naming the reason.
    pub(crate) status: Option<Result<String, String>>,
    /// Bumped whenever `status` changes (the REST snapshot follows it).
    pub(crate) status_revision: u64,
    /// The knob value while its slider is held (a local preview; the release
    /// commits it as `PhenomenaKnob { value }`), with the requested
    /// generation it was taken under: a switch while held (a key, REST)
    /// rebuilds the panel and must not commit the old exhibit's value.
    pub(crate) knob_drag: Option<(f64, u64)>,
}

impl Gallery {
    /// The gallery on the built-in exhibits, opening `selector` (an exhibit
    /// number or title fragment; None: the first).
    pub(crate) fn open(selector: Option<String>) -> Self {
        Self::with_run(run::spawn(selector, 1))
    }
    /// On a run thread whose first frame has generation 1.
    pub(crate) fn with_run(run: RunThread<Command, Frame>) -> Self {
        Self { run, requested: 1, seq: 0, frame: None, status: None, status_revision: 0, knob_drag: None }
    }

    /// The latest frame, once the exhibits are built.
    pub(crate) fn ready(&self) -> Option<&Frame> {
        self.frame.as_ref().filter(|f| !f.catalogue.is_empty())
    }
    /// Why nothing can be done yet.
    pub(crate) fn not_ready(&self) -> String {
        match self.frame.as_ref().and_then(|f| f.failed.as_ref()) {
            Some(failed) => format!("the exhibits could not be built: {failed}"),
            None if self.run.finished() => "the phenomena run thread stopped before the exhibits were built".into(),
            None => "the exhibits are still being built (on the phenomena-run thread); try again in a moment".into(),
        }
    }
    /// The run thread has returned (or panicked).
    pub(crate) fn stopped(&self) -> bool {
        self.run.finished()
    }
    /// The `seq` of the last command the shown frame includes.
    pub(crate) fn applied(&self) -> u64 {
        self.frame.as_ref().map_or(0, |f| f.seq)
    }

    /// Send `op`; a switch bumps the requested generation. Returns its `seq`.
    pub(crate) fn send(&mut self, op: Op) -> Result<u64, String> {
        if op.switches() {
            self.requested += 1;
        }
        self.seq += 1;
        self.run.send(Command { seq: self.seq, generation: self.requested, op }).map_err(|_| "the phenomena run thread has stopped; switch to another mode and back to restart it".to_string())?;
        Ok(self.seq)
    }

    /// JobResults: take the newest frame if it is at the requested
    /// generation or later and not the one shown. True when one was taken.
    pub(crate) fn receive(&mut self) -> bool {
        let newest = self.run.lock().tick;
        if self.frame.as_ref().is_some_and(|f| f.tick == newest) {
            return false;
        }
        match self.run.latest(self.requested) {
            Some(frame) => {
                self.frame = Some(frame);
                true
            }
            None => false,
        }
    }

    pub(crate) fn show(&mut self, status: Option<Result<String, String>>) {
        if self.status != status {
            self.status = status;
            self.status_revision += 1;
        }
    }

    /// The 1-based number of the exhibit shown (None before the exhibits are built).
    pub(crate) fn shown_number(&self) -> Option<usize> {
        self.ready().map(|f| f.current + 1)
    }
}

/// One of the mode's controls: what `system_ui` lists and the panel's
/// buttons are (their `Enabled` follows `ready`).
pub(crate) struct Control {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) action: PhenomenaAction,
    pub(crate) ready: Result<(), String>,
}

/// The mode's controls, in the order `system_ui` lists them. Before the
/// exhibits are built only the fixed controls are listed, each disabled
/// naming why. At the knob's or the speed's limit the step control is
/// disabled (a key still clamps there, as sim-app did).
pub(crate) fn controls(g: &Gallery) -> Vec<Control> {
    let frame = g.ready();
    let base: Result<(), String> = match frame {
        Some(_) => Ok(()),
        None => Err(g.not_ready()),
    };
    let mut out = Vec::new();
    if let Some(f) = frame {
        for (i, e) in f.catalogue.iter().enumerate() {
            out.push(Control { id: format!("phenomena:exhibit:{}", i + 1), label: format!("Exhibit {}: {}", i + 1, e.title), action: PhenomenaAction::PhenomenaSelect { exhibit: ExhibitRef::Number(i + 1) }, ready: Ok(()) });
        }
    }
    let paused = frame.is_some_and(|f| f.paused);
    let speed = frame.map_or(1.0, |f| f.speed);
    let knob = frame.and_then(|f| f.knob.as_ref());
    let limit = |at: bool, why: String| -> Result<(), String> {
        match &base {
            Err(e) => Err(e.clone()),
            Ok(()) if at => Err(why),
            Ok(()) => Ok(()),
        }
    };
    let fixed = [
        ("phenomena:next", "Next exhibit".to_string(), PhenomenaAction::PhenomenaNext, base.clone()),
        ("phenomena:previous", "Previous exhibit".to_string(), PhenomenaAction::PhenomenaPrevious, base.clone()),
        ("phenomena:reset", "Reset".to_string(), PhenomenaAction::PhenomenaReset, base.clone()),
        ("phenomena:pause", if paused { "Run" } else { "Pause" }.to_string(), PhenomenaAction::PhenomenaPause { paused: None }, base.clone()),
        ("phenomena:speed_up", "Faster ×2".to_string(), PhenomenaAction::PhenomenaSpeed { speed: None, steps: Some(1) }, limit(speed >= MAX_SPEED, "already at the fastest speed, ×64".into())),
        ("phenomena:speed_down", "Slower ÷2".to_string(), PhenomenaAction::PhenomenaSpeed { speed: None, steps: Some(-1) }, limit(speed <= MIN_SPEED, "already at the slowest speed, ×1/64".into())),
        (
            "phenomena:knob_up",
            "Knob + step".to_string(),
            PhenomenaAction::PhenomenaKnob { value: None, steps: Some(1.0) },
            limit(knob.is_some_and(|k| k.value >= k.max), knob.map_or_else(String::new, |k| format!("the knob is at its maximum, {} {}", k.max, k.unit).trim_end().to_string())),
        ),
        (
            "phenomena:knob_down",
            "Knob − step".to_string(),
            PhenomenaAction::PhenomenaKnob { value: None, steps: Some(-1.0) },
            limit(knob.is_some_and(|k| k.value <= k.min), knob.map_or_else(String::new, |k| format!("the knob is at its minimum, {} {}", k.min, k.unit).trim_end().to_string())),
        ),
    ];
    for (id, label, action, ready) in fixed {
        out.push(Control { id: id.to_string(), label, action, ready });
    }
    out
}

/// `phenomena_state` (and `state`): the gallery as this window shows it.
/// Nothing is invented: before the exhibits are built the fields that need
/// them are absent and `ready` is false.
pub(crate) fn state_json(g: &Gallery) -> Value {
    let status = g.status.as_ref().map(|s| match s {
        Ok(t) => json!({"ok": true, "text": t}),
        Err(e) => json!({"ok": false, "text": e}),
    });
    let Some(f) = g.ready() else {
        return json!({
            "ready": false,
            "building": g.frame.as_ref().is_none_or(|f| f.failed.is_none()) && !g.stopped(),
            "message": g.not_ready(),
            "requested_generation": g.requested,
            "status": status,
        });
    };
    let entry = f.entry();
    json!({
        "ready": true,
        "exhibits": f.catalogue.iter().enumerate().map(|(i, e)| json!({"number": i + 1, "title": e.title})).collect::<Vec<_>>(),
        "current": {"number": f.current + 1, "title": entry.title, "summary": entry.summary},
        "knob": f.knob.as_ref().map(|k| json!({"label": k.label, "unit": k.unit, "min": k.min, "max": k.max, "step": k.step, "value": k.value})),
        "readouts": f.readouts.iter().map(|r| json!({"label": r.label, "value": r.value, "unit": r.unit})).collect::<Vec<_>>(),
        "verdict": f.verdict,
        "signal": {"label": f.signal.0, "value": f.signal.1},
        "chart": f.chart,
        "time": f.time,
        "time_unit": f.time_unit,
        "speed": f.speed,
        "paused": f.paused,
        "error": f.error,
        "notice": f.notice,
        // The run thread returned (a panic outside the exhibit's guarded calls): the frame shown is its last.
        "stopped": g.stopped().then(|| "the phenomena run thread has stopped; switch to another mode and back to restart it"),
        "generation": f.generation,
        "requested_generation": g.requested,
        "applied": f.seq,
        "status": status,
    })
}

/// A control's action as its REST command (what `system_ui` lists).
pub(crate) fn rest_form(action: &PhenomenaAction) -> Value {
    match action {
        PhenomenaAction::State => json!({"command": "state"}),
        PhenomenaAction::PhenomenaState => json!({"command": "phenomena_state"}),
        PhenomenaAction::PhenomenaSelect { exhibit } => json!({"command": "phenomena_select", "exhibit": match exhibit { ExhibitRef::Number(n) => json!(n), ExhibitRef::Title(t) => json!(t) }}),
        PhenomenaAction::PhenomenaNext => json!({"command": "phenomena_next"}),
        PhenomenaAction::PhenomenaPrevious => json!({"command": "phenomena_previous"}),
        PhenomenaAction::PhenomenaKnob { value, steps } => json!({"command": "phenomena_knob", "value": value, "steps": steps}),
        PhenomenaAction::PhenomenaReset => json!({"command": "phenomena_reset"}),
        PhenomenaAction::PhenomenaPause { paused } => json!({"command": "phenomena_pause", "paused": paused}),
        PhenomenaAction::PhenomenaSpeed { speed, steps } => json!({"command": "phenomena_speed", "speed": speed, "steps": steps}),
        PhenomenaAction::SystemUi(args) => json!({"command": "system_ui", "action": args.get("action")}),
    }
}
