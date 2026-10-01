//! The wall check's thin points, display only: RoboCAD's
//! `("point", (r.point, (1.0, 0.2, 0.2), 9.0))` temporary shapes
//! (ui/app.py:1123), drawn on the tools' gizmo group (over the bodies, as
//! RoboCAD draws its temporary shapes) with model mm mapped through
//! `CadView::world_from_model`. Drawn only while their revision is the
//! shown one (`checks::drawn`); a stale set stays in the state, undrawn.
use super::checks;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::document::CadDocument;
use crate::cad::transform::{ToolGizmos, marker};
use crate::cad::view::CadView;
use bevy::prelude::*;

/// RoboCAD's thin-wall point colour (ui/app.py:1123).
pub const THIN_COLOUR: Color = Color::srgb(1.0, 0.2, 0.2);
/// RoboCAD's point size, pixels (ui/app.py:1123).
pub const THIN_PIXELS: f32 = 9.0;

/// CadPlugin: the points (Present).
pub(super) fn build(app: &mut App) {
    app.add_systems(Update, draw.in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)));
}

/// Present: one marker per thin point at the shown revision.
fn draw(doc: Option<Res<CadDocument>>, view: Option<Res<CadView>>, mut gizmos: Gizmos<ToolGizmos>) {
    let (Some(doc), Some(view)) = (doc, view) else { return };
    if !view.valid {
        return;
    }
    let Some(wall) = checks::drawn(&doc) else { return };
    for p in &wall.points {
        marker(&mut gizmos, &view, *p, THIN_PIXELS, THIN_COLOUR);
    }
}
