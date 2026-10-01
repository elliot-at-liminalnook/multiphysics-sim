//! P3: the one sketch interaction.
use super::SketchShape;
use crate::cad::document::CadDocument;
use bevy::prelude::*;

pub(in crate::cad) fn build(app: &mut App) {
    let _ = app;
}

/// `Flow::Sketch` starts: RoboCAD's `SketchTool.activate`.
pub(in crate::cad) fn begin(doc: &mut CadDocument, shape: SketchShape) -> Result<(), String> {
    doc.ops.sketch = Some(super::SketchState::new(shape));
    Ok(())
}
