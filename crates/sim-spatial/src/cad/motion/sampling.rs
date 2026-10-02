//! Caller-owned continuation, one preview request in flight and one latest
//! queued intent. Only a published receipt can seed the next request. The
//! reference PoseModel performs its own intermediate steps and branch solve.
use super::*;
pub(crate) struct Intent {
    pub stamp: Stamp,
    pub sequence: u64,
    pub serial: u64,
    pub request: PoseRequest,
}
pub(crate) struct Pending {
    pub intent: Intent,
    pub job: Job<PoseSample>,
}
pub(crate) fn current(d: &CadDocument, s: &MotionState, intent: &Intent) -> bool {
    intent.stamp.matches(d)
        && intent.stamp.revision == d.shown_revision()
        && intent.sequence == s.sequence
        && intent.serial == s.sample_sequence
        && intent.request.time == s.cursor
        && !s.cancel_requested
        && guard(d, &intent.stamp).is_ok()
}
pub(crate) fn prior(
    sample: Option<&PoseSample>,
    stamp: &Stamp,
) -> Option<sim_runtime::cad_client::motion::PoseContinuation> {
    sample
        .filter(|p| {
            p.identity.document_id == stamp.document
                && p.identity.revision == Some(stamp.revision)
                && p.identity.source_kind == "live_kinematic"
                && Some(p.identity.source_id.as_str()) == stamp.document.as_deref()
        })
        .map(PoseSample::continuation)
}
pub(crate) fn matches(sample: &PoseSample, stamp: &Stamp, time: f64) -> bool {
    sample.identity.document_id == stamp.document
        && sample.identity.revision == Some(stamp.revision)
        && sample.identity.source_kind == "live_kinematic"
        && Some(sample.identity.source_id.as_str()) == stamp.document.as_deref()
        && sample.time == time
        && sample.time.is_finite()
}
pub(crate) fn request(
    d: &CadDocument,
    s: &mut MotionState,
    program: Option<Value>,
) -> Result<(), String> {
    let stamp = s.identity.clone().ok_or("Enter pose mode first")?;
    guard(d, &stamp)?;
    let request = PoseRequest {
        document_id: stamp.document.clone().ok_or("Document ID missing")?,
        expected_revision: stamp.revision,
        positions: s.positions.clone(),
        program,
        time: s.cursor,
        prior: None,
    };
    s.sample_sequence += 1;
    s.queued_sample = Some(Intent {
        stamp,
        sequence: s.sequence,
        serial: s.sample_sequence,
        request,
    });
    pump(d, s)
}
fn pump(d: &CadDocument, s: &mut MotionState) -> Result<(), String> {
    if s.sampling.is_some() {
        return Ok(());
    }
    let Some(mut intent) = s.queued_sample.take() else {
        return Ok(());
    };
    if intent.sequence != s.sequence
        || intent.serial != s.sample_sequence
        || intent.request.time != s.cursor
        || s.cancel_requested
        || guard(d, &intent.stamp).is_err()
    {
        return Ok(());
    }
    let client = d.client.clone().ok_or("Not connected")?;
    intent.request.prior = prior(s.sample.as_ref(), &intent.stamp);
    let sent = intent.request.clone();
    let job = Job::spawn(
        Pool::Dedicated,
        d.generation,
        "reference pose sample",
        move |_| client.sample_pose(&sent).map_err(|e| e.to_string()),
    );
    s.sampling = Some(Pending { intent, job });
    Ok(())
}
pub(crate) fn receive(d: &CadDocument, s: &mut MotionState) {
    if let Some(answer) = s.sampling.as_ref().and_then(|pending| pending.job.poll()) {
        let pending = s.sampling.take().unwrap();
        let intent = pending.intent;
        if current(d, s, &intent) {
            match answer {
                Ok(sample) => {
                    if matches(&sample, &intent.stamp, intent.request.time) {
                        s.sample = Some(sample);
                    } else {
                        s.error = Some(
                            "Reference sample identity/time mismatch; continuation unchanged"
                                .into(),
                        );
                        s.active = false;
                        s.playing = false;
                    }
                }
                Err(e) => {
                    s.error = Some(e);
                    s.active = false;
                    s.playing = false;
                }
            }
            s.touch();
        }
    }
    if let Err(e) = pump(d, s) {
        s.error = Some(e);
        s.active = false;
        s.playing = false;
        s.touch();
    }
}
