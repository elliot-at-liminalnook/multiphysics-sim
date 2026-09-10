# Research-guided gait search — 9 September 2026

## Current implementation status

The user's later clarification removes the other development rejection gates
from speed discovery as well. [SPEED_OBJECTIVE.md](SPEED_OBJECTIVE.md) defines the
current net-distance/time objective and the running continuous-travel pilot.
The slip-only transformation below is retained as historical evidence.

### User clarification: speed takes priority over low slip

The user questioned rejecting slow gaits for slippage and reiterated the speed
objective. The 5% loaded-slip cutoff was a development acceptance criterion,
not a physical limit. Future speed selection should retain slip measurements
but remove that arbitrary hard rejection. Collision, actuator, stability,
steering, stopping and execution checks remain relevant. The simulated friction
law is unchanged. Original experiments retain their recorded criteria.

[human20-speed-priority-snapshot.json](human20-speed-priority-snapshot.json)
re-ranks the first 13 completed full-driving evaluations with only the slip
rejection removed. The fastest passing the other recorded gates is trial 002:
0.090177/0.091480 m/s, 10.214% loaded slip, 103/103 required lifts, zero sampled
interlink overlap, and 0.25548 rad turn response. Finer timestep and dropout
validation is running as `human20-lhs90`. This is a measured speed candidate,
not a fully qualified gait. The live LogEI pilot still uses its original
constraint context; a subsequent selector context must use this revised
criterion explicitly rather than mix incompatible observations.

The research supports a division between discrete contact exploration,
continuous physics optimization, and measured controller selection:

| Research | Application here | Status |
|---|---|---|
| [CrEGOpt](https://nosalro.github.io/cregopt/) | Adapt categorical contact choices and continuous initializers around coupled trajectory optimization | Mixed CEM pilot running; seven seed orders and one/two-cycle representations. Our inner solve also varies timing, so this is an adaptation, not a reproduction. |
| [SCBO](https://proceedings.mlr.press/v130/eriksson21a.html) | Spend expensive runtime evaluations near promising feasible regions while preserving exploration | Constrained LogEI pilot running against independent LHS; SCBO's trust-region/Thompson algorithm is not implemented. |
| [MCTS with whole-body optimization](https://arxiv.org/abs/2508.12928) | Search contact transitions beyond a fixed list of gait families | Multiple contacts per foot and neighboring-order warm starts implemented; tree search remains unimplemented. |
| [TOPPRA](https://arxiv.org/abs/1707.07239) | Optimize timing along a chosen motion under dynamic limits | Candidate path-specific speed analysis, not a global speed certificate; adapting the constraints to our transmissions and servo law remains necessary. |

The full driving evaluation includes forward motion, a moving turn, reverse and
stopping. The earlier short-screen winners failed longer slip checks, so a
short-screen speed is not the search's final success criterion. The first
full-driving LHS improvement and a numerical issue recovered during contact
exploration are recorded in [CONTACT_BOUNDARY_ROUNDOFF.md](CONTACT_BOUNDARY_ROUNDOFF.md).
Neither local optimizer termination nor these finite search boxes establishes
the robot's physical maximum. The original research proposal follows; later
implementation notes supersede its earlier representation restrictions.

Recommendation: add adaptive selection of experiments around the existing joint
optimizer. The remaining problem includes choosing contact families and starting
motions, allocating expensive evaluations, and learning which planned motions
transfer into measured walking. A longer local solve alone does not address all
three. This note records research and an implementation proposal, not a completed
algorithm or a new speed result.

## What the current evidence says

The [TOWR audit](TOWR_GAP_AUDIT.md) identifies remaining restrictions: one
stance/swing per foot, restricted swing curves, and a local contact event order.
The [256-pattern screen](CONIC_PATTERN_SCREEN.md) optimized forces with most
motion fixed. Its failures cannot eliminate families whose body motion or foot
placement needs to change. The [servo-command finding](SERVO_COMMAND_CONSTRAINTS.md)
also shows why planning feasibility and successful controller execution must be
different observations. Existing exact force derivatives, sparse Ipopt, conic
force solves, and detailed Rust replay remain useful inner components.

## Findings from primary sources

### 1. Contact-sequence tree search plus trajectory optimization

[Amatucci et al., ICRA 2022](https://arxiv.org/abs/2205.14277) select contact
sequences using Monte Carlo tree search (MCTS), with optimization-based rollouts
guiding exploration. [Dhédin et al., 2025](https://arxiv.org/html/2508.12928v1)
combine sequence/patch search with whole-body trajectory optimization and use
optimization residuals and collision outcomes in the search reward.

Application: search discrete contact transitions and phase counts outside the
continuous body/feet/force/timing solve. Spend further solves on promising and
underexplored branches. First generalize the shared phase representation; an
outer tree cannot discover motions the inner representation cannot express.
Keep hard collision and actuator acceptance checks even if violations provide
graded search feedback. A failed local solve is an observation about that start,
not proof that a whole branch is impossible.

[Taouil et al., 2024](https://arxiv.org/html/2408.07508v1) accelerate contact MCTS
with a learned value function and retain model-based rollouts. They explicitly
report that a simplified body model neglects leg dynamics and favors overly
short swings. For this robot, timing checks must use its transmissions and
actuators; copying their timing constants would be inappropriate. Learn value
estimates only after representative local data exists.

### 2. Constrained Bayesian optimization for expensive evaluations

[SCBO, Eriksson and Poloczek, AISTATS 2021](https://proceedings.mlr.press/v130/eriksson21a.html)
uses local trust regions and constrained Thompson sampling. Sampled feasible
points compete on objective; otherwise candidates compete on violation. Regions
adapt and restart, allowing exploration beyond one incumbent.

Application: model measured speed and separate constraint outcomes for a compact
set of gait/controller or initialization parameters. Keep the hundreds of
trajectory coefficients and their available physics derivatives in Ipopt. Use
categorical contact-family records or separate searches per event order; do not
treat a contact sequence ID as a continuous number. This division is our design
proposal, not a result demonstrated for this robot by SCBO.

[FuRBO, Ascia et al., 2025 preprint](https://arxiv.org/html/2506.14619v1) shapes its
trust region using both objective and constraint predictions, targeting narrow
feasible regions. It is a useful comparator if SCBO spends most evaluations
outside feasibility; it is not evidence of superiority on our gait problem.

### 3. Retain diverse promising motions

[MAP-Elites, Mouret and Clune, 2015](https://arxiv.org/abs/1504.04909) keeps strong
solutions across chosen behavioral descriptors. [SAIL, Gaier et al., 2017](https://arxiv.org/pdf/1702.03713)
adds surrogate-guided sampling to reduce expensive evaluations; its reported
study concerns airfoil design.

Application: maintain a small archive indexed by contact pattern, measured belt
excursion, and lateral body excursion. Within each cell retain the fastest
validated gait and a separate restoration candidate. Belt use is a descriptor,
not a reward: greater excursion earns no speed credit by itself. Start with
the archive alone; a full illumination algorithm could spend too much effort
mapping uncompetitive regions when the objective is maximum speed.

### 4. Choose evaluation fidelity by information and cost

[Marco et al., ICRA 2017](https://las.inf.ethz.ch/files/marco17virtualvsreal.pdf)
use multi-fidelity entropy search to choose both controller parameters and
simulation versus physical experiments by expected information gain per effort.

Application: learn the discrepancy between coarse planning, dense CAD audit,
and detailed runtime. These are not interchangeable measurements. Measure paired
outcomes and cost before trusting a cheaper stage to rank runtime performance;
retain exploration of candidates that cheaper models rank poorly. A simple
screening cascade is useful infrastructure but is not itself multi-fidelity
Bayesian optimization. Hardware transfer remains a later, calibrated stage.

### 5. IDTO does not remove the need for outer exploration

The [IDTO paper, Section VIII](https://arxiv.org/html/2309.01813v2) explicitly
describes the method as local and suggests higher-level sampling or graph search.
The published formulation also lacks arbitrary hard torque/joint constraints
and discusses nonphysical forces in unconverged solutions. Contact-implicit
planning can generate new seeds, but its published formulation is not a
drop-in maximum-speed certificate for this transmission-limited robot.

## Concrete integration and comparison

1. Add a reusable Rust experiment record and selector in the shared libraries.
   Record parameter bounds, contact family, exact model/controller identity,
   seed, fidelity, evaluation cost, optimizer termination, physical residuals,
   runtime speed, slip, collisions, tracking and stop performance. Preserve
   continuous margins; do not collapse all failures into a single speed penalty.
   An interrupted run, numerical error, or IK failure has explicit status and
   missing measurements, not fabricated zero speed or family infeasibility.
2. First compare compact SCBO selection with scrambled Sobol multistart using
   identical bounds, initial data, inner-solver budgets and current contact
   representation. Both optimize body, feet, forces and timing during refinement.
   Keep legacy trials with different physical gates in separately labeled data;
   re-evaluate before using them as current feasible examples.
3. Extend the shared representation to multiple stance/swing phases and richer
   foot curves. Then compare MCTS contact selection against unbiased sampling
   over the same representable families, with the same continuous optimizer.
   Do not permanently prune from fixed-motion conic failures or local NLP failure.
4. Rank success by highest detailed-runtime speed satisfying the existing gates,
   time/evaluations to a passing improvement, and passing candidates per compute
   budget. Report multiple seeded runs, candidate diversity and failure reasons.
   Charge surrogate fitting and selection time as well as physics evaluations.
   Benchmark budgets are comparison controls, not a time limit on the overall goal.
5. Add a small diversity archive; test it as an ablation. Add learned fidelity
   selection only after paired data establishes predictive value. Keep final
   dense geometry, timestep, sustained control and browser checks unchanged.

Near-term priority is the experiment selector and a fair multistart comparison;
the largest representational gap is contact-sequence/phase-count discovery.
Implement selection in Rust and keep robot parameters in configuration. No
physics duplication, physical-limit relaxation, new gait promotion, or claim of
global exhaustion follows from this research note. The 0.30 m/s search target
remains experimental; the physical maximum is still unknown.

## Follow-up: learning from failed experiments and broadening the search

Additional primary-source review on 9 September 2026. These are proposed
adaptations; the linked papers do not demonstrate a maximum-speed result for
this robot.

### Learn from missing outcomes without inventing rewards

[Marco et al., Robot Learning with Crash Constraints, RA-L 2021](https://arxiv.org/pdf/2010.08669)
combine binary execution outcomes with continuous constraint measurements in
a Gaussian process for classified regression (GPCR). They demonstrate the
method on a jumping quadruped. An experiment that ends before producing a
reward can still teach the constraint model where execution fails.

Our current adapter preserves failed rows but excludes them from training.
A useful extension is a separate execution-success model alongside speed and
physical-residual models. Distinguish reproducible controller/robot failures
from infrastructure errors, cancellations and optimizer budget exhaustion;
the latter do not establish a physical failure. Preserve measured margins
from completed episodes. The paper also learns an unknown constraint threshold;
our declared actuator, collision and walking thresholds should remain fixed.
A separate classifier would be an adaptation, not an implementation of GPCR.

### Learn which contact transitions deserve expensive refinement

[Akizhanov et al., Learning Feasible Transitions for Efficient Contact Planning,
L4DC 2025](https://proceedings.mlr.press/v283/akizhanov25a.html)
train a dynamic-feasibility classifier and a target-adjustment network, then
use them in MCTS. The latter compensates for errors in reaching requested
contact positions. Their reported setting is constrained stepping-stone
navigation, with improvements demonstrated in simulation.

For our speed objective, collect transition-level training examples from the
same CAD model and Rust runtime: contact state, joint state, requested next
contacts, phase duration, actuator margins and executed touchdown error.
Use predictions to prioritize branches and initialize placements, retaining
some exploratory evaluations to detect false negatives. Do not declare a
contact family impossible because a classifier or one local solve rejects it.
This needs a broader shared phase representation before it can discover new
multi-step families; the current per-foot offset/duty representation permits
only one stance and one swing per cycle.

### Adapt the distribution of diverse starting motions

[Fontaine et al., CMA-ME, GECCO 2020](https://arxiv.org/abs/1912.02400)
combine covariance adaptation with a behavior-space archive. This provides an
alternative to fixed independent perturbations: correlated parameter changes
can evolve while distinct behaviors remain available as future starts.

Consider compact motion seeds described by contact family, belt excursion and
body sway. Keep measured feasible speed as quality; excursion alone is not
progress. Compare against simpler multistart before adopting a full archive
optimizer: the paper's benchmark and game results do not establish an advantage
for constrained robot speed, and maximizing archive coverage is not our goal.

### A gait-specific multi-fidelity example

[Tan et al., Optimal Gait Design for a Soft Quadruped Robot via Multi-fidelity
Bayesian Optimization](https://arxiv.org/abs/2406.07065)
optimize parameters of a central-pattern-generator gait and combine simulation
with physical experiments to address model discrepancy. This supports the
practical relevance of multi-fidelity gait tuning, while its tendon-driven soft
robot and fixed gait parameterization differ from our transmissions and contact
search. Reuse the selection principle, not its robot parameters or speed claims.

### Implementation order after the current pilot

The implemented selector is constrained LogEI with a fixed motion family and
three controller/task variables, as documented in [BAYESIAN_SEARCH.md](BAYESIAN_SEARCH.md).
It is not SCBO and does not search contact sequences. The joint optimizer
separately changes body motion, feet, forces and timing. Merely extending the
three-variable pilot cannot exhaust those broader motion choices.

1. The matched-count pilot has now completed: neither arm found a feasible
   improvement; see [its report](BAYESIAN_SEARCH.md). Retain all outcomes and
   do not select the winning algorithm from a single seed.
2. [Shared multi-step references and repeated seeds](CONTACT_SEQUENCES.md) are
   now implemented and checked against CAD planning. [Per-stance joint forces](JOINT_STEPS.md)
   are integrated and a joint multi-step search has started. Next connect outer family
   selection to joint refinement and measured runtime scoring. This is the
   highest-priority change for escaping the present gait family.
3. Add persistent trust-region/restart selection for compact continuous choices;
   compare it with the existing LogEI and seeded sampling under equal total cost.
4. Add execution-failure learning when representative failures exist, followed
   by transition prediction and fidelity selection when paired data supports them.

The proposed structure is: choose a contact family and seed; jointly optimize
its trajectory; execute and measure it; update the search models. None of these
methods supplies a global physical-speed certificate from local stagnation.

## Further review: distribution search, retiming, and better local solves

The earlier representation restrictions above describe the initial audit.
Multiple steps and per-stance forces are now implemented. The new
[contact-order neighborhood](CONTACT_ORDER_SEARCH.md) connects discrete changes
to coupled refinement, but has no adaptive tree policy or distribution update.
The following are proposed additions, not algorithms already implemented here.

### CrEGOpt: the closest new outer-search candidate

[Tsikelis and Chatzilygeroudis, Humanoids 2024](https://arxiv.org/html/2410.02891v1)
combine categorical sampling of phase counts with Gaussian sampling of phase
durations. After local trajectory solves, the best population members update
both distributions. Their inner formulation follows TOWR with fixed phases and
durations. It uses a single rigid body with massless legs; reported timings
cannot be transferred to our CAD dynamics and transmissions.

Our adaptation: sample multi-step contact schedules and timing seeds, retain
exploration probability, and refine body, feet and forces using the existing
Rust planner. Compare fixed timing against allowing local timing refinement.
Rank physically passing candidates by measured speed, while keeping separate
restoration scores for failures. Do not copy a finite speed-versus-violation
penalty into gait acceptance. Compare with seeded sampling and the neighbor
search under equal total model evaluations and wall cost. Population size and
restart rules need measurement; this is not yet a demonstrated improvement.

### INSAT: reuse neighboring solutions across discrete changes

[Natarajan et al., revised 2024](https://arxiv.org/abs/2210.08627)
interleave graph search with trajectory optimization initialized from nearby
solutions. Lazy INSAT delays expensive solves and reuses trajectories across
boundary conditions. The experiments concern bracing manipulation, not walking.

Useful adaptation: keep a persistent graph of contact orders and multiple
optimized seeds per node, expanding from improved solutions instead of always
returning to the original gait. Our adjacent swaps supply one edge operator;
they do not implement INSAT. Reused trajectories must satisfy our full final
boundary and physical checks before acceptance.

### TOPP-RA: calculate where a chosen motion can accelerate

[Pham and Pham, T-RO 2018](https://arxiv.org/html/1707.07239)
use reachability analysis to optimize traversal time along a prescribed path.
Their legged example includes torque limits and linearized contact friction.

For a CAD-derived path q(s), use q_dot = q_s * s_dot and
q_ddot = q_ss * s_dot^2 + q_s * s_ddot to derive admissible progression rates.
This could reveal phase-dependent bottlenecks and initialize a nonuniform gait
clock. It optimizes timing along a path, not all possible paths or contact
sequences. Our torque-speed envelope, servo command limits, exact friction
cones, periodic boundaries and contact transitions require explicit treatment;
they must not silently become constant-torque LP approximations. Implement the
reusable retiming component in Rust if pursued. This is complementary to gait
discovery, and cannot establish the global physical maximum.

### CRISP: a local-solver comparator, not global search

[Li et al., RSS 2025](https://arxiv.org/html/2502.01055v3)
solve successive convex QPs with an adaptive trust region and weighted L1 merit
function. Their contact-implicit examples include poor and zero initial guesses.
The convergence statements concern stationary points of the merit function
under assumptions, not a globally optimal feasible robot trajectory.

Consider a shared solver experiment if feasibility restoration remains the
bottleneck after broader starts. Our phase-explicit formulation differs from
their complementarity problems. First compare identical starts and hard gates
against Ipopt; replacing the solver alone cannot expand our contact vocabulary.

### Learning later: preserve several successful plans per state

[Dhédin et al., diffusion contact planning, 2025 revision](https://arxiv.org/html/2403.03639v5)
collect MCTS plans evaluated through whole-body NMPC and learn a multimodal
contact proposal policy. Their task is simulated stepping-stone navigation;
gait optimization is listed as future work.

This suggests a path to the requested AI controller: collect diverse executed
solutions and perturbed-state recoveries, then learn state-conditioned proposals
for the existing controller. It does not justify training a diffusion model
from our present small collection or replacing measured validation with a
network prediction.

Priority: finish the current contact-order comparison, add adaptive mixed
discrete/continuous selection, and score candidates in the shared runtime.
Retiming is a focused parallel mathematical avenue for identifying path-specific
speed bottlenecks. SCBO remains appropriate for compact continuous choices;
MCTS remains appropriate for long contact sequences with reusable prefixes.
No reviewed paper establishes a maximum speed for this robot.

Implementation update: [mixed CEM selection and its robot pilot](MIXED_CONTACT_SEARCH.md)
now have shared Rust ask/tell state and bounded initializer decoding. The
measured controller comparison also completed, and its longer validation
failures led to a full human-schedule objective. These are adaptations with
recorded limitations, not complete implementations or confirmations of the
papers' robot results.
