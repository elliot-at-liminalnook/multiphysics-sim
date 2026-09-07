# Joint contact regions must move with the robot

The corrected-motor replay reported about 4.5 micrometres of internal contact
between the -Y curved knee link and sliding crosshead. Inspecting the original
CAD B-reps at captured poses exposed a coordinate-frame bug in the shared Rust
contact preparation: the joint's exclusion sphere was centered at its export
world position even after the joint moved.

At 0.87 s the reported contact sample is 11.9923 mm from that stale center but
only 6.4030 mm from the current joint center. The existing loop-joint radius is
10 mm. The region avoids counting the joint connection as general body collision.
It does not establish a calibrated bearing-contact model. Its center must follow the
joint; a robot's translation through the world must not change which local
geometry belongs to that connection.

## Independent CAD inspection

`robocad.capture_geometry.audit_captured_pair` is a read-only CAD-side inspection
tool. It verifies the CAD SHA-256 and capture provenance, loads only named link
members, and transforms their B-reps from original world millimetres into the
captured rigid poses. It measures every member-pair distance and classifies
reported contact points against each solid. Optional contact seeds follow the
same source-local sample through other frames, including frames with no runtime
contact. It does not integrate physics, change CAD, or infer clearance from a
rendered image.

The sample reported at 0.87 s classifies inside one crosshead solid at all six
inspected times: 0, 0.8, 0.82, 0.87, 0.9 and 1.6 s. Its nearest B-rep distance
stays 0.000267051 mm, while the rod remains over 3 mm away from this sample.
The tiny existing overlap is not a newly closing geometric gap. Shape-to-shape
distance is approximately zero at the joint throughout; that unsigned distance
alone cannot establish absence of overlap. The crosshead's exported SDF cell is
6.1395 mm, so the grid's micron-scale reported penetration should not be read
as an independently verified dimensional measurement.

```sh
PYTHONPATH=cad cad/.venv/bin/python examples/full-robot/audit_captured_geometry.py examples/full-robot/baseline/robot.rcad runs/full-robot/learning/catalog-stall-consistent.scene.json runs/full-robot/learning/catalog-corrected-shift-execution.json '-Y | Rigid curved connecting link' '-Y | Sliding foot crosshead' 0 0.8 0.82 0.87 0.90 1.6 --probe-frame 0.87
```

Run this offline or in a background worker. Exact CAD queries can be expensive;
the CLI does not run on the viewer UI thread. Measurements are sampled, not a
continuous collision certificate or a tolerance assessment of manufactured parts.

## Shared runtime correction

Neighbor metadata now stores the anchor-link index, anchor-local joint point
and existing radius. Each contact query transforms that point with the current
anchor pose. Cached metadata remains immutable and shared, while pose-dependent
contact results are reused only when both involved links have unchanged poses.
The first tree-joint/loop priority and existing region sizes remain in place.
This fixes the frame error; it does not validate every inferred exclusion radius,
the SDF accuracy or flex-dependent bearing deformation.

A regression places analytic-plane contact samples inside, on and outside a
joint sphere. A common translation and rotation must preserve hit identities,
penetration and rotated normals for both tree and loop joints, with fresh and
reused caches. The boundary and outer samples remain active. Other tests retain
duplicate-pair priority and model-edit invalidation. CAD tests independently
check COM-centered unit conversion, rigid-transform rejection, known box gaps,
inside/outside seed transport, provenance checks and preservation of input files.

Two CAD tests and two Rust exclusion tests pass. The two Rust tests also run
without default features. The motor/step/Jacobian suites cover 34 passing tests;
one previously ignored experimental SDF derivative promotion test remains ignored.
The existing CI jobs include the new tests. Full-robot replay results and hashes
are recorded in `moving-joint-band-status.json` after completion.

## Full-robot results

Both corrected 1.6 s runs complete, at 0.25 and 0.125 ms timesteps, with **zero
accepted internal contact samples**. Maximum original position-closure error
is below 1e-12 m in reported frames. Compared with the old band at the coarse
timestep, sampled world-foot position changes by at most 0.001872 mm. Eight
reporting frames lose the old internal contact; event counts remain equal in
that before/after comparison. Current changes by up to 0.04508 A, so small pose
differences should not be taken as proof of identical actuator loading.

The intended stepping behavior remains incomplete. At the planned peak the
selected foot clears the floor by 1.054/1.056 mm. The opposite foot briefly
unloads, reaching 0.154/0.157 mm maximum gap during the lift window. Worst
body-relative marker tracking is 2.482/2.480 mm. At the peak only the two lateral
feet carry floor force. The static three-support margin is therefore a
counterfactual support assumption, not proof of stable three-foot support.

Timestep refinement changes sampled world-foot position by at most 0.04003 mm;
one frame differs in floor-contact pairs, and backlash guard 18 still fires
seven versus nine times. Neither event equivalence nor a stepping gate is
claimed. Diagnostic stepping times are 121.10 and 159.03 wall seconds on the
Intel i9-9980HK, with small checks/analysis overlapping part of the runs; these
are not controlled speedup measurements. The fixed joint frame removes a
collision artifact, not the outstanding throughput or balance problem.

All 27 recorded input/output hashes and the 238-file source/binary snapshot
were verified. The next control experiment should coordinate weight transfer
and stance-foot support, with explicit tracking/clearance margins. A new motor
travel stop is not justified by this contact artifact.
