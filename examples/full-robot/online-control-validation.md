# Online stepping investigation

This is an experimental controller milestone. The accepted two-cycle, fixed-command
crawl remains available. The browser prototype uses the scheduled-posture/0.5 N planning-screen recipe.
General WASD walking is **not yet accepted**: individual
forward and reverse runs pass, but changing direction can unload a support foot,
and turning is sensitive to both geometric reach and the static planning screen.
The optional support-force integrator has not resolved these failures and remains
disabled in the default browser recipe.

## Execution and observations

`sim-domain-control::stepping` turns bounded planar motion requests into sampled
shift/raise/lower/return/settle references. Each request latches at the start of a
foot transfer. A stop completes that transfer and then idles. Planted reference
feet stay fixed in world coordinates. Support and landing observations qualify
phase changes; a bounded wait ends with an explicit failure.

`sim-runtime::step_reference` resolves those references against the CAD linkage
using bounded inverse kinematics. It rejects internal reference collisions and
infeasible static support. Reference poses never overwrite the integrated robot.
The existing Rhai feedback policy and Rust effective servos produce actual motion.
Floor-force observations are privileged simulated signals, not authored hardware
force sensors or a deployable policy contract.

The optional velocity-indexed posture knots are interpolated and latched once per
transfer. Changing a request does not teleport planted feet. These knots encode
experimental gait settings; they do not change CAD mass, geometry or limits.

The planner's optional `minimum_planned_support_force_n` is distinct from actual
lift/landing qualification. The default retains 1 N for both. A separate experiment
uses a 0.5 N stationary prediction screen while retaining the actual 1 N checks.
A stationary load estimate omits motion and tracking errors, so it is not proof
of actual support. Only complete trajectory checks can accept this distinction.

## Reproduction

Generate the versioned recipes with:

```sh
node examples/full-robot/prepare_online_steps.mjs examples/full-robot/browser-online-steps
cargo build --release -p sim-runtime --example run_environment --example evaluate_lift
```

For example, reproduce forward walking and check the exact recorded configuration:

```sh
target/release/examples/run_environment \
  examples/full-robot/browser-online-steps/scene.json \
  examples/full-robot/browser-online-steps/config.json \
  examples/full-robot/browser-online-steps/task.json \
  examples/full-robot/browser-online-steps/forward-stop.actions.json > runs/online-forward.json
node examples/full-robot/check_online_steps.mjs runs/online-forward.json runs/online-forward-check
```

The checker extracts the scene and configuration from the capture. It evaluates
CAD contact-surface clearance and all supporting loads simultaneously for each
completed swing, and writes the exact inputs alongside its reports. Required
consecutive qualifying duration is 200 ms, clearance 1 mm, swing load at most
0.1 N, and each supporting load at least 1 N. Additional provisional budgets are
1 mm final body-position error, 0.01 rad maximum tilt, 0.005 rad final yaw error,
no sampled internal contacts, and a final idle phase. Sampling is 20 ms; this is
not a between-sample collision or hardware-transfer certificate.

## Findings

| Configuration / request | Evidence and limitation |
|---|---|
| Default / forward-stop | Ten supported transfers over 24 s pass; stop reaches idle. |
| Intermediate stance / reverse-stop | Ten supported transfers pass; final body error about 0.89 mm. |
| Intermediate stance / forward-stop | Rear-foot transfers fail simultaneous three-foot support. |
| Force preload, gain 0.005 m/(N s), limit 4 mm | Reverse passes; forward remains unsupported in two transfers. |
| Force preload, gain 0.05 m/(N s), limit 4 mm | Forward support worsens; not promoted. |
| Velocity-indexed posture / reverse-stop | Ten transfers pass, reproducing the reverse stance result. |
| Velocity-indexed posture / forward-reverse-stop | Run completes, but transfer 7 has only 60 ms of qualifying support. Not accepted. |
| Velocity-indexed posture / turn-reverse, 1 N planning screen | Stops at 12.70 s: stationary estimate predicts about 0.893 N on one support. |
| Velocity-indexed posture / turn-reverse, 0.5 N planning screen | All ten executed swings pass the unchanged 1 N actual support requirement; final body error 0.97 mm. |

The preload experiment integrates desired-minus-measured vertical load into a
bounded downward foot-position suggestion. It starts only after the swinging
foot unloads below 0.1 N, and releases over the last fifth of lowering. It acts
through the existing position feedback and motors. It does not impose a contact
force. Faster integral response did not cure the gait failure, so increasing that
gain further is not the preferred next step.

Next work should make direction changes feasible as a sequence of stance
transitions, checking reach and executed support together. A gait that works from
its own initial pose does not establish that it can be entered safely from another
gait. Turning, sustained runs, terrain interaction, realtime p95 latency, learned
control, and hardware validation remain outstanding.

## Browser checkpoint and remaining CI work

The versioned aggregate evidence is `online-control-status.json`. On this Intel
Core i9-9980HK Mac, Chrome 152 with ANGLE/Intel UHD 630 rendered the keyboard-driven
24 s episode in 23.9916 s: 1.00035 simulated seconds per wall second. The p95 worker
round trip was 26.71 ms, above the 20 ms target. The p95 animation-frame interval
was 16.67 ms; that measures browser scheduling, not display presentation latency.
This is one flat-floor episode, not a sustained performance certificate.

Both forward and keyboard-sequence captures pass whole-frame native/WASM
comparison at the existing 1e-7 absolute + 1e-8 relative portability budget.
Same-browser replay and reset remain exact. The saved keyboard recording matches
the accepted native turn/reverse run's entire scene, configuration, seed, task and
input events exactly. Keyboard tests include simultaneous direction/turn requests,
key release, focus loss, typing isolation, and the stop button.

The online forward case and UI/registry/runtime checks are added to CI. Whole CI
is not green: the earlier run failed Levitron convergence at 2.76832412287 s and
strict absolute portability for internal detailed-pendulum rotor speeds. A CAD
catalogue test also hard-coded the old component count; this checkpoint replaces
that with unique component identities and required capabilities. No numerical
solver tolerance or detailed-pendulum comparison threshold is relaxed here.
