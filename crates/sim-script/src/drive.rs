//! Rhai drive functions: a robot's kinematic adapter written in Rhai mixes a
//! body twist into wheel joint rates, and applies the controller-side
//! deadman, only through these functions, which call
//! `sim_domain_control::drive` and nothing else. No mixing arithmetic lives
//! here or in the script, so the Rhai adapter, simloop's Python controller
//! (checked against the same golden vectors) and the Rust tests share one
//! implementation.
//!
//! - `drive_differential_mix(track_width_m, wheel_radius_m, signs[2], twist[3]) -> [left, right]` (rad/s)
//! - `drive_differential_unmix(track_width_m, wheel_radius_m, signs[2], rates[2]) -> [forward, lateral, yaw]`
//! - `drive_mecanum_mix(track_width_m, wheelbase_m, wheel_radius_m, signs[4], twist[3]) -> [fl, fr, rl, rr]`
//! - `drive_mecanum_unmix(track_width_m, wheelbase_m, wheel_radius_m, signs[4], rates[4]) -> [forward, lateral, yaw]`
//! - `drive_update(state, t, request[3], heartbeat, dt, drive) -> #{twist, expired, state}`:
//!   `kinematics::HeartbeatDeadman::update` with the limits and deadman of
//!   `drive` (a `sim.drive.resolved/1` map, e.g. `parameters().drive`).
//!   `state` is `#{}` (or `()`) on the first call, then the returned `state`.
//!
//! Numbers may be INT or FLOAT. Every refusal is a Rhai error naming the
//! function and the argument (`drive_differential_mix: signs must be 2
//! numbers (+1 or -1), got 3`).
use crate::{ScriptResult, error};
use rhai::{Array, Dynamic, Engine, Map};
use sim_domain_control::drive::kinematics::{BodyTwist, DifferentialDrive, HeartbeatDeadman, KinematicsError, Mecanum};
use sim_domain_control::drive::profile::ResolvedDrive;

const DIFFERENTIAL_MIX: &str = "drive_differential_mix";
const DIFFERENTIAL_UNMIX: &str = "drive_differential_unmix";
const MECANUM_MIX: &str = "drive_mecanum_mix";
const MECANUM_UNMIX: &str = "drive_mecanum_unmix";
const UPDATE: &str = "drive_update";

/// A Rhai number (INT or FLOAT) as f64, refused naming `func` and `what`.
fn number(func: &str, what: &str, value: &Dynamic) -> ScriptResult<f64> {
    if let Ok(i) = value.as_int() {
        return Ok(i as f64);
    }
    value.as_float().map_err(|found| error(format!("{func}: {what} must be a number, got {found}")))
}

/// A Rhai array of exactly `N` numbers. `hint` describes the expected values
/// (e.g. ` (+1 or -1)`).
fn numbers<const N: usize>(func: &str, what: &str, hint: &str, value: &Dynamic) -> ScriptResult<[f64; N]> {
    let array: Array = value.clone().into_array().map_err(|found| error(format!("{func}: {what} must be {N} numbers{hint}, got {found}")))?;
    if array.len() != N {
        return Err(error(format!("{func}: {what} must be {N} numbers{hint}, got {}", array.len())));
    }
    let mut out = [0.0; N];
    for (i, v) in array.iter().enumerate() {
        out[i] = number(func, &format!("{what}[{i}]"), v)?;
    }
    Ok(out)
}

fn twist_arg(func: &str, value: &Dynamic) -> ScriptResult<BodyTwist> {
    Ok(BodyTwist::from_array(numbers::<3>(func, "twist", " [forward m/s, lateral m/s, yaw rad/s]", value)?))
}

fn floats(values: &[f64]) -> Array {
    values.iter().map(|v| Dynamic::from_float(*v)).collect()
}

/// A kinematics refusal from the mixer's constructor (its message names the parameter).
fn geometry_error(func: &str) -> impl Fn(KinematicsError) -> Box<rhai::EvalAltResult> + '_ {
    move |e| error(format!("{func}: {e}"))
}

fn differential(func: &str, track_width_m: &Dynamic, wheel_radius_m: &Dynamic, signs: &Dynamic) -> ScriptResult<DifferentialDrive> {
    let track = number(func, "track_width_m", track_width_m)?;
    let radius = number(func, "wheel_radius_m", wheel_radius_m)?;
    let signs = numbers::<2>(func, "signs", " (+1 or -1)", signs)?;
    DifferentialDrive::new(track, radius, signs).map_err(geometry_error(func))
}

fn mecanum(func: &str, track_width_m: &Dynamic, wheelbase_m: &Dynamic, wheel_radius_m: &Dynamic, signs: &Dynamic) -> ScriptResult<Mecanum> {
    let track = number(func, "track_width_m", track_width_m)?;
    let wheelbase = number(func, "wheelbase_m", wheelbase_m)?;
    let radius = number(func, "wheel_radius_m", wheel_radius_m)?;
    let signs = numbers::<4>(func, "signs", " (+1 or -1)", signs)?;
    Mecanum::new(track, wheelbase, radius, signs).map_err(geometry_error(func))
}

/// The plain-number state map of a [`HeartbeatDeadman`]: `#{}` or `()` is a
/// fresh one; otherwise `twist`, `heartbeat` (`()` before the first
/// update), `changed_t` and `expired`, each named when wrong.
fn deadman_state(value: &Dynamic) -> ScriptResult<HeartbeatDeadman> {
    if value.is_unit() {
        return Ok(HeartbeatDeadman::default());
    }
    let map: Map = value.clone().try_cast::<Map>().ok_or_else(|| error(format!("{UPDATE}: state must be a map (#{{}} on the first call), got {}", value.type_name())))?;
    if map.is_empty() {
        return Ok(HeartbeatDeadman::default());
    }
    for key in map.keys() {
        if !["twist", "heartbeat", "changed_t", "expired"].contains(&key.as_str()) {
            return Err(error(format!("{UPDATE}: state.{key}: unknown field; the state is the previous call's `state` (twist, heartbeat, changed_t, expired)")));
        }
    }
    let field = |name: &str| map.get(name).cloned().ok_or_else(|| error(format!("{UPDATE}: state.{name}: missing; pass the previous call's `state`, or #{{}} on the first call")));
    let twist = BodyTwist::from_array(numbers::<3>(UPDATE, "state.twist", "", &field("twist")?)?);
    let heartbeat = field("heartbeat")?;
    let heartbeat = if heartbeat.is_unit() { None } else { Some(number(UPDATE, "state.heartbeat", &heartbeat)?) };
    let changed_t = number(UPDATE, "state.changed_t", &field("changed_t")?)?;
    let expired = field("expired")?;
    let expired = expired.as_bool().map_err(|found| error(format!("{UPDATE}: state.expired must be a boolean, got {found}")))?;
    Ok(HeartbeatDeadman { twist, heartbeat, changed_t, expired })
}

fn deadman_map(d: &HeartbeatDeadman) -> Map {
    let mut map = Map::new();
    map.insert("twist".into(), Dynamic::from_array(floats(&d.twist.to_array())));
    map.insert("heartbeat".into(), d.heartbeat.map_or(Dynamic::UNIT, Dynamic::from_float));
    map.insert("changed_t".into(), Dynamic::from_float(d.changed_t));
    map.insert("expired".into(), Dynamic::from_bool(d.expired));
    map
}

/// The scene's resolved drive (`sim.drive.resolved/1`) from a Rhai map,
/// through JSON so integer literals read as f64, then validated.
fn resolved_drive(value: &Dynamic) -> ScriptResult<ResolvedDrive> {
    if !value.is_map() {
        return Err(error(format!("{UPDATE}: drive must be the resolved drive map (parameters().drive), got {}", value.type_name())));
    }
    let json: serde_json::Value = rhai::serde::from_dynamic(value).map_err(|e| error(format!("{UPDATE}: drive: {e}")))?;
    let drive: ResolvedDrive = serde_json::from_value(json).map_err(|e| error(format!("{UPDATE}: drive: {e}")))?;
    drive.validate().map_err(|e| error(format!("{UPDATE}: drive: {e}")))?;
    Ok(drive)
}

/// `drive_update`: one controller-side deadman sample.
pub fn update(state: &Dynamic, t: &Dynamic, request: &Dynamic, heartbeat: &Dynamic, dt: &Dynamic, drive: &Dynamic) -> ScriptResult<Map> {
    let mut deadman = deadman_state(state)?;
    let t = number(UPDATE, "t", t)?;
    let request = BodyTwist::from_array(numbers::<3>(UPDATE, "request", " [forward m/s, lateral m/s, yaw rad/s]", request)?);
    let heartbeat = number(UPDATE, "heartbeat", heartbeat)?;
    let dt = number(UPDATE, "dt", dt)?;
    let drive = resolved_drive(drive)?;
    let out = deadman
        .update(t, request, heartbeat, dt, &drive.limits(), &drive.deadman())
        .map_err(|e| error(format!("{UPDATE}: request: {e}")))?;
    let mut result = Map::new();
    result.insert("twist".into(), Dynamic::from_array(floats(&out.twist.to_array())));
    result.insert("expired".into(), Dynamic::from_bool(out.expired));
    result.insert("state".into(), Dynamic::from_map(deadman_map(&deadman)));
    Ok(result)
}

/// Register the drive functions on `engine` (every controller and authoring engine).
pub(crate) fn register(engine: &mut Engine) {
    engine.register_fn(DIFFERENTIAL_MIX, |track_width_m: Dynamic, wheel_radius_m: Dynamic, signs: Dynamic, twist: Dynamic| -> ScriptResult<Array> {
        let drive = differential(DIFFERENTIAL_MIX, &track_width_m, &wheel_radius_m, &signs)?;
        let rates = drive.mix(twist_arg(DIFFERENTIAL_MIX, &twist)?).map_err(|e| error(format!("{DIFFERENTIAL_MIX}: twist: {e}")))?;
        Ok(floats(&rates))
    });
    engine.register_fn(DIFFERENTIAL_UNMIX, |track_width_m: Dynamic, wheel_radius_m: Dynamic, signs: Dynamic, rates: Dynamic| -> ScriptResult<Array> {
        let drive = differential(DIFFERENTIAL_UNMIX, &track_width_m, &wheel_radius_m, &signs)?;
        let rates = numbers::<2>(DIFFERENTIAL_UNMIX, "rates", " [left, right] rad/s", &rates)?;
        Ok(floats(&drive.unmix(rates).to_array()))
    });
    engine.register_fn(MECANUM_MIX, |track_width_m: Dynamic, wheelbase_m: Dynamic, wheel_radius_m: Dynamic, signs: Dynamic, twist: Dynamic| -> ScriptResult<Array> {
        let drive = mecanum(MECANUM_MIX, &track_width_m, &wheelbase_m, &wheel_radius_m, &signs)?;
        let rates = drive.mix(twist_arg(MECANUM_MIX, &twist)?).map_err(|e| error(format!("{MECANUM_MIX}: twist: {e}")))?;
        Ok(floats(&rates))
    });
    engine.register_fn(MECANUM_UNMIX, |track_width_m: Dynamic, wheelbase_m: Dynamic, wheel_radius_m: Dynamic, signs: Dynamic, rates: Dynamic| -> ScriptResult<Array> {
        let drive = mecanum(MECANUM_UNMIX, &track_width_m, &wheelbase_m, &wheel_radius_m, &signs)?;
        let rates = numbers::<4>(MECANUM_UNMIX, "rates", " [front_left, front_right, rear_left, rear_right] rad/s", &rates)?;
        Ok(floats(&drive.unmix(rates).to_array()))
    });
    engine.register_fn(UPDATE, |state: Dynamic, t: Dynamic, request: Dynamic, heartbeat: Dynamic, dt: Dynamic, drive: Dynamic| -> ScriptResult<Map> {
        update(&state, &t, &request, &heartbeat, &dt, &drive)
    });
}

#[cfg(test)]
mod tests;
