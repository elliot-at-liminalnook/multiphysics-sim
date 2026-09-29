//! Readable gait files: one periodic contact-phase motion written for people
//! and language models. Legs are named, footholds are offsets from the robot's
//! nominal stance, body rotations are in degrees and the body moves through
//! evenly spaced periodic B-spline controls. `tune` marks numbers a search may
//! vary. Conversion targets the existing [`ContactTemplate`]; no robot
//! topology, compiler or physics lives here.
use crate::{
    contact_phase::ContactPhaseConfig,
    motion_parameters::{MotionChannel, Parameter, ParameterSpace, Scalar, TrajectoryTemplate, Transform, Values},
    motion_primitives::{ContactTemplate, FootTemplate},
    trajectory::{Interpolation, Keyframe, TrajectoryConfig},
};
use serde::{Deserialize, Serialize};
use sim_core::QuantityKind as Q;
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GaitScript {
    pub version: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Free text: the idea behind the gait, what changed and why.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    pub cycle: Cycle,
    #[serde(default)]
    pub body: Body,
    /// Settings every leg uses unless it sets its own.
    #[serde(default)]
    pub leg_defaults: LegSettings,
    /// Keyed by leg name (e.g. "+X"); every leg of the robot must appear.
    pub legs: BTreeMap<String, Leg>,
    /// Controller policy values the study binds by name (e.g. reference
    /// governor limits). Kind follows the unit suffix: _rad_s, _rad_s2, _m, _s.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub controller: BTreeMap<String, f64>,
    /// Search ranges keyed by path, e.g. `cycle.travel_m`, `legs.+X.phase`,
    /// `leg_defaults.stance`, `controller.governor_speed_rad_s`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tune: BTreeMap<String, [f64; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cycle {
    /// One full cycle: every leg lifts and lands once.
    pub period_s: f64,
    /// Body travel per cycle along the travel heading.
    pub travel_m: f64,
    /// Travel heading from the study's walking direction, degrees
    /// counterclockwise about +Z (180 backward, 90 and -90 sideways). The body
    /// does not turn; footholds and timing are unchanged.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub heading_deg: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub height_offset_m: f64,
    /// Evenly spaced over one cycle (at least four). The body follows a smooth
    /// periodic B-spline that stays within these controls; it does not pass
    /// through each one. Empty: body level and still.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<BodyControl>,
}

/// Body offset (m) and rotation-vector components (degrees; roll, pitch and
/// yaw for small angles) in world axes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyControl {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub x_m: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub y_m: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub z_m: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rx_deg: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ry_deg: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rz_deg: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegSettings {
    /// Fraction of the cycle the foot is on the ground.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stance: Option<f64>,
    /// Mid-swing lift above the stance path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swing_height_m: Option<f64>,
    /// Fraction of the swing spent speeding up and slowing down; absent keeps
    /// the exact rest-to-rest profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_ramp: Option<f64>,
    /// Foothold shift from the robot's nominal stance, world axes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_m: Option<[f64; 3]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Leg {
    /// When this foot's stance starts, as a fraction of the cycle (0 to <1).
    pub phase: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swing_height_m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_ramp: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_m: Option<[f64; 3]>,
}

/// A robot leg's name and nominal foothold, in the study's foot order.
#[derive(Clone, Debug, PartialEq)]
pub struct NominalLeg {
    pub name: String,
    pub center_world_m: [f64; 3],
}

fn is_zero(v: &f64) -> bool {
    *v == 0.
}

/// Name legs by the horizontal axis their nominal foothold lies along
/// ("+X", "-Y", …). Fails when two legs share a name.
pub fn name_legs(centers: &[[f64; 3]]) -> Result<Vec<NominalLeg>, String> {
    let mut seen = BTreeSet::new();
    centers
        .iter()
        .map(|c| {
            if c.iter().any(|v| !v.is_finite()) || c[0].hypot(c[1]) < 1e-6 {
                return Err("a nominal foothold has no horizontal direction to name its leg".to_string());
            }
            let name = if c[0].abs() >= c[1].abs() {
                if c[0] > 0. { "+X" } else { "-X" }
            } else if c[1] > 0. {
                "+Y"
            } else {
                "-Y"
            };
            if !seen.insert(name) {
                return Err(format!("two legs would both be named {name}"));
            }
            Ok(NominalLeg { name: name.into(), center_world_m: *c })
        })
        .collect()
}

fn unit(direction: [f64; 3]) -> Result<[f64; 3], String> {
    let n = direction.iter().map(|v| v * v).sum::<f64>().sqrt();
    if !n.is_finite() || n < 1e-9 {
        return Err("walking direction must be a nonzero vector".into());
    }
    Ok(direction.map(|v| v / n))
}

impl Cycle {
    /// The unit travel direction: the study's walking direction turned by
    /// `heading_deg` about +Z.
    pub fn travel_direction(&self, direction_world: [f64; 3]) -> Result<[f64; 3], String> {
        if !self.heading_deg.is_finite() {
            return Err("cycle.heading_deg must be finite".into());
        }
        let d = unit(direction_world)?;
        let xy = crate::planar::rotate(self.heading_deg.to_radians(), [d[0], d[1]]);
        Ok([xy[0], xy[1], d[2]])
    }
}

/// Parameter kind from a controller value's unit suffix.
fn controller_kind(name: &str) -> Q {
    if name.ends_with("_rad_s2") {
        Q::AngularAcceleration
    } else if name.ends_with("_rad_s") {
        Q::AngularVelocity
    } else if name.ends_with("_rad") {
        Q::Angle
    } else if name.ends_with("_m") {
        Q::Length
    } else if name.ends_with("_s") {
        Q::Time
    } else {
        Q::Dimensionless
    }
}

/// Collects search parameters for the paths the file marks in `tune`.
struct Builder<'a> {
    tune: &'a BTreeMap<String, [f64; 2]>,
    parameters: Vec<Parameter>,
    values: Values,
    used: BTreeSet<String>,
}
impl Builder<'_> {
    fn scalar(&mut self, path: &str, value: f64, kind: Q) -> Result<Scalar, String> {
        if !value.is_finite() {
            return Err(format!("{path} must be a finite number"));
        }
        let Some(b) = self.tune.get(path) else { return Ok(Scalar::Constant { value }) };
        if b.iter().any(|v| !v.is_finite()) || b[0] > b[1] || !(b[0]..=b[1]).contains(&value) {
            return Err(format!("tune.{path}: range {b:?} must be finite, ordered and contain the current value {value}"));
        }
        if self.used.insert(path.to_string()) {
            self.parameters.push(Parameter { name: path.into(), kind, bounds: *b, integer: false });
            self.values.insert(path.into(), value);
        } else if self.values[path] != value {
            return Err(format!("{path} is used with two values"));
        }
        Ok(Scalar::Parameter { name: path.into() })
    }
}

impl GaitScript {
    /// Every path `tune` may name for this file and robot.
    pub fn tunable_paths(&self, legs: &[NominalLeg]) -> Vec<String> {
        let mut paths: Vec<String> = ["cycle.period_s", "cycle.travel_m", "body.height_offset_m"].map(String::from).into();
        for field in ["stance", "swing_height_m", "return_ramp"] {
            paths.push(format!("leg_defaults.{field}"));
        }
        for l in legs {
            paths.push(format!("legs.{}.phase", l.name));
            for field in ["stance", "swing_height_m", "return_ramp"] {
                paths.push(format!("legs.{}.{field}", l.name));
            }
        }
        paths.extend(self.controller.keys().map(|k| format!("controller.{k}")));
        paths
    }

    /// The equivalent motion template and its current values. Numbers named
    /// in `tune` become search parameters (named by path); controller values
    /// are always parameters, named as the study binds them.
    pub fn template(&self, legs: &[NominalLeg], direction_world: [f64; 3]) -> Result<(ContactTemplate, Values), String> {
        if self.version != VERSION {
            return Err(format!("gait file version {} is not supported (expected {VERSION})", self.version));
        }
        let names: BTreeSet<&str> = legs.iter().map(|l| l.name.as_str()).collect();
        let given: BTreeSet<&str> = self.legs.keys().map(String::as_str).collect();
        if names != given {
            return Err(format!("legs must be exactly {:?} (file has {:?})", names, given));
        }
        let direction = self.cycle.travel_direction(direction_world)?;
        let mut b = Builder { tune: &self.tune, parameters: vec![], values: Values::new(), used: BTreeSet::new() };
        let p = self.cycle.period_s;
        if !(p.is_finite() && p > 0.) {
            return Err("cycle.period_s must be positive".into());
        }
        // The period also scales the body clock, which takes a dimensionless
        // factor: a tuned period is carried as a multiple of 1 s.
        let (period_s, clock) = match b.scalar("cycle.period_s", p, Q::Dimensionless)? {
            Scalar::Constant { value } => (Scalar::Constant { value }, Scalar::Constant { value }),
            factor => (Scalar::Scaled { value: Box::new(Scalar::Constant { value: 1. }), factor: Box::new(factor.clone()), power: 1 }, factor),
        };
        let travel = b.scalar("cycle.travel_m", self.cycle.travel_m, Q::Length)?;
        let displacement_world_m = direction.map(|d| Scalar::Scaled { value: Box::new(travel.clone()), factor: Box::new(Scalar::Constant { value: d }), power: 1 });
        let controls = if self.body.controls.is_empty() { vec![BodyControl::default(); 4] } else { self.body.controls.clone() };
        if controls.len() < 4 {
            return Err("body.controls needs at least four evenly spaced controls".into());
        }
        let n = controls.len();
        let keyframes = controls
            .iter()
            .chain(std::iter::once(&controls[0]))
            .enumerate()
            .map(|(i, c)| Keyframe {
                time_s: i as f64 / n as f64,
                values: vec![c.x_m, c.y_m, c.z_m, c.rx_deg.to_radians(), c.ry_deg.to_radians(), c.rz_deg.to_radians()],
            })
            .collect();
        let height = b.scalar("body.height_offset_m", self.body.height_offset_m, Q::Length)?;
        let channel = |name: &str, kind| MotionChannel { name: name.into(), kind };
        let body = TrajectoryTemplate {
            channels: vec![channel("x", Q::Length), channel("y", Q::Length), channel("z", Q::Length), channel("rx", Q::Angle), channel("ry", Q::Angle), channel("rz", Q::Angle)],
            reference: TrajectoryConfig { interpolation: Interpolation::PeriodicCubicBSpline, keyframes },
            transforms: vec![
                Transform::TimeScale { factor: clock },
                Transform::Affine { channels: vec!["z".into()], scale: Scalar::Constant { value: 1. }, center: Scalar::Constant { value: 0. }, offset: height },
            ],
        };
        let d = &self.leg_defaults;
        let mut feet = vec![];
        for nominal in legs {
            let name = &nominal.name;
            let leg = &self.legs[name];
            // A leg's own value wins; otherwise the shared default (and its tune path).
            let mut setting = |own: Option<f64>, default: Option<f64>, field: &str, kind: Q, required: bool| -> Result<Option<Scalar>, String> {
                match (own, default) {
                    (Some(v), _) => b.scalar(&format!("legs.{name}.{field}"), v, kind).map(Some),
                    (None, Some(v)) => b.scalar(&format!("leg_defaults.{field}"), v, kind).map(Some),
                    (None, None) if required => Err(format!("legs.{name}: {field} missing (set it on the leg or in leg_defaults)")),
                    (None, None) => Ok(None),
                }
            };
            let stance = setting(leg.stance, d.stance, "stance", Q::Dimensionless, true)?.expect("required");
            let swing = setting(leg.swing_height_m, d.swing_height_m, "swing_height_m", Q::Length, true)?.expect("required");
            let ramp = setting(leg.return_ramp, d.return_ramp, "return_ramp", Q::Dimensionless, false)?;
            let offset = leg.offset_m.or(d.offset_m).unwrap_or([0.; 3]);
            if offset.iter().any(|v| !v.is_finite()) {
                return Err(format!("legs.{name}.offset_m must be finite"));
            }
            let center = std::array::from_fn(|i| Scalar::Constant { value: nominal.center_world_m[i] + offset[i] });
            feet.push(FootTemplate {
                center_world_m: center,
                phase_offset: b.scalar(&format!("legs.{name}.phase"), leg.phase, Q::Dimensionless)?,
                stance_fraction: stance,
                swing_offset_world_m: [Scalar::Constant { value: 0. }, Scalar::Constant { value: 0. }, swing],
                return_ramp_fraction: ramp,
            });
        }
        for (name, &value) in &self.controller {
            if !value.is_finite() {
                return Err(format!("controller.{name} must be finite"));
            }
            let path = format!("controller.{name}");
            let bounds = match self.tune.get(&path) {
                Some(r) if r.iter().all(|v| v.is_finite()) && r[0] <= value && value <= r[1] => *r,
                Some(r) => return Err(format!("tune.{path}: range {r:?} must be finite, ordered and contain the current value {value}")),
                None => [value, value],
            };
            b.used.insert(path);
            b.parameters.push(Parameter { name: name.clone(), kind: controller_kind(name), bounds, integer: false });
            b.values.insert(name.clone(), value);
        }
        let unknown: Vec<&String> = self.tune.keys().filter(|k| !b.used.contains(*k)).collect();
        if !unknown.is_empty() {
            return Err(format!("tune names {unknown:?}, which this file does not use; tunable paths are {:?}", self.tunable_paths(legs)));
        }
        let template = ContactTemplate { space: ParameterSpace { parameters: b.parameters }, body, period_s, displacement_world_m, feet };
        template.materialize(&b.values).map_err(|e| format!("gait is not a valid motion: {e}"))?;
        Ok((template, b.values))
    }

    /// The motion at the file's current values.
    pub fn motion(&self, legs: &[NominalLeg], direction_world: [f64; 3]) -> Result<ContactPhaseConfig, String> {
        let (template, values) = self.template(legs, direction_world)?;
        template.materialize(&values)
    }

    /// Describe an existing motion as a gait file. Fails on features the file
    /// format does not express (extra steps per cycle, sideways swing,
    /// non-uniform body controls, vertical travel off the walking direction).
    /// Horizontal travel off the walking direction becomes a heading.
    pub fn from_motion(motion: &ContactPhaseConfig, legs: &[NominalLeg], direction_world: [f64; 3], controller: BTreeMap<String, f64>) -> Result<Self, String> {
        if motion.feet.len() != legs.len() {
            return Err(format!("motion has {} feet but the robot has {} legs", motion.feet.len(), legs.len()));
        }
        let direction = unit(direction_world)?;
        let d = motion.displacement_world_m;
        let along = (0..3).map(|i| d[i] * direction[i]).sum::<f64>();
        let (travel, heading_deg) = if (0..3).all(|i| (d[i] - along * direction[i]).abs() <= 1e-9) {
            (along, 0.)
        } else {
            // Horizontal travel in another direction: its heading from the walking direction.
            let heading = (direction[0] * d[1] - direction[1] * d[0]).atan2(direction[0] * d[0] + direction[1] * d[1]);
            let cycle = Cycle { period_s: motion.period_s, travel_m: d[0].hypot(d[1]), heading_deg: heading.to_degrees() };
            let turned = cycle.travel_direction(direction_world)?;
            if (0..3).any(|i| (d[i] - cycle.travel_m * turned[i]).abs() > 1e-9) {
                return Err("cycle travel has a vertical component off the walking direction".into());
            }
            (cycle.travel_m, cycle.heading_deg)
        };
        let k = &motion.body.keyframes;
        if !matches!(motion.body.interpolation, Interpolation::PeriodicCubicBSpline) || k.len() < 5 || k.iter().any(|f| f.values.len() != 6) {
            return Err("body motion must be a six-channel periodic cubic B-spline".into());
        }
        let n = k.len() - 1;
        let p = motion.period_s;
        if (k[n].time_s - p).abs() > 1e-9 * p || k.iter().enumerate().any(|(i, f)| (f.time_s - i as f64 * p / n as f64).abs() > 1e-9 * p) {
            return Err("body controls must be evenly spaced over exactly one cycle".into());
        }
        let z0 = k[0].values[2];
        let level = k[..n].iter().all(|f| f.values[2] == z0);
        let height_offset_m = if level { z0 } else { 0. };
        let controls = k[..n]
            .iter()
            .map(|f| BodyControl {
                x_m: f.values[0],
                y_m: f.values[1],
                z_m: f.values[2] - height_offset_m,
                rx_deg: f.values[3].to_degrees(),
                ry_deg: f.values[4].to_degrees(),
                rz_deg: f.values[5].to_degrees(),
            })
            .collect::<Vec<_>>();
        let still = controls.len() == 4 && controls.iter().all(|c| *c == BodyControl::default());
        let body = Body { height_offset_m, controls: if still { vec![] } else { controls } };
        let mut out = BTreeMap::new();
        for (f, nominal) in motion.feet.iter().zip(legs) {
            if !f.additional_steps.is_empty() {
                return Err(format!("{}: several steps per cycle are not expressible in a gait file", nominal.name));
            }
            if f.swing_offset_world_m[0].abs() > 1e-12 || f.swing_offset_world_m[1].abs() > 1e-12 {
                return Err(format!("{}: sideways swing is not expressible in a gait file", nominal.name));
            }
            let offset: [f64; 3] = std::array::from_fn(|i| f.center_world_m[i] - nominal.center_world_m[i]);
            out.insert(
                nominal.name.clone(),
                Leg {
                    phase: f.phase_offset,
                    stance: Some(f.stance_fraction),
                    swing_height_m: Some(f.swing_offset_world_m[2]),
                    return_ramp: f.return_ramp_fraction,
                    offset_m: offset.iter().any(|v| v.abs() > 1e-12).then_some(offset),
                },
            );
        }
        // Values every leg shares move to leg_defaults, so the file stays short.
        let mut leg_defaults = LegSettings::default();
        let shared = |get: fn(&Leg) -> Option<f64>| -> Option<f64> {
            let first = get(out.values().next()?)?;
            out.values().all(|l| get(l) == Some(first)).then_some(first)
        };
        leg_defaults.stance = shared(|l| l.stance);
        leg_defaults.swing_height_m = shared(|l| l.swing_height_m);
        leg_defaults.return_ramp = shared(|l| l.return_ramp);
        for leg in out.values_mut() {
            if leg_defaults.stance.is_some() {
                leg.stance = None;
            }
            if leg_defaults.swing_height_m.is_some() {
                leg.swing_height_m = None;
            }
            if leg_defaults.return_ramp.is_some() {
                leg.return_ramp = None;
            }
        }
        Ok(Self {
            version: VERSION,
            name: String::new(),
            notes: String::new(),
            cycle: Cycle { period_s: p, travel_m: travel, heading_deg },
            body,
            leg_defaults,
            legs: out,
            controller,
            tune: BTreeMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact_phase::FootPhase;

    const DIAGONAL: [f64; 3] = [1., 1., 0.];

    fn motion() -> ContactPhaseConfig {
        let period = 1.4996;
        let centers = [[0.0065, -0.3027, -0.441], [0.3126, -0.0013, -0.441], [-0.0104, 0.2946, -0.441], [-0.2832, 0.006, -0.441]];
        ContactPhaseConfig {
            period_s: period,
            displacement_world_m: [0.1223, 0.1223, 0.],
            body: TrajectoryConfig {
                interpolation: Interpolation::PeriodicCubicBSpline,
                keyframes: (0..5).map(|i| Keyframe { time_s: i as f64 * period / 4., values: vec![0., 0., -0.0127, 0., 0., 0.] }).collect(),
            },
            feet: centers
                .iter()
                .zip([0., 0.4646, 0.9291, 0.3937])
                .map(|(c, phase)| FootPhase {
                    center_world_m: *c,
                    phase_offset: phase,
                    stance_fraction: 0.75,
                    swing_offset_world_m: [0., 0., 0.014],
                    return_ramp_fraction: Some(0.25),
                    additional_steps: vec![],
                })
                .collect(),
        }
    }

    fn legs() -> Vec<NominalLeg> {
        name_legs(&motion().feet.iter().map(|f| f.center_world_m).collect::<Vec<_>>()).unwrap()
    }

    fn assert_same(a: &ContactPhaseConfig, b: &ContactPhaseConfig) {
        let (x, y) = (serde_json::to_value(a).unwrap(), serde_json::to_value(b).unwrap());
        fn walk(x: &serde_json::Value, y: &serde_json::Value, at: &str) {
            match (x, y) {
                (serde_json::Value::Number(p), serde_json::Value::Number(q)) => {
                    let (p, q) = (p.as_f64().unwrap(), q.as_f64().unwrap());
                    assert!((p - q).abs() <= 1e-12 * p.abs().max(1.), "{at}: {p} vs {q}");
                }
                (serde_json::Value::Array(p), serde_json::Value::Array(q)) => {
                    assert_eq!(p.len(), q.len(), "{at}");
                    p.iter().zip(q).enumerate().for_each(|(i, (p, q))| walk(p, q, &format!("{at}[{i}]")));
                }
                (serde_json::Value::Object(p), serde_json::Value::Object(q)) => {
                    assert_eq!(p.keys().collect::<Vec<_>>(), q.keys().collect::<Vec<_>>(), "{at}");
                    p.iter().for_each(|(k, v)| walk(v, &q[k], &format!("{at}.{k}")));
                }
                _ => assert_eq!(x, y, "{at}"),
            }
        }
        walk(&x, &y, "motion");
    }

    #[test]
    fn legs_are_named_by_their_foothold_direction() {
        assert_eq!(legs().iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["-Y", "+X", "+Y", "-X"]);
        assert!(name_legs(&[[0.3, 0., 0.], [0.2, 0.01, 0.]]).is_err(), "two +X legs");
    }

    #[test]
    fn an_existing_motion_round_trips_through_a_gait_file() {
        let m = motion();
        let controller = BTreeMap::from([("governor_speed_rad_s".to_string(), 2.7)]);
        let script = GaitScript::from_motion(&m, &legs(), DIAGONAL, controller).unwrap();
        assert_eq!(script.leg_defaults.stance, Some(0.75), "shared values move to leg_defaults");
        assert!(script.body.controls.is_empty() && script.body.height_offset_m == -0.0127);
        let text = serde_json::to_string(&script).unwrap();
        let back: GaitScript = serde_json::from_str(&text).unwrap();
        assert_eq!(back, script);
        assert_same(&back.motion(&legs(), DIAGONAL).unwrap(), &m);
        let (template, values) = back.template(&legs(), DIAGONAL).unwrap();
        assert_eq!(template.space.parameters.len(), 1, "controller values are parameters");
        assert_eq!(values["governor_speed_rad_s"], 2.7);
    }

    #[test]
    fn tune_turns_named_numbers_into_search_parameters() {
        let mut script = GaitScript::from_motion(&motion(), &legs(), DIAGONAL, BTreeMap::new()).unwrap();
        script.tune = BTreeMap::from([
            ("cycle.travel_m".into(), [0.15, 0.26]),
            ("cycle.period_s".into(), [1.0, 2.0]),
            ("leg_defaults.stance".into(), [0.6, 0.8]),
            ("legs.+X.phase".into(), [0., 0.999]),
        ]);
        script.cycle.travel_m = 0.2;
        let (template, mut values) = script.template(&legs(), DIAGONAL).unwrap();
        assert_eq!(template.space.parameters.len(), 4);
        values.insert("cycle.travel_m".into(), 0.25);
        values.insert("cycle.period_s".into(), 1.2);
        values.insert("leg_defaults.stance".into(), 0.7);
        let m = template.materialize(&values).unwrap();
        assert!((m.displacement_world_m[0] - 0.25 / 2f64.sqrt()).abs() < 1e-12);
        assert!((m.period_s - 1.2).abs() < 1e-12 && (m.body.keyframes.last().unwrap().time_s - 1.2).abs() < 1e-12);
        assert!(m.feet.iter().all(|f| f.stance_fraction == 0.7), "every leg follows the shared default");
    }

    #[test]
    fn mistakes_are_reported_by_path() {
        let base = GaitScript::from_motion(&motion(), &legs(), DIAGONAL, BTreeMap::new()).unwrap();
        let err = |f: fn(&mut GaitScript)| {
            let mut s = base.clone();
            f(&mut s);
            s.template(&legs(), DIAGONAL).unwrap_err()
        };
        assert!(err(|s| { s.legs.remove("+X"); }).contains("legs must be exactly"));
        assert!(err(|s| { s.tune.insert("cycle.cadence".into(), [0., 1.]); }).contains("tunable paths"));
        assert!(err(|s| { s.tune.insert("cycle.travel_m".into(), [0.2, 0.3]); }).contains("contain the current value"));
        assert!(err(|s| { s.leg_defaults.stance = None; }).contains("stance missing"));
        assert!(err(|s| { s.body.controls = vec![BodyControl::default(); 3]; }).contains("at least four"));
        assert!(err(|s| { s.cycle.period_s = 0.; }).contains("positive"));
    }

    #[test]
    fn body_controls_tilt_the_body_in_degrees() {
        let mut s = GaitScript::from_motion(&motion(), &legs(), DIAGONAL, BTreeMap::new()).unwrap();
        s.body.controls = (0..4).map(|i| BodyControl { ry_deg: [0., 2., 0., -2.][i], ..Default::default() }).collect();
        let m = s.motion(&legs(), DIAGONAL).unwrap();
        assert_eq!(m.body.keyframes.len(), 5);
        assert!((m.body.keyframes[1].values[4] - 2f64.to_radians()).abs() < 1e-15);
        assert!((m.body.keyframes[1].values[2] + 0.0127).abs() < 1e-15, "height offset applies to every control");
        let back = GaitScript::from_motion(&m, &legs(), DIAGONAL, BTreeMap::new()).unwrap();
        assert_eq!(back.body.controls.len(), 4);
        assert!((back.body.controls[1].ry_deg - 2.).abs() < 1e-12);
    }

    #[test]
    fn heading_turns_travel_without_turning_the_body() {
        let mut s = GaitScript::from_motion(&motion(), &legs(), DIAGONAL, BTreeMap::new()).unwrap();
        let forward = s.motion(&legs(), DIAGONAL).unwrap();
        for (heading, expected) in [(180., [-1., -1.]), (90., [-1., 1.]), (-90., [1., -1.])] {
            s.cycle.heading_deg = heading;
            let m = s.motion(&legs(), DIAGONAL).unwrap();
            let scale = forward.displacement_world_m[0];
            for i in 0..2 {
                assert!((m.displacement_world_m[i] - expected[i] * scale).abs() < 1e-12, "{heading}");
            }
            assert_eq!(m.body.keyframes[1].values, forward.body.keyframes[1].values);
            let back = GaitScript::from_motion(&m, &legs(), DIAGONAL, BTreeMap::new()).unwrap();
            // Straight backward reads back as negative travel; sideways as a heading.
            assert_same(&back.motion(&legs(), DIAGONAL).unwrap(), &m);
            assert_eq!(back.cycle.heading_deg == 0., heading == 180.);
        }
        s.cycle.heading_deg = f64::NAN;
        assert!(s.motion(&legs(), DIAGONAL).unwrap_err().contains("heading_deg"));
    }
}
