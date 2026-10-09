//! RoboCAD's export formats as data (`io/exporters.py` and `api.py`
//! `Service.export`): each `POST /export` format with its settings,
//! labels, ranges and defaults from the desktop's `ExportDialog`
//! (ui/widgets.py:917-950), whose defaults are the settings dataclasses'
//! (`StlSettings`, `ThreeMfSettings`, `ObjSettings`, `StepSettings`).
//! IGES has no settings (the desktop exports it without a dialog); the
//! sketch SVG takes the sketch node (the desktop's selected sketch); the
//! drawing (`export_drawing_svg`) takes the views, the title and the
//! section plane (`Service.export`'s `drawing` branch; the desktop draws
//! front, top, right and iso, adds "Section A-A" when the section tool is
//! on, and titles the sheet with the document's file name).
//!
//! One validation for the form and REST ([`settings`]): an unknown setting
//! is refused by name with the format's list, a value of the wrong type or
//! outside the dialog's range names the setting, absent ones take
//! RoboCAD's defaults (sent explicitly, as the desktop's dialog sends every
//! value).
use crate::ui_kit::form::{FieldKind, FieldValue, Unit, evaluate};
use serde_json::{Map, Value, json};

/// How a setting is entered and sent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    /// A checkbox; sent as a bool.
    Check,
    /// STL's "Format" combo (binary | ascii), sent as `binary: bool`
    /// (`ExportDialog.values`).
    Binary,
    /// One of the options, sent as the option's text.
    Choice(&'static [&'static str]),
    /// A spin box's number (inclusive range, the dialog's decimals).
    Number { unit: Unit, min: f64, max: f64, decimals: u8 },
    /// Text as typed (the sketch node id, the drawing's title).
    Text,
    /// The drawing's views: a non-empty list of [`DRAWING_VIEWS`].
    Views,
    /// The drawing's section plane: anything RoboCAD's `ArgConverter.plane`
    /// takes ("xy" | "xz" | "yz" | a plane node id | {origin, normal[,
    /// x_axis]} | {axis, offset}), or true (the section tool's plane, as
    /// the desktop does) or false (none). Absent: the section tool's plane
    /// while it is on, else none.
    Section,
}

/// One setting of a format.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Setting {
    /// The settings key `POST /export` takes.
    pub name: &'static str,
    /// The desktop dialog's label.
    pub label: &'static str,
    pub kind: Kind,
    /// RoboCAD's default as text ("" for a default the document supplies:
    /// the sketch, the title, the section).
    pub default: &'static str,
}

/// One `POST /export` format.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Format {
    /// `POST /export`'s `format`.
    pub id: &'static str,
    /// The desktop's name for it (its Export dialog's filter, or the command label).
    pub label: &'static str,
    /// The file extensions the desktop maps to it (the first is written by default).
    pub extensions: &'static [&'static str],
    pub settings: &'static [Setting],
}

const fn s(name: &'static str, label: &'static str, kind: Kind, default: &'static str) -> Setting {
    Setting { name, label, kind, default }
}

const TOLERANCE: Setting = s("tolerance", "Chord tolerance (mm)", Kind::Number { unit: Unit::Length, min: 0.001, max: 1.0, decimals: 3 }, "0.05");

/// The drawing's standard views (`io/drawing.py` `STANDARD_VIEWS`), in the desktop's order.
pub(crate) const DRAWING_VIEWS: [(&str, &str); 4] = [("front", "Front"), ("top", "Top"), ("right", "Right"), ("iso", "Isometric")];

/// Every format `POST /export` takes, in the desktop's Export filter order,
/// then the drawing.
pub(crate) static FORMATS: &[Format] = &[
    Format {
        id: "stl",
        label: "STL",
        extensions: &["stl"],
        settings: &[
            s("binary", "Format", Kind::Binary, "binary"),
            s("unit", "Unit", Kind::Choice(&["mm", "cm", "m", "in", "ft"]), "mm"),
            TOLERANCE,
            s("angular_deg", "Angular tolerance (°)", Kind::Number { unit: Unit::Angle, min: 1.0, max: 60.0, decimals: 1 }, "20"),
        ],
    },
    Format { id: "3mf", label: "3MF", extensions: &["3mf"], settings: &[TOLERANCE, s("colors", "Write colours", Kind::Check, "true"), s("names", "Write names", Kind::Check, "true")] },
    Format {
        id: "step",
        label: "STEP",
        extensions: &["step", "stp"],
        settings: &[s("schema", "Schema", Kind::Choice(&["AP203", "AP214", "AP242"]), "AP214"), s("names", "Write names", Kind::Check, "true"), s("colors", "Write colours", Kind::Check, "true")],
    },
    Format { id: "iges", label: "IGES", extensions: &["iges", "igs"], settings: &[] },
    Format {
        id: "obj",
        label: "OBJ",
        extensions: &["obj"],
        settings: &[
            TOLERANCE,
            s("scale", "Scale", Kind::Number { unit: Unit::Plain, min: 0.0001, max: 1000.0, decimals: 4 }, "1"),
            s("up_axis", "Up axis", Kind::Choice(&["Z", "Y"]), "Z"),
            s("quads", "Quads where possible", Kind::Check, "false"),
            s("ngons", "N-gons where possible", Kind::Check, "false"),
            s("mtl", "Write MTL", Kind::Check, "true"),
            s("uvs", "Write UVs", Kind::Check, "true"),
        ],
    },
    Format { id: "svg", label: "Sketch SVG", extensions: &["svg"], settings: &[s("sketch", "Sketch (node id)", Kind::Text, "")] },
    Format {
        id: "drawing",
        label: "Drawing (SVG)",
        extensions: &["svg"],
        settings: &[s("views", "Views", Kind::Views, "front,top,right,iso"), s("title", "Title", Kind::Text, ""), s("section", "Section A-A (the section tool's plane)", Kind::Section, "")],
    },
];

/// The format ids, for the form's choice (same order as [`FORMATS`]).
pub(crate) const FORMAT_IDS: &[&str] = &["stl", "3mf", "step", "iges", "obj", "svg", "drawing"];

/// The format `id` (`POST /export`'s name).
pub(crate) fn format(id: &str) -> Option<&'static Format> {
    FORMATS.iter().find(|f| f.id == id)
}

/// Refusal for an unknown format, listing the known ones.
pub(crate) fn unknown(id: &str) -> String {
    format!("unknown export format {id:?}: RoboCAD exports {}", FORMAT_IDS.join(", "))
}

/// What the document supplies for the defaults RoboCAD's desktop takes
/// from its window: the selected sketch, the file name, the section tool.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Context {
    /// The first selected sketch node (the sketch SVG's sketch).
    pub sketch: Option<String>,
    /// `basename(doc.path or "untitled")` (the drawing's title).
    pub title: String,
    /// The section tool's plane while it is on (`{origin, normal, x_axis}`).
    pub section: Option<Value>,
}

/// A setting's value as `POST /export` takes it, checked: Err names the
/// setting and why.
fn check(fmt: &Format, st: &Setting, value: &Value) -> Result<Value, String> {
    let name = format!("{} setting {} ({})", fmt.label, st.name, st.label);
    match st.kind {
        Kind::Check | Kind::Binary => value.as_bool().map(Value::Bool).ok_or_else(|| format!("{name}: expected true or false, got {value}")),
        Kind::Choice(options) => match value.as_str() {
            Some(v) if options.contains(&v) => Ok(json!(v)),
            _ => Err(format!("{name}: expected one of {}, got {value}", options.join(", "))),
        },
        Kind::Number { unit, min, max, decimals } => {
            let kind = FieldKind::Number { unit, min: Some(min), max: Some(max), decimals };
            let text = match value {
                Value::Number(n) => n.to_string(),
                Value::String(t) => t.clone(),
                other => return Err(format!("{name}: expected a number, got {other}")),
            };
            match evaluate(&kind, &text) {
                Ok(FieldValue::Number(v)) => Ok(json!(v)),
                Ok(_) => Err(format!("{name}: expected a number")),
                Err(e) => Err(format!("{name}: {e}")),
            }
        }
        Kind::Text => match value.as_str() {
            Some(t) if !t.trim().is_empty() || st.name == "title" => Ok(json!(t)),
            _ => Err(format!("{name}: expected text, got {value}")),
        },
        Kind::Views => {
            let views: Option<Vec<&str>> = value.as_array().map(|a| a.iter().filter_map(Value::as_str).collect());
            match views {
                Some(v) if !v.is_empty() && value.as_array().is_some_and(|a| a.len() == v.len()) && v.iter().all(|x| DRAWING_VIEWS.iter().any(|(id, _)| id == x)) => Ok(json!(v)),
                _ => Err(format!("{name}: expected a non-empty list of {}, got {value}", DRAWING_VIEWS.map(|(id, _)| id).join(", "))),
            }
        }
        Kind::Section => match value {
            Value::String(t) if !t.trim().is_empty() => Ok(value.clone()),
            Value::Object(m) if (m.contains_key("origin") && m.contains_key("normal")) || (m.contains_key("axis") && m.contains_key("offset")) => Ok(value.clone()),
            _ => Err(format!("{name}: expected a plane as RoboCAD takes it (xy | xz | yz | a plane node id | {{origin, normal, x_axis?}} | {{axis, offset}}), got {value}")),
        },
    }
}

/// A setting's default as `POST /export` takes it (None: left out, as
/// the drawing's section when the section tool is off).
fn default(st: &Setting, cx: &Context) -> Result<Option<Value>, String> {
    Ok(match st.kind {
        Kind::Check => Some(Value::Bool(st.default == "true")),
        Kind::Binary => Some(Value::Bool(st.default == "binary")),
        Kind::Choice(_) => Some(json!(st.default)),
        Kind::Number { .. } => Some(json!(st.default.parse::<f64>().map_err(|e| format!("{}: bad default {}: {e}", st.name, st.default))?)),
        Kind::Views => Some(json!(st.default.split(',').collect::<Vec<_>>())),
        Kind::Section => cx.section.clone(),
        Kind::Text if st.name == "title" => Some(json!(cx.title)),
        Kind::Text => match &cx.sketch {
            Some(id) => Some(json!(id)),
            None => return Err("the sketch SVG needs settings.sketch: a sketch node id (RoboCAD exports the selected sketch: select one, or name it)".into()),
        },
    })
}

/// The settings `POST /export` is sent for `fmt`: `given` checked (an
/// unknown key is refused naming the format's settings), the rest from
/// RoboCAD's defaults and the document (`cx`).
pub(crate) fn settings(fmt: &Format, given: &Map<String, Value>, cx: &Context) -> Result<Map<String, Value>, String> {
    if let Some(key) = given.keys().find(|k| !fmt.settings.iter().any(|st| st.name == k.as_str())) {
        let names: Vec<&str> = fmt.settings.iter().map(|st| st.name).collect();
        return Err(format!("{} has no setting {key:?} (its settings: {})", fmt.label, if names.is_empty() { "none".to_string() } else { names.join(", ") }));
    }
    let mut out = Map::new();
    for st in fmt.settings {
        let value = match (st.kind, given.get(st.name)) {
            // The drawing's section: false leaves it out, true is the section tool's plane.
            (Kind::Section, Some(Value::Bool(false))) => None,
            (Kind::Section, Some(Value::Bool(true))) => Some(cx.section.clone().ok_or("the drawing's section A-A takes the section tool's plane, and the section tool is off: turn it on, or name a plane")?),
            (_, Some(Value::Null) | None) => default(st, cx)?,
            (_, Some(v)) => Some(check(fmt, st, v)?),
        };
        if let Some(v) = value {
            out.insert(st.name.to_string(), v);
        }
    }
    Ok(out)
}

/// Whether `path`'s extension is one the desktop maps to `fmt`.
pub(crate) fn extension_fits(fmt: &Format, path: &str) -> bool {
    let ext = crate::cad::types::extension(path);
    fmt.extensions.contains(&ext.as_str())
}
