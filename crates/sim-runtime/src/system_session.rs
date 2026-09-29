//! General systems execution and observation playback. No renderer or integrator.
//!
//! Hosts drive `tick` on a worker, independently of UI cadence. Reset invokes the
//! captured factory again; plant-only snapshots are deliberately not checkpoints.
use crate::physics_context::RuntimeIdentity;
use serde::{Deserialize, Serialize};
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld};
use sim_dynamics::Integrator;
use sim_inspect::{
    FrameGate, SampleFrame, SystemDescription,
    model::IdentityBindings,
    runtime::{FrameStamp, RuntimeInspection, Subscription},
};
use std::collections::BTreeSet;
use web_time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    pub interval: f64,
    /// Default requested from the factory. Recordings also retain each actual
    /// island's integrator and multirate step size.
    pub integrator: Integrator,
    pub seed: u64,
    /// Snap each island's clock onto its step grid so scheduled switching
    /// events land on step ends (`Runtime::set_grid_snapping`). Part of the
    /// recorded configuration so every host running a launch agrees.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub grid_snapping: bool,
}
impl SessionConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !self.interval.is_finite() || self.interval <= 0. {
            return Err("simulation interval must be finite and positive".into());
        }
        if let Integrator::ImplicitMidpoint(n) | Integrator::BackwardEuler(n) = self.integrator {
            if !n.absolute_tolerance.is_finite()
                || n.absolute_tolerance < 0.
                || !n.relative_tolerance.is_finite()
                || n.relative_tolerance < 0.
                || (n.absolute_tolerance == 0. && n.relative_tolerance == 0.)
                || n.max_iterations == 0
                || !n.min_line_search.is_finite()
                || n.min_line_search <= 0.
                || n.min_line_search > 1.
            {
                return Err("invalid Newton settings".into());
            }
        }
        Ok(())
    }
}
/// A factory must create new behavior/controller instances, using the supplied
/// solver configuration. The session seeds its newly constructed runtime.
pub struct PreparedSystem {
    pub runtime: Runtime,
    pub inspection: RuntimeInspection,
}
type Factory = Box<dyn FnMut(&SessionConfig) -> Result<PreparedSystem, String>>;

/// Captured Rust/Rhai ModelWorlds use the same general compiler path. CAD adapters
/// can supply a factory that retains their existing controller/coupler setup.
#[derive(Clone)]
pub struct ModelSource {
    pub model: ModelWorld,
    pub registry: BehaviorRegistry,
    pub identities: IdentityBindings,
    pub source_hash: String,
    pub revision: u64,
}
impl ModelSource {
    pub fn build(&self, config: &SessionConfig) -> Result<PreparedSystem, String> {
        config.validate()?;
        let runtime = Runtime::new(self.model.clone(), &self.registry, config.integrator)
            .map_err(|e| e.to_string())?;
        let inspection = RuntimeInspection::new(
            &runtime,
            &self.registry,
            &self.source_hash,
            self.revision,
            &self.identities,
        )
        .map_err(|e| e.to_string())?;
        Ok(PreparedSystem {
            runtime,
            inspection,
        })
    }
}

pub use sim_inspect::live::{Phase, SessionStatus};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
    pub version: u32,
    pub description: SystemDescription,
    pub runtime_identity: RuntimeIdentity,
    pub config: SessionConfig,
    pub islands: Vec<IslandSettings>,
    pub target_arch: String,
    pub target_os: String,
    pub observables: BTreeSet<String>,
    pub frames: Vec<SampleFrame>,
    pub completion: RecordingCompletion,
    pub end_status: SessionStatus,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IslandSettings {
    pub integrator: Integrator,
    pub step_size: f64,
    pub event_tolerance: f64,
    pub event_jacobian_reuse: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingCompletion {
    Active,
    Stopped,
    Reset,
    Cancelled,
    Failed,
    CapacityReached,
}
impl Recording {
    /// Playback validates captured data and never constructs or steps a runtime.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.frames.is_empty() || self.observables.is_empty() {
            return Err("unsupported or empty observation recording".into());
        }
        self.description.validate().map_err(|e| e.to_string())?;
        self.runtime_identity.validate()?;
        self.config.validate()?;
        for island in &self.islands {
            SessionConfig {
                interval: island.step_size,
                integrator: island.integrator,
                seed: self.config.seed,
                grid_snapping: self.config.grid_snapping,
            }
            .validate()?;
            if !island.event_tolerance.is_finite() || island.event_tolerance <= 0. {
                return Err("invalid event tolerance".into());
            }
        }
        if self.completion == RecordingCompletion::Active {
            return Err("recording has not been finalized".into());
        }
        let consistent_end = match self.completion {
            RecordingCompletion::Stopped => {
                matches!(self.end_status.phase, Phase::Paused | Phase::Running)
            }
            RecordingCompletion::Reset | RecordingCompletion::Cancelled => {
                self.end_status.phase == Phase::Cancelled
            }
            RecordingCompletion::Failed => self.end_status.phase == Phase::Failed,
            RecordingCompletion::CapacityReached => self.end_status.phase == Phase::RecordingFull,
            RecordingCompletion::Active => false,
        };
        if !consistent_end {
            return Err("recording completion differs from final status".into());
        }
        if !self.end_status.time.is_finite()
            || self.end_status.time < 0.
            || !self.end_status.step_wall_seconds.is_finite()
            || self.end_status.step_wall_seconds < 0.
            || self.target_arch.is_empty()
            || self.target_os.is_empty()
        {
            return Err("invalid recording status or target".into());
        }
        let mut gate = FrameGate::new(self.end_status.run_id.clone(), self.end_status.generation);
        for frame in &self.frames {
            if frame.values.keys().cloned().collect::<BTreeSet<_>>() != self.observables {
                return Err("recorded frame differs from requested observables".into());
            }
            gate.accept(&self.description, frame)
                .map_err(|e| e.to_string())?;
        }
        let last = self.frames.last().unwrap();
        if last.step != self.end_status.step
            || last.time != self.end_status.time
            || last.sequence > self.end_status.sequence
        {
            return Err("recording does not end at its last completed interval".into());
        }
        Ok(())
    }
    /// Exact stored frame at/before the cursor. No interpolation is implied.
    pub fn at_or_before(&self, time: f64) -> Option<&SampleFrame> {
        if !time.is_finite() {
            return None;
        }
        self.frames
            .partition_point(|frame| frame.time <= time)
            .checked_sub(1)
            .map(|index| &self.frames[index])
    }
}
struct ActiveRecording {
    subscription: Subscription,
    capacity: usize,
    data: Recording,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Describe,
    Subscribe {
        observables: Vec<String>,
    },
    Start,
    Pause,
    Step,
    Reset,
    Cancel,
    BeginRecording {
        observables: Vec<String>,
        capacity: usize,
    },
    TakeRecording,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub status: SessionStatus,
    pub description: Option<SystemDescription>,
    pub frame: Option<SampleFrame>,
    pub recording: Option<Recording>,
}

pub struct SystemSession {
    factory: Factory,
    config: SessionConfig,
    system: PreparedSystem,
    display: Subscription,
    selected: BTreeSet<String>,
    status: SessionStatus,
    last_frame: SampleFrame,
    recording: Option<ActiveRecording>,
}
impl SystemSession {
    pub fn new(
        run_id: String,
        config: SessionConfig,
        factory: impl FnMut(&SessionConfig) -> Result<PreparedSystem, String> + 'static,
    ) -> Result<Self, String> {
        config.validate()?;
        if run_id.trim().is_empty() {
            return Err("run identity must not be empty".into());
        }
        let mut factory: Factory = Box::new(factory);
        let mut system = factory(&config)?;
        Self::initialize(&mut system, &config)?;
        let display = system
            .inspection
            .subscribe(std::iter::empty())
            .map_err(|e| e.to_string())?;
        let status = SessionStatus {
            phase: Phase::Paused,
            run_id,
            generation: 0,
            step: 0,
            time: 0.,
            sequence: 0,
            step_wall_seconds: 0.,
            events: 0,
            message: None,
        };
        let last_frame = display
            .sample(&system.runtime, Self::stamp(&status))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            factory,
            config,
            system,
            display,
            selected: BTreeSet::new(),
            status,
            last_frame,
            recording: None,
        })
    }
    fn initialize(system: &mut PreparedSystem, config: &SessionConfig) -> Result<(), String> {
        if system.runtime.time != 0. {
            return Err("session factory must return a fresh runtime at time zero".into());
        }
        system.runtime.set_grid_snapping(config.grid_snapping);
        system
            .inspection
            .description
            .validate()
            .map_err(|e| e.to_string())?;
        for (island, step) in system
            .runtime
            .islands
            .iter()
            .zip(system.runtime.effective_step_sizes(config.interval))
        {
            SessionConfig {
                interval: step,
                integrator: island.integrator,
                seed: config.seed,
                grid_snapping: config.grid_snapping,
            }
            .validate()?;
            if island.time != 0. {
                return Err("factory returned an advanced island".into());
            }
            if !island.event_tolerance.is_finite() || island.event_tolerance <= 0. {
                return Err("factory returned an invalid event tolerance".into());
            }
        }
        system.runtime.seed(config.seed);
        system.runtime.set_observation_capture(true);
        for island in &mut system.runtime.islands {
            // This session owns recording. Do not retain an additional trajectory.
            island.record_every = 0;
            island.events.clear();
        }
        Ok(())
    }
    fn stamp(status: &SessionStatus) -> FrameStamp<'_> {
        FrameStamp {
            run_id: &status.run_id,
            generation: status.generation,
            sequence: status.sequence,
            step: status.step,
        }
    }
    pub fn status(&self) -> &SessionStatus {
        &self.status
    }
    pub fn description(&self) -> &SystemDescription {
        &self.system.inspection.description
    }
    pub fn latest(&self) -> &SampleFrame {
        &self.last_frame
    }
    pub fn recording_len(&self) -> usize {
        self.recording.as_ref().map_or(0, |r| r.data.frames.len())
    }
    fn usable(&self) -> Result<(), String> {
        match self.status.phase {
            Phase::Paused | Phase::Running => Ok(()),
            _ => {
                Err("session must be reset, or a full recording retrieved, before advancing".into())
            }
        }
    }
    pub fn subscribe(&mut self, ids: Vec<String>) -> Result<(), String> {
        // Preserve last committed values after a failed/partially advanced solve.
        self.usable()?;
        let selected: BTreeSet<_> = ids.into_iter().collect();
        let subscription = self
            .system
            .inspection
            .subscribe(selected.iter().map(String::as_str))
            .map_err(|e| e.to_string())?;
        let mut next = self.status.clone();
        next.sequence = next.sequence.checked_add(1).ok_or("sequence exhausted")?;
        let frame = subscription
            .sample(&self.system.runtime, Self::stamp(&next))
            .map_err(|e| e.to_string())?;
        self.status = next;
        self.selected = selected;
        self.display = subscription;
        self.last_frame = frame;
        Ok(())
    }
    pub fn begin_recording(&mut self, ids: Vec<String>, capacity: usize) -> Result<(), String> {
        self.usable()?;
        if self.recording.is_some() {
            return Err("retrieve the current recording before starting another".into());
        }
        if capacity == 0 || ids.is_empty() {
            return Err("recording needs observables and a positive frame capacity".into());
        }
        let observables: BTreeSet<_> = ids.into_iter().collect();
        let subscription = self
            .system
            .inspection
            .subscribe(observables.iter().map(String::as_str))
            .map_err(|e| e.to_string())?;
        let initial = subscription
            .sample(&self.system.runtime, Self::stamp(&self.status))
            .map_err(|e| e.to_string())?;
        self.recording = Some(ActiveRecording {
            subscription,
            capacity,
            data: Recording {
                version: 1,
                description: self.description().clone(),
                runtime_identity: RuntimeIdentity::current(),
                config: self.config.clone(),
                islands: self
                    .system
                    .runtime
                    .islands
                    .iter()
                    .zip(
                        self.system
                            .runtime
                            .effective_step_sizes(self.config.interval),
                    )
                    .map(|(island, step_size)| IslandSettings {
                        integrator: island.integrator,
                        step_size,
                        event_tolerance: island.event_tolerance,
                        event_jacobian_reuse: island.event_jacobian_reuse,
                    })
                    .collect(),
                target_arch: std::env::consts::ARCH.into(),
                target_os: std::env::consts::OS.into(),
                observables,
                frames: vec![initial],
                completion: RecordingCompletion::Active,
                end_status: self.status.clone(),
            },
        });
        Ok(())
    }
    pub fn take_recording(&mut self) -> Option<Recording> {
        let mut recording = self.recording.take()?;
        recording.data.end_status = self.status.clone();
        recording.data.completion = match self.status.phase {
            Phase::Failed => RecordingCompletion::Failed,
            Phase::Cancelled => RecordingCompletion::Cancelled,
            Phase::RecordingFull => RecordingCompletion::CapacityReached,
            _ => RecordingCompletion::Stopped,
        };
        if self.status.phase == Phase::RecordingFull {
            self.status.phase = Phase::Paused;
            self.status.message = None;
        }
        Some(recording.data)
    }
    /// Called by the worker scheduler. A paused tick is a true no-op.
    pub fn tick(&mut self) -> Result<Option<&SampleFrame>, String> {
        if self.status.phase != Phase::Running {
            return Ok(None);
        }
        self.advance()?;
        Ok(Some(&self.last_frame))
    }
    fn advance(&mut self) -> Result<(), String> {
        self.usable()?;
        if self
            .recording
            .as_ref()
            .is_some_and(|r| r.data.frames.len() >= r.capacity)
        {
            self.status.phase = Phase::RecordingFull;
            self.status.message =
                Some("recording capacity reached; retrieve it before continuing".into());
            return Err(self.status.message.clone().unwrap());
        }
        let mut next = self.status.clone();
        next.step = next.step.checked_add(1).ok_or("step counter exhausted")?;
        next.sequence = next.sequence.checked_add(1).ok_or("sequence exhausted")?;
        let start = Instant::now();
        let result = (|| {
            // Tick onto the exact grid `step · interval`: the duration is
            // measured from the current clock, so rounding never accumulates
            // (repeated `t += h` random-walks the clock away from scheduled
            // event times until an edge splits off a femtosecond step).
            let target = next.step as f64 * self.config.interval;
            let duration = if self.system.runtime.grid_snapping() {
                target - self.system.runtime.time
            } else {
                self.config.interval
            };
            self.system
                .runtime
                .advance(duration, self.config.interval)
                .map_err(|e| e.to_string())?;
            next.time = self.system.runtime.time;
            let display = self
                .display
                .sample(&self.system.runtime, Self::stamp(&next))
                .map_err(|e| e.to_string())?;
            let recorded = self
                .recording
                .as_ref()
                .map(|r| {
                    r.subscription
                        .sample(&self.system.runtime, Self::stamp(&next))
                        .map_err(|e| e.to_string())
                })
                .transpose()?;
            Ok::<_, String>((display, recorded))
        })();
        self.status.step_wall_seconds = start.elapsed().as_secs_f64();
        // The integrator uses the log within an advance for event location, so
        // drain only between calls. Its cumulative RunStats stay untouched.
        let events = self
            .system
            .runtime
            .islands
            .iter()
            .fold(0_u64, |n, i| n.saturating_add(i.stats.events));
        for island in &mut self.system.runtime.islands {
            island.events.clear();
        }
        match result {
            Ok((display, recorded)) => {
                next.events = events;
                next.step_wall_seconds = self.status.step_wall_seconds;
                self.status = next;
                self.last_frame = display;
                if let (Some(r), Some(frame)) = (&mut self.recording, recorded) {
                    r.data.frames.push(frame);
                    r.data.end_status = self.status.clone();
                }
                Ok(())
            }
            Err(error) => {
                self.status.phase = Phase::Failed;
                self.status.message = Some(error.clone());
                Err(error)
            }
        }
    }
    /// Reconstruct before swapping, so a failed reset preserves the old session.
    /// Replace the model while running (an edit in a viewer). When the new
    /// model has the same structure (a parameter edit), the committed state
    /// and clock carry over and the run continues; otherwise (a structural
    /// edit) the new model starts at t = 0. Subscriptions are kept where the
    /// new description still has the observables. Returns whether the state
    /// was preserved. Any active recording ends.
    pub fn hot_swap(&mut self, factory: impl FnMut(&SessionConfig) -> Result<PreparedSystem, String> + 'static) -> Result<bool, String> {
        let mut factory: Factory = Box::new(factory);
        let mut system = factory(&self.config)?;
        Self::initialize(&mut system, &self.config)?;
        let snapshot = self.system.runtime.snapshot();
        let preserved = system.runtime.restore(&snapshot).is_ok();
        if !preserved {
            // A failed restore may have partially written state: rebuild clean.
            system = factory(&self.config)?;
            Self::initialize(&mut system, &self.config)?;
        }
        let known: BTreeSet<String> = self.selected.iter().filter(|id| system.inspection.description.observables.contains_key(*id)).cloned().collect();
        let display = system.inspection.subscribe(known.iter().map(String::as_str)).map_err(|e| e.to_string())?;
        let mut status = self.status.clone();
        status.generation = status.generation.checked_add(1).ok_or("generation exhausted")?;
        status.sequence = 0;
        if !preserved {
            status.time = 0.;
            status.step = 0;
            status.events = 0;
        }
        status.message = Some(if preserved { "model updated; state carried over".into() } else { "model structure changed; restarted at t = 0".into() });
        let frame = display.sample(&system.runtime, Self::stamp(&status)).map_err(|e| e.to_string())?;
        let _ = self.take_recording();
        self.factory = factory;
        self.system = system;
        self.display = display;
        self.selected = known;
        self.status = status;
        self.last_frame = frame;
        Ok(preserved)
    }

    fn reset(&mut self) -> Result<Option<Recording>, String> {
        let mut system = (self.factory)(&self.config)?;
        Self::initialize(&mut system, &self.config)?;
        if system.inspection.description.id != self.description().id {
            return Err("reset changed the captured description; open it as a new session".into());
        }
        let generation = self
            .status
            .generation
            .checked_add(1)
            .ok_or("generation exhausted")?;
        let display = system
            .inspection
            .subscribe(self.selected.iter().map(String::as_str))
            .map_err(|e| e.to_string())?;
        let status = SessionStatus {
            phase: Phase::Paused,
            run_id: self.status.run_id.clone(),
            generation,
            step: 0,
            time: 0.,
            sequence: 0,
            step_wall_seconds: 0.,
            events: 0,
            message: None,
        };
        let frame = display
            .sample(&system.runtime, Self::stamp(&status))
            .map_err(|e| e.to_string())?;
        let mut archived = self.take_recording();
        if let Some(r) = &mut archived
            && r.end_status.phase != Phase::Failed
        {
            // Rebuilding must not overwrite the cause of a failed recorded run.
            r.completion = RecordingCompletion::Reset;
            r.end_status.phase = Phase::Cancelled;
            r.end_status.message = Some("session reset".into());
        }
        self.system = system;
        self.display = display;
        self.status = status;
        self.last_frame = frame;
        Ok(archived)
    }
    pub fn execute(&mut self, command: Command) -> Result<Reply, String> {
        let mut description = None;
        let mut frame = None;
        let mut recording = None;
        match command {
            Command::Describe => description = Some(self.description().clone()),
            Command::Subscribe { observables } => {
                self.subscribe(observables)?;
                frame = Some(self.last_frame.clone());
            }
            Command::Start => {
                self.usable()?;
                self.status.phase = Phase::Running;
            }
            Command::Pause => {
                self.usable()?;
                self.status.phase = Phase::Paused;
                // Freeze all linked views at the final committed sample, even if
                // it had not yet reached the coalesced display transport.
                frame = Some(self.last_frame.clone());
            }
            Command::Step => {
                if self.status.phase != Phase::Paused {
                    return Err("single-step requires a paused session".into());
                }
                self.advance()?;
                frame = Some(self.last_frame.clone());
            }
            Command::Reset => {
                recording = self.reset()?;
                frame = Some(self.last_frame.clone());
            }
            Command::Cancel => {
                self.status.phase = Phase::Cancelled;
                self.status.message = Some("cancelled by caller".into());
            }
            Command::BeginRecording {
                observables,
                capacity,
            } => self.begin_recording(observables, capacity)?,
            Command::TakeRecording => recording = self.take_recording(),
        }
        Ok(Reply {
            status: self.status.clone(),
            description,
            frame,
            recording,
        })
    }
}
