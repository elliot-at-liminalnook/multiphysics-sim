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
