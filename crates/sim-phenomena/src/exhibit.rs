//! The standard interface between a phenomenon and an interactive viewer.
//!
//! An [`Exhibit`] owns a live simulation, exposes exactly one knob — the
//! boundary parameter the phenomenon turns on — and describes itself to a
//! renderer as a list of [`Shape`]s in display units (y up, metres-ish,
//! a scene a few units across). Nothing here depends on a graphics library,
//! so the same exhibit can drive a Bevy scene, a headless recorder, or a
//! test.

pub type Rgb = [f32; 3];

/// A small shared palette so every exhibit reads the same way.
pub mod paint {
    use super::Rgb;
    pub const INK: Rgb = [0.16, 0.19, 0.23];
    pub const STEEL: Rgb = [0.62, 0.66, 0.71];
    pub const SURPRISE: Rgb = [0.86, 0.32, 0.20];
    pub const CONTROL: Rgb = [0.24, 0.40, 0.80];
    pub const GLOW: Rgb = [0.93, 0.66, 0.24];
    pub const PASS: Rgb = [0.24, 0.62, 0.42];
    pub const GROUND: Rgb = [0.30, 0.33, 0.37];
    pub const COPPER: Rgb = [0.78, 0.42, 0.22];
    /// Linear blend from cold (cobalt) through neutral to hot (vermilion).
    pub fn heat(fraction: f32) -> Rgb {
        let f = fraction.clamp(-1.0, 1.0);
        if f >= 0.0 {
            [0.62 + 0.24 * f, 0.66 - 0.34 * f, 0.71 - 0.51 * f]
        } else {
            [0.62 + 0.38 * f, 0.66 - 0.26 * f, 0.71 + 0.09 * f]
        }
    }
}

#[derive(Debug, Clone)]
pub enum Shape {
    Sphere { center: [f64; 3], radius: f64, color: Rgb },
    /// A capsule-like cylinder between two points.
    Rod { from: [f64; 3], to: [f64; 3], radius: f64, color: Rgb },
    /// An oriented box; `rotation` is a unit quaternion `(w, x, y, z)`.
    Block { center: [f64; 3], half: [f64; 3], rotation: [f64; 4], color: Rgb },
    Line { from: [f64; 3], to: [f64; 3], color: Rgb },
    Arrow { from: [f64; 3], to: [f64; 3], color: Rgb },
    Polyline { points: Vec<[f64; 3]>, color: Rgb },
}

impl Shape {
    pub fn block(center: [f64; 3], half: [f64; 3], color: Rgb) -> Self {
        Self::Block { center, half, rotation: [1.0, 0.0, 0.0, 0.0], color }
    }
}

/// The one boundary parameter the exhibit exposes.
#[derive(Debug, Clone)]
pub struct Knob {
    pub label: &'static str,
    pub unit: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub value: f64,
}

#[derive(Debug, Clone)]
pub struct Readout {
    pub label: String,
    pub value: f64,
    pub unit: &'static str,
}

impl Readout {
    pub fn new(label: impl Into<String>, value: f64, unit: &'static str) -> Self {
        Self { label: label.into(), value, unit }
    }
}

pub trait Exhibit: Send + Sync {
    fn title(&self) -> &'static str;
    /// One sentence: what to watch for.
    fn summary(&self) -> &'static str;
    fn knob(&self) -> Knob;
    /// Rebuild the simulation with a new knob value.
    fn set_knob(&mut self, value: f64);
    fn reset(&mut self);
    fn time(&self) -> f64;
    fn time_unit(&self) -> &'static str {
        "s"
    }
    /// Simulated seconds per real second at normal speed.
    fn time_scale(&self) -> f64 {
        1.0
    }
    /// Advance by `duration` simulated seconds.
    /// The step the exhibit integrates on, or `0` for any. A viewer
    /// advances a gridded exhibit in whole steps so the solver's cached
    /// factorisation, keyed on the step, is reused frame to frame.
    fn grid(&self) -> f64 {
        0.0
    }
    fn advance(&mut self, duration: f64) -> Result<(), String>;
    fn shapes(&self, out: &mut Vec<Shape>);
    fn readouts(&self) -> Vec<Readout>;
    /// The signal worth strip-charting, with its label.
    fn signal(&self) -> (&'static str, f64);
    /// Which side of the threshold the knob currently sits on.
    fn verdict(&self) -> String;
}

/// Quaternion `(w, x, y, z)` rotating `from` onto `to` (both unit).
pub fn rotation_between(from: [f64; 3], to: [f64; 3]) -> [f64; 4] {
    let cross = [
        from[1] * to[2] - from[2] * to[1],
        from[2] * to[0] - from[0] * to[2],
        from[0] * to[1] - from[1] * to[0],
    ];
    let dot = from[0] * to[0] + from[1] * to[1] + from[2] * to[2];
    let w = 1.0 + dot;
    let norm = (w * w + cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    if norm < 1.0e-12 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    [w / norm, cross[0] / norm, cross[1] / norm, cross[2] / norm]
}

/// Zig-zag spring polyline between two points.
pub fn spring_polyline(from: [f64; 3], to: [f64; 3], coils: usize, width: f64) -> Vec<[f64; 3]> {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1.0e-9);
    let u = [d[0] / len, d[1] / len, d[2] / len];
    // A perpendicular in the x–y plane when possible.
    let p = if u[0].abs() + u[1].abs() > 1.0e-6 { [-u[1], u[0], 0.0] } else { [1.0, 0.0, 0.0] };
    let n = coils * 2;
    (0..=n)
        .map(|i| {
            let s = i as f64 / n as f64;
            let side = if i == 0 || i == n { 0.0 } else if i % 2 == 1 { width } else { -width };
            [
                from[0] + d[0] * s + p[0] * side,
                from[1] + d[1] * s + p[1] * side,
                from[2] + d[2] * s + p[2] * side,
            ]
        })
        .collect()
}
