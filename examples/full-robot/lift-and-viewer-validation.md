# Supported lift diagnostic and browser workspace

The 5 mm requested lift now clears the floor under the modeled motor dynamics.
This is a sampled lift result, not an accepted complete step, calibrated hardware
model, or learned walking policy. It follows the failed 3 mm lift in
[support-load-and-grid-validation.md](support-load-and-grid-validation.md).

## What changed and what was measured

The reference retains a 16 mm body shift, a -0.35 rad initial foot-crank posture,
and an explicit -15.5 mm initial base translation. The lift request increases
from 3 to 5 mm. The reference base trajectory is not imposed on the simulated
robot: sampled servo firmware, registered motor equations, linkages and contact
determine its actual movement. Supply voltage and winding temperature remain
imposed boundaries; calibrated battery, thermal and hardware sensor behavior is
not established by this experiment.

The shared Rust `lift` module requires simultaneous geometric clearance, swing
foot unloading and load on three named supporting feet for a consecutive span.
The recipe requires at least 1 mm clearance, at most 0.1 N absolute swing-foot
vertical force, and at least 1 N on each support for 50 ms. The evaluation window
is 0.5–1.1 s with no more than 10 ms between samples. These are explicit provisional
diagnostic margins. Missing forces, gaps, invalid times and isolated qualifying
peaks cannot establish success. The check says nothing about unsampled intervals.

The first 5 mm run passed those conditions for 120 ms but reported internal worm
contact. A read-only B-rep audit of the reported point at 0.74 s places it outside
all target solids, 0.5002 mm from the nearest worm enclosure surface. The entire
worm/enclosure pair nevertheless has zero unsigned minimum distance: that point
audit does not prove the whole pair is separated. No contact was disabled.

The four hip-assembly distance grids were regenerated using the shared corrected
triangle-AABB exporter, retaining geometry, grid domains and contact rules. Grid
node changes reach 5.77–7.11 mm; export takes about 19–22 s per assembly. The new
motor runs report no internal contacts at accepted continuous endpoints. This
corrects the inspected proxy error; remaining coarse-grid and CAD tolerances
still limit clearance claims.

| Corrected thigh + hip grids | 0.25 ms step | 0.125 ms step |
| --- | ---: | ---: |
| Simulated duration | 1.6 s | 1.6 s |
| Stepping wall time, one development-CPU run | 92.46 s | 128.62 s |
| Peak selected-foot clearance | 1.6817 mm | 1.6821 mm |
| Consecutive qualifying sampled span | 120 ms | 120 ms |
| Qualifying span begins | 0.80 s | 0.80 s |
| Internal accepted-step contact observations | 0 | 0 |

Aligned world foot-marker differences reach 0.03112 mm, with no sampled contact
pair mismatches. This does not mean every physical quantity agrees: sampled
current differs by up to 0.0495 A and torque by 0.0298 N m; an ordered backlash
event differs by up to 22.48 ms. Firmware ticks agree to floating-point precision.
The full diagnostic retains contact impulses and event comparisons. Neither
runtime is realtime, and two timestep runs do not establish a converged physical
reference. Landing quality, support slip, dynamic stability, full energy balance,
hardware transfer and learning gates remain open.

Recompiling the geometric reference against the corrected hip grids also passes
its sampled checks. The executed body-relative foot paths still differ from that
reference by up to 4.632 mm over the program. A successful brief lift therefore
does not establish sufficiently accurate placement for a planned foothold.

## Reproduce

First reproduce the explicit catalog and thigh-grid corrections using the linked
previous audit. Then regenerate the additional four hip grids:

```sh
PYTHONPATH=cad cad/.venv/bin/python examples/full-robot/rederive_collision_grids.py \
  examples/full-robot/hip-collision-grid-correction-experiment.json \
  runs/full-robot/learning/corrected-thigh-hip-grids.scene.json
cargo build --locked --release -p sim-runtime --example integrate_embedding \
  --example evaluate_lift --example compare_embedding
target/release/examples/integrate_embedding \
  runs/full-robot/learning/corrected-thigh-hip-grids.scene.json \
  examples/full-robot/mechanical-servo-load-16mm-lift-5mm.json \
  > runs/full-robot/learning/hip-grid-lift-5mm-execution.json
target/release/examples/integrate_embedding \
  runs/full-robot/learning/corrected-thigh-hip-grids.scene.json \
  examples/full-robot/mechanical-servo-load-16mm-lift-5mm-refined.json \
  > runs/full-robot/learning/hip-grid-lift-5mm-refined-execution.json
target/release/examples/evaluate_lift \
  runs/full-robot/learning/corrected-thigh-hip-grids.scene.json \
  runs/full-robot/learning/hip-grid-lift-5mm-execution.json \
  examples/full-robot/single-foot-lift-requirements.json
target/release/examples/compare_embedding \
  runs/full-robot/learning/hip-grid-lift-5mm-execution.json \
  runs/full-robot/learning/hip-grid-lift-5mm-refined-execution.json \
  examples/full-robot/foot-markers.json
```

`evaluate_lift` returns a diagnostic JSON report: callers must inspect
`report.passed`; exit zero alone means the input was evaluated successfully.
Historical captures predate independent world recording, so their declared
original scene and hashes are required. Newly generated embedded captures also
record the world. The associated status JSON preserves result summaries and
input/source hashes; generated `runs/` files are evidence, not the CAD baseline.

## Browser delivery and remaining integration

[The browser workspace](../../web/README.md) now provides robot part selection,
fit controls, a scrub timeline, contact arrows and requested/actual joint angles.
Its default recorded experiment uses the corrected grids above. The failed 3 mm
attempt remains available for comparison. A separate live motorized pendulum
uses the existing Rust/Rhai session through WASM, including target adjustment,
reset, input recording and replay. Modes and limitations are labeled in the UI.

Real-browser checks cover both recorded robot interaction and live fixture
control, replay, failed-load recovery, cancellation, desktop transport visibility
and narrow-screen layout. Two focused Rust lift tests pass. The live fixture UI
test is added to browser CI alongside the existing numerical parity tests; this
records local test results, not a claimed remote CI run.

The next runtime work must extract the newer embedded motor orchestration from
`integrate_embedding` into a shared incremental session used by headless and
WASM hosts. Preserve firmware state, hybrid event history, implicit workspace,
motion-clock state and exact simulation time across calls. Verify chunked versus
uninterrupted execution before offering live quadruped controls. A recorded
robot view and a live fixture are useful groundwork, but do not satisfy that
full-robot requirement or the teacher/student learning objective.
