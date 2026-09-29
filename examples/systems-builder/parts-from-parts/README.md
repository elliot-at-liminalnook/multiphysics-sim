# Parts from parts

A DC motor subsystem (`dc_motor_12v`) inside gearmotors (`worm_gearmotor_30`,
`planetary_gearmotor_30`, same interface) inside a joint actuator
(`joint_actuator`), used by two systems:

- `winch.system.json`: a gearmotor winding a 2 kg load;
- `leg.system.json`: a joint actuator raising a thigh against gravity, which
  comes from the authored part `library/parts/pendulum_gravity.part`.

Both are generated through the shared commands and library API by
`cargo run -p sim-runtime --example build_parts_from_parts`.

## One edit, every system

Edit the motor anywhere it is placed, then **Publish to library** (inspector)
or `system_publish`. Library files that bundle the motor (the gearmotors, the
actuator) get the new copy and a new version. Every system that imported them
shows **Library updates** and takes them with **Update from library** (one
undoable edit). `sim-runtime --test parts_from_parts` does this end to end:
k 0.012 → 0.015 slows the winch from 26.7 to 22.4 rad/s and changes the leg.

## Realtime

Both systems carry a realtime profile (every part's realtime model, 2 ms
step) with a published error bound, checked natively
(`--test realtime_profiles`) and in Chrome
(`node web/system-realtime-check.mjs`; open the runner with
`node web/build-system-builder.mjs` then
`node runs/interactive/system-builder/serve-viewer.mjs runs/interactive/system-builder 4174`).
