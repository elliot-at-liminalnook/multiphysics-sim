# From hand-selected gait variants to general motion planning

Research checked 2026-09-08 after the user's request to prioritize generalized
algorithms and literature over further manually selected gait tweaks. This
changes the next development priority; it does not cancel validation already
running or change the active speed objective.

## Current direction after the user's TOWR review — 9 September 2026

The user correctly identified a return to manually selected gait changes.
TOWR's joint formulation has **not** been exhausted: the phase planner still
allocates contact loads in a nested wrench-only solve, and different phase
counts and broad initial gait families have not been exhausted. The next work
is to jointly optimize forces, body motion, foot placement and timing toward
greater measured speed. See the source-level
[TOWR gap audit and concrete next experiment](../contact-planning/TOWR_GAP_AUDIT.md).
This supersedes the historical priorities below. The existing phase prototype
is partial implementation, not completion of the TOWR approach.

## Earlier direction after the user's IDTO reminder

The user explicitly reiterated https://idto.github.io/ while reviewing the .21 m/s
diagonal candidate. The TOWR-inspired phase planner is now implemented and tested;
it is not contact-implicit. Its independent foot phases, durations, placement and
body splines still prescribe one stance/swing sequence per foot per cycle. Do not
represent that as unrestricted contact-pattern discovery or as the full PLANC
pipeline. PLANC's CLF-guided teacher/student RL is not implemented for this gait.

The algorithmic work at that point was **contact-implicit whole-body planning**. The first shared Rust implementation and experiments are recorded in [contact-implicit/README.md](../contact-implicit/README.md); it has not produced a qualified robot gait. IDTO's
state-dependent smooth contact law determines force from signed distance and
relative velocity (paper equations 3–6). Generalized positions are the decision
variables; finite differences determine velocities/accelerations, and inverse
dynamics gives actuator forces and unactuated balance residuals (equations 9–13).
There is no prescribed support schedule. Its planning contact model deliberately
differs from its validation simulator; that distinction is central, not incidental.

Shared Rust contact/embedding/optimization components now implement the position-only formulation. Analytic gravity, equilibrium and unprescribed touchdown checks pass, and whole-CAD trials run. Weighted damped Gauss–Newton with optional IDTO-style Hessian scaling, an experimental equality-constrained dogleg solver, and an inequality augmented-Lagrangian solver are implemented. Persistent multiplier checkpoints and sampled actual-slip inequalities are also implemented. A subsequent strict slip barrier retains a dense slip pass but still fails balance; a paired native diagnostic exposes a discontinuity at the sampled load cutoff. See the [current experiment index](../contact-implicit/README.md) for evidence and subsequent solver work. Sparse trajectory derivatives and online MPC remain unimplemented. These local refinements have not demonstrated broad gait-family discovery or produced a qualified contact-implicit gait.
Keep smooth-contact length, dissipation and stiction scales explicit in a planner
recipe, never silently substitute them for CAD or runtime contact properties.
Optimize floating-base and independent-joint trajectories with the requested 45°
travel objective and actual actuator envelopes. Preserve the detailed runtime,
geometry, command-loss, timestep and rendered-browser checks. A successful local
solve or a smoothed-contact gait will not establish the physical speed maximum.
The first implementation may be offline; published MPC rates are not promised.

Sources rechecked on 8 September 2026:
[IDTO project and unconstrained gait demonstration](https://idto.github.io/),
[IDTO formulation](https://arxiv.org/html/2309.01813v2).

## What the papers actually do

**PLANC (Dai et al., 2026).** A momentum-based linear inverted pendulum supplies
state-dependent step durations and center-of-mass targets. Vertical body motion
regulates momentum across foot impact. Terrain supplies the next foothold;
parametric swing curves connect it to the current state. Control Lyapunov
Function rewards guide teacher/student reinforcement learning. This is a
general reference-generation rule within a structured bipedal model, not
unrestricted discovery of every contact pattern. Its single-support, virtual
slope and impact assumptions need reassessment for this four-legged linkage.
[Paper, Sections II–IV](https://arxiv.org/html/2601.06286v1).

**TOWR (Winkler et al., 2018).** Optimize body motion, foot locations, forces and
per-foot swing/stance durations together. Varying phase durations changes the
relative contact schedule; each foot still has a specified alternating phase
structure/count. The original model simplifies body dynamics and reachability,
so it does not by itself enforce this robot's worm, belt and slider-crank
actuation limits. Its formulation is a strong starting point for an efficient
offline planner using our existing splines, CAD kinematics and force allocation.
[Authors' implementation and publication](https://github.com/ethz-adrl/towr),
[authors' presentation of the formulation](https://slides.com/alexanderwinkler/ral_18/fullscreen).

**Contact-implicit whole-body optimization (Neunert et al., 2016; IDTO,
Kurtz et al., 2023).** These methods let contact placement and timing emerge from
the dynamics and task objective. IDTO optimizes generalized-position trajectories
using inverse dynamics and a smooth compliant contact approximation. It can
represent richer whole-body motion, but smoothing can create nonphysical contact
effects and local optima remain possible. Published real-time rates are specific
to their models and implementations; they do not establish performance here.
[Automatic gait discovery](https://arxiv.org/abs/1607.04537),
[IDTO formulation and limitations](https://arxiv.org/html/2309.01813v2).

**Reference-free sampling MPC (Schramm et al., 2025).** Joint position and velocity
spline nodes are optimized using sampled rollouts, annealed perturbations and
weighted updates. Gaits can emerge without a prescribed contact sequence. This
avoids requiring dynamics derivatives but still needs many fast rollouts.
It is an algorithmic search method, not an escape from evaluating candidates.
Our detailed simulation is currently too slow to assume their reported online
performance transfers directly.
[Paper, Sections III–IV](https://arxiv.org/html/2511.19204v1).

## Repository assessment

We already have shared Rust trajectory evaluation and derivatives, CAD-derived
kinematics, inverse loads, signed actuator torque envelopes, constrained support
force allocation, deterministic environment replay and dense geometry checks.
The existing `policy_search` is explicitly paired random perturbation of neural
weights, not MPC, PPO, contact-sequence optimization or a general constrained
trajectory solver. The current force optimizer holds reference motion and
support weights fixed: it optimizes only one part of the motion problem.

## Original selected direction (partially implemented)

Start with an **offline, joint motion-and-contact-phase optimizer**, using a
TOWR-style phase parameterization and whole-body inverse-dynamics feasibility
checks. Keep contact-implicit whole-body optimization as the next comparison
when the phase representation restricts discovery. This choice is an engineering
judgment based on the existing reusable components, not a result claimed by any
paper or a claim that implementation is complete.

1. Define a shared Rust optimization problem over cycle duration, per-foot phase
   durations and 3D placements, body translation/orientation, and reference
   spline coefficients. Allow hip/belt, worm and foot coordinates to participate
   through CAD kinematics. Do not fix a 52 mm stride or copy the present paired
   schedule into every initialization. Try multiple phase counts and seeds.
2. Maximize periodic forward progress per time. Enforce dynamics, repeatability
   up to translation, reachability, nonpenetration, friction, joint travel and
   the signed speed-dependent actuator envelope. Include transmission and leg
   inertia through shared whole-body inverse dynamics. Keep any reduced model
   explicit and measure its error; do not introduce a second simulator in scripts.
3. Rank feasibility before speed. Penalize contact slip, excess actuation and
   abrupt motion without rewarding sliding as effective walking. Inspect the
   binding constraints and constraint residuals after every solve. Failure or
   stagnation of a local optimizer is not a physical speed ceiling.
4. Validate optimized candidates in the unchanged detailed runtime, including
   acceleration, reverse, steering and stopping. Lift/contact audits must follow
   the declared generated schedule. The current opposite-pair support condition
   describes the old gait; it must not silently forbid every new gait or flight
   phase. Retain collision checks and assess dynamic balance explicitly.
5. Turn a successful planner into a state-dependent controller, then use its
   trajectories and tracking errors to train/distill a policy, following PLANC's
   planner-guided learning principle. A small deployable policy or a measured
   planning profile must meet browser responsiveness; runtime MPC is not assumed.

The immediate milestone is one reproducible optimizer run that jointly changes
step timing, foot placement and body motion, reports objective/feasibility
history, and survives detailed replay. More manually chosen lift-height sweeps
are no longer the main development path. Current gait captures remain useful
warm starts, baselines and regression tests.

## Implementation status and first falsification

The shared Rust prototype now optimizes 53 variables together. It retains one
stance and one swing per foot per cycle and point-support contacts; it is neither
a reproduction of TOWR nor contact-implicit optimization. Support allocation
minimizes wrench error inside friction cones but does not jointly choose loads
to satisfy motor limits. A torque failure can therefore reflect this allocation
as well as the motion. Foot markers also require comparison with actual material
contact geometry. These are explicit model restrictions to remove or assess.

The first apparent 0.196 m/s plan fails independent dense balance checking: a
brief all-feet-swing interval requires 41.6 N of nonexistent vertical support.
Coarse samples missed it. Contact-event and interval sampling, feasibility-first
optimization, multiple phase seeds, and detailed dynamic replay are required
before reporting a new gait. See ../contact-planning/README.md.

The event-aware prototype now has a dense-audited feasible 0.059272 m/s seed
and sampled feasible starts at wider 30°/45° hip postures. Speed continuation
reaches 0.200021 m/s but is infeasible; restoring it retains 0.187895 m/s,
which still fails independent dense balance and torque checks. This motivates
constraint-mesh refinement and possibly more body spline freedom, before any
claim of a faster controller. The algorithm now retains feasible candidates
separately from its final least-squares iterate. See the contact-planning report
for numerical tolerances and the specific limiting phases.
