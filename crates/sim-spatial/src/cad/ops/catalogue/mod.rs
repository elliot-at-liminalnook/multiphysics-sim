//! The catalogue's data: one `OpEntry` per cad-modify operation, with
//! RoboCAD's ids, labels, categories, keys, prompts, defaults, ranges and
//! messages, each citing its RoboCAD source (paths under `cad/robocad/`).
//!
//! Where RoboCAD says nothing (a handler that silently does nothing on an
//! empty selection), the refusal is a plain sentence of ours; those are
//! marked "(ours)" in `source`. Ranges are the dialog's (`QInputDialog`,
//! `QSpinBox`); the tools' `NumericField`s have none. `decimals` is the
//! dialog's, else 6 (what the viewer sends: rounded to 1e-6).
//!
//! The entries live in one file per part of RoboCAD's registry, joined here
//! mostly in RoboCAD's order (`ui/app.py` `_build_commands`: Edit, Create
//! primitives, extrude and the other solids, Modify, Planes, mirror to set
//! pivot, Sketch, booleans and Inspect), then the Print part with
//! `print_split` beside it (cad-print), the Robot menu with the robot `Ops`
//! methods beside it, the Outliner's two commands (cad-organize), then the
//! `Ops` methods without a RoboCAD command.
//! The Print part is grouped, not in RoboCAD's place: RoboCAD registers
//! `tool.fastener` and `tool.clearance` among the tools (ui/app.py:358-359)
//! and the `print.*` commands at ui/app.py:390-400. The menus and the
//! command palette do not follow this order: they follow
//! `surfaces/registry.rs`, which keeps RoboCAD's.
mod arrange;
mod boolean;
mod edit_create;
mod modify;
mod organize;
mod plane;
mod print;
mod rest_only;
mod robot;
mod sketch;
mod solid;
mod view;

use super::OpEntry;
use super::kinds::BASE;

/// The parts, in RoboCAD's registry order except the grouped Print part
/// (see the module doc).
const PARTS: &[&[OpEntry]] = &[edit_create::ENTRIES, view::ENTRIES, solid::ENTRIES, modify::ENTRIES, plane::ENTRIES, arrange::ENTRIES, sketch::ENTRIES, boolean::ENTRIES, print::ENTRIES, robot::ENTRIES, organize::ENTRIES, rest_only::ENTRIES];

/// How many entries there are.
const COUNT: usize = {
    let mut n = 0;
    let mut i = 0;
    while i < PARTS.len() {
        n += PARTS[i].len();
        i += 1;
    }
    n
};

/// The parts joined, at compile time.
const fn join() -> [OpEntry; COUNT] {
    let mut out = [BASE; COUNT];
    let mut k = 0;
    let mut i = 0;
    while i < PARTS.len() {
        let part = PARTS[i];
        let mut j = 0;
        while j < part.len() {
            out[k] = part[j];
            k += 1;
            j += 1;
        }
        i += 1;
    }
    out
}

static ALL: [OpEntry; COUNT] = join();

/// Every operation, in the parts' order (RoboCAD's registry order,
/// `ui/app.py` `_build_commands`, except the grouped Print part), then the
/// `Ops` methods without a RoboCAD command (`ops.<name>`).
pub(crate) static CATALOGUE: &[OpEntry] = &ALL;
