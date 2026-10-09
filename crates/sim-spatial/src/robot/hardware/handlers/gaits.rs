//! The gait list: REST `hardware_gaits`'s answer and the load it waits on.
use super::*;

/// The gait list as REST `hardware_gaits` answers it.
pub(in crate::robot::hardware) fn gaits_json(hw: &Hardware) -> Value {
    let gaits: Vec<Value> = hw
        .snapshot
        .gaits
        .iter()
        .enumerate()
        .map(|(i, g)| json!({"index": i, "label": crate::robot::hardware::view::gait_option_label(g), "path": g.path, "kind": g.kind, "study": g.study, "trial": g.trial, "speed_m_s": g.speed_m_s, "measured_actuators": g.measured_actuators, "summary": g.summary}))
        .collect();
    json!({"gaits": gaits, "selected": hw.form.gait_index})
}

/// The gait list: a click (opening Gait playback) asks the link; REST
/// `hardware_gaits` answers the loaded list, loading it first if needed.
pub(in crate::robot::hardware) fn load_gaits(hw: &mut Hardware, call: &mut Call) -> Answer {
    let Some(link) = hw.link.as_ref() else { return Answer::Done(Err(NOT_CONNECTED.into())) };
    let s = &hw.snapshot;
    if !call.rest() {
        link.send(LinkCommand::LoadGaits);
        return done();
    }
    if s.gaits_loaded {
        return Answer::Done(Ok(Some(gaits_json(hw))));
    }
    let Some(since) = call.continuation.get("since").and_then(Value::as_u64) else {
        link.send(LinkCommand::LoadGaits);
        *call.continuation = json!({"since": s.revision, "generation": s.generation});
        return Answer::Pending;
    };
    if call.continuation.get("generation").and_then(Value::as_u64) != Some(s.generation) && s.generation != 0 {
        return Answer::Done(Err("the calibration server's link was replaced while the gait list loaded".into()));
    }
    match &s.gait_notice {
        Some(notice) if s.revision > since && notice.starts_with("Gait list unavailable") => Answer::Done(Err(notice.clone())),
        _ if call.cancelled => Answer::Done(Err("hardware_gaits: cancelled".into())),
        _ => Answer::Pending,
    }
}
