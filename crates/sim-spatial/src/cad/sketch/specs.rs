//! P3: the sketch tools as data and the one builder.
use super::{SketchShape, SketchSpec};
use crate::cad::document::CadDocument;
use crate::cad::ops::{Built, Env, OpEntry};
use serde_json::{Map, Value};

/// The tool's data.
pub(crate) fn spec(shape: SketchShape) -> &'static SketchSpec {
    let _ = shape;
    unimplemented!("P3")
}

/// `Shape::Sketch`: the shape's calls and where they go.
pub(crate) fn calls(entry: &OpEntry, shape: SketchShape, values: &Map<String, Value>, doc: &CadDocument, env: &Env) -> Result<Built, String> {
    let _ = (entry, shape, values, doc, env);
    Err("P3".into())
}
