//! The catalogue's data: one `OpEntry` per cad-modify operation, with
//! RoboCAD's ids, labels, categories, keys, prompts, defaults, ranges and
//! messages, each citing its RoboCAD source.
use super::*;

/// Every operation, in RoboCAD's registry order (`ui/app.py` `_build_commands`).
pub(crate) static CATALOGUE: &[OpEntry] = &[];
