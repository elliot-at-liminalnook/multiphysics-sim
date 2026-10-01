//! CAD mode's command surfaces (cad-modify), all built from the op
//! catalogue (`super::ops::CATALOGUE`) and RoboCAD's command table
//! (`registry`) on the kit's widgets: the tools toolbar (`toolbar`), the
//! viewport right-click menu (`context_menu`), the Space view radial and
//! the Q selection-mode radial (`radial`), the command palette
//! (`palette`), the menus by category (`menus`) and the parameter form
//! (`form`). Every entry carries the same `CadAction` REST and
//! `system_ui` send (`CadInvoke`, the existing CAD actions); opening and
//! closing a surface is `CadSurface`.
use serde::{Deserialize, Serialize};

/// A command surface to open, or none (`CadSurface { surface }`).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Surface {
    /// Close whatever is open.
    Closed,
    /// The command palette, searching `query`.
    Palette {
        #[serde(default)]
        query: String,
    },
    /// The menu of one RoboCAD category ("Modify").
    Menu { category: String },
    /// The viewport right-click menu at `at` (window logical px; the cursor when absent).
    Context {
        #[serde(default)]
        at: Option<[f32; 2]>,
    },
    /// The Space view radial at `at`.
    ViewRadial {
        #[serde(default)]
        at: Option<[f32; 2]>,
    },
    /// The Q selection-mode radial at `at`.
    SelectRadial {
        #[serde(default)]
        at: Option<[f32; 2]>,
    },
}

/// The surface open now (`CadDocument::ops.surface`): what is shown and
/// where, the palette's query and highlighted row, the radial's hovered entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    pub surface: Surface,
    /// Where it opened (window logical px).
    pub at: [f32; 2],
    /// The palette's highlighted row, or a radial's hovered entry.
    pub highlight: Option<usize>,
}

use super::actions::{CadAction, Cx};
use crate::app::actions::Call;
use sim_api::Outcome;

/// `CadSurface`: open or close a command surface (from `actions::handle`).
pub(super) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    let _ = (action, call, cx);
    todo!("surfaces::handle")
}

/// `CadInvoke` of a RoboCAD command id that is not a catalogue operation
/// (`registry`): its native CAD action (undo, fit, a selection mode, …),
/// RoboCAD's `POST /commands/{id}` when its desktop window serves the
/// document, or a refusal naming the epic that owns it or "GUI-only".
pub(super) fn invoke_command(id: &str, call: &mut Call, cx: &mut Cx) -> Outcome {
    let _ = (id, call, cx);
    todo!("surfaces::invoke_command")
}

/// CAD mode's surfaces' systems (Input keys, Present drawing).
pub(super) fn build(app: &mut bevy::prelude::App) {
    let _ = app;
}
