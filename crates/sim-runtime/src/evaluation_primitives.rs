//! Pure offline evaluation primitives. These never propose a candidate, start
//! an environment, advance physics, acquire hardware or promote a model.
use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorRegistry, QuantityKind as Q,
    primitive::{Descriptor, Field as F},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    pub quantity: Q,
    pub value: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bound {
    pub quantity: Q,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub objective: String,
    pub objective_quantity: Q,
    pub maximize: bool,
    pub required: BTreeMap<String, Bound>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    /// One matched-comparison group, including policy, robot/world/input recipe.
    pub context_id: String,
    pub candidate_id: String,
    pub completed: bool,
    pub failures: Vec<String>,
    pub metrics: BTreeMap<String, Metric>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreRequest {
    pub policy: Policy,
    pub outcome: Outcome,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Score {
    pub context_id: String,
    pub candidate_id: String,
    pub eligible_score: Option<f64>,
    pub rejection_reasons: Vec<String>,
}
pub fn score(r: ScoreRequest) -> Result<Score, String> {
    let p = r.policy;
    let o = r.outcome;
    if p.objective.is_empty()
        || p.required.is_empty()
        || o.context_id.is_empty()
        || o.candidate_id.is_empty()
        || o.metrics
            .iter()
            .any(|(n, m)| n.is_empty() || !m.value.is_finite())
    {
        return Err(
            "named context, candidate, objective, nonempty gates and finite metrics required"
                .into(),
        );
    }
    let mut reasons = o.failures;
    if !o.completed {
        reasons.push("episode incomplete".into());
    }
    for (name, bound) in &p.required {
        if name.is_empty()
            || (bound.minimum.is_none() && bound.maximum.is_none())
            || bound
                .minimum
                .iter()
                .chain(bound.maximum.iter())
                .any(|v| !v.is_finite())
            || bound.minimum.zip(bound.maximum).is_some_and(|(a, b)| a > b)
        {
            return Err("invalid explicit gate bounds".into());
        }
        match o.metrics.get(name) {
            None => reasons.push(format!("missing required metric {name}")),
            Some(m) if m.quantity != bound.quantity => {
                return Err(format!("quantity mismatch for {name}"));
            }
            Some(m) => {
                if bound.minimum.is_some_and(|v| m.value < v)
                    || bound.maximum.is_some_and(|v| m.value > v)
                {
                    reasons.push(format!("gate failed: {name}"));
                }
            }
        }
    }
    let value = match o.metrics.get(&p.objective) {
        None => {
            reasons.push("missing objective".into());
            None
        }
        Some(m) if m.quantity != p.objective_quantity => {
            return Err("objective quantity mismatch".into());
        }
        Some(m) => Some(m.value),
    };
    Ok(Score {
        context_id: o.context_id,
        candidate_id: o.candidate_id,
        eligible_score: if reasons.is_empty() { value } else { None },
        rejection_reasons: reasons,
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RankRequest {
    pub policy: Policy,
    pub outcomes: Vec<Outcome>,
}
/// Sort existing results only. No optimizer call or new experiment occurs.
pub fn rank(r: RankRequest) -> Result<Vec<Score>, String> {
    if r.outcomes.is_empty() {
        return Err("nonempty comparison set required".into());
    }
    let mut names = BTreeSet::new();
    let context = &r.outcomes[0].context_id;
    if r.outcomes
        .iter()
        .any(|o| o.context_id != *context || !names.insert(&o.candidate_id))
    {
        return Err("unique candidates from one declared comparison context required".into());
    }
    let mut scores = r
        .outcomes
        .into_iter()
        .map(|outcome| {
            score(ScoreRequest {
                policy: r.policy.clone(),
                outcome,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    scores.sort_by(|a, b| match (a.eligible_score, b.eligible_score) {
        (Some(x), Some(y)) => {
            let order = if r.policy.maximize {
                y.total_cmp(&x)
            } else {
                x.total_cmp(&y)
            };
            order.then(a.candidate_id.cmp(&b.candidate_id))
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.candidate_id.cmp(&b.candidate_id),
    });
    Ok(scores)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackingRequest {
    pub names: Vec<String>,
    pub quantities: Vec<Q>,
    pub time_s: Vec<f64>,
    pub targets: Vec<Vec<f64>>,
    pub observations: Vec<Vec<f64>>,
    pub after_startup_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackingStatistics {
    pub name: String,
    pub quantity: Q,
    pub samples: usize,
    pub rms_error: f64,
    pub maximum_absolute_error: f64,
    pub maximum_command_rate: f64,
    pub maximum_command_acceleration: f64,
}
pub fn tracking(r: TrackingRequest) -> Result<Vec<TrackingStatistics>, String> {
    let n = r.names.len();
    let mut names = BTreeSet::new();
    let rows = r.time_s.len();
    if n == 0
        || r.quantities.len() != n
        || rows < 3
        || r.targets.len() != rows
        || r.observations.len() != rows
        || r.names.iter().any(|n| n.is_empty() || !names.insert(n))
        || !r.after_startup_s.is_finite()
        || r.after_startup_s < 0.
        || r.time_s.iter().any(|t| !t.is_finite() || *t < 0.)
        || r.time_s.windows(2).any(|w| w[1] <= w[0])
        || r.targets
            .iter()
            .chain(&r.observations)
            .any(|v| v.len() != n || v.iter().any(|x| !x.is_finite()))
    {
        return Err("ordered finite matched samples, named channels and explicit startup exclusion required".into());
    }
    let begin = r
        .time_s
        .iter()
        .position(|t| *t >= r.after_startup_s)
        .ok_or("no samples after startup")?;
    let mut out = vec![];
    for j in 0..n {
        let errors: Vec<_> = (begin..rows)
            .map(|i| r.observations[i][j] - r.targets[i][j])
            .collect();
        let peak = errors.iter().map(|v| v.abs()).fold(0., f64::max);
        let rms = if peak == 0. {
            0.
        } else {
            peak * (errors.iter().map(|v| (v / peak).powi(2)).sum::<f64>() / errors.len() as f64)
                .sqrt()
        };
        let rates: Vec<_> = (1..rows)
            .map(|i| (r.targets[i][j] - r.targets[i - 1][j]) / (r.time_s[i] - r.time_s[i - 1]))
            .collect();
        let acceleration: Vec<_> = (1..rates.len())
            .map(|i| (rates[i] - rates[i - 1]) / ((r.time_s[i + 1] - r.time_s[i - 1]) * 0.5))
            .collect();
        if errors
            .iter()
            .chain(&rates)
            .chain(&acceleration)
            .any(|x| !x.is_finite())
            || !rms.is_finite()
        {
            return Err("tracking statistic overflow".into());
        }
        out.push(TrackingStatistics {
            name: r.names[j].clone(),
            quantity: r.quantities[j].clone(),
            samples: errors.len(),
            rms_error: rms,
            maximum_absolute_error: peak,
            maximum_command_rate: rates.iter().map(|v| v.abs()).fold(0., f64::max),
            maximum_command_acceleration: acceleration.iter().map(|v| v.abs()).fold(0., f64::max),
        });
    }
    Ok(out)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FidelityRequest {
    pub reference: crate::fidelity::EnvironmentCapture,
    pub candidate: crate::fidelity::EnvironmentCapture,
    pub plan: crate::fidelity::ComparisonPlan,
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), String> {
    for (name, description) in [(
        "experiment.score",
        "Gate a completed outcome using declared physical quantities",
    )] {
        registry.register_primitive(Descriptor::new(name,description,
            vec![F::structured("policy","explicit quantity SI units","Policy"),F::structured("outcome","explicit quantity SI units","Outcome")],
            vec![F::structured("$","objective SI unit; reasons and identities dimensionless","Score")],
            &["Incomplete, failed or missing-required-metric results cannot receive a score", "Context identity is supplied by the experiment journal; no calibration is inferred"]),score)?;
    }
    registry.register_primitive(Descriptor::new("experiment.rank","Rank saved gated outcomes without running search",
        vec![F::structured("policy","explicit quantity SI units","Policy"),F::structured("outcomes","explicit quantity SI units","Outcome array")],
        vec![F::structured("$","objective SI unit","Score array")],&["Requires unique candidate IDs in one comparison context; rejected outcomes are retained"]),rank)?;
    registry.register_primitive(Descriptor::new("experiment.tracking_statistics","Matched-time tracking and command derivative statistics",
        vec![F::structured("names","1","string array"),F::structured("quantities","SI quantities","quantity array"),F::quantity("time_s",Q::Time,"vector"),
            F::structured("targets","per-channel canonical SI units","sample-by-channel matrix"),F::structured("observations","per-channel canonical SI units","sample-by-channel matrix"),F::quantity("after_startup_s",Q::Time,"scalar")],
        vec![F::structured("$","channel unit; channel unit/s; channel unit/s²","TrackingStatistics array")],
        &["No resampling or lag fitting; RMS weights samples equally", "Rates use adjacent differences and accelerations use interval-center differences; command derivatives include startup"]),tracking)?;
    registry.register_primitive(
        Descriptor::new(
            "experiment.compare_fidelity",
            "Compare existing captures with explicit configuration and accuracy changes",
            vec![
                F::structured("reference", "SI capture quantities", "EnvironmentCapture"),
                F::structured("candidate", "SI capture quantities", "EnvironmentCapture"),
                F::structured("plan", "explicit SI tolerances", "ComparisonPlan"),
            ],
            vec![F::structured(
                "$",
                "SI channel errors and recorded timing",
                "ComparisonReport",
            )],
            &[
                "Uses the existing matched-input fidelity comparator; neither capture is executed",
                "Fidelity agreement does not establish hardware accuracy",
            ],
        ),
        |r: FidelityRequest| crate::fidelity::compare(&r.reference, &r.candidate, &r.plan),
    )
}
