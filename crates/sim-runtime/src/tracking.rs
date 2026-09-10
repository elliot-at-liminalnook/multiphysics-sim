//! Sampled task-space tracking evidence for model reduction and calibration.
//! All positions are metres in an explicitly shared frame, times are seconds.
//! This gate does not infer calibration, contact quality, or walking success.
use crate::session::{EpisodeFrame, Recording, Session};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const TIME_EPS_S: f64 = 1e-9;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub id: String,
    /// Exact CAD-exported link name. Ambiguous names are rejected.
    pub link: String,
    pub local_point_m: [f64; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureConfig {
    pub experiment_id: String,
    /// Identifies the registered world frame, including origin and axes.
    pub coordinate_frame: String,
    /// Optional guard for markers derived from a particular CAD revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_cad_sha256: Option<String>,
    pub markers: Vec<Marker>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceSource {
    /// The complete input recipe travels with the measurements.
    Simulation {
        recording: Box<Recording>,
        markers: Vec<Marker>,
    },
    /// Artifact must durably identify raw measurements; procedure describes
    /// registration, clock alignment, and uncertainty estimation.
    Hardware {
        artifact: String,
        procedure: String,
    },
    Synthetic {
        description: String,
    },
}
impl EvidenceSource {
    fn label(&self) -> &'static str {
        match self {
            Self::Simulation { .. } => "simulation",
            Self::Hardware { .. } => "hardware",
            Self::Synthetic { .. } => "synthetic",
        }
    }
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Simulation { recording, markers } => {
                if recording.version != 1 {
                    return Err("unsupported recording version".into());
                }
                validate_markers(markers)
            }
            Self::Hardware {
                artifact,
                procedure,
            } => {
                nonempty(artifact, "hardware artifact")?;
                nonempty(procedure, "measurement procedure")
            }
            Self::Synthetic { description } => nonempty(description, "synthetic description"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointMeasurement {
    pub position_m: [f64; 3],
    /// Conservative spatial error bound, not a standard deviation. Include
    /// measurement, registration, and clock-alignment error here for hardware.
    pub uncertainty_m: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackingSample {
    pub time_s: f64,
    pub points: BTreeMap<String, PointMeasurement>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackingEvidence {
    pub version: u32,
    pub experiment_id: String,
    pub coordinate_frame: String,
    pub source: EvidenceSource,
    pub completed: bool,
    pub samples: Vec<TrackingSample>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointBudget {
    pub id: String,
    /// Task's total allowed spatial error, not a claim of measured accuracy.
    pub placement_margin_m: f64,
    /// Budget already reserved for errors outside this comparison, e.g. sensing
    /// or controller tracking not represented by either trajectory.
    pub other_error_reserve_m: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackingRequirements {
    pub version: u32,
    pub experiment_id: String,
    pub coordinate_frame: String,
    pub start_s: f64,
    pub end_s: f64,
    /// Prevents sparse reporting from silently passing a dense-motion test.
    /// No continuous-time bound is implied even when this requirement passes.
    pub max_sample_gap_s: f64,
    pub points: Vec<PointBudget>,
}

#[derive(Debug, Serialize)]
pub struct PointReport {
    pub id: String,
    pub samples: usize,
    pub violations: usize,
    pub rms_error_m: f64,
    pub max_error_m: f64,
    pub worst_margin_time_s: f64,
    /// margin - reserve - distance - both uncertainty bounds.
    pub minimum_remaining_margin_m: f64,
}
#[derive(Debug, Serialize)]
pub struct TrackingReport {
    pub passed: bool,
    pub candidate_source: String,
    pub reference_source: String,
    pub requirements: TrackingRequirements,
    pub points: Vec<PointReport>,
    pub scope: String,
}

fn nonempty(value: &str, name: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{name} must not be empty"))
    } else {
        Ok(())
    }
}
fn nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}
pub(crate) fn validate_markers(markers: &[Marker]) -> Result<(), String> {
    if markers.is_empty() {
        return Err("at least one marker is required".into());
    }
    let mut ids = BTreeSet::new();
    for marker in markers {
        nonempty(&marker.id, "marker id")?;
        nonempty(&marker.link, "marker link")?;
        if !ids.insert(&marker.id) || marker.local_point_m.iter().any(|x| !x.is_finite()) {
            return Err(format!("duplicate or invalid marker {}", marker.id));
        }
    }
    Ok(())
}

/// Select every compiled runtime contact vertex on explicitly named CAD links.
/// These are sampled surfaces, not exact CAD geometry. Locations are relative
/// to each rigid link COM, matching runtime poses and EmbeddedPoint conventions.
/// Stiffness/force weighting is not changed or inferred by this selection.
pub fn compiled_surface_markers(
    art: &sim_domain_robot::Articulated,
    links: &[String],
    expected_cad_sha256: &str,
) -> Result<Vec<Marker>, String> {
    if expected_cad_sha256.is_empty()
        || art.model.source["cad_sha256"].as_str() != Some(expected_cad_sha256)
        || links.is_empty()
        || links.iter().collect::<BTreeSet<_>>().len() != links.len()
    {
        return Err("matching explicit CAD identity and unique nonempty surface links required".into());
    }
    let mut markers = vec![];
    for name in links {
        let matches = art.links.iter().filter(|l| &l.name == name).collect::<Vec<_>>();
        if matches.len() != 1 || matches[0].contact.is_empty() {
            return Err(format!("unique link with compiled contact samples required: {name}"));
        }
        for (index, point) in matches[0].contact.iter().enumerate() {
            markers.push(Marker {
                id: format!("{name}/surface/{index}"),
                link: name.clone(),
                local_point_m: (*point).into(),
            });
        }
    }
    validate_markers(&markers)?;
    Ok(markers)
}

/// Transform physical marker locations, rather than comparing link origins.
pub fn sample_markers(frame: &EpisodeFrame, markers: &[Marker]) -> Result<TrackingSample, String> {
    validate_markers(markers)?;
    if frame.error.is_some() || !nonnegative(frame.time_s) {
        return Err("cannot sample an invalid or failed episode frame".into());
    }
    let mut points = BTreeMap::new();
    for marker in markers {
        let mut matches = frame.poses.iter().filter(|p| p.name == marker.link);
        let pose = matches
            .next()
            .ok_or_else(|| format!("missing link {}", marker.link))?;
        if matches.next().is_some() {
            return Err(format!("ambiguous link {}", marker.link));
        }
        if !pose.valid_rigid_transform() {
            return Err("invalid marker rigid transform".into());
        }
        let position_m = std::array::from_fn(|i| {
            pose.position_m[i]
                + (0..3)
                    .map(|j| pose.rotation[i][j] * marker.local_point_m[j])
                    .sum::<f64>()
        });
        if position_m.iter().any(|x| !x.is_finite()) {
            return Err("nonfinite marker position".into());
        }
        points.insert(
            marker.id.clone(),
            PointMeasurement {
                position_m,
                uncertainty_m: 0.0,
            },
        );
    }
    Ok(TrackingSample {
        time_s: frame.time_s,
        points,
    })
}

/// Marker coordinates in an explicitly named moving link frame. This separates
/// mechanism tracking from overall body motion; it does not measure world-frame
/// foothold error. The returned uncertainty radius is unchanged by rigid rotation.
pub fn sample_markers_in_link_frame(
    frame: &EpisodeFrame,
    markers: &[Marker],
    reference_link: &str,
) -> Result<TrackingSample, String> {
    let mut matching = frame.poses.iter().filter(|p| p.name == reference_link);
    let reference = matching.next().ok_or("missing marker reference link")?;
    if matching.next().is_some() || !reference.valid_rigid_transform() {
        return Err("ambiguous or invalid marker reference link".into());
    }
    let mut result = sample_markers(frame, markers)?;
    for point in result.points.values_mut() {
        let delta: [f64; 3] =
            std::array::from_fn(|i| point.position_m[i] - reference.position_m[i]);
        point.position_m =
            std::array::from_fn(|i| (0..3).map(|j| reference.rotation[j][i] * delta[j]).sum());
        if point.position_m.iter().any(|x| !x.is_finite()) {
            return Err("nonfinite relative marker position".into());
        }
    }
    Ok(result)
}

/// Run the existing shared runtime. Zero uncertainty describes deterministic
/// sampled output only; it does not assert zero numerical or physical error.
pub fn capture_tracking(
    recording: &Recording,
    config: &CaptureConfig,
) -> Result<TrackingEvidence, String> {
    if recording.version != 1 {
        return Err("unsupported recording version".into());
    }
    nonempty(&config.experiment_id, "experiment id")?;
    nonempty(&config.coordinate_frame, "coordinate frame")?;
    validate_markers(&config.markers)?;
    if let Some(expected) = &config.expected_cad_sha256 {
        if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("expected_cad_sha256 must be a 64-digit hexadecimal hash".into());
        }
        let actual = recording
            .scene
            .robot
            .source
            .get("cad_sha256")
            .and_then(|v| v.as_str());
        if !actual.is_some_and(|actual| actual.eq_ignore_ascii_case(expected)) {
            return Err("marker configuration CAD hash does not match recording source".into());
        }
    }
    let mut session = Session::new(recording.scene.clone(), recording.seed)?;
    let mut samples = vec![sample_markers(&session.frame(), &config.markers)?];
    for action in &recording.actions {
        let frame = session.step(action)?;
        samples.push(sample_markers(&frame, &config.markers)?);
    }
    Ok(TrackingEvidence {
        version: 1,
        experiment_id: config.experiment_id.clone(),
        coordinate_frame: config.coordinate_frame.clone(),
        source: EvidenceSource::Simulation {
            recording: Box::new(recording.clone()),
            markers: config.markers.clone(),
        },
        completed: true,
        samples,
    })
}

fn validate_evidence(e: &TrackingEvidence, r: &TrackingRequirements) -> Result<(), String> {
    if e.version != 1 || !e.completed {
        return Err("unsupported or incomplete tracking evidence".into());
    }
    if e.experiment_id != r.experiment_id || e.coordinate_frame != r.coordinate_frame {
        return Err(
            "experiment or coordinate frame mismatch; register measurements explicitly".into(),
        );
    }
    e.source.validate()?;
    if e.samples.len() < 2 {
        return Err("at least two tracking samples are required".into());
    }
    if (e.samples[0].time_s - r.start_s).abs() > TIME_EPS_S
        || (e.samples.last().unwrap().time_s - r.end_s).abs() > TIME_EPS_S
    {
        return Err("tracking evidence does not cover the exact required interval".into());
    }
    for (i, sample) in e.samples.iter().enumerate() {
        if !nonnegative(sample.time_s) {
            return Err("invalid sample time".into());
        }
        if i > 0 {
            let gap = sample.time_s - e.samples[i - 1].time_s;
            if gap <= 0.0 || gap > r.max_sample_gap_s + TIME_EPS_S {
                return Err("sample times must increase and respect max_sample_gap_s".into());
            }
        }
        for budget in &r.points {
            let point = sample
                .points
                .get(&budget.id)
                .ok_or_else(|| format!("missing point {} at {}", budget.id, sample.time_s))?;
            if !nonnegative(point.uncertainty_m) || point.position_m.iter().any(|x| !x.is_finite())
            {
                return Err(format!("invalid point {} at {}", budget.id, sample.time_s));
            }
        }
    }
    Ok(())
}

/// Compare aligned samples with a conservative additive error budget. No time
/// shifting, nearest-neighbor matching, interpolation, or dropped channels can
/// hide latency or missing evidence. Input errors fail closed with `Err`.
pub fn compare_tracking(
    candidate: &TrackingEvidence,
    reference: &TrackingEvidence,
    r: &TrackingRequirements,
) -> Result<TrackingReport, String> {
    nonempty(&r.experiment_id, "experiment id")?;
    nonempty(&r.coordinate_frame, "coordinate frame")?;
    if r.version != 1
        || !nonnegative(r.start_s)
        || !r.end_s.is_finite()
        || r.end_s <= r.start_s
        || !r.max_sample_gap_s.is_finite()
        || r.max_sample_gap_s <= 0.0
        || r.points.is_empty()
    {
        return Err("invalid tracking requirements".into());
    }
    let mut ids = BTreeSet::new();
    for b in &r.points {
        nonempty(&b.id, "budget point id")?;
        if !ids.insert(&b.id)
            || !b.placement_margin_m.is_finite()
            || b.placement_margin_m <= 0.0
            || !nonnegative(b.other_error_reserve_m)
            || b.other_error_reserve_m >= b.placement_margin_m
        {
            return Err(format!("invalid or duplicate point budget {}", b.id));
        }
    }
    validate_evidence(candidate, r)?;
    validate_evidence(reference, r)?;
    if candidate.samples.len() != reference.samples.len() {
        return Err("sample count mismatch".into());
    }
    for (a, b) in candidate.samples.iter().zip(&reference.samples) {
        if (a.time_s - b.time_s).abs() > TIME_EPS_S {
            return Err("sample timestamps are not aligned".into());
        }
    }
    let mut reports = Vec::new();
    for b in &r.points {
        let mut report = PointReport {
            id: b.id.clone(),
            samples: 0,
            violations: 0,
            rms_error_m: 0.0,
            max_error_m: 0.0,
            worst_margin_time_s: r.start_s,
            minimum_remaining_margin_m: f64::INFINITY,
        };
        for (a, z) in candidate.samples.iter().zip(&reference.samples) {
            let p = &a.points[&b.id];
            let q = &z.points[&b.id];
            let error = (0..3).fold(0.0_f64, |d, i| d.hypot(p.position_m[i] - q.position_m[i]));
            let remaining = b.placement_margin_m
                - b.other_error_reserve_m
                - error
                - p.uncertainty_m
                - q.uncertainty_m;
            if !error.is_finite() || !remaining.is_finite() {
                return Err("tracking error overflow".into());
            }
            report.samples += 1;
            report.violations += usize::from(remaining < 0.0);
            let n = report.samples as f64;
            report.rms_error_m =
                (report.rms_error_m * ((n - 1.0) / n).sqrt()).hypot(error / n.sqrt());
            report.max_error_m = report.max_error_m.max(error);
            if remaining < report.minimum_remaining_margin_m {
                report.minimum_remaining_margin_m = remaining;
                report.worst_margin_time_s = a.time_s;
            }
        }
        reports.push(report);
    }
    Ok(TrackingReport {
        passed: reports.iter().all(|p| p.violations == 0),
        candidate_source: candidate.source.label().into(), reference_source: reference.source.label().into(),
        requirements: r.clone(), points: reports,
        scope: "Sampled point tracking only. A pass does not establish between-sample clearance, loaded actuator capability, contact accuracy, balance, walking success, or hardware readiness. Source labels and uncertainty bounds are supplied evidence, not independently certified measurements.".into(),
    })
}
