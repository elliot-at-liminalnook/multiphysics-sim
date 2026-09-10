# Joint contact-force, motion and timing optimization

The shared Rust planner now optimizes independent force trajectories alongside
body motion, footsteps and phase timing. This implements the central missing
freedom identified in the [TOWR audit](TOWR_GAP_AUDIT.md). It has not yet produced
a qualified faster robot gait or established the physical speed maximum.

Latest: [exact force-evaluation reuse and a numerical bound on the current
force-basis balance error](FORCE_BASIS_AUDIT.md). The warm search is complete and
infeasible; the next audit separates force-curve restrictions from support geometry.

## What runs

`JointContactMotion` uses the existing contact-phase motion and one three-channel
force template per foot and operating clock. Forward and reverse forces can
differ. Templates use the existing shared trajectory component, with a unit
template duration stretched to the foot's stance, zero endpoints and zero force
throughout swing. Linear or quintic interpolation preserves the convex force
cone when every control node is inside it. No new physical contact law is added.

The ordinary CAD kinematics and whole-body inverse dynamics evaluate required
wrenches and motor torques for these independent forces. Force/moment balance,
signed torque-speed limits, floor penetration and zero inter-link overlap enter
the shared inequality augmented-Lagrangian solver. Their constraints are not
downweighted by contact-interval duration. Coincident events retain fixed slots.
Force cones are checked at all nodes; other constraints remain sampled and need
independent mesh refinement and detailed runtime validation.

The target-speed objective still uses an explicit recipe target (.30 m/s here),
not a proven global upper bound. Finite-difference trajectory derivatives and
one stance/swing per foot remain limitations. This is not a reproduction of
TOWR's sparse NLP or completion of a gait-family/multiple-phase-count search.

`seed_joint_forces` automatically initializes interior force nodes from the
existing wrench allocator at their exact phases. This is an initializer only;
loads subsequently vary independently. The compiler accepts optional forward
and reverse templates and derives ordinary motor feedforward from them. Static
pause loads retain their separate audited allocator. The simulator continues to
generate actual contact forces; the reference never injects physical forces.

## Analytic and robot evidence

The redundant-support analytic case has four actuators supporting 10 N, with
opposite capacity pairs 2-v/2 and 4-v/2 at unit moment arms. Summing capacities
proves v<=1 in this test model. Equal force allocation overloads the weak pair
even at v=0. The joint solve reaches v=1 and the corresponding opposite loads
1.5/3.5, within 1e-5. This is a test of load redistribution and speed coupling,
not a numerical speed bound for the CAD robot. Wrench derivatives, friction
violations and force scheduling checks also pass.

The first CAD run starts from the .211710 m/s reference whose detailed runtime
had measured .233944/.225110 m/s with 10.58% slip. It frees 53 motion variables
and 120 independent forward/reverse force components. All eight variable groups
change in its three accepted steps. At its 1,400-evaluation limit, planned speed
falls to .078253 m/s, force error falls 40.553→6.914 N, but moment error remains
1.528 Nm and friction-cone violation reaches 10.014 N. It retains no feasible
candidate. **This is a rejected initialization/search diagnostic, not a speed
improvement and not a reason to continue a low-speed restoration campaign.**

Automatic force initialization keeps the original motion and speed while
reducing initial maximum force error to 13.091 N. The corresponding warm-start
joint search also ends infeasible at .054309 m/s after three accepted steps; its
recipe uses the same bounds, physical gates and target. The coarse template still has substantial balance error, so
neither initialization is physically feasible. Review the resulting constraints
and force/motion representation before increasing search budgets.

The force-aware compiler passes interpolation and static-pause checks on this
warm start while retaining failed nominal/reverse audits. Verified comparison
shows identical motion, joint reference, initial coordinates, pause windows and
static feedforward; independent loads change dynamic and directional feedforward.
The default compiler output remains byte-identical to the committed original
reference. No force-aware reference has yet been qualified in runtime or browser.

## Reproduction and next acceptance

- `prepare_joint_contact.mjs` records the CAD/model inputs, bounds and first seed.
- `optimize_joint_contact scene markers recipe --seed-forces` produces the
  automatic initializer; `--evaluate` performs an independent single evaluation.
- Omitting the flag runs joint optimization with objective/constraint progress.
- `summarize_joint_contact.mjs STEM` verifies result dimensions and records each
  variable group's actual changes and final physical violations.
- `check_joint_compilation.mjs` reproduces the compiler behavior comparisons.
- `joint-force-v1-build.json` preserves the first solver binary/source identity;
  the source archive supports reproduction after initializer/compiler changes.

Nine shared robot motion-capability tests and four runtime phase-planning tests
pass. `joint-x25.summary.json` and the raw recipe/result record the first failed
solve. The warm-start recipe and initial evaluation are separate artifacts.
Before a speed claim, a candidate must pass independent dense feasibility and
the unchanged runtime slip, lift, collision, timestep and WASD checks. Browser
responsiveness remains required. No acceptance threshold is loosened.
