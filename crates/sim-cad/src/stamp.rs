//! RoboCAD's comment-pin geometry stamp, reproduced exactly (annotations.py
//! `stamp`): sha256 of `json.dumps([rounded([volume, area, centroid,
//! bbox_min, bbox_max]), sorted(json.dumps(rounded(face.to_json()),
//! sort_keys=True) for each face)], sort_keys=True)`, floats rounded to 9
//! decimals and printed as Python's `repr`. Both editors run OCCT 7.7.2, so a
//! pin placed in either reads attached in the other.
//!
//! RoboCAD's bounding box reads the display triangulation when the body has
//! been meshed (`BRepBndLib.Add(shape, box, True)`), so a stamp depends on
//! whether the body was drawn: [`stamps`] gives both, the drawn one first.
use crate::kernel::{Measure, measure};
use sha2::{Digest, Sha256};

/// Python's `round(x, 9)`: the correctly rounded decimal, as a float.
pub fn round9(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.9}").parse().unwrap_or(x)
}

/// Python's `repr(float)`: the shortest round-trip digits, fixed notation
/// for decimal exponents −4…15, else `d.ddde±XX`.
pub fn py_repr(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0. { "Infinity".into() } else { "-Infinity".into() };
    }
    if x == 0. {
        return if x.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    // Rust's `{:e}` is the shortest round-trip form: "1.2345e-5".
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci.split_once('e').expect("scientific");
    let exp: i32 = exp.parse().expect("exponent");
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if negative { "-" } else { "" };
    if (-4..16).contains(&exp) {
        let point = exp + 1; // digits before the decimal point
        let out = if point <= 0 {
            format!("0.{}{}", "0".repeat((-point) as usize), digits)
        } else if point as usize >= digits.len() {
            format!("{}{}.0", digits, "0".repeat(point as usize - digits.len()))
        } else {
            format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
        };
        format!("{sign}{out}")
    } else {
        let rest = &digits[1..];
        let m = if rest.is_empty() { digits[..1].to_string() } else { format!("{}.{}", &digits[..1], rest) };
        format!("{sign}{m}e{}{:02}", if exp < 0 { "-" } else { "+" }, exp.abs())
    }
}

fn num(x: f64) -> String {
    py_repr(round9(x))
}
fn list(xs: &[f64]) -> String {
    format!("[{}]", xs.iter().map(|x| num(*x)).collect::<Vec<_>>().join(", "))
}

const KINDS: [&str; 8] = ["plane", "cylinder", "cone", "sphere", "torus", "bspline", "bezier", "other"];

/// RoboCAD's stamp of a body (B-rep text) of `body_kind`, meshed at
/// `tolerance` first when given.
pub fn stamp(brep: &[u8], solid: bool, tolerance: Option<f64>) -> Result<String, String> {
    let v = measure(Measure::RobocadStamp, &[brep], &[tolerance.unwrap_or(0.)], &[i32::from(solid)])?;
    let head = list(&v[0..2]);
    let props = format!("[{}, {}, {}, {}, {}]", num(v[0]), num(v[1]), list(&v[2..5]), list(&v[5..8]), list(&v[8..11]));
    let _ = head;
    let nf = v[11] as usize;
    let mut faces: Vec<String> = Vec::with_capacity(nf);
    for i in 0..nf {
        let f = &v[12 + 19 * i..12 + 19 * (i + 1)];
        let axis = f[8] != 0.;
        // `json.dumps(rounded(f.to_json()), sort_keys=True)`: keys in order.
        let s = format!(
            "{{\"area\": {}, \"axis_dir\": {}, \"axis_point\": {}, \"centroid\": {}, \"kind\": \"{}\", \"normal\": {}, \"point\": {}, \"radius\": {}}}",
            num(f[7]),
            if axis { list(&f[12..15]) } else { "null".into() },
            if axis { list(&f[9..12]) } else { "null".into() },
            list(&f[1..4]),
            KINDS[f[0] as usize],
            list(&f[4..7]),
            list(&f[16..19]),
            if axis { num(f[15]) } else { "null".into() },
        );
        faces.push(s);
    }
    faces.sort();
    // The outer `json.dumps`: each face string escaped as a JSON string.
    let escaped: Vec<String> = faces.iter().map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))).collect();
    let data = format!("[{props}, [{}]]", escaped.join(", "));
    Ok(format!("{:x}", Sha256::digest(data.as_bytes())))
}

/// Node `id`'s RoboCAD stamps: as drawn (meshed at its tessellation
/// tolerance), then as freshly read. None for a node without B-rep geometry.
pub fn stamps(doc: &crate::ArchiveDocument, id: &str) -> Option<[String; 2]> {
    let n = doc.node(id)?;
    let brep = crate::geometry::resolved_brep(doc, id).ok()?;
    let solid = n["body_kind"].as_str().unwrap_or("solid") == "solid";
    let tol = n["tessellation_tolerance"].as_f64().unwrap_or(0.05);
    Some([stamp(&brep, solid, Some(tol)).ok()?, stamp(&brep, solid, None).ok()?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_as_python_repr() {
        for (x, want) in [(1.0, "1.0"), (0.1, "0.1"), (1e-5, "1e-05"), (0.0001, "0.0001"), (123456.789, "123456.789"), (1e16, "1e+16"), (-2.5, "-2.5"), (-0.0, "-0.0"), (100.0, "100.0"), (1234567890123456.0, "1234567890123456.0"), (3.14159265358979, "3.14159265358979")] {
            assert_eq!(py_repr(x), want, "{x}");
        }
        assert_eq!(py_repr(round9(0.1 + 0.2)), "0.3");
        assert_eq!(py_repr(round9(2.0000000004)), "2.0");
    }
}
