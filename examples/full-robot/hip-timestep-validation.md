# Hip timestep and inferred-backlash investigation

The next physical-model priority is to distinguish radial bearing clearance
from lost rotation in a drivetrain. This experiment does not establish zero
hardware backlash, accepted walking, a training model, or realtime performance.

## What was compared

The unchanged analytic-position controller recipe was advanced from its original
initial state at nominal steps of 0.25, 0.125 and 0.0625 ms. Each run captures
0.7–1.65 s every 1 ms using the production Rust `EmbeddedSession`. The command
advances every preceding step; it does not jump to the observation window.
All three windows complete without error. The first two reproduce all 96
overlapping full frames from the earlier complete runs exactly.

The moving -Y foot differs by a maximum 2.570065 mm between the first two
steps, and 1.406103 mm between the finer pair. RMS differences in this window
are 0.905060 and 0.534111 mm. The hip's sampled backlash modes disagree at
305 and 72 of the 951 observations, respectively. This is endpoint evidence;
the recorded mode-change intervals are not exact event times. Feedback
observations retain their own policy timestamp, distinct from the frame time.

## Physical provenance finding

`cad/robocad/physical.py::joint_physics` derives `physics.backlash` as
`clearance / max(COM lever length, 0.005 m)`. For the -Y hip motor joint, the
0.00015 m inferred hole/shaft radial clearance and 0.005255864 m lever produce
0.028539551 rad (1.635°). The joint marks its source as `inferred`.

`cad_motor_unit_parameters` adds this value to the motor gearbox backlash.
The authored motor gearbox value is zero; the inferred joint value supplies
the entire effective gap. `MotorUnit` interprets the sum as full rotational
gap width, with engagement at either half-width boundary. Inside the gap it
retains 5% of the coupling damping and no coupling spring torque.

A radial clearance estimate is not by itself evidence of rotational lost
motion through the servo/pulley attachment. Its conversion through the child
COM lever requires justification. Also, the zero motor gearbox value is not
a measured hardware result: the motor notes identify backlash and other
internal parameters as estimates pending identification.

## Isolated sensitivity screen

A separate scene changes only the -Y hip joint's inferred `physics.backlash`
from 0.028539551 rad to zero. The script records the exact before/after path,
reason, source hashes and promotion requirement. Other joints, CAD bearing
clearance, friction, geometry, masses, motors, controller gains and timing are
unchanged. The baseline CAD and original scene are preserved.

| Nominal timestep pair | Original maximum -Y foot difference | Override maximum | Override RMS |
|---|---:|---:|---:|
| 0.25 / 0.125 ms | 2.570065 mm | 0.183616 mm | 0.101745 mm |
| 0.125 / 0.0625 ms | 1.406103 mm | 0.030240 mm | 0.014891 mm |

All three override windows finish. This establishes strong sensitivity to
that single modeling assumption under this controller. It does not isolate
all consequences of the change: deleting the gap also changes the coupling
branch and removes its engagement events. These are different physical models,
so better timestep agreement is not proof of better hardware accuracy.
No speedup benchmark, full-motion acceptance or controller promotion is claimed.

## Reproduction and retained tooling

```sh
cargo build --locked --release -p sim-runtime --example capture_embedded_window
node examples/full-robot/prepare_hip_timestep_trace.mjs
# Repeat with 125us and 62p5us:
target/release/examples/capture_embedded_window \
  runs/full-robot/learning/point-feedback/scene.json \
  runs/full-robot/learning/hip-timestep/250us.config.json \
  0.7 1.65 0.001 > runs/full-robot/learning/hip-timestep/250us.window.json
node examples/full-robot/analyze_hip_timestep_trace.mjs
node examples/full-robot/prepare_hip_backlash_screen.mjs
# Repeat the capture command for all three configs in hip-backlash-screen,
# using that directory's scene.json and writing its corresponding windows.
node examples/full-robot/analyze_hip_timestep_trace.mjs \
  runs/full-robot/learning/hip-backlash-screen
```

The generic capture command records model/component metadata in Latest mode,
enforces aligned bounded observation windows, labels window versus whole-motion
completion, and reports latched runtime failures. It adds no physics equations.
Nine session tests and its sampling unit test pass. The real CLI also matches
four independently captured fixture frames, rejects five invalid requests,
and correctly labels a successful partial window. These checks are added to CI.

## Next decision

Correct the shared CAD/export contract so geometry-derived bearing play cannot
silently stand in for drivetrain backlash. Preserve and label legacy data;
require explicit provenance for drivetrain lost motion, and provide a repeatable
loaded reversal measurement. Then rerun complete motions, contact/closure and
task gates, timestep refinement, and native/browser checks before exposing a
new controller/model preset. Test larger steps on that physically defensible
model. Do not calibrate by selecting the value that makes the solver easiest.
