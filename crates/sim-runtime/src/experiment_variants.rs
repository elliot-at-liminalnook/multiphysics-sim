//! Explicit, reproducible validation cases over an existing experiment.
//! No physics is advanced and no override is promoted back into CAD.
use crate::{
    experiment::ExperimentSpec,
    physics_context::{RuntimeIdentity, fingerprint},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Robot,
    Config,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    Scale { factor: f64 },
    Set { value: f64 },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub target: Target,
    pub pointer: String,
    /// Authored unit at this pointer. The model registry still owns validation.
    pub unit: String,
    pub change: Change,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceIncrement {
    pub input: String,
    pub increment: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldTail {
    /// Other commands retain the last source row. Increment only these named
    /// channels (for example the explicit teleoperation lease sequence).
    pub sequence: Vec<SequenceIncrement>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    pub rationale: String,
    pub duration_s: f64,
    pub tail: Option<HeldTail>,
    pub edits: Vec<Edit>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppliedEdit {
    pub target: Target,
    pub pointer: String,
    pub unit: String,
    pub before: f64,
    pub after: f64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Prepared {
    pub spec: ExperimentSpec,
    pub case: Case,
    pub source_spec_blake3: String,
    pub prepared_spec_blake3: String,
    pub preparer_runtime: RuntimeIdentity,
    pub source_duration_s: f64,
    pub added_action_intervals: usize,
    pub edits: Vec<AppliedEdit>,
    pub scope: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub source: ExperimentSpec,
    pub case: Case,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimestepRequest {
    pub source: ExperimentSpec,
    pub divisor: usize,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct TimestepRefinement {
    pub spec: ExperimentSpec,
    pub divisor: usize,
    pub source_step_s: f64,
    pub refined_step_s: f64,
    pub source_spec_blake3: String,
    pub prepared_spec_blake3: String,
    pub preparer_runtime: RuntimeIdentity,
}

/// Refine only the physics clock, preserving elapsed horizon, command cadence,
/// reporting cadence, model and all source inputs. Does not execute physics.
pub fn refine_timestep(
    source: &ExperimentSpec,
    divisor: usize,
) -> Result<TimestepRefinement, String> {
    let c = &source.config;
    if divisor < 2
        || !c.step_s.is_finite()
        || c.step_s <= 0.
        || c.steps == 0
        || c.report_every == 0
        || c.steps % c.report_every != 0
    {
        return Err("valid source clock and timestep divisor >= 2 required".into());
    }
    let mut spec = source.clone();
    spec.config.step_s /= divisor as f64;
    spec.config.steps = c.steps.checked_mul(divisor).ok_or("step count overflow")?;
    spec.config.report_every = c
        .report_every
        .checked_mul(divisor)
        .ok_or("report stride overflow")?;
    if spec.config.steps as u128 > (1_u128 << 53)
        || spec.config.step_s <= 0.
        || spec.config.step_s >= c.step_s
    {
        return Err("refined physics clock cannot be represented exactly enough".into());
    }
    for period in [source.scene.period_s, source.task.period_s] {
        let ticks = period / spec.config.step_s;
        if !ticks.is_finite()
            || ticks < 1.
            || ticks > 1_000_000.
            || (ticks - ticks.round()).abs() > 1e-8
        {
            return Err("refinement must preserve aligned controller/task clocks".into());
        }
    }
    let source_spec_blake3 = fingerprint(&serde_json::to_value(source).map_err(|e| e.to_string())?);
    let prepared_spec_blake3 =
        fingerprint(&serde_json::to_value(&spec).map_err(|e| e.to_string())?);
    Ok(TimestepRefinement {
        refined_step_s: spec.config.step_s,
        spec,
        divisor,
        source_step_s: c.step_s,
        source_spec_blake3,
        prepared_spec_blake3,
        preparer_runtime: RuntimeIdentity::current(),
    })
}

/// Prepare a full-spec case, retaining Scene's original-input override receipt.
/// Numerical edits are explicit; this function never invents missing fields.
pub fn prepare(source: &ExperimentSpec, case: &Case) -> Result<Prepared, String> {
    if case.name.is_empty()
        || !case
            .name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        || case.rationale.trim().is_empty()
        || !case.duration_s.is_finite()
        || case.duration_s <= 0.
    {
        return Err(
            "validation case needs safe name, rationale and finite positive duration".into(),
        );
    }
    let source_duration_s = source.config.steps as f64 * source.config.step_s;
    let ticks = case.duration_s / source.config.step_s;
    let intervals = case.duration_s / source.task.period_s;
    if !source_duration_s.is_finite()
        || source_duration_s <= 0.
        || source.source_actions.is_empty()
        || !ticks.is_finite()
        || ticks < 1.
        || ticks > usize::MAX as f64
        || (ticks - ticks.round()).abs() > 1e-7
        || !intervals.is_finite()
        || intervals < 1.
        || intervals > 1_000_000.
        || (intervals - intervals.round()).abs() > 1e-8
        || ((source.source_actions.len() as f64) * source.task.period_s - source_duration_s).abs()
            > 1e-8
        || case.duration_s < source_duration_s - 1e-10
    {
        return Err(
            "validation must retain/extend a complete source and align physics/task clocks".into(),
        );
    }
    let mut spec = source.clone();
    let count = intervals.round() as usize;
    let extra = count
        .checked_sub(source.source_actions.len())
        .ok_or("validation cannot truncate source inputs")?;
    let inputs = &source
        .scene
        .controller
        .as_ref()
        .ok_or("missing explicit controller input contract")?
        .inputs;
    for row in &source.source_actions {
        crate::forecast_actions::validate_values(inputs, row)?;
    }
    if extra > 0 {
        let tail = case
            .tail
            .as_ref()
            .ok_or("longer validation requires explicit held-tail policy")?;
        let mut seen = BTreeSet::new();
        let mut increments = vec![];
        for item in &tail.sequence {
            if !seen.insert(&item.input) || !item.increment.is_finite() || item.increment == 0. {
                return Err(
                    "unique sequence inputs with finite nonzero increments required".into(),
                );
            }
            let index = inputs
                .iter()
                .position(|i| i.name == item.input)
                .ok_or("missing tail sequence input")?;
            increments.push((index, item.increment));
        }
        let mut row = source.source_actions.last().unwrap().clone();
        for _ in 0..extra {
            for (index, increment) in &increments {
                row[*index] += increment;
            }
            crate::forecast_actions::validate_values(inputs, &row)?;
            spec.source_actions.push(row.clone());
        }
    } else if case.tail.is_some() {
        return Err("tail policy supplied without horizon extension".into());
    }
    spec.config.steps = ticks.round() as usize;
    spec.scene.duration_s = case.duration_s;
    let mut robot = serde_json::to_value(&spec.scene.robot).map_err(|e| e.to_string())?;
    let mut config = serde_json::to_value(&spec.config).map_err(|e| e.to_string())?;
    let mut seen = BTreeSet::new();
    let mut receipts = vec![];
    for e in &case.edits {
        if e.pointer.is_empty()
            || !e.pointer.starts_with('/')
            || e.unit.trim().is_empty()
            || !seen.insert((e.target, e.pointer.clone()))
        {
            return Err("unique explicit numeric paths and authored units required".into());
        }
        // Clock edits belong to duration/tail preparation, never a physical case.
        if e.target == Target::Config
            && ["/step_s", "/steps", "/report_every"].contains(&e.pointer.as_str())
        {
            return Err("physical validation edits cannot change numerical clocks".into());
        }
        let document = match e.target {
            Target::Robot => &mut robot,
            Target::Config => &mut config,
        };
        let slot = document
            .pointer_mut(&e.pointer)
            .ok_or_else(|| format!("missing case field {}", e.pointer))?;
        let before = slot
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or("case edit needs an existing finite number")?;
        let after = match e.change {
            Change::Scale { factor } if factor.is_finite() && factor > 0. => before * factor,
            Change::Set { value } => value,
            _ => return Err("positive finite scale required".into()),
        };
        if !after.is_finite() || after == before {
            return Err("ineffective or overflowing validation edit".into());
        }
        *slot = json!(after);
        receipts.push(AppliedEdit {
            target: e.target,
            pointer: e.pointer.clone(),
            unit: e.unit.clone(),
            before,
            after,
        });
    }
    spec.scene.robot = serde_json::from_value(robot).map_err(|e| e.to_string())?;
    spec.config = serde_json::from_value(config).map_err(|e| e.to_string())?;
    // Re-read through Scene's ordinary boundary, which verifies the generated
    // RobotInput override receipt. Existing parameterization remains unchanged.
    let value = serde_json::to_value(&spec).map_err(|e| e.to_string())?;
    let _: ExperimentSpec = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    spec.parameterization
        .materialize(&spec.scene, &spec.source_actions, &spec.baseline)?;
    Ok(Prepared{spec,case:case.clone(),source_spec_blake3:fingerprint(&serde_json::to_value(source).map_err(|e|e.to_string())?),prepared_spec_blake3:fingerprint(&value),preparer_runtime:RuntimeIdentity::current(),source_duration_s,added_action_intervals:extra,edits:receipts,
        scope:"Prepared sensitivity experiment only. No simulation or hardware result and no CAD promotion. Units are authored case metadata; model registry validation still applies. Source input prefix is preserved and any held-tail renewal is explicit.".into()})
}
pub fn register(registry: &mut sim_core::BehaviorRegistry) -> Result<(), String> {
    use sim_core::primitive::{Descriptor, Field};
    registry.register_primitive(
        Descriptor::new(
            "experiment.refine_timestep",
            "Prepare a matched experiment at a finer physics timestep",
            vec![Field::structured(
                "$",
                "s; divisor dimensionless",
                "experiment_variants::TimestepRequest",
            )],
            vec![Field::structured(
                "$",
                "s",
                "experiment_variants::TimestepRefinement",
            )],
            &[
                "Preserves model, commands, controller/task clocks and elapsed horizon",
                "Preparation only; runtime binding and trajectory comparison remain required",
            ],
        ),
        |r: TimestepRequest| refine_timestep(&r.source, r.divisor),
    )?;
    registry.register_primitive(
        Descriptor::new(
            "experiment.prepare_validation",
            "Prepare explicit physical sensitivity and extended-horizon cases",
            vec![Field::structured(
                "$",
                "s; units declared per existing numeric field",
                "experiment_variants::Request",
            )],
            vec![Field::structured(
                "$",
                "same authored units",
                "experiment_variants::Prepared",
            )],
            &[
                "No physics advance or CAD mutation",
                "Extending a horizon requires explicit held commands and sequence renewal",
                "Overrides retain original values and provenance",
            ],
        ),
        |r: Request| prepare(&r.source, &r.case),
    )
}
