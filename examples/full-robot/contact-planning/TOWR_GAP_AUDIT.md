# TOWR gap audit — 9 September 2026

**TOWR's main ideas have not been exhausted.** Recent measured speed gains came
from manually selected posture, lift, cadence, feedback and swing-shape changes.
Those are useful observations and seeds, but do not demonstrate the limit of a
joint trajectory optimizer. The user's latest direction is to resume that joint
optimization in service of maximum measured speed.

## Follow-up implementation

The shared Rust phase planner now has a separate `JointContactMotion` path with
independent forward/reverse stance-force trajectories. A 173-variable CAD trial
has jointly changed all motion, timing and force groups, using explicit physical
inequalities in the shared augmented-Lagrangian solver. The analytic redundant
support test reaches its independently derived actuator-limited speed; the first
robot trial remains infeasible and is not a speed gain. Automatic force-node
initialization and force-aware reference compilation have passed the recorded
initialization and compiler behavior checks. See
[joint-force implementation and experiments](JOINT_FORCES.md) for current status.
The table below records the implementation at the time of the original audit;
it is not a claim that force variables are still absent from the new path.

The subsequent implementation now includes [sparse native joint optimization](JOINT_IPOPT.md),
[contact-relative force timing](CONTACT_TIMING.md),
[shared conic force optimization](FORCE_CONIC_SOLVE.md), and
[adaptive collocation](JOINT_MESH_REFINEMENT.md). A completed
[256-pattern fixed-motion screen](CONIC_PATTERN_SCREEN.md) found no feasible
fast initializer; it does not exhaust jointly optimized contact families.
Multiple stance/swing counts, richer swing paths, and qualified runtime transfer
remain open. No new measured speed gain or global physical maximum follows
from the solver implementation or these unsuccessful searches.

## Source and implementation comparison

The authors' [NLP formulation](https://github.com/ethz-adrl/towr/blob/master/towr/src/nlp_formulation.cc)
adds base motion, end-effector motion, end-effector force and optionally phase
duration variables to one problem. Its [dynamic constraint](https://github.com/ethz-adrl/towr/blob/master/towr/src/dynamic_constraint.cc)
provides derivatives with respect to all four groups. The
[phase-duration implementation](https://github.com/ethz-adrl/towr/blob/master/towr/src/phase_durations.cc)
varies the durations of a supplied alternating contact structure; the number of
phases is supplied, not freely discovered within one solve. Sources checked
9 September 2026. These are formulation facts, not a claim that TOWR would
automatically solve this robot or prove its global speed maximum.

| Part | Existing shared Rust phase planner | Remaining gap |
| --- | --- | --- |
| Body and foot motion | Body spline controls, 3D foot centers and swing offsets can be decision variables. | Foot swing shapes are restricted templates; the latest manual runs fixed most variables. |
| Timing | Period, per-foot phase offset and stance fraction can be optimized. | One stance and swing per foot per cycle. Different phase counts and broad starting families have not been exhausted. |
| Contact forces | `constrained_point_forces` allocates loads inside friction cones for each prescribed motion sample. | Forces are absent from `ContactDecision`. The inner allocation minimizes wrench error; it does not jointly optimize motor feasibility with trajectory variables. |
| Dynamics and actuation | CAD kinematics, whole-body inverse dynamics and signed motor torque-speed margins are evaluated. | The outer weighted least-squares residual can trade speed against violations. A lower objective is not physical feasibility. |
| Solver | Finite-difference bounded least squares, event-aware sampling, mesh refinement and separate feasible-candidate retention. | No full sparse constrained trajectory NLP or complete trajectory Jacobians in this phase planner. |
| Validation | Detailed Rust runtime, measured slip, lift, stop/heading and sampled geometry checks. | No fully qualified fast gait or demonstrated physical maximum. |

Local evidence: `crates/sim-runtime/src/contact_planning.rs`, especially
`ContactDecision`, the call to `constrained_point_forces`, the subsequent motor
torque calculation, and `bounded_least_squares`. Force allocation does respond
indirectly to motion changes; the missing freedom is independent load choice
subject to the same dynamics and actuator constraints. A bad torque margin can
therefore reflect the allocation as well as the chosen motion.

## Next implementation and experiment

1. Extend shared Rust planning with explicit per-contact force trajectory
   variables. Couple their force and moment balance, unilateral/friction limits,
   zero swing force and CAD-derived motor torque-speed limits to body motion,
   footholds and durations in the same constrained problem. Reuse the existing
   kinematics, inverse dynamics, optimization and trajectory components; do not
   introduce another simulation path or change CAD physics.
2. First verify the missing degree of freedom on a redundant-support analytic
   case: several load distributions balance the body, but only some satisfy an
   actuator limit. Check force derivatives and rejection of infeasible loads.
   This isolates whether the optimizer can make the tradeoff our nested
   wrench-only allocation misses.
3. Use the recorded +45-degree gaits as warm starts and optimize progress per
   time while freeing forces, body motion, placement and timing together.
   Record changes in each variable group, feasibility and objective history.
   Compare the joint formulation with the old allocator from identical seeds.
   Follow with systematic alternate initial phase patterns and multiple phase
   counts; do not describe a handful of hand-selected variants as exhaustion.
4. Accept speed gains only after independent dense checks and detailed replay
   with the existing slip, clearance, collision, control and timestep gates.
   Sustained WASD and browser checks remain required for promotion. Diagnose
   model-to-runtime discrepancies rather than rewarding sliding as walking.

This was the next development specification; the follow-up above records its
partial implementation. **It is not a completed multistart search or a qualified
robot gait**. It supersedes the earlier priority
to continue either manual single-parameter variants or fixed-speed
contact-implicit restoration. The physical maximum remains unknown; local
optimizer stagnation cannot establish it.
