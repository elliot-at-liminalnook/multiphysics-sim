//! The pie (radial) menu (cad-modify; RoboCAD's `RadialMenu`,
//! ui/widgets.py:821-882): entries around the point it opens at, the
//! first straight up and the rest clockwise; the entry under the pointer
//! is highlighted; a release or click on one runs it; Escape or a click
//! in the 18 px dead centre closes it.
//!
//! No intent logic: the caller owns whether it is open and where, passes
//! one action component per entry, and asks [`index_at`] which entry a
//! pointer position picks.
use super::Kit;
use bevy::prelude::*;

/// RoboCAD's radius from the centre to each entry (px).
pub(crate) const RADIUS: f32 = 78.0;
/// The dead centre: no entry within this distance (px).
pub(crate) const DEAD: f32 = 18.0;

/// The entry a pointer at `cursor` picks in a pie of `n` entries opened at
/// `centre` (RoboCAD's `_index_at`), or None in the dead centre.
pub(crate) fn index_at(centre: Vec2, cursor: Vec2, n: usize) -> Option<usize> {
    let _ = (centre, cursor, n);
    todo!("ui_kit::pie::index_at")
}

/// Entry `i` of `n`'s centre, relative to the pie's centre (px, y down).
pub(crate) fn slot(i: usize, n: usize) -> Vec2 {
    let _ = (i, n);
    todo!("ui_kit::pie::slot")
}

impl Kit<'_> {
    /// The pie, absolutely placed at `at` (window logical px): one pill per
    /// entry `(label, action, enabled)`, `hover` highlighted, and a centre
    /// dot. `label` is its accessible label ("View radial menu").
    pub(crate) fn pie<A: Component>(&self, commands: &mut Commands, at: Vec2, label: &str, entries: Vec<(String, A, bool)>, hover: Option<usize>) -> Entity {
        let _ = (commands, at, label, entries, hover);
        todo!("Kit::pie")
    }
}
