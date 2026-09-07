# Effective servo experiment

This browser-profile candidate replaces the electrical and firmware actuator
integration with the registered `robot.effective_servo` Rust component. It retains
the detailed articulated mechanism and contact solve. It is not yet a walking
controller, a realtime acceptance result, or a calibrated hardware model.

The component requests `stiffness * angle_error - damping * shaft_speed` torque,
limited by CAD stall torque and a linear motoring torque-speed envelope using CAD
no-load speed. Opposing motion can be braked at stall torque. External loads can
drive the shaft above no-load speed; it is not an artificial velocity clamp.

`derive_effective_servo.mjs` binds each coordinate to exactly one CAD motor. It
converts estimated voltage-output firmware gains to effective mechanical gains
using `torque_constant / resistance`. The present CAD values give approximately
33.7 Nm/rad and 0.674 Nm s/rad. These are a static initial estimate, not measured
loaded servo response. CAD torque and speed specifications remain uncalibrated.

Omitted effects include electrical and thermal evolution, internal rotor storage,
firmware delay and sampling, quantization, gearbox compliance/backlash, and a
separate identified gearbox friction/efficiency law. The effective model reports
shaft torque and speed, and does not fabricate motor current or temperature.
Its teacher task explicitly removes current observations. The source CAD artifact
is unchanged; generated profiles record source hashes and assumptions.

Reproduce after building `run_environment` in release mode:

```sh
node examples/full-robot/derive_effective_servo.mjs
target/release/examples/run_environment runs/full-robot/learning/effective-servo/scene.json runs/full-robot/learning/effective-servo/config.json runs/full-robot/learning/effective-servo/task.json > runs/full-robot/learning/effective-servo/native.json
```

Compare whole trajectories and timestep sensitivity before promoting this profile.
The detailed 2.8 s single-foot reference is a comparison experiment, not a full gait.

The browser candidate uses a 20 ms step and 20 ms controller period. Gate events
are rounded to that controller grid (the return guard begins at 1.66 s instead
of 1.65 s). Its absolute Newton residual tolerance is explicitly 1e-8; the detailed
reference retains 1e-10. The initial WASM screen at 1e-10 failed on a mechanical
velocity residual of 1.165e-10. Numerical tolerance is an explicit browser-profile
tradeoff and must still pass trajectory/refinement comparisons; it is not a claim
that raw solver residuals directly measure foot-position accuracy.

The first 20 ms headless Chrome screen completed 140 task transitions (2.8 s)
in 1.633 s of worker round trips, or 1.71× realtime. p95 was 22.3 ms, above the
20 ms target. Native/WASM maximum numeric difference was 1.33e-10; replay/reset
were exact. Versus the 1 ms, 50 Hz-controller reference, maximum sampled foot
difference was 0.627 mm and body position difference was 0.414 mm. These short
captures do not establish sustained walking, rendering latency, between-sample
impact behavior, disturbance recovery or sim-to-real accuracy.

The versioned `browser-effective-servo/` recipe is the viewer's experimental
preset. Regenerate it with:

```sh
node examples/full-robot/derive_effective_servo.mjs examples/full-robot/browser-effective-servo 0.02 0.02
```

`effective-servo-status.json` records evidence and remaining limitations. The
comparison script accepts two `run_environment` captures and a report path.
Its comparisons use actual simulated poses and CAD-local foot markers.
