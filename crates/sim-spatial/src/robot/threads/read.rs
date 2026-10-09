//! The CAD source's threads read for Robot mode, in process: the `.rcad`
//! the export names (`cad_link`), read on one `Pool::Io` job per (export,
//! CAD source, epoch) with the shared `sim_cad::annotations::list` (the same
//! listing CAD mode shows), and its node parents for the ancestor walk. The
//! file is the only copy while Robot mode is shown: CAD mode cannot be left
//! with unsaved edits and drops its document when left. A probe (every
//! [`PROBE`] while the section is shown or a REST caller asked) re-reads
//! only when the file's size or modification time moved, so a change saved
//! from CAD mode or written by another tool appears without a refresh. A job for an older key is dropped, which cancels it. The
//! epoch moves after every landed change and on Refresh.
use super::{Base, Parents, RobotThreads, base_of, land, shown};
use crate::jobs::{Ctx, Job, Pool};
use crate::robot::RobotView;
use bevy::prelude::*;
use serde_json::Value;
use crate::cad::types::CadThread;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

/// How often a shown section checks whether the CAD file changed.
pub(crate) const PROBE: Duration = Duration::from_secs(2);

/// (subject, epoch) a read was made for.
pub(crate) type Key = (Base, u64);

/// The file's size and modification time (the probe's comparison).
pub(crate) type Stat = (u64, Option<SystemTime>);

/// The threads as read.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Listed {
    pub base: Base,
    /// The archive's manifest revision when read.
    pub revision: u64,
    /// The archive's document identity (manifest `document_id`).
    pub document_id: Option<String>,
    /// The bytes' identity (`ArchiveDocument::identity`) a change checks before writing.
    pub identity: String,
    /// The file's stat when read.
    pub stat: Stat,
    pub threads: Vec<CadThread>,
    pub parents: Parents,
}

/// What the last answer for a subject said about the CAD source.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Reach {
    /// The file was read (changes write it).
    Open,
    /// It could not be read.
    Failed(String),
}

/// A read job's answer.
pub(crate) enum Answer {
    /// The file's stat is the one already read.
    Unchanged,
    Read(Listed),
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
    /// The threads are read.
    pub(crate) fn open(&self, base: &Base) -> bool {
        matches!(self.reach(base), Some(Reach::Open)) && self.listed_for(base).is_some()
    }
    /// The read at the current key has answered.
    pub(crate) fn current(&self, base: &Base) -> bool {
        self.answered.as_ref().is_some_and(|((b, e), _)| b == base && *e == self.epoch)
    }
}

/// The threads of an open archive (as CAD mode lists them) and its node parents.
pub(crate) fn list_archive(archive: &sim_cad::ArchiveDocument, stamps: &sim_cad::annotations::Stamps) -> Result<(Vec<CadThread>, Parents), String> {
    let threads = sim_cad::annotations::list(archive, stamps, None, None, None)?
        .into_iter()
        .map(|t| serde_json::from_value(t).map_err(|e| format!("comment thread: {e}")))
        .collect::<Result<Vec<CadThread>, String>>()?;
    let parents: Parents = archive.manifest["nodes"].as_array().into_iter().flatten().filter_map(|n| Some((n["id"].as_str()?.to_string(), n["parent"].as_str().map(str::to_string)))).collect();
    Ok((threads, parents))
}

/// What [`tick`] has to do (found through shared borrows: a frame that
/// only waits marks nothing changed).
pub(crate) enum Work {
    /// The job's key is superseded: dropping it cancels it.
    Drop,
    Land(Key, Result<Answer, String>),
    /// A read (`known`: the stat already read, for a probe).
    Start { known: Option<Stat> },
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
    let due = !r.current(base) || r.checked.is_none_or(|t| now.saturating_duration_since(t) >= PROBE);
    if !due {
        return None;
    }
    let known = if r.current(base) { r.listed_for(base).map(|l| l.stat) } else { None };
    Some(Work::Start { known })
}

/// Apply one step. Returns whether anything shown changed: a probe that
/// finds the same file changes nothing shown, so the section is not
/// rebuilt every [`PROBE`].
pub(crate) fn tick(r: &mut ReadState, base: Option<&Base>, work: Work, now: Instant) -> bool {
    match work {
        Work::Drop => {
            r.job = None;
            r.listed.is_none()
        }
        Work::Land(key, result) => {
            r.job = None;
            r.checked = Some(now);
            let mut changed = r.listed.is_none();
            let reach = match result {
                Ok(Answer::Unchanged) => Reach::Open,
                Ok(Answer::Read(listed)) => {
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
            let b = base.clone();
            let job = Job::spawn(Pool::Io, r.epoch, "robot-cad-threads", move |ctx| read(ctx, &b, known));
            r.job = Some((key, job));
            r.listed.is_none()
        }
    }
}

/// The file's stat (the probe's comparison).
pub(crate) fn stat(path: &Path) -> Result<Stat, String> {
    let m = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((m.len(), m.modified().ok()))
}

/// The read job (see the module doc).
fn read(ctx: &Ctx, base: &Base, known: Option<Stat>) -> Result<Answer, String> {
    let now = stat(&base.cad)?;
    if known.is_some_and(|k| k == now) {
        return Ok(Answer::Unchanged);
    }
    let archive = sim_cad::ArchiveDocument::open_with(&base.cad, &|| ctx.cancelled(), &|_| {})?;
    if ctx.cancelled() {
        return Err("superseded by a newer read".to_string());
    }
    let stamps = sim_cad::annotations::pinned_stamps(&archive);
    let (threads, parents) = list_archive(&archive, &stamps)?;
    Ok(Answer::Read(Listed {
        base: base.clone(),
        revision: archive.manifest["revision"].as_u64().unwrap_or(0),
        document_id: archive.manifest["document_id"].as_str().map(str::to_string),
        identity: archive.identity().to_string(),
        stat: now,
        threads,
        parents,
    }))
}

/// JobResults: changes answered land (the threads are read again), then
/// the read steps. Frames keep coming while either runs.
pub(super) fn results(view: Res<RobotView>, mut st: ResMut<RobotThreads>, redraw: Option<MessageWriter<bevy::window::RequestRedraw>>) {
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
    let base = base_of(&view);
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
