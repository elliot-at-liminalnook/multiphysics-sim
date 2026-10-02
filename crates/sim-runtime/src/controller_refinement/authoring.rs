//! Reusable offline authoring validation. No physics or UI ownership.
use super::{calibration::{self, Coordinate, Family, Variant}, control::{Experiment, Policy}};

pub fn experiment(e: &Experiment) -> Result<(), String> {
    for (path, ok) in [
        ("version",e.version==1),("name",!e.name.trim().is_empty()),
        ("device",(1..=253).contains(&e.device)),("fixture",!e.fixture.trim().is_empty()),
        ("component_id",e.component_id.as_ref().is_none_or(|s|!s.trim().is_empty())),
        ("duration_s",e.duration_s.is_finite() && e.duration_s>=e.timing.period_s && e.duration_s<=60.),
        ("voltage_v",e.voltage_v.is_finite() && e.voltage_v>0.),
        ("temperature_c",e.temperature_c.is_finite() && e.temperature_c> -273.15),
        ("initial_encoder_rad",e.initial_encoder_rad.is_finite()),
        ("timing.period_s",e.timing.period_s.is_finite() && (0.001..=1.).contains(&e.timing.period_s)),
        ("timing.encoder_quantum_rad",e.timing.encoder_quantum_rad.is_finite() && (0. ..=std::f64::consts::TAU).contains(&e.timing.encoder_quantum_rad) && e.timing.encoder_quantum_rad>0.),
        ("timing.velocity_filter_s",e.timing.velocity_filter_s.is_finite() && e.timing.velocity_filter_s>=0.),
        ("timing.maximum_observation_age_s",e.timing.maximum_observation_age_s.is_finite() && e.timing.maximum_observation_age_s>=e.timing.period_s),
        ("timing.observation_delay_ticks",e.timing.observation_delay_ticks<=100),
        ("timing.command_delay_ticks",e.timing.command_delay_ticks<=100),
        ("timing.evidence",!e.timing.evidence.trim().is_empty()),
    ] { if !ok {return Err(format!("refinement.experiment.{path}: invalid value"));} }
    if e.trajectory.len()<2 {return Err("refinement.experiment.trajectory: at least two knots required".into());}
    for (i,k) in e.trajectory.iter().enumerate() {
        if !k.time_s.is_finite() || k.time_s<0. || k.time_s>e.duration_s || (i==0 && k.time_s!=0.) || (i>0 && e.trajectory[i-1].time_s>=k.time_s) {return Err(format!("refinement.experiment.trajectory.{i}.time_s: strictly ordered knots starting at zero within duration required"));}
        if !k.position_rad.is_finite() {return Err(format!("refinement.experiment.trajectory.{i}.position_rad: finite angle required"));}
    }
    match &e.controller {
        Policy::RustPid{parameters:p} => {
            for (name,value) in [("kp",p.kp),("ki",p.ki),("kd",p.kd),("integral_limit",p.integral_limit),("duty_limit",p.duty_limit)] {
                if !value.is_finite() || value<0. || (name=="duty_limit" && (value==0. || value>1.)) {return Err(format!("refinement.experiment.controller.parameters.{name}: invalid PID value"));}
            }
        }
        Policy::Rhai{source,parameters,duty_limit} => {
            if source.len()>32_768 {return Err("refinement.experiment.controller.source: maximum 32 KiB for responsive static validation".into());}
            if source.trim().is_empty() {return Err("refinement.experiment.controller.source: source required".into());}
            if !parameters.is_object() {return Err("refinement.experiment.controller.parameters: object required".into());}
            if !duty_limit.is_finite() || *duty_limit<=0. || *duty_limit>1. {return Err("refinement.experiment.controller.duty_limit: expected (0,1]".into());}
            let params=sim_script::parameter_map(parameters).map_err(|s|format!("refinement.experiment.controller.parameters: {s}"))?;
            sim_script::RhaiController::validate_source(sim_script::Sources::single("controller.rhai",source),params,e.seed,&crate::registry()).map_err(|s|format!("refinement.experiment.controller.source: {s}"))?;
        }
    }
    for (name,v) in [("rms_rad",e.limits.rms_rad),("peak_rad",e.limits.peak_rad),("settled_rad",e.limits.settled_rad),("maximum_saturation_fraction",e.limits.maximum_saturation_fraction)] {
        if !v.is_finite() || v<0. || (name=="maximum_saturation_fraction" && v>1.) {return Err(format!("refinement.experiment.limits.{name}: invalid tracking limit"));}
    }
    e.validate().map_err(|s|format!("refinement.experiment: {s}"))?;
    Ok(())
}
pub fn coordinates(f: &Family, coords: &[Coordinate]) -> Result<(),String> {
    for (i,c) in coords.iter().enumerate() {
        if c.device.is_some_and(|d|d==0 || d>253) {return Err(format!("refinement.coordinates.{i}.device: expected 1–253"));}
        if !c.lower.is_finite() {return Err(format!("refinement.coordinates.{i}.lower: finite bound required"));}
        if !c.upper.is_finite() || c.upper<=c.lower {return Err(format!("refinement.coordinates.{i}.upper: finite upper bound greater than lower required"));}
        calibration::validate_coordinates(f,std::slice::from_ref(c)).map_err(|e|format!("refinement.coordinates.{i}.{}: {e}",c.path))?;
    }
    calibration::validate_coordinates(f,coords).map(|_|()).map_err(|e|format!("refinement.coordinates: {e}"))
}
pub fn scenarios(e: &Experiment, variants: &[Variant]) -> Result<(),String> {
    if variants.is_empty() || variants.len()>32 {return Err("refinement.scenarios: expected 1–32 scenarios".into());}
    let mut labels=std::collections::BTreeSet::new();
    for (i,v) in variants.iter().enumerate() {
        if v.label.trim().is_empty() || !labels.insert(&v.label) {return Err(format!("refinement.scenarios.{i}.label: unique nonempty label required"));}
        if v.evidence.trim().is_empty() {return Err(format!("refinement.scenarios.{i}.evidence: hypothesis or measured provenance required"));}
        v.model.validate().map_err(|s|format!("refinement.scenarios.{i}.model: {s}"))?;
        let mut spec=e.clone();spec.timing=v.timing.clone();experiment(&spec).map_err(|s|format!("refinement.scenarios.{i}: {s}"))?;
    }
    Ok(())
}
