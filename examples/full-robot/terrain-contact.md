# Browser contact reduction

The experimental **Terrain-contact** preset retains the shared Rust floor and
height-field contact model but omits all link-to-link contact forces. The CAD
geometry, masses, joints, transmissions, effective actuators, controller and
numerical tolerances remain those of the retained reversal crawl. This is an
explicit physical approximation, not a replacement for the detailed model.

## Why this reduction

The [latency profile](latency-profile.md) attributed a nested 26.5% of native
runtime to repeated inter-part collision queries. The tested crawl has no active
inter-link contacts. Performing these queries for every nonlinear derivative
probe is expensive even when each query finds nothing.

`BuildOptions.omit_inter_link_contact` records the reduction in scenes and replay.
The shared registry exposes `collision.omit_inter_link` as a dimensionless 0/1
parameter, default 0. Floor/terrain samples, normal forces, friction and bristle
history still use the existing library implementation. The default remains full
contact. This flag also omits collisions with any obstacle represented as another
link; it is not a general obstacle-contact model.

## Independent collision checks

`Articulated::inter_link_penetrations` queries the compiled sample/SDF geometry
regardless of contact-force settings. It retains authored sample and moving
joint-region exclusions, validates rigid poses and does not update physical state.

The online controller checks both its reference poses and, for the reduced
profile, observed poses at each controller sample. An overlap stops the run as
outside the model's supported operating range. This supplies no impact response
and no between-sample collision guarantee.

The lift-audit tool separately checks every recorded pose when forces are omitted.
The walking checker requires complete audit coverage and zero geometric overlaps;
it cannot pass simply because the reduced simulator emitted no inter-link forces.
Unit tests preserve floor forces and nonzero bristle-history rates under the
reduction while detecting deliberate overlaps, missing poses and invalid transforms.

## Current evidence

`terrain-contact-status.json` preserves results and capture hashes:

- The 24 s forward/reverse sequence passes all nine swings and all 1,201 recorded
  geometry checks. Foot/body positions and motor angles match the retained model
  exactly at reporting times.
- The sustained minute passes 28 swings and all 3,001 geometry checks. Every
  physical/telemetry frame and task transition matches the retained native capture
  exactly, excluding wall timing and the explicit profile flag. Final body error
  is 0.965 mm under the unchanged 1 mm provisional budget.
- The 60 s turn/reverse/stop sequence passes ten swings and every geometry check;
  its sampled foot/body positions and motor angles match the retained profile.
- Restoring the known unsafe 20 mm reverse body shift fails at 10.64 s on the
  front thigh-gear/hip-pulley intersection. Omitting its forces does not hide it.
- All 3,000 sustained native/WASM transitions agree within 1.78e-10 maximum
  numeric difference. Same-host replay/reset are exact. The rendered keyboard
  recording exactly matches the native recipe, seed, actions and task.
- Browser UI tests include the new preset, WASD, release/focus loss, recording,
  replay, reset and narrow layout. Native workspace checks and the WASM build pass.

On the i9-9980HK Mac with Chrome 152.0.7977.76 and Intel UHD 630/ANGLE Metal, the
WebGL-enabled sustained run maintains realtime average speed. Active-motion p95
round trips are **21.64 ms**, still above the 20 ms target. The previous measured
profile was approximately 29 ms; these are individual runs, not a repeated
controlled speedup benchmark. rAF scheduling p95 is 16.67 ms, not a measurement of
display presentation or command-to-visible-response latency.

These are flat-floor commissioning cases at approximately 1.25 mm/s, not normal
walking speed, general teleoperation, uneven-terrain robustness, learned control
or hardware calibration. The name identifies the retained contact formulation;
it does not claim successful terrain walking. Planning/learning and the remaining
realtime/usability requirements of the active goal are still unfinished.

## Reproduce

```sh
node examples/full-robot/prepare_terrain_contact.mjs
cargo build --release -p sim-runtime --example run_environment --example evaluate_lift
target/release/examples/run_environment \
  examples/full-robot/browser-terrain-contact/scene.json \
  examples/full-robot/browser-reversal/config.json \
  examples/full-robot/browser-reversal/task.json \
  examples/full-robot/browser-reversal/sustained.actions.json > runs/terrain-contact.json
node examples/full-robot/check_online_steps.mjs runs/terrain-contact.json runs/terrain-contact-check
node examples/full-robot/check_inter_link_reduction.mjs runs/retained-sustained.json runs/terrain-contact.json
```

Generate `retained-sustained.json` with the same command and the original
`browser-reversal/scene.json`. Use the original `short.config.json` and
`forward-reverse.actions.json` for the short case. The generated
`unsafe-posture.config.json` with `switch-4.4.actions.json` must fail on internal
contact. CI checks the reduced minute against its retained-force counterpart,
independent geometry, browser parity and the unsafe-posture rejection.

After building `sim-web` for `wasm32-unknown-unknown`, package with
`node web/build-viewer.mjs runs/terrain-contact-viewer --environment-only`.
The preset ID is `robot-terrain-contact`; the existing reversal and fixed-crawl
presets remain available. Full CI acceptance remains separate from these focused
local checks.
