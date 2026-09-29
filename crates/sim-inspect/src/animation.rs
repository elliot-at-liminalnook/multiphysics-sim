//! Explicit presentation bindings to accepted observations, independent of graphics.
use crate::{spatial::SpatialDescription, *};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationDescription {
    pub version: u32,
    pub description_id: String,
    pub provenance: String,
    pub rotations: Vec<RotationBinding>,
    pub colors: Vec<ColorBinding>,
    pub readouts: Vec<Readout>,
    /// Moving pieces drawn inside a part (a motor's armature, a coupling's
    /// halves, a gear train), each turned by its own port's angle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub internals: Vec<InternalBinding>,
    /// Parts moved along an axis by a simulated position.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub translations: Vec<TranslationBinding>,
    /// A rope, belt or rack drawn from a fixed anchor to a moving part.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tethers: Vec<Tether>,
    /// Physical ports with their effort and flow observables, for power,
    /// torque and force arrows, current and heat flow.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flows: Vec<FlowBinding>,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InternalElement {
    /// A plain shaft end with a key flat.
    Shaft,
    /// A motor's armature: laminated core with wound poles.
    Armature { poles: u32 },
    /// One half of a jaw coupling (the spring between halves twists).
    CouplingHalf { lugs: u32 },
    /// A spur gear.
    Gear { teeth: u32 },
    /// A link swinging about the joint at the element's center: a rod of
    /// the binding's `length` that hangs straight down at angle zero, with
    /// a bob of the binding's `radius` at its end (a leg, an arm, a pendulum).
    Arm,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InternalBinding {
    pub part: String,
    pub observable: String,
    /// Angle multiplier applied to the observable (display kinematics, e.g.
    /// an idler turning at −r_pinion/r_idler of the pinion). 1 for a port.
    #[serde(default = "one")]
    pub gain: f64,
    pub element: InternalElement,
    /// World center and unit axis of the element, meters.
    pub center: [f32; 3],
    pub axis: [f32; 3],
    pub radius: f32,
    pub length: f32,
    pub color_srgb: [f32; 3],
}
fn one() -> f64 {
    1.
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranslationBinding {
    pub part: String,
    /// Position in meters; the part is drawn at its layout position when the
    /// position equals `reference_m`.
    pub observable: String,
    pub axis: [f32; 3],
    pub reference_m: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tether {
    /// Fixed world point the rope or belt leaves from (a drum's rim).
    pub anchor: [f32; 3],
    /// The translating part it pulls, and the point on it (part-local).
    pub part: String,
    pub attach: [f32; 3],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowDomain {
    Electrical,
    Rotational,
    Translational,
    Thermal,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowBinding {
    pub part: String,
    pub component: String,
    pub port: String,
    pub net: String,
    pub domain: FlowDomain,
    /// Voltage, angular velocity, velocity or temperature.
    pub effort: Option<String>,
    /// Current, torque, force or heat flow; positive into the component.
    pub flow: Option<String>,
}
impl FlowBinding {
    /// Power into the component through this port (W), when both are known.
    /// Heat flow is itself a power.
    pub fn power(&self, frame: Option<&SampleFrame>) -> Option<f64> {
        let flow = scalar(frame, self.flow.as_deref()?)?.value;
        if self.domain == FlowDomain::Thermal {
            return Some(flow);
        }
        Some(scalar(frame, self.effort.as_deref()?)?.value * flow)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotationBinding {
    pub part: String,
    pub observable: String,
    /// Unit axis and pivot in the declared spatial world frame, in meters.
    pub axis: [f32; 3],
    pub pivot: [f32; 3],
    /// Draw a radial orientation guide for otherwise rotationally symmetric geometry.
    pub marker_radius: Option<f32>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorBinding {
    pub part: String,
    pub observable: String,
    /// Temperature in canonical kelvin. Clamped, uniform color; never a surface field.
    pub range_kelvin: [f64; 2],
    pub cold_srgb: [f32; 3],
    pub hot_srgb: [f32; 3],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Readout {
    pub label: String,
    pub observable: String,
}
impl AnimationDescription {
    pub fn validate(
        &self,
        source: &SystemDescription,
        spatial: &SpatialDescription,
    ) -> Result<(), InspectionError> {
        spatial.validate(source)?;
        ensure(
            self.version == 1 && self.description_id == source.id,
            "foreign animation description",
        )?;
        ensure(
            !self.provenance.trim().is_empty(),
            "missing animation provenance",
        )?;
        let mut rotations = BTreeSet::new();
        let mut colors = BTreeSet::new();
        for r in &self.rotations {
            ensure(
                rotations.insert(&r.part) && spatial.parts.iter().any(|p| p.id == r.part),
                "unknown/duplicate rotation part",
            )?;
            let o = source
                .observables
                .get(&r.observable)
                .ok_or_else(|| InspectionError("unknown rotation observable".into()))?;
            ensure(
                o.quantity.name == "sim.quantity.Angle"
                    && o.quantity.version == 1
                    && plot::unit(source, o) == "rad",
                "rotation requires angle in radians",
            )?;
            ensure(
                r.axis.iter().chain(&r.pivot).all(|x| x.is_finite())
                    && (r.axis.iter().map(|x| x * x).sum::<f32>() - 1.).abs() < 1e-5
                    && r.marker_radius.is_none_or(|v| v.is_finite() && v > 0.),
                "invalid rotation axis/pivot/marker",
            )?;
        }
        for c in &self.colors {
            ensure(
                colors.insert(&c.part) && spatial.parts.iter().any(|p| p.id == c.part),
                "unknown/duplicate color part",
            )?;
            let o = source
                .observables
                .get(&c.observable)
                .ok_or_else(|| InspectionError("unknown color observable".into()))?;
            ensure(
                o.quantity.name == "sim.quantity.Temperature"
                    && o.quantity.version == 1
                    && plot::unit(source, o) == "K",
                "color requires temperature in kelvin",
            )?;
            ensure(
                c.range_kelvin.iter().all(|v| v.is_finite() && *v >= 0.)
                    && c.range_kelvin[0] < c.range_kelvin[1]
                    && c.cold_srgb
                        .iter()
                        .chain(&c.hot_srgb)
                        .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
                "invalid temperature color scale",
            )?;
        }
        let part = |id: &str| spatial.parts.iter().any(|p| p.id == id);
        let unit = |v: [f32; 3]| v.iter().all(|x| x.is_finite()) && (v.iter().map(|x| x * x).sum::<f32>() - 1.).abs() < 1e-4;
        for b in &self.internals {
            let o = source.observables.get(&b.observable).ok_or_else(|| InspectionError("unknown internal observable".into()))?;
            ensure(part(&b.part) && o.quantity.name == "sim.quantity.Angle", "internal element needs a part and an angle")?;
            ensure(unit(b.axis) && b.center.iter().all(|x| x.is_finite()) && b.radius > 0. && b.length > 0. && b.gain.is_finite(), "invalid internal element geometry")?;
        }
        for t in &self.translations {
            let o = source.observables.get(&t.observable).ok_or_else(|| InspectionError("unknown translation observable".into()))?;
            ensure(part(&t.part) && plot::unit(source, o) == "m" && unit(t.axis) && t.reference_m.is_finite(), "translation needs a part, a position in m and a unit axis")?;
        }
        for t in &self.tethers {
            ensure(part(&t.part) && t.anchor.iter().chain(&t.attach).all(|x| x.is_finite()), "invalid tether")?;
        }
        for f in &self.flows {
            ensure(part(&f.part) && source.nets.contains_key(&f.net), "flow binding needs a part and a net")?;
            for o in f.effort.iter().chain(&f.flow) {
                ensure(source.observables.contains_key(o), "unknown flow observable")?;
            }
        }
        ensure(self.readouts.len() <= 16, "too many animation readouts")?;
        for r in &self.readouts {
            ensure(
                !r.label.is_empty() && source.observables.contains_key(&r.observable),
                "invalid readout",
            )?;
        }
        Ok(())
    }
    pub fn observables(&self) -> BTreeSet<String> {
        self.rotations
            .iter()
            .map(|b| b.observable.clone())
            .chain(self.colors.iter().map(|b| b.observable.clone()))
            .chain(self.readouts.iter().map(|b| b.observable.clone()))
            .chain(self.internals.iter().map(|b| b.observable.clone()))
            .chain(self.translations.iter().map(|b| b.observable.clone()))
            .chain(self.flows.iter().flat_map(|f| f.effort.iter().chain(&f.flow).cloned()))
            .collect()
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasuredScalar {
    pub value: f64,
    pub time: f64,
    pub accepted_stage: bool,
}
/// Zero-order hold of the actual sample; no wall-clock integration or extrapolation.
pub fn scalar(frame: Option<&SampleFrame>, id: &str) -> Option<MeasuredScalar> {
    match frame?.values.get(id)? {
        SampleValue::Committed { value, sample_time } => Some(MeasuredScalar {
            value: *value,
            time: *sample_time,
            accepted_stage: false,
        }),
        SampleValue::AcceptedStage {
            value, sample_time, ..
        } => Some(MeasuredScalar {
            value: *value,
            time: *sample_time,
            accepted_stage: true,
        }),
        SampleValue::Unavailable { .. } => None,
    }
}
impl ColorBinding {
    pub fn color(&self, kelvin: f64) -> [f32; 3] {
        if kelvin <= self.range_kelvin[0] {
            return self.cold_srgb;
        }
        if kelvin >= self.range_kelvin[1] {
            return self.hot_srgb;
        }
        let t = ((kelvin - self.range_kelvin[0]) / (self.range_kelvin[1] - self.range_kelvin[0]))
            .clamp(0., 1.) as f32;
        std::array::from_fn(|i| self.cold_srgb[i] + (self.hot_srgb[i] - self.cold_srgb[i]) * t)
    }
}
