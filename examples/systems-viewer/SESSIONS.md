# General system sessions and observation playback

`sim-runtime::system_session` owns a general compiled system, independently of
any robot or renderer. Its serializable `Command`/`Reply` service is intended for
both headless callers and the native worker. `tick()` advances one declared
simulation interval only while running. Rendering never invokes physics.

```rust
use sim_runtime::system_session::{Command, ModelSource, SessionConfig, SystemSession};
let source = ModelSource {
    model, registry, identities, source_hash, revision: 1,
};
let config = SessionConfig {
    interval: 0.001,
    integrator: sim_dynamics::Integrator::implicit_midpoint(),
    seed: 71,
};
let mut session = SystemSession::new("run-1".into(), config, move |config| {
    source.build(config)
})?;
let selected = session.description().observables.keys().cloned().collect();
session.execute(Command::Subscribe { observables: selected })?;
session.execute(Command::Step)?; // Paused, one interval
session.execute(Command::Start)?;
session.tick()?;                 // Worker scheduler calls this, never the UI
session.execute(Command::Pause)?;
```

`ModelSource` accepts ordinary Rust/Rhai-composed `ModelWorld`s with a supplied
registry and identity map. Its factory recompiles fresh behaviors on reset.
Custom factories return `PreparedSystem` with the runtime and its inspection;
they must recreate their controller/coupler state and honor the requested
configuration. CAD/controller factory adapters are not connected yet. The
session records actual per-island integrators, fixed steps (including multirate
overrides), event tolerance and event-Jacobian reuse alongside requested defaults.

## Commands and state

Commands: describe, subscribe, start, pause, step, reset, cancel, begin recording,
and retrieve recording. Start changes scheduling state; it does not itself step.
Single-step requires pause. A paused tick does nothing. Failed or cancelled
sessions require reset. Recording capacity exhaustion requires retrieval or reset.

Status carries run identity, restart generation, sequence, completed step/time,
last stepping wall time, cumulative committed event count, phase and error text.
A failed partial advance never replaces the last completed frame. Display values
stay available to the host, which must mark them stale using the status.

Reset builds and validates a replacement before swapping. Failed resets preserve
old values and recording. A changed description is rejected as a different
capture. Successful resets increment the generation, clear the live cursor,
reseed the new runtime and return any previous recording in the reply. They do
not use the plant-only `RuntimeSnapshot` as a controller/RNG checkpoint.

Cancel here is a command between steps. Interrupting a stuck solve still requires
the planned parent-owned native worker process; this synchronous service does
not claim a cancellation latency guarantee.

## Recording and playback

Display subscriptions and recording subscriptions are independent. Starting a
recording captures the current point and fixes its observable set. Capacity is an
explicit number of frames, including that first point. If another point would
exceed capacity, the session enters `RecordingFull` **before advancing**. It
neither overwrites old samples nor silently drops requested ones. Retrieval ends
the recording and allows a full session to continue from pause.

The version-1 observation archive includes the sealed description, source-bound
runtime identity, seed, solver settings, target architecture/OS, selected IDs,
versioned sample frames, completion reason and final status. Completion distinguishes
caller stop, reset, cancellation, failure and capacity exhaustion. Serialization
is at the boundary; the session retains typed frames internally.

`Recording::validate()` checks the description, frame identities, generations,
monotonic sequence/step/time, observable coverage and final completed point.
`at_or_before(time)` chooses an exact stored frame at/before the cursor. It does
not interpolate. Callers must show that frame's actual time and value availability.
It never creates a runtime, executes equations or calls controllers.

The session keeps one current display frame. The explicitly bounded recording
owns selected history. Integrator trajectory capture stays disabled; per-event
logs are drained between intervals, while cumulative event statistics remain.
A hybrid pulse test compares this behavior against an unmodified runtime over
100 intervals and at least 199 events. This bounds history across intervals;
it is not a measured bound on work or allocations inside one stalled solve.

## Current limits

This is a shared command/session implementation with focused tests. An initial
native process host/client now has one basic smoke test (see the implementation
record), but process stress/error acceptance, the CAD launch adapter, UI transport/
plots, disk streaming, recorded external controller commands, and overhead gates remain
unfinished. The in-memory recording is lost if its owning process is terminated
before retrieval; the worker integration must preserve committed recordings
separately before claiming cancellation-safe capture.

These are **observation playback** archives, not a complete input re-execution
or checkpoint format. Factories may attach controllers, but this API does not yet
provide the required recorded ingress for changing their commands during a run.
Do not infer complete sim-to-real reproducibility from the archived source hash,
settings or successful playback alone.
