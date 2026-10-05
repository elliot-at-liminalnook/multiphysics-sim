//! Studies: run one system several ways and put the results side by side.
//!
//! A comparison swaps one instance for each alternative (shared `Swap`
//! command, same port-contract check as the editors); a sweep sets one
//! parameter to each value (shared `SetParameter`). Every variant runs
//! headlessly through [`system_builder::simulate`], the same session the
//! live viewers use, so a study is reproducible from the system file alone.
//! Variants run on worker threads; hosts call this off the UI thread.
use crate::system_builder::{self, Series};
use serde::Serialize;
use sim_core::{BehaviorRegistry, DerivedValue};
use sim_system::{Command, InstanceKind, Metric, ParameterBinding, Reduce, Study, StudyKind, SystemDocument};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Debug, Clone, Serialize)]
pub struct VariantResult {
    pub label: String,
    /// Sweep value, for sweeps.
    pub value: Option<f64>,
    pub series: Vec<Series>,
    /// Metric label → value, in the study's metric order.
    pub metrics: Vec<(String, f64)>,
    /// Values derived from the notes of the varied element(s).
    pub derived: Vec<DerivedValue>,
    pub error: Option<String>,
    /// Wall time of this variant's run, seconds.
    pub wall_seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StudyResult {
    pub name: String,
    pub kind: &'static str,
    pub parameter: Option<String>,
    /// Content hash of the document the study ran on.
    pub source_hash: String,
    pub variants: Vec<VariantResult>,
}

/// The document for one variant, or why it cannot be built.
pub fn variant_document(document: &SystemDocument, registry: &BehaviorRegistry, study: &Study, index: usize) -> Result<(String, Option<f64>, SystemDocument), String> {
    let mut doc = document.clone();
    doc.studies.clear();
    match &study.kind {
        StudyKind::Compare { alternatives } => {
            if index == 0 {
                let current = current_kind(document, registry, study)?;
                return Ok((format!("{} (current)", kind_name(document, registry, &current)), None, doc));
            }
            let kind = alternatives.get(index - 1).ok_or("variant index out of range")?;
            let mut commands = Vec::new();
            if let InstanceKind::Subsystem { definition } = kind {
                if !doc.definitions.contains_key(definition) {
                    return Err(format!("definition `{definition}` is not in the document; import it first"));
                }
            }
            commands.push(Command::Swap { at: study.at.clone(), name: study.instance.clone(), kind: kind.clone(), keep_parameters: true });
            sim_system::apply(&mut doc, registry, &commands).map_err(|e| e.to_string())?;
            // Values the replacement needs but the original did not have come
            // from its notes' typical values, recorded as estimates.
            if let InstanceKind::Element { component_type } = kind {
                let starter = sim_system::snap::starter(registry, kind, "");
                let parent = sim_system::Resolver::new(&doc, registry).definition_id_at(&study.at).map_err(|e| e.to_string())?;
                let have = doc.definitions[&parent].instances[&study.instance].parameters.clone();
                let fill: Vec<Command> = starter
                    .parameters
                    .into_iter()
                    .filter(|(k, _)| !have.contains_key(k))
                    .map(|(parameter, binding)| Command::SetParameter { at: study.at.clone(), name: study.instance.clone(), parameter, binding: Some(binding) })
                    .collect();
                let _ = component_type;
                sim_system::apply(&mut doc, registry, &fill).map_err(|e| e.to_string())?;
            }
            Ok((kind_name(document, registry, kind), None, doc))
        }
        StudyKind::Sweep { parameter, values } => {
            let value = *values.get(index).ok_or("variant index out of range")?;
            let (at, name, parameter) = parameter_target(study, parameter);
            sim_system::apply(&mut doc, registry, &[Command::SetParameter { at, name, parameter: parameter.clone(), binding: Some(ParameterBinding::value(value)) }]).map_err(|e| e.to_string())?;
            Ok((format!("{parameter} = {}", trim(value)), Some(value), doc))
        }
    }
}

fn trim(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `"mesh/worm_starts"` on instance `gearbox` → (at "gearbox", name "mesh", "worm_starts").
fn parameter_target(study: &Study, parameter: &str) -> (String, String, String) {
    match parameter.rsplit_once('/') {
        Some((inner, p)) => {
            let path = sim_system::join_path(&sim_system::join_path(&study.at, &study.instance), inner);
            let (at, name) = path.rsplit_once('/').map(|(a, n)| (a.to_string(), n.to_string())).unwrap_or((String::new(), path.clone()));
            (at, name, p.to_string())
        }
        None => (study.at.clone(), study.instance.clone(), parameter.to_string()),
    }
}

fn current_kind(document: &SystemDocument, registry: &BehaviorRegistry, study: &Study) -> Result<InstanceKind, String> {
    let r = sim_system::Resolver::new(document, registry);
    let id = r.definition_id_at(&study.at).map_err(|e| e.to_string())?;
    Ok(document.definitions[&id].instances.get(&study.instance).ok_or_else(|| format!("no instance `{}`", study.instance))?.kind.clone())
}

fn kind_name(document: &SystemDocument, registry: &BehaviorRegistry, kind: &InstanceKind) -> String {
    match kind {
        InstanceKind::Subsystem { definition } => document.definitions.get(definition).map(|d| d.label.clone()).unwrap_or_else(|| definition.clone()),
        InstanceKind::Element { component_type } => registry.get(&component_type.as_str().into()).map(|d| d.display_name.to_string()).unwrap_or_else(|_| component_type.clone()),
        other => sim_system::kind_label(other),
    }
}

/// Derived values (from component notes) of every noted element the study
/// varies: the instance itself, or the elements inside a subsystem.
fn derived_for(document: &SystemDocument, registry: &BehaviorRegistry, study: &Study) -> Vec<DerivedValue> {
    let r = sim_system::Resolver::new(document, registry);
    let Ok(parent) = r.definition_id_at(&study.at) else { return Vec::new() };
    let Some(spec) = document.definitions[&parent].instances.get(&study.instance) else { return Vec::new() };
    let mut elements: Vec<(String, &sim_system::InstanceSpec)> = Vec::new();
    match &spec.kind {
        InstanceKind::Element { .. } => elements.push((String::new(), spec)),
        InstanceKind::Subsystem { definition } => {
            if let Some(d) = document.definitions.get(definition) {
                for (n, i) in &d.instances {
                    if matches!(i.kind, InstanceKind::Element { .. }) {
                        elements.push((format!("{n}: "), i));
                    }
                }
            }
        }
        InstanceKind::Generated { .. } | InstanceKind::Block { .. } => {}
    }
    let mut out = Vec::new();
    for (prefix, i) in elements {
        let InstanceKind::Element { component_type } = &i.kind else { continue };
        let Some(notes) = registry.get(&component_type.as_str().into()).ok().and_then(|d| d.notes).filter(|n| n.has_derived()) else { continue };
        let explicit = i.parameters.iter().filter_map(|(k, v)| match v {
            ParameterBinding::Value { value, .. } => Some((k.clone(), *value)),
            _ => None,
        }).collect();
        for mut d in notes.derive(&sim_system::library::effective_parameters(registry, component_type, &explicit)) {
            d.name = format!("{prefix}{}", d.name);
            out.push(d);
        }
    }
    out
}

pub fn reduce(series: &Series, metric: &Metric) -> Option<f64> {
    let (a, b) = metric.window.map(|w| (w[0], w[1])).unwrap_or((f64::NEG_INFINITY, f64::INFINITY));
    let points: Vec<(f64, f64)> = series.times.iter().zip(&series.values).filter(|(t, _)| **t >= a - 1e-12 && **t <= b + 1e-12).map(|(t, v)| (*t, *v)).collect();
    let (first, last) = (points.first()?, points.last()?);
    Some(match metric.reduce {
        Reduce::Final => last.1,
        Reduce::Mean => points.iter().map(|p| p.1).sum::<f64>() / points.len() as f64,
        Reduce::Max => points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
        Reduce::Min => points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
        Reduce::Peak => points.iter().map(|p| p.1.abs()).fold(0., f64::max),
        Reduce::Change => last.1 - first.1,
        Reduce::Integral => points.windows(2).map(|w| 0.5 * (w[0].1 + w[1].1) * (w[1].0 - w[0].0)).sum(),
    })
}

pub fn variant_count(study: &Study) -> usize {
    match &study.kind {
        StudyKind::Compare { alternatives } => alternatives.len() + 1,
        StudyKind::Sweep { values, .. } => values.len(),
    }
}

/// Run every variant (in parallel, at most `threads` at once). `progress`
/// receives (finished, total); `cancel` stops before starting more variants.
/// `base`: the system file's directory (generated robots, FMU blocks);
/// None for a document that came from no file.
#[allow(clippy::too_many_arguments)]
pub fn run(document: &SystemDocument, registry: &BehaviorRegistry, base: Option<&std::path::Path>, name: &str, study: &Study, threads: usize, cancel: Option<&AtomicBool>, progress: &(dyn Fn(usize, usize) + Sync)) -> Result<StudyResult, String> {
    let total = variant_count(study);
    if total == 0 {
        return Err("the study has no variants".into());
    }
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results: std::sync::Mutex<Vec<Option<VariantResult>>> = std::sync::Mutex::new(vec![None; total]);
    std::thread::scope(|scope| {
        for _ in 0..threads.clamp(1, total) {
            scope.spawn(|| loop {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return;
                }
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= total {
                    return;
                }
                let started = std::time::Instant::now();
                let result = match variant_document(document, registry, study, i) {
                    Err(e) => VariantResult { label: format!("variant {i}"), value: None, series: Vec::new(), metrics: Vec::new(), derived: Vec::new(), error: Some(e), wall_seconds: 0. },
                    Ok((label, value, doc)) => {
                        let derived = derived_for(&doc, registry, study);
                        let config = system_builder::config_for(&doc);
                        let run = match base {
                            Some(base) => system_builder::simulate_at(&doc, registry, base, study.duration, config, &study.observe),
                            None => system_builder::simulate(&doc, registry, study.duration, config, &study.observe),
                        };
                        match run {
                            Ok(series) => {
                                let metrics = study
                                    .metrics
                                    .iter()
                                    .map(|m| {
                                        let s = series.iter().find(|s| s.label == m.observable || s.observable == m.observable).or_else(|| series.iter().find(|s| s.label.contains(&m.observable)));
                                        (m.label.clone(), s.and_then(|s| reduce(s, m)).unwrap_or(f64::NAN))
                                    })
                                    .collect();
                                VariantResult { label, value, series, metrics, derived, error: None, wall_seconds: started.elapsed().as_secs_f64() }
                            }
                            Err(e) => VariantResult { label, value, series: Vec::new(), metrics: Vec::new(), derived, error: Some(e), wall_seconds: started.elapsed().as_secs_f64() },
                        }
                    }
                };
                results.lock().unwrap()[i] = Some(result);
                progress(done.fetch_add(1, Ordering::Relaxed) + 1, total);
            });
        }
    });
    let variants: Vec<VariantResult> = results.into_inner().unwrap().into_iter().flatten().collect();
    if variants.len() < total {
        return Err("cancelled".into());
    }
    Ok(StudyResult {
        name: name.to_string(),
        kind: match study.kind {
            StudyKind::Compare { .. } => "compare",
            StudyKind::Sweep { .. } => "sweep",
        },
        parameter: match &study.kind {
            StudyKind::Sweep { parameter, .. } => Some(parameter.clone()),
            _ => None,
        },
        source_hash: document.content_hash(),
        variants,
    })
}

/// A plain-text trade-off table: one row per variant, one column per metric
/// and derived value.
pub fn table(result: &StudyResult) -> String {
    let mut columns: Vec<String> = Vec::new();
    for v in &result.variants {
        for (k, _) in &v.metrics {
            if !columns.contains(k) {
                columns.push(k.clone());
            }
        }
        for d in &v.derived {
            if !columns.contains(&d.name) {
                columns.push(d.name.clone());
            }
        }
    }
    let mut out = format!("| variant | {} |\n|---|{}\n", columns.join(" | "), "---|".repeat(columns.len()));
    for v in &result.variants {
        let cells: Vec<String> = columns
            .iter()
            .map(|c| {
                v.metrics.iter().find(|(k, _)| k == c).map(|(_, x)| format!("{x:.4}")).or_else(|| v.derived.iter().find(|d| &d.name == c).map(|d| format!("{:.3} {}", d.value, d.unit))).unwrap_or_else(|| "–".into())
            })
            .collect();
        out.push_str(&format!("| {} | {} |\n", v.error.as_ref().map(|e| format!("{} (failed: {e})", v.label)).unwrap_or_else(|| v.label.clone()), cells.join(" | ")));
    }
    out
}
