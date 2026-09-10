# Broader contact-pattern initialization at the fast target

The current refined joint search remains tied to one contact event order.
This screen uses the same convex force solver to compare other starting
patterns at the prior fast reference speed, 0.211710 m/s. It is a systematic
initialization experiment, not an exhaustive gait search or physical maximum.
The independent joint searches continue while it runs.

## Shared implementation

The whole-force optimization formerly assembled in the CLI now lives in
`ContactPlanner::optimize_joint_forces_conic`, returning a typed result with
native convergence diagnostics and an independent full CAD audit of primal
iterates. The CLI only loads inputs and calls that method. Its feasible-control
replay is byte-identical to the prior implementation, including the native
iteration result and every physical report field.

`JointContactMotion::force_variables_for_bounds` enumerates interior force
coefficients from explicit per-clock/per-foot xyz force bounds. It validates
layout and finite ordered bounds; it does not derive or invent robot properties.
The original control's variable order and bounds match the prior recipe exactly.
The existing batch CLI gains an optional `conic` configuration. Its original
nested-allocation path is unchanged: a legacy row replays byte-for-byte.

## Trial grid and controls

There are 256 experimental starts: 64 assignments of quarter-cycle offsets to
feet 1–3, two common stance fractions (0.60 and 0.75), and two body references
(the unchanged fast curve and its constant control-point mean). Foot 0 anchors
the phase origin. Declared phase offsets `[0, .0005, .001, .0015]` avoid exact
contact-event coincidences in the current timing representation. Exact
coincident patterns and arbitrary event counts remain outside this screen.

The 0.396087 s period, displacement, heading, foot centers and swing paths stay
at the fast reference. All changed motion parameters remain in their existing
search bounds. Trials start from three zero linear force nodes, then use the
shared contact/body-event refinement and contact-timing binding to construct
the force basis. Every coefficient is optimized under the original experimental
force boxes and model friction cones. These are fixed-motion solves; the body
and placements must subsequently be optimized before rejecting a gait family.

Two additional controls retain their original bases: the prior fast motion and
the low-speed motion whose convex force fit is sampled feasible. Both reproduce
their prior optimized forces and physical results. The warm control is a
validation case, not one of the 256 fast trials.

## Completed pilot evidence

| Start | Normalized minimax balance | Physical result |
|---|---:|---|
| Fast control | 34.311055 | Balance/actuator failure |
| Warm control | 0.716108 | Passes original sampled gates; earlier dense failure retained |
| Nearly in-phase, duty .60, original body | 828.614367 | Balance/actuator failure |
| Quarter-cycle crawl, duty .75, mean body | 183.438402 | Balance/actuator failure |

The crawl's independent full report places a largest failure at phase .0005,
at a contact handover. A separate diagnostic at the existing upper stance bound
.80 makes this fixed-motion candidate worse: balance 278.060304 and torque margin
-4.052879 Nm. Its tiny positive cone error (2.60e-10 N) remains a failed strict
physical check despite native `Solved`. This probe was not adopted as a new
strategy or used to exclude other .80-duty motions.

Six shared planner tests pass. The conic CI workflow now builds the shared CLI,
mesh refiner and batch path; remote CI has not run here. The full 258-row batch has completed with exit zero. All 256 experimental
conic programs return `Solved`; none passes the original physical checks.
Its output retains errors, optimized candidate/force decisions, scalar physical
checks and convergence diagnostics for each ID. Selected promising results
still require an independent dense report and detailed runtime validation.

## Reproduction

Build with `cargo build --locked --release -p sim-runtime --features conic
--example solve_joint_force_cones --example screen_joint_contact_starts`.
`prepare_joint_conic_patterns.mjs` records the deterministic input grid and its
provenance; it uses exclusive output creation. Run
`screen_joint_contact_starts scene.json markers.json batch.json`.
`check_joint_conic_patterns.mjs` checks control/legacy replay, bounds and pilots.
Source archives, input/binary identities, completed logs and the full-batch launch
record accompany this report. No new runtime speed gain is established.

## Completed screen and sixteen-control comparison

The best experimental start is `original-d0.6-p0202`, a paired pattern close to
the original contact family. Its force error is 2.825740 N, moment error
0.743322 Nm and torque margin -0.170980 Nm; normalized minimax balance is
56.514802. It is worse than the original fast control (34.311055). The fixed
body/placement starts provide no better initializer at the fast target. They
do not exclude optimized motions in those families. Full results are retained
in a verified gzip JSONL archive with uncompressed and compressed hashes.

The older sixteen-control native search has separately finished at its
8,000-model budget without a feasible candidate: 0.025224 m/s, force error
0.045422 N, moment error 0.015217 Nm and cone violation 1.071196 N. Re-optimizing
its forces with the shared convex method produces a sampled feasible 266-frame
reference with force error 0.036219 N, moment error 0.012876 Nm and zero cone
violation. Its dense audit then stops at a bounded inverse-kinematics failure
(5.3008e-9 m point-plane residual; coordinate 5 at its upper bound). The error
and empty result are preserved; no dense or runtime feasibility is claimed.

The refined eight-control joint search is still running. Further progress
requires coordinated motion/placement changes, not treating these fixed-motion
force optimizations as an exhaustive gait-family or global speed proof.

The shared conic assembly also retains unilateral support when model friction
is exactly zero: the degenerate circular cone alone cannot constrain the normal
force sign. An analytic test requests a -1 N normal load from a box allowing
negative forces and verifies the physical optimum remains zero normal force
with unit residual. This branch does not change the robot’s positive-friction
problem; its feasible-control replay is checked separately.
