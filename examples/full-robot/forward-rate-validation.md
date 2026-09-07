# Slower placement and a false hip contact

The experiment stretches the previous 10 mm forward placement to 1.5 times its
original duration. Its reference lasts 2.4 seconds, with a 2.8 second physical
horizon. Motor gains, controller clocks, mechanics and contact parameters remain
unchanged. The geometric command knots differ by less than 4.8e-15 radians;
only their timing changes. The 100 ms support qualification and 300 ms maximum
pause also remain physical-time requirements.

## Measured behavior before collision correction

| Measurement | 0.25 ms step | 0.125 ms step | 1 ms exploratory step |
| --- | ---: | ---: | ---: |
| Supported lift duration | 270 ms | 180 ms | 330 ms |
| Peak clearance | 2.667 mm | 2.707 mm | 2.692 mm |
| Final world XY foot error | 1.326 mm | 1.259 mm | 1.468 mm |
| Accepted internal contact samples | 37 | 110 | 0 |
| Native wall time for 2.8 simulated seconds | 158.8 s | 217.3 s | 64.2 s |

All three satisfy the existing **sampled lift** requirement of 1 mm clearance,
at most 0.1 N swing load, and at least 1 N on every support foot simultaneously
for 50 ms. The original-rate motion qualified for only 40 ms. At the planned
peak, the slower body's +Y shift undershoot is 1.82 / 1.74 mm, versus 2.53 mm
at the original rate. This supports further timing/weight-transfer investigation;
it does not isolate every contribution from actuator lag, slip and dynamics.

Passing lift is not accepted placement or walking. The two smaller timesteps
differ by up to **1.73 mm** in sampled foot position. The 1 ms run differs from
0.25 ms by **4.03 mm** along the foot path, despite a similar final error
magnitude. It is not promoted. Timings are development measurements with other
work running, not isolated throughput benchmarks.

## Exact CAD evidence

Both smaller steps report contact between `+Y | Hip motor pulley and shaft
125c` and the chassis. At refined-run times 1.33 and 1.34 seconds, the reported
contact point is **0.19991 mm outside** the nearest CAD surface and outside every
target CAD solid. Thus those particular contacts are false positives.

The chassis distance grid has 5.023 mm spacing. Two surrounding nodes have
stored distances of -0.815 and -2.765 mm, yet exact CAD classification says
both are outside, with distances approximately +0.814 and +2.748 mm. Repeating
the exporter's triangle-ray classification identifies the combined chassis mesh
as the source of both incorrect inside flags. That combined mesh is not
watertight. Its 28 separate CAD solids each tessellate to a watertight mesh;
classifying them separately produces the correct outside flags at both nodes.

`solid_collision_meshes` now derives separate closed CAD solid meshes for sign
classification. `signed_distance_grid` accepts these independently of the
surface meshes used for distance magnitudes. Interiors are unioned across
solids. Free sheets contribute unsigned distance but cannot invent an interior;
open solid tessellations fail explicitly. Collision cache keys include the new
sign algorithm. This is reusable CAD derivation, with no Python dynamics and no
new robot-specific runtime. Legacy mesh-only regeneration remains explicitly
available for older experiment recipes.

`chassis-sign-correction-experiment.json` regenerates only the chassis grid and
its derivation metadata. The grid dimensions/cell size, other geometry, motor
configuration and physical parameters remain unchanged. The observed nodes now
have the correct signs. **Coarse-grid interpolation still differs from exact
surface distance**, so this does not establish continuous clearance or eliminate
all possible collision error.

Both corrected physical runs now finish without any accepted internal contacts.
Their supported-lift durations remain 270 / 180 ms. Relative to the prior grid,
the largest sampled foot-position changes are only 0.00103 / 0.00612 mm; the
two timesteps still differ by 1.729 mm. The false contact was a real export bug,
but it does not explain the dominant placement sensitivity. Native wall times
are 176.2 / 243.7 s in concurrently executed development runs. Browser parity
for the corrected model passes all 281 frames with exact replay. The original
and corrected models have separate labeled presets. Body/foot tracking and balance remain the next
control work, rather than attributing the residual error to this tiny contact.

The shared captured-geometry auditor now has `--points-only`. It preserves exact
contact-point/solid checks while skipping expensive whole-part distance queries;
its report explicitly marks those pair distances unmeasured. A full pair-distance
job was deliberately cancelled after focused probes established the local error.
The focused result is not a whole-assembly clearance certificate.

## Browser milestone

`Quadruped · slower 10 mm placement` is a live Rust/WASM preset for the original
slower experiment, clearly labeled with its unresolved contact and timestep
limitations. All **281 native/browser frames** pass the 1e-7 absolute entry
portability tolerance; the largest difference is 5.70e-9 N. Replay and reset are
exact. All **14 UI checks** pass, including the new preset, prior controllers,
selection/fit, support status, recording/replay, timeout recovery and narrow
layout. The browser takes 183.9 wall seconds for 2.8 simulated seconds, with a
maximum 2.61 second worker chunk. Rendering remains responsive; physics is not
realtime. This bundle does not silently substitute the pending corrected grid.

## Reproduce

Prepare the preceding forward-placement experiment, then:

```sh
cargo build --locked --release -p sim-runtime --example plan_marker_motion --example integrate_embedding --example compare_motion --example evaluate_lift --example compare_embedding
node examples/full-robot/prepare_forward_rate.mjs
```

Run `integrate_embedding` with `forward-slow/scene.json` and each prepared
`config.json`, `refined.config.json`, and `coarse.config.json`. Use `compare_motion`
against `plan.json`, `foot-markers.json`, and reference link
`Robot | Chassis and hip mounts`. Use `evaluate_lift` with the prepared
`lift-requirements.json` and `--simulation-time`. Compare complete executions
using `compare_embedding` and the same marker definitions.

```sh
PYTHONPATH=cad cad/.venv/bin/python examples/full-robot/audit_captured_geometry.py \
  examples/full-robot/baseline/robot.rcad \
  runs/full-robot/learning/forward-slow/scene.json \
  runs/full-robot/learning/forward-slow/refined.execution.json \
  '+Y | Hip motor pulley and shaft 125c' 'Robot | Chassis and hip mounts' \
  1.33 1.34 --points-only --probe-sdf-cells
PYTHONPATH=cad cad/.venv/bin/python examples/full-robot/rederive_collision_grids.py \
  examples/full-robot/chassis-sign-correction-experiment.json \
  runs/full-robot/learning/forward-slow/chassis-solid-sign.scene.json
cargo test --locked -p sim-runtime --test motion_tracking
PYTHONPATH=cad cad/.venv/bin/python -m pytest cad/tests/test_physical.py cad/tests/test_capture_geometry.py cad/tests/test_export.py -q
```

Build WASM and package as described in `web/README.md`, then run
`web/tests/embedded.mjs` for `robot-forward-slow` against its native execution,
and `web/tests/viewer.mjs`. `summarize_forward_rate.mjs` collects compact native,
geometry and browser results and artifact hashes in `forward-rate-status.json`.
The physical runs use the preserved landing-checkpoint native runner; its source
manifest is separately identified. The CAD baseline remains unchanged.
