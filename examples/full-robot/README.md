# Four-leg robot commissioning

The [two-cycle crawl](crawl-validation.md) now lifts and replaces all four feet
twice through live Rust physics. Its browser preset reaches realtime average
speed in one rendered test; p95 latency, live steering, sustained walking and
learned control remain open. This is the latest controller/viewer milestone.

The active walking/learning delivery and acceptance backlog is in
[learning-plan.md](learning-plan.md). Sampled foot tracking can now use the
shared Rust [tracking tool](../interactive/tracking/README.md). Neither the fast
training model nor a learned walking policy is complete yet.
The [explicit drive definition](drive-backlash-validation.md) separates bearing
clearance from drivetrain lost motion in CAD and both Rust hosts. Its labeled
robot experiment passes the existing preliminary motion screen at 1 ms and
runs in the maintained WASM viewer. Native stepping improves 2.47× versus
0.25 ms within this model; hardware calibration and learned walking remain open.
The [hip timestep investigation](hip-timestep-validation.md) traces a large
portion of the lowering-motion disagreement to an inferred bearing-clearance
value used as motor backlash. An isolated override improves timestep agreement,
but requires a corrected CAD physical definition and hardware validation before
model promotion.
The [Newton convergence investigation](newton-convergence-validation.md) resolves
the refined motor-current discrepancy in the sample-reuse experiment by using
fresh final corrections at unchanged tolerances. Both complete native timestep
comparisons pass strict numerical limits; this is solver validation, not walking
or realtime acceptance.
The [fast-motor screen](fast-motor-validation.md) tests explicit internal motor
reductions. Omitting winding dynamics fails the initial task screen; omitting
rotor inertia has negligible trajectory effect but no measured speed benefit.
Larger timesteps also remain outside the chosen accuracy screen.
The [local motor solve experiment](auxiliary-condensation-validation.md) keeps
the detailed equations and reduces outer mechanical work, but inner derivative
cost offsets the saving. Both robot runs complete; subdivision differences and
electrical discrepancies prevent numerical promotion. A maintained browser
fixture and full viewer regression accompany the default-off implementation.
The [compressed derivative experiment](auxiliary-coloring-validation.md) reduces
inner motor probes from 48 to four where the registered boundaries are
independent. Two full native trajectories match the uncompressed path exactly;
a sequential four-run benchmark measures 1.42× speedup. Parent solver accuracy
and full-robot browser portability remain separate unresolved gates.
The [endpoint correction experiment](endpoint-correction-validation.md) removes
some tiny-step rejections by checking motor corrections in physical state units.
Both complete motions retain the sampled lift gate, but the local/simultaneous
solver discrepancy remains; the option stays experimental.
The [contact-history optimization](contact-history-validation.md) removes
unused inverse-dynamics work and geometry queries from the history update.
Both complete native runs preserve trajectories, contacts and subdivisions
exactly. It retains the existing physics and controller limitations.
The [closure preparation optimization](closure-preparation-validation.md)
removes repeated diagnostic-label work, fixed-unit lookup through kinematics,
and unused SVD vectors. Original equations, rank checks and configurations remain.
The [analytic position experiment](analytic-positions-validation.md) directly
reconstructs certified slider-cranks and transmissions while retaining numeric
rank, tangent, curvature and all original closure checks. Both complete native
timestep runs pass the existing numerical comparison and sampled lift gates.
The [contact cost audit](contact-query-validation.md) adds a finer profile and
records two rejected low-yield contact optimizations. It redirects investigation
toward timestep-sensitive loaded motor motion and fewer whole-robot evaluations.
The [actuator consistency audit](actuator-consistency-validation.md) records a
catalog resistance correction and loaded-motion experiments; it does not yet
establish calibrated actuator behavior or successful stepping.
The [moving joint-region correction](moving-joint-band-validation.md) documents
the CAD contact investigation and a shared runtime coordinate-frame fix.
The [support-gated execution experiment](support-gate-validation.md) adds
checkpointed reference pausing and timeout reporting; active balance control
and accepted stepping remain unfinished.
The [support-load and collision-grid audit](support-load-and-grid-validation.md)
checks whether planned support loads are achievable and diagnoses a false
collision caused by an incorrect nearest-triangle search during CAD export.
The [supported lift and browser workspace](lift-and-viewer-validation.md) records
a 1.68 mm motor-driven foot clearance at two timesteps. The
[incremental session validation](embedded-session-validation.md) adds live
quadruped execution in the browser through the same Rust motor runner, alongside
recorded comparisons and a Rhai fixture. Accepted landing, walking commands and
learned control remain unfinished.
The [mechanism reduction](mechanism-reduction.md) and
[integration experiment](embedded-integration.md) document the current shared
Rust work, including the stiff-force cases that have not passed promotion.

The robot has four repeated leg mechanisms, twelve HX-30HM servos, 105
connectors, four closed knee loops, and eight ideal angular transmissions. Each
leg has an internal belt-driven hip, a 5:1 worm/sector thigh drive, and a
crank/rigid-link/sliding-foot mechanism. The lower foot guide remains fixed.

`baseline/robot.rcad` is the versioned CAD baseline at revision 1357;
`baseline/manifest.json` records its SHA-256 and provenance. The live working
archive in `runs/robot-imports/` can continue changing independently. Reference
videos are in `references/robot/2026-09-05/`. `assembly-contract.json` records
component IDs and calibration assumptions without duplicating geometry.

## Reproduce the floating Rust/Rhai session

Export in a separate process so physical derivation cannot block the CAD UI:

```sh
# Optional accelerator for signed-distance derivation:
cad/.venv/bin/pip install -r cad/requirements-acceleration.txt
PYTHONPATH=cad cad/.venv/bin/python examples/full-robot/export.py --free \
  --out runs/full-robot/floating.simrobot.json \
  --scene runs/full-robot/floating.scene.json
cargo build --locked --release -p sim-runtime --bin sim-session
SIM_SESSION_PROFILE=1 ./target/release/sim-session \
  runs/full-robot/floating.scene.json 10 \
  runs/full-robot/hold.recording.json > runs/full-robot/hold.frame.json
```

`--cad /path/to/robot.rcad` selects a different CAD archive explicitly. Without
`--free`, the export copy fixes the chassis to a bench and its packaged session
disables contact. Neither option edits the source CAD. Derivations are cached;
the model records source hashes, backend versions, and the explicit fixture.
The adjacent `.derivation.json` file records cache statistics.

The packaged session uses `controller.rhai` to hold twelve motor targets at the
imported pose. It uses the same physical runtime, controller contract, and
simulation clock as the browser worker. Each frame advances 2 ms using 0.5 ms
physics steps; ten frames are only 20 ms of simulation. Restore a recording with:

```sh
./target/release/sim-session --replay runs/full-robot/hold.recording.json
```

Replay rebuilds the scene and replays seeded actions, including the Rhai
controller. It is not a constant-time state checkpoint. `controller.py` remains
an older compatibility example; the new workflow runs controllers in Rust/Rhai.

## Acceptance status

The physical export contains 29 merged links, twelve actuators and eight
transmissions. Native commissioning runs detect floor contact. A shallow-contact
stiffness regression and conservative pair filtering improved the measured
20 ms run from 195.7 to 39.6 wall seconds on the development machine. This is
still far too slow for interactive control and does not demonstrate standing.
See `robot-playground-progress.md` at the repository root for current evidence.

The smaller motorized benchmark in `examples/interactive/` verifies native/WASM
agreement, browser worker responsiveness and recording/replay. It does not
validate this full robot. Native/browser rendered playgrounds, WASD locomotion,
and a complete CAD web-bundle export remain unfinished.

Mass, composite layup, nylon grade/process, servo dynamics, transmission losses,
backlash and friction require measurements. Servo internals are counted once in
each motor's mass. Carbon tubes have provisional properties; pins retain their
own material regions. Ideal transmissions do not model worm self-locking or belt
compliance. Current sessions disable modal flexibility explicitly. Automatically
created ideal joint measurements are diagnostic conveniences, not proof that the
CAD declares deployable hardware sensors. The robot is not yet a validated
digital twin or a hardware-ready controller.

Runtime profiling and the measured bottlenecks are documented in
[runtime-audit.md](runtime-audit.md). Use `sim-profile` to repeat the full-robot
measurement before promoting performance changes.

## Foot-motion evidence

`foot-markers.json` defines one physical surface marker per sliding foot for
CAD revision 1357 (hash in `baseline/manifest.json`). Each offset is the exported
collision-mesh vertex with the lowest local Z, breaking ties by X then Y, from
the corresponding merged sliding-foot/crosshead link. Coordinates are metres
relative to that link's exported COM frame. These are fixed surface vertices,
not contact-centroid estimates or newly authored hardware sensors. Re-derive
them if geometry, meshing or mass/COM changes; do not silently reuse with another
CAD revision. The configuration rejects a different declared CAD source hash.
The motion recorder includes the input robot and marker recipe.

`foot-tracking-requirements.json` is a **provisional numerical diagnostic**:
compare all four markers over a 100 ms hold, sampled every 2 ms, with a 1 mm
discrepancy budget. This is not a validated foothold margin, a standing test, or
permission to begin walking-policy training. Compare recordings with identical
commands/sensor sampling but different physics steps; also test full operating
motions and longer runs before promoting a model.

```sh
cargo run --locked --release -p sim-runtime --bin sim-track -- capture \
  runs/full-robot/hold-100ms.recording.json examples/full-robot/foot-markers.json \
  > runs/full-robot/foot-track.json
cargo run --locked --release -p sim-runtime --bin sim-track -- compare \
  runs/full-robot/foot-track.json runs/full-robot/foot-track-refined.json \
  examples/full-robot/foot-tracking-requirements.json
```

Both recordings must cover the required 100 ms interval. The refined evidence
is a separate run with a smaller physics step and the same input timeline.
Raw measurements are not yet available in this schema; follow the tracking
tool's registration/uncertainty procedure to import them without concealing
delay or fitting on the held-out trial.
