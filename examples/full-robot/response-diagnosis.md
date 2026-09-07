# Faster-gait response and floor dissipation

The faster controller remains unpromoted. This investigation separates controller
feedback from the simulated response, and exposes an existing contact coefficient
with explicit units. It does not change any maintained browser preset or CAD file.

## Holding motor decisions fixed

The source is the 24 s, 1.5×-speed candidate in `speed-envelope.md`. A generated
Rhai script replays its sampled motor targets at 50 Hz through the existing
controller interface. Motors, linkage dynamics and contact remain live. Online
geometry guards and observation calculations remain enabled; their feedback
corrections no longer determine motor targets in this diagnostic.

At 20 ms physics, every one of the 1,201 full physical/policy frames reproduces
the source exactly, excluding wall time. This establishes that target extraction
and sample timing are correct for the experiment. At 5 ms physics, the same motor
targets still produce different motion and eventually fail numerically at 11.42 s.

| Common interval | Maximum foot difference with fixed targets | With ordinary feedback |
| --- | --- | --- |
| First 1 s | 1.195 mm | 0.871 mm |
| First 5 s | 1.787 mm | 1.405 mm |
| First 10 s | 2.895 mm | 1.917 mm |
| Through 11.42 s | 2.991 mm | 1.970 mm |

The full feedback comparison first exceeds 0.1 mm during the initial body shift
at 0.34 s, before the first foot lifts. Feedback reduces the discrepancy in these
windows; it is not sufficient to explain the underlying timestep sensitivity.
This comparison does not independently separate actuator dynamics from contact,
inertia, friction and numerical integration. Failed captures are compared only
through their common committed prefix, never described as completed trajectories.

## What the floor damping means

The articulated contact implementation uses, for a penetrated sampled point,

`normal_force = max(0, stiffness * depth * (1 - alpha * separation_speed))`.

`alpha` has units **s/m**. At zero normal speed, its local damping slope is
`alpha * stiffness * depth`, in N·s/m. Thus a point with a 20 N static spring load
and alpha=0.2 s/m has a local slope of 4 N·s/m. The stored world `floor_damping`
value is not directly applied as a linear dashpot coefficient by this law.

Historically alpha was assigned by the heuristic
`clamp(world.floor_damping / world.floor_stiffness, 0.2, 3.0)`. That historical
mapping remains the default for compatibility; it should not be interpreted as
a calibrated or general dimensional conversion. For this scene it produces 0.2.

New optional runtime/scene setting:

```json
{"floor_dissipation_s_m": 100.0}
```

This field belongs inside `scene.options`. Its shared articulated-registry
parameter is `floor.dissipation`, declared in s/m, optional without a numeric
default, finite and nonnegative. Rust callers use
`Options::floor_dissipation_s_m`. The direct articulated model and the compiled
registry equations receive the same value. An absent value preserves historical
serialization and forces. The override affects floor contact only; inter-link
dissipation keeps its existing behavior.

The value 100 is an **uncalibrated sensitivity trial**, not a recommendation or
newly measured property. It changes the physical contact response. It must remain
an explicit recorded profile/environment assumption until supported by contact
measurements. Normal force still vanishes outside penetration and cannot pull
the body toward the floor.

## Contact trials

| Variant | Result |
| --- | --- |
| Alpha=100, 20 ms physics | All 13 swings pass; final body error 0.901 mm. |
| Alpha=100, 10 ms physics | Completes, but only 12 swings pass and final body error is 1.670 mm. Maximum foot difference versus 20 ms is 2.293 mm. |
| Alpha=100, 5 ms physics | Fails numerically at 16.9 s. |
| Alpha=100, floor stiffness reduced from 200,000 to 20,000 N/m, 20 ms | Completes, but only 4 of 13 swings meet the original clearance/support requirement. |
| Same softer spring, 5 ms | Fails numerically at 5.04 s. |

The instrumented 5 ms failure has corrections alternating between about
±0.00350 in mechanical unknown 12, including freshly rebuilt Jacobians. These
corrections exceed their allowed scale by roughly 350,000×. This is not merely a
residual sitting just above the acceptance tolerance. The audit is observational
and preserves the failed physical prefix exactly.

Increasing damping alone is not a validated solution; softening contact is not a
validated solution. Neither variant is promoted and no tolerance is relaxed.
These results motivate examining the coupled response and convergence, rather
than attributing the issue solely to feedback or tuning one floor constant.

## Reproduce and verify

First reproduce the source candidate using `prepare_speed_envelope.mjs` and the
commands in `speed-envelope.md`, then:

```sh
cargo build --locked --release -p sim-runtime --example run_environment --example evaluate_lift
node examples/full-robot/prepare_response_diagnosis.mjs
trial_dir=runs/full-robot/learning/response-diagnosis
target/release/examples/run_environment "$trial_dir/frozen.scene.json" "$trial_dir/frozen-20ms.config.json" "$trial_dir/task.json" "$trial_dir/actions.json" > "$trial_dir/frozen-20ms.native.json"
```

The generator also emits frozen 5 ms, closed-loop 20/10/5 ms configurations,
the alpha=100 and softer-contact scenes, and the focused Newton audit recipe.
It hashes the source capture and generator. Capture each named trial; simulator
failure is an expected outcome for the failed cases above. Run
`check_online_steps.mjs` on completed contact trials to retain their acceptance
reports, including failures. `summarize_response_diagnosis.mjs` requires the
expected captures, verifies exact target replay/default compatibility and checks
that acceptance reports match capture hashes.

For `default-20ms.native.json`, rerun the original
`1.5x-stop-feedback-1.scene.json` and `.config.json` from the speed-envelope
experiment using the rebuilt executable, task and actions above. This is a fresh
compatibility capture, not a copy of the historical output.

Validation at this checkpoint:

- Analytic single-point loading, approach/separation dissipation and non-tension
  checks; invalid-value rejection; registry parameter units and initial
  acceleration through the compiled equations all pass.
- Scene override recording and exact runtime replay pass.
- With the override absent, all 1,201 source frames remain exactly unchanged.
- Workspace/all-target checking and native/WASM release builds pass.
- The alpha=100 short run agrees across all 1,200 native/WASM transitions, with
  maximum numeric difference 2.817e-10 and exact replay/reset.
- Maintained browser presets pass the viewer usability suite. No rendered
  performance improvement is claimed; independent validation jobs overlapped.

Machine-readable evidence is in `response-diagnosis-status.json` and
`response-diagnosis-parity.json`. Detailed model calibration, faster robust
walking, planning/teacher/student learning and terrain/disturbance evaluation
remain outstanding. Initial learning work can use the previously accepted slow
baseline within its tested envelope while the faster candidate is investigated.
