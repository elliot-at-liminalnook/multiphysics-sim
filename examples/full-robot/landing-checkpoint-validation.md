# Support-qualified body return

This experiment adds a support checkpoint to the gain-0.5 sampled Rhai lift.
The robot, floor law, servo dynamics, trajectory and observation contract are
unchanged. The registered Rust `control.motion_clock` qualifies the existing
ideal floor-force observations and retimes the reference. Physics, Rhai feedback
and servo firmware continue while reference time waits.

`landing-checkpoint.json` requires all four named foot crossheads to report at
least 1 N upward support for 100 ms, sampled every 2 ms. The guarded reference
interval is 1.1–1.4 s, covering body return after planned foot descent. The dwell
can begin before guard entry. A maximum 300 ms pause stops the experiment with
an explicit timeout. The 2 s physics horizon includes waiting and final hold.
These are provisional experiment settings, not measured hardware limits or a
complete landing detector: foot position, body balance, slip and real sensing
still need separate acceptance.

The 0.25 ms run waited from 1.100 to 1.110 s. Its qualification window began at
1.010 s; every sampled foot force in that window exceeded 1 N. Reference motion
completed at 1.610 s. The accepted contact history contained no internal part
contacts. Original constraint rows remain in every reporting frame. See the
machine-readable audit for closure and penetration diagnostics.

Halving the timestep to 0.125 ms preserves the 10 ms pause, 1.110 s resume and
1.610 s reference completion. Across 201 aligned frames, the largest world-foot
position difference is 0.344 mm. Sampled contact pairs match. Original position
closure stays below 8.3e-13 m in both runs, and neither reports accepted internal
contact. Maximum accepted floor penetration is 0.131 / 0.142 mm respectively.

The runs are not numerically identical: two event guards have different event
counts, sampled shaft torque differs by up to 0.126 N m and current by 0.085 A.
The largest per-foot integrated force-vector difference over 2 s is 0.117 N s.
These differences remain diagnostics, not accepted motor/contact calibration.
The tested support decision is stable under this refinement; a converged
reference and full landing/balance acceptance have not been established.

Observed native wall times are 118.0 s and 164.1 s for 2 simulated seconds on an
Intel Core i9-9980HK. Browser checks ran concurrently during parts of these runs;
these are development timings, not isolated throughput benchmarks. Realtime
performance remains unmet.

## Reproduce

Prepare the preceding joint-feedback and task-observation inputs as documented
in their validation notes, then run from the repository root:

```sh
node examples/full-robot/prepare_landing_checkpoint.mjs
cargo build --locked --release -p sim-runtime --example integrate_embedding --example compare_embedding
target/release/examples/integrate_embedding runs/full-robot/learning/landing-checkpoint/scene.json runs/full-robot/learning/landing-checkpoint/config.json > runs/full-robot/learning/landing-checkpoint/execution.json
target/release/examples/integrate_embedding runs/full-robot/learning/landing-checkpoint/scene.json runs/full-robot/learning/landing-checkpoint/refined.config.json > runs/full-robot/learning/landing-checkpoint/refined.execution.json
node examples/full-robot/audit_landing_checkpoint.mjs runs/full-robot/learning/landing-checkpoint/execution.json runs/full-robot/learning/landing-checkpoint/audit.json
node examples/full-robot/audit_landing_checkpoint.mjs runs/full-robot/learning/landing-checkpoint/refined.execution.json runs/full-robot/learning/landing-checkpoint/refined.audit.json
target/release/examples/compare_embedding runs/full-robot/learning/landing-checkpoint/execution.json runs/full-robot/learning/landing-checkpoint/refined.execution.json examples/full-robot/foot-markers.json > runs/full-robot/learning/landing-checkpoint/refinement.json
```

The audit requires a completed run that actually pauses and resumes, checks
the force history preceding resume, and rejects reported internal contacts.
Closure and penetration are reported with their original units. The comparison
tool measures trajectory, event and accepted-step impulse differences; it does
not independently establish timestep convergence or physical accuracy.

## Viewer and reproducible failures

The `robot-landing-checkpoint` preset executes the same scene/config through
WASM. It shows reference time, support qualification duration, waiting and
timeout. Status comes from Rust's persistent motion-clock state, including in
bounded-history interactive sessions. There is no browser-side controller.

Embedded recording version 3 retains the original failure reason. Replay
executes the failed attempt after the successful prefix, then verifies the
committed step index and failure reason. Earlier recordings remain readable,
but cannot recover an unrecorded failure. Rust regressions cover support
timeouts, first-step policy errors, horizon exhaustion and mismatched failure
recipes. A synthetic missing-support browser fixture checks visible timeout,
save/replay and reset recovery. The synthetic provenance exists only in the
test, not in a robot preset.

The full robot browser check passes across all 201 native reporting frames
(maximum entry difference 4.57e-9 N in an observed foot force, below the 1e-7
portability tolerance). Browser replay and reset are exact excluding wall time;
changed-scene/config replay is rejected without mutating the session. Live WASM
takes 123.2 s for 2 simulated seconds, with a slowest 10 ms request of 2.39 s.
The main thread remains responsive while the worker computes. Twelve UI checks
pass, including the new status, failure replay, earlier presets and narrow layout.

`landing-checkpoint-status.json` records hashes, both native audits, refinement
diagnostics and browser reports. A source snapshot and native executable are
preserved alongside the captures. The tested ten-preset browser bundle is
`runs/interactive/robot-lab-landing-checkpoint-2026-09-07.zip`; its ZIP integrity
check passes. The same bundle is served locally from `runs/interactive/viewer`.

Build/test the viewer using `web/README.md`, then run the full robot portability
check with `web/tests/embedded.mjs`, preset `robot-landing-checkpoint`, and the
native `execution.json`. Browser UI tests and native session/motion-clock tests
are part of the existing CI workflow; the full robot captures are local evidence.

This is a support-qualified transition milestone. Repeated stepping, task-space
landing correction, active balance, realistic deployed observations, teacher RL,
distillation, WASD locomotion and realtime throughput remain unfinished.
