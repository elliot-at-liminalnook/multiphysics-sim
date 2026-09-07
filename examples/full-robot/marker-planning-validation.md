# From foot paths to motor-driven stepping references

The shared Rust library now compiles full 3D marker paths into named motor
trajectories through the CAD-derived closed mechanism. Optional floating-base
translation lets a reference coordinate body weight transfer with foot motion.
This is an inverse-kinematics and validation building block; it is not yet a
quadruped footstep search, balance controller, PLANC teacher or learned policy.

## Implementation and boundaries

`RigidEmbedding::place_points` reuses bounded point-plane placement for full
world-space point targets. It preserves original closure and checks authored
limits, including dependent joints. Its metre tolerance bounds Euclidean point
error. A failed local search reports current coordinates and active bounds;
it does not infer a hardware travel stop or global unreachability.

`sim_runtime::planning::plan_marker_motion` requires exact CAD hashes, marker
order/frames, motor-coordinate names, explicit search intervals and initial pose.
The input contains world-axis marker displacements and optional base translation.
The compiler fits each command knot, then inspects knots and midpoints with the
shared closure, collision and interpolation routines. It rejects reported internal
penetration, authored limit violations and excessive sampled marker-path error.
Missing joint limits are not invented. The output is the existing shared linear
trajectory format: firmware samples it at its normal clock, and registered motors
and drivers apply forces. Base references are not directly applied to simulation
state during execution.

The current examples use 10 ms motor-command knots and 5 ms geometric inspection.
Linear interpolation of densely sampled commands is continuous in position but
not velocity at knots. No velocity/acceleration, torque, braking-distance or
continuous collision-clearance certificate is implied. Local search bounds are
experimental, not calibrated travel limits. Successful references contain every
original joint's closed pose for inspection and replay.

Two shared diagnostics prevent confusing successful animation with stepping:

- `sampled_floor_clearances` transforms the runtime's compiled foot surface
  vertices against the supplied world floor/height field. Positive gap is
  calculated geometrically, not inferred from zero contact force. It is not an
  exact full-CAD surface clearance. The caller must supply the experiment world.
- `static_support_geometry` reports compiled moving-link COM and its signed
  projected distance from the convex hull of explicitly assumed point supports.
  It requires world -Z gravity and excludes authored world-ground objects. The
  chosen supports are not asserted to be in contact. Foot patch extent, friction,
  acceleration and terrain wrench feasibility remain outside this static test.

## First single-foot attempts

The initial diagnostic sets all four foot cranks to -0.1 rad, retains the earlier
worm startup angles and translates the base by -8 mm in world Z. These are
explicit experimental initial conditions, not a physically executed transition
from a previous pose. The feet begin about 0.675 mm above the floor and settle
under gravity. Motor internal alignment and firmware targets match the startup.

The requested -Y foot path travels +2 mm in world X and either +1 or +3 mm in Z,
then returns. The other three marker targets stay fixed; prescribed base pose
stays fixed. Both one-second motor-driven runs complete with no accepted internal
contact, but neither establishes the intended three-foot-supported step.

| At the planned peak (0.5 s) | 1 mm lift request, 0.25 ms step | 3 mm request, 0.25 ms | 3 mm request, 0.125 ms |
| --- | ---: | ---: | ---: |
| Selected -Y foot surface clearance | -0.00393 mm | +0.42470 mm | +0.42500 mm |
| Opposite +Y foot surface clearance | -0.00607 mm | +0.57246 mm | +0.57357 mm |
| Selected -Y foot upward contact force | 1.431 N | 0 N | 0 N |
| Opposite +Y foot upward contact force | 1.214 N | 0 N | 0 N |
| COM margin against the assumed other three marker supports | -0.416 mm | -1.168 mm | -1.159 mm |

Negative clearance is penetration in the compliant floor model. The 1 mm request
does not unload the selected foot. The 3 mm request unloads both the selected
and opposite foot, leaving the two lateral feet bearing the robot near the peak.
Initial COM margin against the intended three-marker support triangle is only
0.09782 mm. The checks identify the need for coordinated weight transfer rather
than counting the visually moving foot as a successful step.

Halving the timestep for the 3 mm run changes sampled world-foot position by up
to 0.0700 mm and duty by 0.06333. Backlash guard 7 fires six versus eight times.
The unwanted opposite-foot lift persists. These are complete diagnostic
comparisons, not a promoted timestep or a hardware accuracy claim.

## Weight-transfer experiment

The next reference shifts the base +5 mm in world Y, then lifts/returns the -Y
foot before shifting back. The initial -0.1 rad crank stance fails local placement
at 0.34 s: -Y's foot crank reaches the provisional -0.02 rad extension bound.
That failed input and error are retained. It is not silently clamped or executed.

A revised explicit startup retracts all cranks to -0.2 rad and lowers the base
translation to -10 mm, leaving more extension available while retaining similar
initial floor gaps. Search intervals for foot cranks are [-0.27,-0.02] rad and
worm coordinates are within 0.08 rad of startup. These are local search choices;
authored joint limits and sampled internal collision rejection remain enforced.
This 1.6 s reference compiles successfully. At its planned peak (0.8 s), COM
margin against the assumed remaining supports is 4.067 mm. The planner's maximum
sampled marker error is 4.01 micrometres.

The complete 1.6 s motor-driven replay shifts the chassis +3.501 mm in Y at the
0.8 s peak, versus the requested +5 mm. Actual COM margin against the assumed
three supports is +2.760 mm. The opposite foot stays loaded (0.6715 N upward at
the peak), but the selected foot also remains loaded (0.7097 N). Its sampled
minimum surface gap never becomes positive during the requested 0.5–1.1 s lift
window; the largest gap is -0.000357 mm. The other three feet likewise remain in
contact at those samples. This is a weight-transfer/tracking experiment, not a
successful lift or stepping controller.

At peak, the selected hip/worm/foot motor coordinates are approximately
[-0.000250, -0.049653, -0.232890] rad versus targets
[0.009028, -0.058071, -0.247480] rad. Maximum body-relative marker tracking error
over the complete episode is 2.905 mm. Motor tracking and loaded offsets therefore
remain material to achieving the reference; additional raw geometric clearance
is not sufficient by itself. The shifted case has not yet been timestep-refined.

All 12 servos retain 1,600 ticks in this replay; no accepted internal contact is
reported. Native physics wall time is 123.19 s for 1.6 simulated seconds. The
unshifted 1/3 mm cases cost 63.49/68.76 s for one simulated second. These single
runs overlap some builds, and are not controlled speedups or realtime results.

## Reproduction and evidence

Use the CAD/scene derivation in `embedded-integration.md`; recipes retain the
source CAD SHA-256. No live CAD geometry, calibration or unsaved viewer state was
modified by these experiments.

```sh
cargo run --locked --release -p sim-runtime --example plan_marker_motion -- runs/full-robot/learning/servo-regularized-floor.scene.json examples/full-robot/foot-markers.json examples/full-robot/single-foot-marker-motion-3mm.json > runs/full-robot/learning/single-foot-marker-plan-3mm.json
cargo run --locked --release -p sim-runtime --example integrate_embedding -- runs/full-robot/learning/servo-regularized-floor.scene.json examples/full-robot/mechanical-servo-single-foot-marker-motion-3mm.json > runs/full-robot/learning/single-foot-marker-execution-3mm.json
cargo run --locked --release -p sim-runtime --example summarize_marker_clearance -- runs/full-robot/learning/servo-regularized-floor.scene.json runs/full-robot/learning/single-foot-marker-execution-3mm.json examples/full-robot/foot-markers.json -Y-foot-surface
```

The integration recipes contain the generated trajectory and matching startup
motor targets; they do not replace physical dynamics with prescribed kinematics.
`marker-planning-status.json` records all completed and rejected attempts,
source/executable snapshots, input/output hashes, reference recompilation checks
and the retained interrupted empty capture. Recompiling all three successful
geometric recipes with the final source reproduces their command trajectories
and sampled poses exactly.
`compare_motion` compares body-relative marker tracking to the geometric plan;
`compare_embedding` compares completed coarse/refined physical runs, including
accepted contact impulses and event counts. The full scene/world and source
hashes must accompany these diagnostics.

Tests include a rotated analytic closed linkage, an analytic translating fixture,
unreachable targets and identity mismatches, interpolation-error rejection,
midpoint internal collision rejection, base-translation compensation, surface
clearance under rotation, and known COM/support polygons. CI includes the shared
planning and support tests plus CLI compilation. The broader calibrated walking,
teacher/student training, robust evaluation and interactive policy goal remains
unfinished.
