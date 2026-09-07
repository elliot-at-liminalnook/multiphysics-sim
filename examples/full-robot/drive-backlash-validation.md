# Explicit drive definition and larger-step robot experiment

The CAD export now separates radial bearing clearance from rotational drive
backlash. The full robot has a labeled v4 experiment that passes the existing
provisional motion screen at 1 ms and is runnable in the WASM viewer. This is
not calibrated hardware behavior, accepted walking, or a completed training model.

## Physical definition and compatibility

New `simrobot` v4 joints carry `physics.drive_backlash`: full `width_rad`,
`provenance` (unmeasured, estimated, measured or derived), a nonempty reference,
and optional uncertainty in radians. New geometric inference leaves the width
unknown. Both runtime hosts reject unknown drive values for motorized joints.
The old bearing-clearance angle remains available as a diagnostic.

CAD commands, REST and the inspector use the same validation and undo path.
The inspector shows unknown explicitly; entering a number declares an estimate.
A fitted result updates the drive value. A subsequent manual edit preserves
the older fit as superseded evidence, and one undo restores both the property
and its fitted record. Invalid requests leave the document unchanged.

Existing v3 scenes without the new record preserve their legacy behavior.
The full robot's v4 compatibility scene explicitly authors the old numbers as
estimates and reproduces every frame, terminal frame, contact impulse, event,
subdivision and solve diagnostic in the prior 2.8 s native capture exactly.
No existing CAD document or running viewer session was reloaded or overwritten.

## Robot assumption and scope

The new experiment sets **additional lost rotation at each of twelve drive
connections to an explicit estimated zero**. This idealizes those attachments;
it does not establish that the real motor, belt or gearbox has zero lost motion.
The motor gearbox values remain as previously authored (also uncalibrated).
Bearing clearance, friction, masses, geometry, controller gains and other
parameters remain unchanged. The preparation script verifies and records each
override, including the legacy number and its source label. These values can
be promoted back through CAD `set_joint_physics` once justified.

The CAD's broad joint source label is not sufficient parameter provenance:
one foot joint says `declared` after an unrelated override even though its
backlash number still equals the geometric heuristic. The new drive record
avoids relying on that broad label.

## Complete motions and numerical screening

All six 2.8 s runs complete at nominal steps 2, 1, 0.5, 0.25, 0.125 and
0.0625 ms. Every run passes 360 ms of sampled supported lift; peak foot
clearance ranges from 3.171 to 3.174 mm. The 0.5 ms case has one rejected
trial; the other cases have none. Original closure rows remain checked.

Successive sampled foot-position differences decrease with refinement:

| Comparison | Maximum across four foot markers |
|---|---:|
| 1 / 0.125 ms | 0.120 mm |
| 0.5 / 0.25 ms | 0.050 mm |
| 0.25 / 0.125 ms | 0.034 mm |
| 0.125 / 0.0625 ms | 0.019 mm |
| 1 / 0.0625 ms | 0.131 mm |

The 1 ms candidate is checked against the 0.0625 ms reference using the
previously declared preliminary screen: maximum/RMS foot difference
0.5/0.2 mm, independent motor-angle difference 0.005 rad, per-foot impulse
difference 1% with a 0.001 N·s absolute floor, and supported lift. This is a
simulation-only screening step, not proof of sufficient hardware accuracy.
The full machine-readable results are in `drive-backlash-status.json`.

Electrical transients remain sensitive: sampled current differs by up to
0.173 A between 1 and 0.0625 ms. Pointwise velocity, torque and heating
differences are reported rather than hidden behind the foot-position result.
No electrical-energy, thermal, contact-event accuracy or sim-to-real promotion
follows from this screen. Reports sample at 10 ms and do not prove bounds
between samples.

## Performance and browser delivery

An isolated native ABBA comparison uses the same v4 model and binary at
0.25 and 1 ms, with profiling disabled. Each repeat exactly matches its own
captured trajectory and diagnostics. Mean stepping times are 35.951 and
14.568 s for 2.8 simulated seconds: **2.468× faster**, or **0.192 simulated
seconds per wall second**. This remains approximately 5.2× slower than realtime.
The benchmark records its Intel i9-9980HK/macOS host and excludes process
startup, serialization and rendering. These figures compare timesteps within
this physical model, not equivalent hardware accuracy against the old model.

At 2 ms the firmware scheduler still splits the motion at each 1 ms motor
update. Both nominal settings take 2,800 accepted integration segments and
produce nearly identical motion. Further nominal-step increases alone cannot
remove that work without a different integration strategy or actuator model.

The **Quadruped · explicit drive definition** browser preset runs the same
Rust controller and configuration (profiling disabled). All 281 native/browser
frames pass the 1e-7 entry diagnostic; the maximum discrepancy is 3.31e-10 N.
Replay and reset are exact. Loading an unknown drive property is rejected
without replacing the current session. The v4 motor fixture also matches exactly.
All 24 viewer checks pass, including the new robot preset and fixture,
selection/camera controls, recording/replay, error recovery and narrow layouts.
Browser wall time from concurrent usability checks is not a latency benchmark.

## Reproduction

```sh
cargo test --locked -p sim-runtime --test drive_backlash --test embedded_session
cad/.venv/bin/python -m pytest cad/tests/test_drive_backlash.py cad/tests/test_physical.py -q
cargo build --locked --release -p sim-runtime --example integrate_embedding --example compare_embedding --example evaluate_lift
node examples/full-robot/prepare_drive_backlash_model.mjs
# Run each named config against that directory's scene.json, preserving output.
target/release/examples/integrate_embedding \
  runs/full-robot/learning/drive-backlash-definition/scene.json \
  runs/full-robot/learning/drive-backlash-definition/1000us.config.json \
  > runs/full-robot/learning/drive-backlash-definition/1000us.execution.json
node examples/full-robot/summarize_drive_backlash_model.mjs
```

The summary verifies captured evidence, source/configuration hashes and the
existing screening limits. Its complete input set is preserved in the delivery
archive, including source and native runner; ignored run directories alone are
not the baseline.

Next, evaluate a task-appropriate effective actuator model and the remaining
per-segment dynamics cost on this explicit physical definition. Keep the current
experiment available in the browser while adding planned step sequences and
the learning pipeline. Hardware reversal and loaded tracking measurements,
landing/slip validation and deployable sensing remain necessary work.
