# Finite CAD foot support at the fast reference

Allowing the actual near-floor CAD surface samples to carry independent forces
does **not** balance the fixed .211710 m/s reference across its sampled cycle.
Only **66 of 160 frames** pass the original .05 N / .02 Nm balance tolerances.
All 160 force allocations converge and satisfy unilateral/friction cones.
Maximum errors are **.938142 N** and **.300122 Nm**, both at phase .71875 in
forward and reverse motion. This is an allocation diagnostic, not a proof that
every possible finite-surface force assignment fails.

The audit verifies all **96 compiled contact samples**, 24 per foot, against
the unchanged CAD-derived validation model. Samples are placed using the same
whole-body pose and solved mechanism coordinates as the reference planner.
Only planned stance samples within an explicitly supplied **1 mm** floor gap
are eligible. At the worst instant there are three eligible samples on +X and
four on -X; the other two feet are in swing. The 1 mm gap is an optimistic
eligibility relaxation, not a change to physical compliance or a claim that
all eligible samples touch the floor.

Reconstructed original marker geometry and per-foot minimum floor clearances
agree exactly with the independent original reference evaluation: both maximum
errors are **0 m**. The original point-support audit also remains byte-identical
after extracting the shared body-pose helper.

The allocator retains the existing planner's floor-friction coefficient
.31724137931034485, regularization .0001, 20,000 iteration limit and 1e-6 N
stationarity tolerance. Its finite-surface result omits actuator limits,
force-trajectory restrictions, slip/velocity constraints and total per-foot
force bounds. Thus it cannot qualify a gait or establish the physical speed
ceiling. The detailed runtime contact law remains unchanged. The earlier
point-support dual bounds and this cone-constrained allocation are different
diagnostics; their pass counts are not a controlled comparison of force models.

## Wider optimization result and next experiment

The 173-variable workspace-expanded joint search completed all **8,000
evaluations**, with 23 accepted inner steps across three outer iterations.
It ended at the lower allowed speed, **.025 m/s**, still infeasible:
.910675 N force error, .234415 Nm moment error, -.232742 Nm motor margin and
.043238 N cone violation. All variable groups changed. This local restoration
failure is not the robot's maximum speed and is not a promoted controller.

Repeating the same local trot is therefore not the next experiment. The
prepared contact-start screen enumerates all 64 quarter-cycle combinations
for three feet with the fourth fixing the phase gauge, at stance fractions
.5 and .75, with either the original body oscillation or its constant mean.
The original reference is included, making **257 starts** at the same .211710
m/s reference speed. All 257 completed without evaluation errors; none passed
sampled feasibility. The original reference remains the best initial score.
The best new start is a sequential pattern with phases [0, .75, .5, .25],
.75 stance fractions and constant mean body pose. Its maximum normalized
inequality is 301.497 versus the original's 260.818; its motor margin is
-1.793962 Nm. These are unoptimized initializations, not speed results.

Shared Rust CAD dynamics initialize the force trajectories and evaluate every
start. The original start's entire inequality vector reproduces the previous
full reference evaluation exactly, and force initialization changes no motion.
This ranks starting points for joint optimization;
it does not exclude a family merely because its initial motion is infeasible.
Multiple phase counts, continuous timing and unrestricted body/foot paths
remain unexplored. TOWR's ideas have not been exhausted.

`prepare_joint_multistart.mjs` selects the lowest maximum inequality in each
of the four body/duty groups for a 3,500-evaluation joint solve. The 173 variables,
CAD workspace bounds, .30 m/s target and physical gates remain unchanged. Four
prepared seeds do not constitute optimization of the complete grid.

## Reproduction

`audit_joint_surface_support scene.json motion-markers.json surface-markers.json recipe.json 0.001`
uses the unchanged diagonal validation scene, workspace motion markers,
`../contact-implicit/surface-markers.json`, and `joint-x25-warm.recipe.json`.
The raw report is `joint-surface-1mm.result.json`. The new shared
`joint_contact_surface_frames` API only exposes CAD geometry and required
wrenches; it assigns neither forces nor contact eligibility.

`prepare_joint_starts.mjs` prepares the durable batch and its provenance.
`screen_joint_contact_starts scene.json markers.json batch.json` emits one
JSON line per start, retaining the initialized candidate and every normalized
inequality, or its explicit failure. It flushes results and progress after each
start. The completed raw stream is losslessly preserved as
`joint-start-screen.result.jsonl.gz`; the reducer reads either raw or compressed
form. No source CAD, browser bundle or runtime contact law is changed.
