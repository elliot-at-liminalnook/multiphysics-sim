//! `GET /nodes/{id}/section?plane=…`: RoboCAD's exact B-rep section of one
//! node (`api.py` `section`, `analysis.section_outline`: OCCT's
//! `BRepAlgoAPI_Section`, each edge sampled as a polyline in mm; a mesh
//! node's triangles cut instead).
//!
//! - **The plane is a query string, not JSON.** api.py's route passes
//!   `q.get("plane", "xz")` (a string, `parse_qs`-decoded) to
//!   `ArgConverter.plane`, whose string forms are the named planes `xy`,
//!   `xz`, `yz` (through the origin) and a plane node's id. Its dict forms
//!   (`{origin, normal}`, `{axis, offset}`) are unreachable from the query,
//!   so an offset or tilted plane has no exact section unless a plane node
//!   holds it ([`SectionQuery`]).
//! - **What comes back**: a list of polylines, each a list of `[x, y, z]`
//!   (mm, RoboCAD's model frame). A hidden node, an unknown id or a node
//!   without geometry answers `[]` (`section_outline` skips them); a kernel
//!   failure is also `[]` (it swallows the exception). Read tolerantly: a
//!   polyline with a malformed point (or a bare `NaN`, read as null) is
//!   dropped and counted in [`SectionCurves::dropped`], not the whole answer.

/// The `plane=` value of a section request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SectionQuery {
    /// `Plane.xy()`: z = 0.
    Xy,
    /// `Plane.xz()`: y = 0 (normal −Y).
    Xz,
    /// `Plane.yz()`: x = 0.
    Yz,
    /// A plane node's id (its `plane`).
    Node(String),
}

impl SectionQuery {
    /// The query value as api.py reads it.
    pub fn as_str(&self) -> &str {
        match self {
            SectionQuery::Xy => "xy",
            SectionQuery::Xz => "xz",
            SectionQuery::Yz => "yz",
            SectionQuery::Node(id) => id,
        }
    }

    /// The named plane that is the same set of points as the plane through
    /// `origin` with `normal` (mm; either sign of the normal: a section
    /// does not depend on it), within `tolerance` mm and 1e-9 in direction.
    /// None for an offset or tilted plane.
    pub fn named(origin: [f64; 3], normal: [f64; 3], tolerance: f64) -> Option<SectionQuery> {
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if !(length.is_finite() && length > 1e-12) || origin.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let n = normal.map(|v| v / length);
        let axis = (0..3).find(|&i| (n[i].abs() - 1.0).abs() < 1e-9)?;
        if origin[axis].abs() > tolerance {
            return None;
        }
        Some(match axis {
            0 => SectionQuery::Yz,
            1 => SectionQuery::Xz,
            _ => SectionQuery::Xy,
        })
    }
}

/// An exact section: the polylines RoboCAD sampled (mm).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SectionCurves {
    pub polylines: Vec<Vec<[f64; 3]>>,
    /// Polylines dropped because a point was not three finite numbers.
    pub dropped: usize,
}


