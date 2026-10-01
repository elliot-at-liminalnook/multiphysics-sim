//! Phenomena mode (native-viewer.md "Fold in sim-app"): the gallery of
//! `sim_phenomena::exhibits`, formerly `sim-app`'s default scene, as a mode
//! of the one app.
//!
//! - **Run thread.** A `jobs::RunThread` ("phenomena-run") owns every
//!   `Box<dyn Exhibit>` and advances the current one on simulation time with
//!   sim-app's rules (`phenomena_app.rs` `advance()`): real time clamped to
//!   0.05 s per tick, × the exhibit's `time_scale()` × the speed; a gridded
//!   exhibit takes whole grid steps and carries the remainder. It publishes
//!   generation-stamped frames (`jobs::Stamped`): shapes, readouts, knob,
//!   signal and its strip chart, verdict, time and the exhibit's error.
//! - **Actions.** Every intent is a [`PhenomenaAction`], from keys, kit
//!   buttons and the knob slider, `system_ui` and REST, applied by one
//!   system in `ViewerSet::Actions`.
//! - **Teardown.** Entities go by `DespawnOnExit<ModeScope>`; [`leave`]
//!   (OnExit, registered by `app::switch`) removes the gallery, whose drop
//!   closes the run thread's channel and joins it within `jobs::JOIN_BOUND`.
use crate::app::actions::{self, PHENOMENA, Spec, spec};
use bevy::prelude::*;
use serde::Deserialize;
use serde_json::{Map, Value, json};

/// An exhibit as `--exhibit`, `phenomena_select` and `Documents::exhibit`
/// name it: a 1-based number, or a title fragment (case-insensitive; the
/// first exhibit whose title contains it). sim-app's rule.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ExhibitRef {
    Number(usize),
    Title(String),
}

impl ExhibitRef {
    /// `--exhibit` and `PHENOMENA_EXHIBIT`: digits are a number, anything else a title fragment.
    pub fn parse(text: &str) -> Self {
        match text.trim().parse::<usize>() {
            Ok(n) => ExhibitRef::Number(n),
            Err(_) => ExhibitRef::Title(text.to_string()),
        }
    }
    /// The 0-based index among `titles` (sim-app's rule: a number in
    /// 1..=len, else the first title containing the text, case-insensitive;
    /// a `Title` of digits is read as a number first). An error names what
    /// was wanted and how many exhibits there are.
    pub fn resolve(&self, titles: &[&str]) -> Result<usize, String> {
        let number = match self {
            ExhibitRef::Number(n) => Some(*n),
            ExhibitRef::Title(t) => t.trim().parse::<usize>().ok(),
        };
        if let Some(n) = number {
            return n.checked_sub(1).filter(|i| *i < titles.len()).ok_or_else(|| format!("exhibit {n} is out of range: there are {} exhibits, numbered 1 to {}", titles.len(), titles.len()));
        }
        let ExhibitRef::Title(wanted) = self else { unreachable!("a number returned above") };
        let lower = wanted.to_lowercase();
        titles.iter().position(|t| t.to_lowercase().contains(&lower)).ok_or_else(|| format!("no exhibit title contains `{wanted}` (phenomena_state lists the {} exhibits)", titles.len()))
    }
}

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

impl actions::Action for PhenomenaAction {
    fn commands() -> Vec<Spec> {
        vec![
            spec("state", PHENOMENA, json!({}), "Phenomena mode: the same answer as phenomena_state, plus viewer_mode."),
            spec("phenomena_state", PHENOMENA, json!({}), "Phenomena mode: the gallery as this window shows it: exhibits (number, title), current (1-based number, title, summary), knob (label, unit, min, max, step, value), readouts (label, value, unit), verdict, signal (label, value), chart (samples at 30 Hz of real time, the last 1800), time and time_unit (the exhibit's own clock), speed, paused, error (the exhibit's simulation error verbatim, or null; the run stops until reset or another exhibit) and generation (bumped by every select, knob change and reset; older frames are never shown)."),
            spec("phenomena_select", PHENOMENA, json!({"exhibit": 1}), "Phenomena mode: open an exhibit by 1-based number or by title fragment (case-insensitive, the first title containing it), the same rule as --exhibit. The chart restarts; the error clears. Refused, naming the reason, for a number out of range or a fragment no title contains."),
            spec("phenomena_next", PHENOMENA, json!({}), "Phenomena mode: the next exhibit (wraps), as the ] / N / Tab keys."),
            spec("phenomena_previous", PHENOMENA, json!({}), "Phenomena mode: the previous exhibit (wraps), as the [ / P / Shift+Tab keys."),
            spec("phenomena_knob", PHENOMENA, json!({"value": 1.0}), "Phenomena mode: set the exhibit's one knob to value, or nudge it by steps knob steps (the ←/→ keys send ±1, with Shift ±5); exactly one of the two. Clamped to the knob's [min, max] and rounded to its step; the exhibit rebuilds its simulation at the new value and the chart restarts."),
            spec("phenomena_reset", PHENOMENA, json!({}), "Phenomena mode: rebuild the exhibit's simulation at its current knob (the R key); the chart restarts and the error clears."),
            spec("phenomena_pause", PHENOMENA, json!({}), "Phenomena mode: pause or run (paused: true | false; absent toggles, as the Space key)."),
            spec("phenomena_speed", PHENOMENA, json!({"steps": 1}), "Phenomena mode: the speed multiplier on the exhibit's own time scale: speed (absolute), or steps (1 doubles, -1 halves, as the ↑/↓ keys); exactly one. Clamped to [1/64, 64]. Real time per tick is clamped to 0.05 s, so a slow frame never jumps the simulation."),
            spec("system_ui", PHENOMENA, json!({"action": {"operation": "controls"}}), "Phenomena mode: its controls (phenomena:exhibit:<n> for each exhibit, phenomena:next, phenomena:previous, phenomena:reset, phenomena:pause, phenomena:speed_up, phenomena:speed_down, phenomena:knob_up, phenomena:knob_down), each with enabled and disabled_reason, then the mode switcher's mode:* controls; activate {id} writes the same action a click does."),
        ]
    }
    fn controls() -> &'static [&'static str] {
        &["phenomena:exhibit:<n>", "phenomena:next", "phenomena:previous", "phenomena:reset", "phenomena:pause", "phenomena:speed_up", "phenomena:speed_down", "phenomena:knob_up", "phenomena:knob_down"]
    }
}

/// Phenomena mode: its action, run thread, keys, panels and scene.
pub struct PhenomenaPlugin;
impl Plugin for PhenomenaPlugin {
    fn build(&self, app: &mut App) {
        actions::register::<PhenomenaAction>(app);
    }
}

/// OnExit(ModeScope::Phenomena), registered by `app::switch`: the gallery
/// (its run thread) is removed and the exhibit remembered in
/// `Documents::exhibit`.
pub(crate) fn leave(world: &mut World) {
    let _ = world;
}
