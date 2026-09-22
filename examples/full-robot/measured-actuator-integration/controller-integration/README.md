# CAD-selected FPGA controller in the quadruped

The shared incremental session and controller environment now execute the
registered FPGA integer controller from CAD actuator profiles. Encoder origin,
polarity, sample phase and initial reference are explicit per-motor bindings.
Each controller checks the declared implementation hash against the same source
used to generate FPGA RTL. Missing mappings and silent catalog/effective-servo
fallbacks fail visibly.

## Verified evidence

- `runtime-tests.log`: 28 Rust tests pass, including profile resolution, model
  round trips, controller/plant response, replay across host chunks and matching
  environment/headless physics under changing commands.
- `cad-feedback-tests.log`: CAD command/API validation, undo/redo and archive /
  physical-export round trips retain the explicit feedback bindings.
- `collision-regression-tests.log`: 21 CAD checks pass. The retained thin-solid
  regression verifies CAD membership, cache reuse and unchanged mass/geometry.
- `wasm-check.log`: the shared runtime compiles for `wasm32-unknown-unknown`.
  This is not browser execution or realtime qualification.
- `cad-receipt.json`: twelve controller-bound motors in a separately saved and
  reloaded CAD revision; all 114 geometry entries are unchanged. Encoder frames
  remain explicit estimates and physical motor identities remain unassigned.
- `robot-controller.simrobot.receipt.json`: full 29-link / 105-joint / twelve-motor
  export and native profile resolution. Collision-sign provenance is retained.
- `nominal-window.json` and `half-step-window.json`: all twelve simulated motors
  run through 50 ms, at 0.15625 ms and 0.078125 ms steps, using historical seed
  2301. Both windows complete without runtime errors.

These windows used the scene's default zero motion commands, not the historical
walking command packet sequence. They verify motor/controller integration, not
walking. The subsequent [gait pilot](../gait-generation/README.md) explicitly
reintroduces the packet schedule for locomotion experiments.

`numerical-check-request.json` fixes the short-window budget before comparison:
0.0264° RMS per motor, one tenth of the existing bench criterion. All twelve pass.
The largest RMS difference is 0.002092°, peak difference 0.004603°, and sampled
PWM commands match. See `numerical-comparison.json`. This checks two timestep
levels over a short interval; it establishes neither convergence order nor
sim-to-real accuracy. The earlier default-seed window and rejected setup checks
are also retained.

## Comparison scope and assumptions

The historical Rhai gait, numeric policy parameters, initial references, world,
uncertainty settings and 300-second requested horizon are retained. Only the
first 50 ms has run here. `comparison-recipe.json` records controller, provenance
and solver-adapter changes. Profile control uses the existing event integrator;
the mechanical-only restart adapter cannot be selected simultaneously.

The preserved gait had twelve explicit zero-backlash drive-connection estimates
which are still unmeasured in CAD. `comparison-overrides.json` retains those
historical assumptions as experimental overrides, separate from motor-internal
losses. They have not been promoted as measured CAD properties. The frozen gait
and original CAD archive remain unchanged.

Controller limit 350/1000 is the provisional profile's explicit experiment
setting. The window is not a maximum-speed or full-drive characterization.
Voltage and temperature remain imposed boundaries. Current, torque and power
are uncalibrated predictions; shared battery/branch loading is not integrated.

## Next work

Integrate CAD-authored power branches and battery state in the same motor solve,
complete viewer comparisons / accepted-profile promotion, improve motor fits
without changing the accuracy gate, then run complete unchanged-gait comparisons
and browser fidelity/performance qualification. No realistic sustained walking
speed or accepted motor calibration is claimed by these artifacts.

## Reproduce the numerical windows

Build `capture_embedded_window`, then run:

```sh
target/debug/examples/capture_embedded_window provisional.scene.json provisional.config.json 0 0.05 0.005 --seed 2301
target/debug/examples/capture_embedded_window provisional.scene.json half-step.config.json 0 0.05 0.005 --seed 2301
```

Use this directory's absolute scene/config paths when invoking the binary from
the repository root. `cad/scripts/export_simrobot.py` provides cached, read-only
CAD export with source hashes and native profile validation; the authoring recipe
is `cad/scripts/prepare_profiled_controller_revision.py`.
