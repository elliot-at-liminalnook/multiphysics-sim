//! RoboCAD's threads read for Robot mode, on one `Pool::Dedicated` job per
//! (service, export, CAD source, epoch) (network requests, the jobs
//! module's pool rule). The job asks `GET /` first: a service whose
//! document is not the CAD source answers `Elsewhere` and nothing else is
//! read; a probe (every [`PROBE`] while the section is shown or a REST
//! caller asked) answers `Unchanged` while RoboCAD's revision and document
//! id have not
//! moved; otherwise `GET /threads` and `GET /nodes` (the parents for the
//! ancestor walk). A job for an older key is dropped, which cancels it. The
//! epoch moves after every landed change and on Refresh.
use super::{Base, Parents, RobotThreads, base_of, land, service_has, shown};
use crate::document::DocumentRegistry;
use crate::jobs::{Ctx, Job, Pool};
use crate::robot::RobotView;
use bevy::prelude::*;
use serde_json::Value;
use sim_runtime::cad_client::{CadClient, CadThread};
use std::path::Path;
use std::time::{Duration, Instant};

/// How often a shown section asks RoboCAD whether its document moved.
pub(crate) const PROBE: Duration = Duration::from_secs(2);

/// (subject, epoch) a read was made for.
pub(crate) type Key = (Base, u64);

/// The threads as read.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Listed {
    pub base: Base,
    /// RoboCAD's revision (`GET /`) just before the threads were read.
    pub revision: u64,
    /// RoboCAD's document identity (`GET /` `document_id`) at that revision:
    /// another document at the same path and revision is not this one.
    pub document_id: Option<String>,
    pub threads: Vec<CadThread>,
    pub parents: Parents,
}

/// What the last answer for a subject said about the service.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Reach {
    /// It has the CAD source open (its threads are read).
    Open,
    /// It has another document open (its path; None: a new document).
    Elsewhere(Option<String>),
    /// It could not be read.
    Failed(String),
}

/// A read job's answer.
pub(crate) enum Answer {
    Elsewhere(Option<String>),
    /// The probe's revision and document id are the ones already read.
    Unchanged,
    Read { revision: u64, document_id: Option<String>, threads: Vec<CadThread>, parents: Parents },
}

/// The read state (on `RobotThreads`).
#[derive(Default)]
pub(crate) struct ReadState {
    pub(super) listed: Option<Listed>,
    /// The last answer's key and what it said.
    pub(super) answered: Option<(Key, Reach)>,
    job: Option<(Key, Job<Answer>)>,
    pub(super) epoch: u64,
    /// When the last answer landed (the probe's clock).
    checked: Option<Instant>,
}
impl ReadState {
    /// Read again at the next frame.
    pub(crate) fn again(&mut self) {
        self.epoch += 1;
    }
    /// A read is in flight.
    pub(crate) fn reading(&self) -> bool {
        self.job.is_some()
    }
    /// The threads read for `base` (any epoch).
    pub(crate) fn listed_for(&self, base: &Base) -> Option<&Listed> {
        self.listed.as_ref().filter(|l| l.base == *base)
    }
    /// What the last answer for `base` said.
    pub(crate) fn reach(&self, base: &Base) -> Option<&Reach> {
        self.answered.as_ref().filter(|((b, _), _)| b == base).map(|(_, r)| r)
    }
    /// The service has the CAD source open and its threads are read.
    pub(crate) fn open(&self, base: &Base) -> bool {
        matches!(self.reach(base), Some(Reach::Open)) && self.listed_for(base).is_some()
    }
    /// The read at the current key has answered.
    pub(crate) fn current(&self, base: &Base) -> bool {
        self.answered.as_ref().is_some_and(|((b, e), _)| b == base && *e == self.epoch)
    }
}

/// What [`tick`] has to do (found through shared borrows: a frame that
/// only waits marks nothing changed).
pub(crate) enum Work {
    /// The job's key is superseded: dropping it cancels it.
    Drop,
    Land(Key, Result<Answer, String>),
    /// A read (`known`: the revision and document id already read, for a probe).
    Start { known: Option<(u64, Option<String>)> },
}

/// The read's next step for `base` now.
pub(crate) fn needs(r: &ReadState, base: Option<&Base>, wanted: bool, now: Instant) -> Option<Work> {
    match &r.job {
        Some((k, _)) if base.is_none_or(|b| k.0 != *b || k.1 != r.epoch) => return Some(Work::Drop),
        Some((k, job)) => return job.poll().map(|result| Work::Land(k.clone(), result)),
        None => {}
    }
    let base = base?;
    if !wanted {
        return None;
    }
    if !r.current(base) {
        return Some(Work::Start { known: None });
    }
    let due = r.checked.is_none_or(|t| now.saturating_duration_since(t) >= PROBE);
    due.then(|| Work::Start { known: r.listed_for(base).map(|l| (l.revision, l.document_id.clone())) })
}

/// Apply one step. Returns whether anything shown changed: a probe that
/// finds the same revision changes nothing shown, so the section is not
/// rebuilt every [`PROBE`].
pub(crate) fn tick(r: &mut ReadState, base: Option<&Base>, work: Work, now: Instant) -> bool {
    match work {
        Work::Drop => {
            r.job = None;
            // "Reading…" is shown only before anything was read.
            r.listed.is_none()
        }
        Work::Land(key, result) => {
            r.job = None;
            r.checked = Some(now);
            let mut changed = r.listed.is_none();
            let reach = match result {
                Ok(Answer::Elsewhere(path)) => Reach::Elsewhere(path),
                Ok(Answer::Unchanged) => Reach::Open,
                Ok(Answer::Read { revision, document_id, threads, parents }) => {
                    let listed = Listed { base: key.0.clone(), revision, document_id, threads, parents };
                    changed |= r.listed.as_ref() != Some(&listed);
                    r.listed = Some(listed);
                    Reach::Open
                }
                Err(e) => Reach::Failed(e),
            };
            changed |= r.reach(&key.0) != Some(&reach);
            r.answered = Some((key, reach));
            changed
        }
        Work::Start { known } => {
            let Some(base) = base else { return false };
            let key = (base.clone(), r.epoch);
            let client = match CadClient::new(&base.url) {
                Ok(c) => c,
                Err(e) => {
                    r.checked = Some(now);
                    r.answered = Some((key, Reach::Failed(e.message)));
                    return true;
                }
            };
            let cad = base.cad.clone();
            let job = Job::spawn(Pool::Dedicated, r.epoch, "robot-cad-threads", move |ctx| read(ctx, &client, &cad, known));
            r.job = Some((key, job));
            r.listed.is_none()
        }
    }
}

/// The read job (see the module doc). RoboCAD's own words on failure.
fn read(ctx: &Ctx, client: &CadClient, cad: &Path, known: Option<(u64, Option<String>)>) -> Result<Answer, String> {
    let health = client.health().map_err(|e| e.message)?;
    if !service_has(health.path.as_deref(), cad) {
        return Ok(Answer::Elsewhere(health.path));
    }
    // The same revision of the same document (a reopened file restarts its revisions).
    if known.as_ref().is_some_and(|(revision, id)| *revision == health.revision && *id == health.document_id) {
        return Ok(Answer::Unchanged);
    }
    if ctx.cancelled() {
        return Err("superseded by a newer read".to_string());
    }
    let threads = client.threads(None, None, None).map_err(|e| e.message)?;
    let nodes = client.nodes(None).map_err(|e| e.message)?;
    let parents: Parents = nodes.into_iter().map(|n| (n.id, n.parent)).collect();
    Ok(Answer::Read { revision: health.revision, document_id: health.document_id, threads, parents })
}

/// JobResults: changes answered land (the threads are read again), then
/// the read steps. Frames keep coming while either runs.
pub(super) fn results(view: Res<RobotView>, registry: Res<DocumentRegistry>, mut st: ResMut<RobotThreads>, redraw: Option<MessageWriter<bevy::window::RequestRedraw>>) {
    let now = Instant::now();
    // Polled through shared borrows: a `ResMut` deref would mark the state changed every frame.
    let finished: Vec<(u64, Result<Value, String>)> = st.commits.iter().filter_map(|(seq, job)| job.poll().map(|r| (*seq, r))).collect();
    if !finished.is_empty() {
        let st = &mut *st;
        for (seq, result) in finished {
            st.commits.retain(|(s, _)| *s != seq);
            land(st, seq, result);
        }
    }
    let base = base_of(&view, &registry);
    let wanted = shown(&view) || st.asked;
    if let Some(work) = needs(&st.read, base.as_ref(), wanted, now) {
        // The probe's bookkeeping marks nothing; a change shown does.
        if tick(&mut st.bypass_change_detection().read, base.as_ref(), work, now) {
            st.set_changed();
        }
    }
    if st.read.reading() || !st.commits.is_empty() {
        if let Some(mut redraw) = redraw {
            redraw.write(bevy::window::RequestRedraw);
        }
    }
}
