# Support-load feasibility and CAD distance-grid correction

This investigation follows the failed support-gated motion, not a successful
step. A paused reference cannot redistribute weight when the reference posture
itself asks too little load of the third supporting foot. Larger posture changes
then exposed a false collision in the CAD-derived distance grid.

## Static load checks

The shared Rust support module now solves vertical force and moment balance for
three explicitly selected point supports. Negative forces remain visible as an
infeasible unilateral-support request. A second function projects the COM to the
nearest point satisfying declared minimum loads. Its result is a planning target,
not an applied body translation or evidence of actual contact.

The marker planner accepts phase-specific minimum loads and rejects infeasible
references at its existing command knots and midpoints. Phase boundaries must
lie on that inspection grid; a requirement cannot silently fall between samples.
The checks retain original linkage closure, authored limits and internal contact
inspection. They do not establish continuous clearance, dynamic balance, friction
capacity, loaded actuator tracking or feasibility on uneven terrain.

At 0.8 s in the previous motor-driven 5 mm body-shift experiment, the assumed
tripod load solution is approximately 19.700, **0.350**, 18.957 N for +X, +Y, -X.
With feet held fixed, its nearest COM correction for at least 1.5 N per support
is about +9.314 mm in world Y. This is a COM shift, not an equal base displacement:
the robot's moving limbs also contribute mass. The 1.5 N value is an explicit
diagnostic margin above the earlier 1 N execution gate, not a calibrated limit.

The original 5 mm geometric reference fails this requirement at 0.5 s with only
0.5035 N predicted on +Y. Original-bound 15/20 mm candidates fail local placement
at 0.35/0.33 s. Expanded search bounds with the original -0.2 rad crank stance
still fail at 0.36/0.34 s, reaching the -Y foot extension search bound. These are
local-search failures, not proof of global physical unreachability.

## Collision error and shared exporter fix

Starting all foot cranks at -0.35 rad was rejected at time zero: the runtime
reported 0.3935 mm internal penetration between the -Y sliding crosshead assembly
and the link named `-Y | Thigh sector gear`. That rigid link contains eight CAD
members, including the carbon-fiber guide tube and fixed lower guide.

Read-only B-rep inspection at the captured pose finds the contact point outside
every target solid. Its nearest actual surface is 1.5579 mm away on the guide
tube. All inspected member pairs have positive unsigned distances; the minimum
is 0.1000 mm at the sliding rod / lower guide. This proves a false reported
contact at this pose, not continuous or hardware clearance over the motion.

The old exporter chose only eight nearest triangle **centroids** when refining
distance. Long faces can be near a point while their centroids are far away.
A small closed 200 x 2 x 2 mm beam with nearby small boxes reproduces the bug:
the old algorithm reports -4.5 mm at a point whose known distance is -1 mm.
The regression also checks a point outside the beam.

The shared exporter now uses the installed trimesh triangle-AABB candidate
search and closest-point calculation, in bounded batches. Every possible nearest
triangle remains eligible. This corrects distances to the tessellated surfaces;
solid membership still uses the existing deterministic per-member ray tests.
The collision cache includes the new algorithm identity. The obsolete custom
point-triangle helper has been removed.

The captured-pose audit can inspect the eight surrounding grid nodes using
`--probe-sdf-cells`. It reports solid classification and distance to CAD faces
separately, since unsigned distance to a solid is zero for an interior point.
Two old nodes around the false contact read -3.595 and -10.230 mm; both are only
about 0.3275 mm inside the actual tube wall. Interpolating the exact CAD boundary
distances at all eight nodes yields +2.0677 mm, rather than the old negative
value. Remaining grid interpolation and tessellation errors are still explicit:
the target grid has 6.689 mm cells and is not an exact CAD collision oracle.

Interior magnitudes within overlapping mesh members are distances to member
surfaces, not necessarily to the boundary of their union. Non-watertight meshes,
ray classification, contact sampling density and unresolved clearances still
need their own validation. This correction does not certify the entire model.

## Reproduction and checks

The CAD baseline is unchanged (revision 1357; SHA-256
`2fc4523f1fefa5ff3530f1d6814ec4f1a9e5c345b4ce8a924686cd654e3c0589`).
`collision-grid-correction-experiment.json` records exact input CAD/scene hashes,
four named thigh-assembly links, tessellation tolerance and algorithm. Regeneration
preserves input grid sizes, contact samples, exclusions and dynamics parameters.
Other link grids remain from the old export in this isolated experiment.

```sh
PYTHONPATH=cad cad/.venv/bin/python examples/full-robot/rederive_collision_grids.py \
  examples/full-robot/collision-grid-correction-experiment.json \
  runs/full-robot/learning/corrected-thigh-grids.scene.json
cargo run --locked --release -p sim-runtime --example audit_embedding -- \
  runs/full-robot/learning/corrected-thigh-grids.scene.json 3 \
  examples/full-robot/load-stance-retraction-sweep.json
PYTHONPATH=cad cad/.venv/bin/python examples/full-robot/audit_captured_geometry.py \
  examples/full-robot/baseline/robot.rcad \
  runs/full-robot/learning/catalog-stall-consistent.scene.json \
  runs/full-robot/learning/load-stance-retraction-audit.json \
  '-Y | Sliding foot crosshead' '-Y | Thigh sector gear' 0 --probe-sdf-cells
```

Five focused CAD tests pass, including an analytic distance regression, captured
transforms, transported probes and affine grid interpolation. Four Rust support
tests and eight planning tests pass, including force balance, minimum-load COM
projection, rejected infeasible support requests and missed-phase protection.
The WASM library check passes (one existing unused `rotor_speed` warning).
Existing CAD and browser CI jobs include these test files/modules; no CI run is
claimed here. Timing of CAD rederivation is export cost, not simulation throughput.

Robot replays and source/artifact hashes are recorded in
`support-load-and-grid-status.json`. Active weight-transfer feedback, a successful
motor-driven step, calibration, teacher/student training, interactive policy
delivery and realtime throughput remain unfinished.

## Corrected-grid reference experiments

All four selected grids regenerated successfully in 46.7, 52.1, 45.5 and 47.2 s
respectively. The largest node changes are about 30 mm, despite preserving grid
domains. The old retracted-pose contact disappears at all three inspected times
(0, 0.01, 0.02 s); the exact CAD audit, not disappearance alone, supports this fix.

With the corrected grids, the 15 mm retracted candidate reaches the support
check and narrowly fails at 1.4837 N. The 20 mm candidate encounters a different
reported contact at 0.41 s: +Y worm/input spindle versus hip output shaft/pulley,
0.0337 mm penetration. Those two links retain old grids. That contact has not
yet been validated against CAD and was not suppressed.

Intermediate 15.25 and 16 mm candidates pass all sampled reference checks. The
16 mm reference's minimum predicted support load is 1.5801 N and its maximum
sampled marker error is 0.01284 mm. This establishes reference feasibility under
the declared checks, not motor execution. Its -0.35 rad starting foot posture
puts feet about 5.92 mm above the floor with the old base translation. A separately
recorded low-start recipe lowers the initial base by another 5.5 mm, leaving
about 0.42 mm for settling; it also passes the reference checks. This is an
explicit initial-condition experiment, not a simulated transition from standing.

`mechanical-servo-load-16mm.json` and its refined companion consume the compiled
low-start motor commands through the existing Rust servo/dynamics path. They
retain the 1.6 s program, 1 kHz servo timing and 0.25/0.125 ms physics steps.
The reference base path is not imposed on the physical state. These runs are
ungated diagnostics so physical/reference time can be compared directly; they
do not replace the registered support-gated clock or provide active balance
feedback. The trial must still demonstrate actual load transfer and foot lift.

Both complete 1.6 s physical runs remain failed lift attempts. Peak -Y clearance
is -0.001133 / -0.001148 mm: the selected foot never clears the sampled floor.
The coarse run reaches a minimum upward foot force of 0.2265 N at 0.81 s.
At 0.8 s it carries 0.5509 N, while +Y carries 1.9147 N. Supporting load has
improved relative to the old reference, but the swing foot remains loaded.
Maximum body-relative marker error is 4.4637 / 4.4654 mm across the program;
the largest differences occur during body-shift transitions. Original position
closure remains below 9.6e-13 m and neither accepted-step audit reports an
internal contact.

Halving the timestep changes world foot markers by at most 0.03112 mm at aligned
10 ms samples, with no sampled contact-pair mismatch. Contact event times and
motor states are not identical; the full comparison includes force impulses and
electrical differences. This supports the same failed-lift conclusion at both
resolutions, not comprehensive convergence or hardware accuracy. Physics stepping
takes 88.30 / 127.45 s for 1.6 simulated seconds on the development machine;
these diagnostic timings are not a repeated throughput benchmark.

The next 5 mm lift candidate retains the 16 mm body shift and passes geometric
and static-load checks. Its motor recipe is saved as
`mechanical-servo-load-16mm-lift-5mm.json`; it has not been physically executed.
It tests additional clearance demand without silently changing motor strength,
geometry, contact exclusions or accuracy thresholds. Slower scheduling and
active tracking remain separate experiments if that demand cannot be executed.
