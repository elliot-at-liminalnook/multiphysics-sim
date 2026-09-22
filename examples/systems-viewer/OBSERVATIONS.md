# Runtime observations and accepted solver samples

The compiler can retain per-port through contributions from solver evaluations
that already occur. Sampling does not reevaluate behavior equations, draw noise,
run controllers/events, or touch the Newton factorization cache.

Enable capture before advancing. Bind each requested port/lane once, then retain
the numeric binding:

```rust
runtime.set_observation_capture(true);
let binding = runtime.bind_through(port, lane)?;
runtime.advance(duration, step)?;
let flow = runtime.read_flow(&binding)?;
// binding.quantity(), binding.unit()
// flow.value, flow.evaluation_time, flow.step_start, flow.step_end
```

Bindings are local to one runtime instance. A binding from another runtime returns
`ForeignBinding`, even if its disposable port IDs happen to match. Invalid physical
ports or lanes return `UnknownLane`. There is a convenience `observe_through`
lookup, but repeated subscribers should use `bind_through` and `read_flow`.

## What the time means

Backward Euler evaluates at the step endpoint using its solved discrete rate.
Implicit midpoint evaluates at the solver stage: differential midpoint values,
solved algebraic values and the matching increment-derived rates. Its contribution
must not be labelled an instantaneous endpoint flow. For example, a 0.1 s midpoint
step from zero carries `evaluation_time = 0.05` and `step_end = 0.1`.

A successful implicit solve is only a candidate. Retention first matches the exact
stage state, rate and time against a captured residual evaluation, then matches the
candidate endpoint against the state actually committed by the integrator. Reads
also require a successful runtime state-store commit at that island time; a
controller or second-law failure cannot publish its newer trial as committed. Event
jumps, seeding, restoration and consistency solves invalidate retained observations.
No match, disabled capture, a just-enabled subscription, or an unsupported integrator
returns `Unavailable`; there is no fallback equation evaluation or invented zero.
The opt-in ordered residual path is not captured yet. Exact matching deliberately
favours an explicit unavailable result over associating a nearby trial with a commit.

Only a bounded evaluation/candidate/committed set is retained per island. This is
not a trajectory recording buffer. Capture currently retains all port contributions
while enabled; the subscription-overhead performance gate remains to be measured.

## Physical interpretation

Values use registered canonical units and the convention positive into a component.
For balanced connectors they are the contributions written by the behavior.
For an owned rigid-body frame, the owner's contribution additionally includes the
constitutive balance written into its registered state rows. This matches the
compiler's assembled connection balance; reporting only the explicit port write
would falsely report zero for an accelerating body.

`residual_max` is the maximum absolute unscaled full-system residual from the same
evaluation. Its rows may have different units. It is diagnostic context, not a
single physically meaningful norm or an automatic acceptance threshold.

## Evidence and remaining integration

Tests compare capture on/off for both deterministic and seeded noisy systems:
complete runtime snapshots and residual call counts are identical. Repeated reads
make no equation calls. Flow sums, evaluation times, foreign bindings, invalid
lanes, restore and event invalidation are checked. A 2 kg body under a 4 N force
has matching +4/-4 N boundary contributions and reaches 0.2 m/s after 0.1 s.
The independent diffusion domain retains mol/s metadata, analytic flow values and
nested composite port mappings.

The static viewer has not gained live numbers yet. Optional diagnostics, process
transport, plots, replay and the 64-observable/30 Hz performance gate remain in
the full plan. Existing entropy-production state units also still need the
separately documented rate-unit correction.

## Compiled inspection subscriptions

Enable the `runtime` feature of `sim-inspect` to bind a description to a compiler
runtime. Contract-only users do not need that dependency. `RuntimeInspection`
resolves authoring observable IDs to numeric state/port bindings once; each
subscription selects a subset without resending topology or parameters:

```rust
use sim_inspect::runtime::{RuntimeInspection, FrameStamp};
let inspection = RuntimeInspection::new(
    &runtime, &registry, source_hash, revision, &identities,
)?;
let subscription = inspection.subscribe(selected_ids.iter().map(String::as_str))?;
runtime.set_observation_capture(true);
runtime.advance(0.1, 0.1)?;
let frame = subscription.sample(&runtime, FrameStamp {
    run_id: "run-1", generation: 0, sequence: 1, step: 1,
})?;
```

The caller owns run identity, step counts and generations. This adapter does not
implement a session, worker or recording policy. Frames are native typed data;
serialization is needed only at transport/storage boundaries. Repeated sampling
runs no equations. Unknown subscription IDs are errors; unsupported capabilities
and missing accepted evaluations have explicit reasons. Runtime-bound quantities
must match registered observation metadata. Legacy wildcard signals may acquire
their compiler-resolved quantity.

Frame format **2** distinguishes:

- `Committed`: retained differential state at the successful runtime commit time.
  Initial differential values can be read before the first step or with capture off.
- `AcceptedStage`: algebraic/derived values and terminal flows from a matching
  accepted implicit evaluation, with `sample_time`, `step_start` and `step_end`.
- `Unavailable`: no trustworthy value at this point, with a reason.

For `xdot + x = 0`, `x(0) = 1`, a 0.1 s midpoint step gives the endpoint state
`0.95/1.05` at 0.1 s and a signal that reads that state during evaluation gives
`1/1.05` at 0.05 s. They must not be displayed as simultaneous endpoint values.
Frame validation rejects invalid stage intervals. Version 1 endpoint-only frames
still decode and validate; stage values require version 2. Description format
version remains 1.

Runtime capability availability is sealed independently of the selected subset,
and can change the description ID relative to a static authoring capture. Do not
apply static sidecars to that new ID without explicit identity reconciliation.
Source component/port/observable IDs remain available for this future integration.
The retained quadruped structure capture is not overwritten by this adapter.


## Legacy wildcard signal resolution

A connected signal now resolves to the concrete quantity declared by its typed
terminals, regardless of terminal order. A generic controller output connected
to an angle input therefore binds as radians. Conflicting concrete consumers
are rejected even when the first terminal is a legacy wildcard. This corrects
12 firmware-target bindings in the quadruped acceptance capture. It does not
complete the separate migration from legacy Dimensionless-as-wildcard to an
explicit authoring `SignalType::Any`.
