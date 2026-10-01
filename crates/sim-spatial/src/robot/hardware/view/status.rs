//! REST `hardware_status`: the panel's whole state as JSON ([`status_json`]).
use crate::robot::hardware::Hardware;
use crate::robot::hardware::panel::panel_view;
use serde_json::{Value, json};
use sim_runtime::hardware_client::calibration::Status;
use std::collections::BTreeMap;
use std::time::Instant;

/// REST `hardware_status`: the link (connected, url, generation, stale and
/// the age of the last read), the page's session state, the form, every
/// rendered text and flag, the mirror and live-sync state, and the
/// server's last status.
pub fn status_json(hw: &Hardware, now: Instant) -> Value {
    let s = &hw.snapshot;
    let view = panel_view(hw, now);
    let connected = hw.link.is_some();
    let age_ms = s.read_at.map(|t| now.duration_since(t).as_millis() as u64);
    let gait = s.gait.as_ref().map(|g| json!({"mode": g.mode, "period_s": g.period_s, "t": g.t, "playing": g.playing, "scale": g.scale, "leg": g.leg, "skipped": g.skipped, "started": g.started}));
    let f = &hw.form;
    json!({
        "open": hw.open,
        "connected": connected,
        "connecting": hw.connecting.is_some(),
        "url": hw.url(),
        "generation": hw.link.as_ref().map(|l| l.generation),
        "stale": connected && s.stale(now),
        "age_ms": if connected { age_ms } else { None },
        "notice": hw.notice,
        "export": {"running": hw.export.is_some(), "last": hw.export_line},
        "session": {
            "id": s.id, "ready": s.ready, "busy": s.busy, "run": s.run, "starting": s.starting,
            "intent": s.intent.motion(), "target_raw": s.target_raw, "sweeping": s.sweeping, "learning": s.learning,
            "tuning": s.tuning, "campaigning": s.campaigning, "sweep_all": s.sweep_all, "sequence": s.sequence_text,
            "warnings": s.warnings.iter().map(|(at, text)| json!({"at": at, "text": text})).collect::<Vec<_>>(),
            "gait": gait, "gait_notice": s.gait_notice, "gaits_loaded": s.gaits_loaded,
        },
        "form": {
            "speed_percent": f.inputs.speed_percent, "pwm_percent": f.inputs.pwm_percent, "hold_others": f.inputs.hold_others,
            "drive_mode": f.inputs.drive_mode, "gait_speed_percent": f.inputs.gait_speed_percent, "gait_effort_percent": f.inputs.gait_effort_percent,
            "target_percent": f.target_percent, "tune_ok": f.tune_ok, "campaign_ok": f.campaign_ok, "gait_ok": f.gait_ok,
            "gait_index": f.gait_index, "gait_mode": f.gait_mode, "step": f.step, "open_sections": f.open,
        },
        "mirror": hw.mirror.state_json(),
        "sync": hw.sync.state_json(),
        "panel": serde_json::to_value(&view).unwrap_or(Value::Null),
        "server": if connected { server_json(&s.state) } else { Value::Null },
    })
}

/// The server's status as JSON (the calibration types are Deserialize only).
fn server_json(st: &Status) -> Value {
    let axes: BTreeMap<String, Value> = st
        .calibration
        .iter()
        .flat_map(|c| c.axes.iter())
        .map(|(id, a)| {
            (
                id.to_string(),
                json!({"role": a.role, "lower": a.lower, "upper": a.upper, "reference": a.reference, "reverse": a.reverse, "disabled": a.disabled,
                       "coordinate_session": a.coordinate_session, "reference_joint_rad": a.reference_joint_rad,
                       "tuning": a.tuning.as_ref().map(|t| json!({"kp": t.pid.kp, "ki": t.pid.ki, "kd": t.pid.kd, "friction_duty": t.friction_duty, "record": t.record}))}),
            )
        })
        .collect();
    let samples: BTreeMap<String, Value> = st.samples.iter().map(|(id, t)| (id.to_string(), json!({"position_raw": t.position_raw, "position_continuous": t.position_continuous, "voltage_v": t.voltage_v, "temperature_c": t.temperature_c}))).collect();
    let sweep = st.sweep.as_ref().map(|sw| {
        json!({"running": sw.running, "run_id": sw.run_id, "motor_id": sw.motor_id, "motor_ids": sw.motor_ids, "skipped": sw.skipped, "all": sw.all,
               "motion_error": sw.motion_error, "teaching": sw.teaching, "samples": sw.samples.len(),
               "latest": sw.latest.as_ref().map(|l| json!({"elapsed_ms": l.elapsed_ms, "position_raw": l.position_raw, "position_continuous": l.position_continuous,
                   "target_raw": l.target_raw, "velocity_counts_s": l.velocity_counts_s, "pwm": l.pwm, "holding": l.holding, "toward_upper": l.toward_upper, "warnings": l.warnings}))})
    });
    json!({
        "connected": st.connected, "enabled_id": st.enabled_id, "busy": st.busy, "message": st.message, "error": st.error,
        "output": st.output, "coordinate_session": st.coordinate_session, "maximum_speed_counts_s": st.maximum_speed_counts_s,
        "capture_message": st.capture_message, "samples": samples, "axes": axes, "sweep": sweep,
        "tuning": st.tuning.as_ref().map(|t| json!({"running": t.running, "motor_id": t.motor_id, "stage": t.stage, "error": t.error})),
        "campaign": st.campaign.as_ref().map(|c| json!({"running": c.running, "stage": c.stage, "completed": c.completed, "error": c.error,
            "result": c.result.as_ref().map(|r| json!({"headline": r.headline, "directory": r.directory}))})),
        "gait": st.gait.as_ref().map(|g| json!({"running": g.running, "t": g.t, "speed_scale": g.speed_scale, "phase": g.phase, "clamped": g.clamped, "error": g.error})),
        "gait_runs": st.gait_runs.len(),
    })
}
