//! Promote a characterization campaign's fits into a CAD actuator family.
//!
//! The campaign measures in the servo's duty domain, so only combinations of
//! physical parameters are identified; this module derives exactly those and
//! leaves everything else at the base family's (estimated) values:
//!   * steady speed slope ω/d = V/(Ke·N) → Ke = Kt given the base ratio N;
//!   * zero-speed intercepts per direction → friction at speed (half-sum) and
//!     gravity (half-difference); friction beyond the rotor loss becomes
//!     output gear friction through the base resistance and efficiency;
//!   * the measured operating envelope (full-drive speed, acceleration,
//!     coast deceleration, low-speed friction, breakaway) at the supply voltage.
//! The derivation record lists every input, equation and intermediate value.
use super::characterization::{Report, StageResult};
use serde_json::{Value, json};
use sim_domain_robot::actuator_profile::{Envelope, Evidence, Family, Parameter, Provenance};

const RAD: f64 = std::f64::consts::TAU / 4096.;
type R<T> = Result<T, String>;

/// Where the campaign and the promotion code live, for the family's evidence.
pub struct Sources {
    pub report_path: String,
    pub report_sha256: String,
    pub scope: String,
    pub code_path: String,
    pub code_sha256: String,
}

fn p(value: f64, unit: &str, provenance: Provenance, uncertainty: Option<f64>, evidence: &str) -> Parameter {
    Parameter { value, unit: unit.into(), provenance, uncertainty, evidence: evidence.into() }
}
fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len().max(1) as f64
}
fn half_spread(x: &[f64]) -> f64 {
    let (lo, hi) = x.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    if x.is_empty() { 0. } else { 0.5 * (hi - lo) }
}

/// Per stage-D run and direction: (speed slope counts/s per duty, zero-speed duty).
fn step_lines(stage: &StageResult) -> R<[(f64, f64); 2]> {
    let fits = stage.metrics["fits"].as_array().ok_or("stage D has no fits")?;
    let mut out = [(0., 0.); 2];
    for (k, sign) in [(0usize, 1.), (1, -1.)] {
        let mut pts: Vec<(f64, f64)> = fits.iter().filter_map(|f| {
            let d = f["duty"].as_f64()?;
            (d * sign > 0.).then(|| (d.abs(), f["speed_counts_s"].as_f64().unwrap_or(0.).abs()))
        }).collect();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (Some(lo), Some(hi)) = (pts.first(), pts.last()) else { return Err("stage D needs steps in both directions".into()) };
        if hi.0 - lo.0 < 0.05 {
            return Err("stage D needs two drive levels per direction".into());
        }
        let slope = (hi.1 - lo.1) / (hi.0 - lo.0);
        out[k] = (slope, hi.0 - hi.1 / slope);
    }
    Ok(out)
}

/// Derive a family for campaign axis `id` from `base`. Returns the family and
/// the derivation record.
pub fn derive_family(report: &Report, id: u8, base: &Family, description: &str, sources: &Sources) -> R<(Family, Value)> {
    let d_runs: Vec<&StageResult> = report.stages.iter().filter(|s| s.stage == "D" && s.id == id && s.completed).collect();
    if d_runs.is_empty() {
        return Err(format!("axis {id}: no completed stage D"));
    }
    let lines: Vec<[(f64, f64); 2]> = d_runs.iter().map(|s| step_lines(s)).collect::<R<_>>()?;
    let slopes: Vec<f64> = lines.iter().flat_map(|l| [l[0].0, l[1].0]).collect();
    let frictions: Vec<f64> = lines.iter().map(|l| 0.5 * (l[0].1 + l[1].1)).collect();
    let gravities: Vec<f64> = lines.iter().map(|l| 0.5 * (l[0].1 - l[1].1)).collect();
    let mut volts: Vec<f64> = report.samples.iter().filter(|x| x.id == id && x.duty.is_finite() && x.duty.abs() > 0.1).map(|x| x.voltage_v).collect();
    if volts.is_empty() {
        return Err(format!("axis {id}: no driven samples for the supply voltage"));
    }
    volts.sort_by(f64::total_cmp);
    let supply = volts[volts.len() / 2];
    let voltage_resolution = 0.1;

    let get = |name: &str| base.motor.get(name).map(|q| q.value).ok_or(format!("base family lacks motor.{name}"));
    let (ratio, resistance, efficiency, rotor_loss) = (get("ratio")?, get("resistance")?, get("efficiency")?, get("no_load_current")?);

    let slope = mean(&slopes);
    let slope_u = half_spread(&slopes).max(0.05 * slope);
    let speed_per_duty = slope * RAD;
    let ke_n = supply / speed_per_duty;
    let ke_n_u = ke_n * (slope_u / slope).hypot(voltage_resolution / supply);
    let (ke, ke_u) = (ke_n / ratio, ke_n_u / ratio);

    let friction = mean(&frictions);
    let friction_u = half_spread(&frictions).max(0.005);
    let friction_current = supply * friction / resistance;
    let gear_current = friction_current - rotor_loss;
    if gear_current < 0. {
        return Err(format!("axis {id}: friction {friction_current:.3} A is below the base rotor loss {rotor_loss} A; revisit no_load_current"));
    }
    let gear_friction = ratio * efficiency * ke * gear_current;
    let gear_friction_u = ratio * efficiency * (ke * supply * friction_u / resistance).hypot(ke_u * gear_current);

    // Envelope from the fitted campaign quantities (their own uncertainties).
    let fits = report.fitted.iter().find(|(i, _)| *i == id).map(|(_, f)| f).ok_or("no fits for axis")?;
    let fit = |name: &str| fits.iter().find(|f| f.name == name).map(|f| (f.value, f.uncertainty));
    let pair_min = |a: &str, b: &str| -> R<(f64, f64)> {
        let (x, y) = (fit(a).ok_or(format!("missing {a}"))?, fit(b).ok_or(format!("missing {b}"))?);
        Ok(if x.0 <= y.0 { x } else { y })
    };
    let full_drive = speed_per_duty * (1. - friction);
    let full_drive_u = full_drive * ((slope_u / slope).powi(2) + (friction_u / (1. - friction)).powi(2)).sqrt();
    let acceleration = pair_min("acceleration_increasing", "acceleration_decreasing")?;
    let coast = pair_min("braking_increasing", "braking_decreasing")?;
    let low_friction = fit("moving_friction").ok_or("missing moving_friction")?;
    let breakaway = fit("breakaway").ok_or("missing breakaway")?;
    let top_drive = report.stages.iter().filter(|s| s.stage == "G" && s.id == id && s.completed)
        .filter_map(|s| s.metrics["ladder"].as_array().map(|l| l.iter().filter_map(|r| r["duty"].as_f64()).fold(0f64, f64::max)))
        .fold(0f64, f64::max);

    let ev = "campaign";
    let mut family = base.clone();
    family.version = 1;
    family.description = description.into();
    family.evidence.insert(ev.into(), Evidence { path: sources.report_path.clone(), sha256: sources.report_sha256.clone(), scope: sources.scope.clone() });
    family.evidence.insert("promotion".into(), Evidence { path: sources.code_path.clone(), sha256: sources.code_sha256.clone(), scope: "Explicit duty-domain to physical-parameter derivation (actuator_promotion.rs).".into() });
    family.motor.insert("back_emf_constant".into(), p(ke, "V·s/rad", Provenance::Derived, Some(ke_u), ev));
    family.motor.insert("torque_constant".into(), p(ke, "N·m/A", Provenance::Derived, Some(ke_u), ev));
    family.motor.insert("gear_friction".into(), p(gear_friction, "N·m", Provenance::Derived, Some(gear_friction_u), ev));
    family.envelope = Some(Envelope {
        supply_voltage: p(supply, "V", Provenance::Measured, Some(voltage_resolution), ev),
        full_drive_speed: p(full_drive, "rad/s", Provenance::Derived, Some(full_drive_u), ev),
        acceleration: p(acceleration.0 * RAD, "rad/s²", Provenance::Measured, Some(acceleration.1 * RAD), ev),
        coast_deceleration: p(coast.0 * RAD, "rad/s²", Provenance::Measured, Some(coast.1 * RAD), ev),
        low_speed_friction: p(low_friction.0, "1", Provenance::Measured, Some(low_friction.1), ev),
        breakaway: p(breakaway.0, "1", Provenance::Measured, Some(breakaway.1), ev),
    });
    let mut limitations = vec![
        "One physical servo on a suspended printed leg (no ground load); applied to every joint of this role as a type profile, without per-unit identities.".to_string(),
        format!("Only Ke·N is identified ({ke_n:.3} V·s/rad at the output); Ke = Kt is derived given the estimated {ratio}:1 ratio."),
        format!("Gear friction comes from at-speed friction ({friction:.3} duty) through the estimated resistance {resistance} Ω, efficiency {efficiency:.3} and rotor loss {rotor_loss} A; its uncertainty is conditional on those estimates."),
        format!("Low-speed friction ({:.3}) and breakaway ({:.3}) exceed the at-speed value; the regularized Coulomb model does not represent stiction. They are kept in the envelope.", low_friction.0, breakaway.0),
        format!("Envelope acceleration is the lower direction's value at up to {:.0}% drive in the air; coast deceleration is with drive removed.", top_drive * 100.),
        "Step timing in the campaign is dominated by the host serial loop (~30 ms); no latency or inertia is derived from it.".to_string(),
        "Resistance, inductance, inertia, efficiency, backlash, stiffness, current and thermal values are not identified and remain the base family's estimates.".to_string(),
    ];
    limitations.extend(base.limitations.iter().filter(|l| !l.contains("Unloaded bench only") && !l.contains("No physical motor identities")).cloned());
    family.limitations = limitations;

    let derivation = json!({
        "version": 1,
        "axis": id,
        "campaign_report": {"path": sources.report_path, "sha256": sources.report_sha256},
        "base_family_hash": base.content_hash(),
        "inputs": {"stage_D_lines": lines.iter().map(|l| json!({"increasing": {"slope_counts_s_per_duty": l[0].0, "zero_speed_duty": l[0].1}, "decreasing": {"slope_counts_s_per_duty": l[1].0, "zero_speed_duty": l[1].1}})).collect::<Vec<_>>(),
                   "supply_v_median_while_driven": supply, "estimated_ratio": ratio, "estimated_resistance_ohm": resistance,
                   "estimated_efficiency": efficiency, "estimated_rotor_loss_a": rotor_loss},
        "equations": {"back_emf": "Ke = Kt = V/(slope·N), slope in output rad/s per duty",
                      "friction_duty": "half-sum of both directions' zero-speed intercepts",
                      "gravity_duty": "half-difference of the intercepts (increasing minus decreasing)",
                      "gear_friction": "N·η·Kt·(V·d_f/R − I0)",
                      "full_drive_speed": "slope·(1 − d_f)"},
        "results": {"slope_counts_s_per_duty": slope, "slope_uncertainty": slope_u, "ke_times_ratio": ke_n, "ke": ke, "ke_uncertainty": ke_u,
                    "friction_duty_at_speed": friction, "friction_uncertainty": friction_u, "gravity_duty_mid_range": mean(&gravities),
                    "gear_friction_nm": gear_friction, "gear_friction_uncertainty": gear_friction_u,
                    "full_drive_speed_rad_s": full_drive, "base_ke": base.motor.get("back_emf_constant").map(|q| q.value)},
        "family_hash": family.content_hash(),
    });
    Ok((family, derivation))
}
