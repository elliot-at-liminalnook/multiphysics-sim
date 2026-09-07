# Guarded Jacobian reuse across servo samples

The complete point-feedback profile constructs roughly ten thousand Jacobians
for 11,200 nominal physics steps. The motor stepper previously discarded its
numerical workspace at every event, including firmware samples that only update
held commands. This experiment tests whether keeping a modified-Newton proposal
through those samples reduces total work.

`ImplicitStepConfig.reuse_controller_sample_jacobian` is opt-in and defaults to
false. It requires cross-step Jacobian reuse. The `SampledMotorControl` adapter
must explicitly identify an eligible sample; unknown adapters default to false.
The registered servo adapter identifies its existing per-servo sampling guards.
Motor-mode events still invalidate, as do timestep/coordinate changes and the
existing contact-change checks. Physical states, contact history, loads and
residual values are never reused by this option.

A kept matrix is not assumed correct. The existing Newton solver evaluates the
new equations, rejects nondecreasing stale proposals, refreshes after poor
contraction or prolonged small corrections, and verifies original residual
limits before accepting. All existing closure, state and event checks remain.
Rejected intervals preserve caller state and held-controller data.

The clocked motor fixture agrees with its independent linear circuit/mechanical
reference and preserves sampling and rollback with reuse enabled. However, its
successful-trial endpoint evaluations increase from 22 to 39. Reuse is therefore
not automatically a speedup. All 12 motor and eight embedded-step tests pass;
the first full robot run also completes. At 0.25 ms, successful-trial endpoint
evaluations fall from 785,939 to 474,955 and Jacobian builds from 9,948 to 5,117.
Stepping takes 116.24 s for 2.8 simulated seconds. Those work counters omit
failed-trial internals; the wall time includes the run's rejected work.

The first physical comparison has maximum foot-marker difference 5.54e-12 m,
current difference 2.75e-9 A, and contact-impulse difference 1.42e-9 N·s. Event
counts and sampled contact pairs match; maximum event-time difference is 1.83e-9 s.
Supported lift still qualifies for 200 ms with 3.211 mm peak clearance. This passes
the previous strict roundoff diagnostic limits at this timestep, not hardware
accuracy or walking acceptance. A fresh same-build reference takes 159.86 s versus 116.24 s with reuse, an
observed 1.375× improvement. This is a single development pair, not a repeated
isolated benchmark.

At 0.125 ms, a fresh reference completes in 223.21 s versus 196.34 s with reuse.
Both pass sampled supported lift (210 ms, 3.203 mm peak clearance), but the strict
roundoff comparison fails: maximum current difference is 0.0409956 A, foot-marker
difference 4.35659e-8 m, contact impulse difference 0.000103334 N·s, and event-time
difference 2.54437 microseconds. Event counts and sampled contact pairs match.
The fresh reference reproduces the older reference discrepancy, ruling out a
stale capture as the explanation. This does not establish which numerical path
is closer to continuous-time behavior. No strict thresholds have been relaxed.

The first reported current difference above 1e-7 A occurs at 1.52 s. In the
preceding nominal interval (index 12158, ending 1.519875 s), reuse accepts three
segments versus two in the reference, with nine versus seven continuous trials.
Both locate motor guard 6 near 1.519829312 s, within 5e-12 s of each other. The
minimum accepted remainder is halved in the candidate. This identifies a local
subdivision difference that precedes the visible electrical discrepancy; it is
not yet proof of the reason for that failed trial. Different backward-Euler
subdivisions can give different discrete trajectories.

All 281 browser samples pass the native/WASM limit (maximum difference 7.34e-8 N),
with exact input replay and reset. Nineteen UI checks pass, and the rendered
robot/telemetry view was inspected. Browser stepping takes 141.10 s for 2.8 s,
with a maximum worker chunk of 2.195 s; the main thread continues responding.
The last passing installed point-feedback viewer remains unchanged. The
optimization remains opt-in and unpromoted because refined-state equivalence
has not passed. These runs do not establish accepted walking or realtime.

The robot recipe retains the whole-matrix closure path. It does not combine the
previous unpromoted block-factor experiment with this new variable. The CAD,
world, controller, reference knots, gains, timestep and tolerances are unchanged.
Native profiling is enabled only to measure the work; the browser recipe disables
those native-only timers.

## Reproduce

Prepare the point-feedback inputs, then:

```sh
cargo test --locked -p sim-domain-robot --test embedded_motor --test embedded_step
cargo build --locked --release -p sim-runtime --example integrate_embedding --example compare_embedding --example evaluate_lift
node examples/full-robot/prepare_sample_reuse.mjs
```

Run `integrate_embedding` with `point-feedback/scene.json` and each config in
`sample-reuse/`. Preserve captures and source identities. Compare complete runs
with `compare_embedding` and `foot-markers.json`; apply `evaluate_lift` using the
same scene, `forward-slow/lift-requirements.json`, and `--simulation-time`.
Measure original closure rows, motor states, foot motion, forces/impulses,
event timing/counts, Jacobian/residual work and total runtime. Test refinement
and native/WASM execution before any promotion. Sampled lift alone is not a
walking, motor-calibration or realtime acceptance gate.

## Recovered-failure diagnosis

Shared `HybridDiagnostics` now reports the number of rejected outer trials and
up to sixteen rejection records per interval (start time, attempted duration,
and up to 2048 characters of reason). It records event-location errors too;
outer duration is explicitly distinguished from an inner location-trial step.
Counts are complete when details are capped. This is diagnostic instrumentation,
not a change to acceptance, state updates, event ordering or subdivision policy.
Five scheduler tests and the 20 motor/embedded-step tests pass, including bounded
Unicode reasons, recovered failures, event ordering and atomic rollback.

The diagnostic pair intentionally runs only through 1.52 s. Both advance all
12,160 requested intervals, then return the explicit incomplete-motion-horizon
error. They are not successful full-motion captures. Every one of their 153
sampled physical frames and every original scheduler field matches the
corresponding prefix of its earlier full run exactly (`diagnostic-prefix-check.json`).

At 1.5198293121371398 s, after guard 6 has fired, the reuse run rejects a
45.68786286 microsecond remainder: Newton reaches its 40-iteration cap, with
largest final residual 3.77979e-11 in auxiliary row 9. That is the internal
rotor-speed equation of the -Y foot motor; the auxiliary-rate unknown is about
-1.0271e4 rad/s². Its old rotor speed is about 0.561946 rad/s. The reference
accepts its almost identical remainder without rejection. Because a motor-mode
jump clears the matrix, this failure is not directly a stale matrix surviving
that jump. Earlier roundoff differences can affect the subsequent Newton path.
The iteration history is still needed to determine why the correction-based
convergence check does not finish within its cap. A small residual alone does
not prove the required state correction is small, and no tolerance was relaxed.

Run the diagnostic configs with the instrumented `integrate_embedding`; expect
exit status 1 only for the recorded incomplete-motion-horizon error, and verify
that all requested steps were reached. `audit_hybrid_divergence.mjs` requires
completed runs by default; its explicit `--allow-incomplete-motion-horizon`
option accepts only this fully advanced, prematurely ended motion case. It
preserves completion/error fields in its report, and is not an acceptance gate.

```sh
node examples/interactive/audit_hybrid_divergence.mjs runs/full-robot/learning/sample-reuse/diagnostic.execution.json runs/full-robot/learning/sample-reuse/diagnostic.reference.execution.json runs/full-robot/learning/sample-reuse/diagnostic-divergence.json 1e-7 --allow-incomplete-motion-horizon
```

The source snapshot records the post-instrumentation sources and
`diagnostic-native-runner`. The earlier timing `native-runner` and tested browser
WASM predate this additive instrumentation; these identities are kept separate.
