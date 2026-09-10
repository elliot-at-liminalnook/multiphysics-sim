# Constrained Bayesian selection for measured gait speed

The runner now also supports an explicit `human20` profile with the historical
forward/turn/reverse/stop schedule and a ninth turn-response residual. The
default remains the eight-second screen described below. See
[mixed search and extended validation](MIXED_CONTACT_SEARCH.md) for the completed
diagonal68 pilot, its rejected longer-validation candidates, and the new full
human-schedule search. These profiles use separate context identities.

This implements an adaptive experiment selector around the shared Rust execution
path. It does not replace the joint body/foot/force/timing optimizer, which remains
active, or claim that contact families have been exhausted.

## Shared implementation and research choice

`sim-solve::bayesian` is an optional native Rust adapter for pinned
[EGObox EGO 0.38.1](https://docs.rs/egobox-ego/0.38.1/egobox_ego/). It fits separate
objective/constraint Gaussian processes with constant means and Matérn-5/2
kernels, normalizes decision variables to their declared box, and requests one
point using constrained LogEI. Model constraint tolerance is explicitly zero.
Observed feasibility is recomputed independently from the supplied signed
residuals; predictions never turn into accepted physical results.

The initial ordinary-EI integration improved the analytic constrained example
near its known optimum but repeated an evaluated point at iteration seven.
That failure and its sources are retained. [Ament et al., NeurIPS 2023](https://papers.neurips.cc/paper_files/paper/2023/hash/419f72cbd568ad62183f8132a3605a2a-Abstract-Conference.html)
describe numerical loss of EI values and propose LogEI. The backend implements
both; switching to LogEI passes the same eight-proposal test without relaxing
the test's physical constraint. This is evidence for the adapter, not a general
performance comparison between acquisition methods or proof of the cause of
the earlier repeated point.

This implementation is **EGO, not SCBO or TREGO**. Inspection of the pinned
backend's service API shows it creates proposal state anew from the supplied
history; it does not retain the full optimizer's trust-region state. SCBO,
contact-sequence tree search, multiple contact phase counts and a diversity
archive remain separate follow-ups from [the research review](SEARCH_STRATEGY_RESEARCH.md).

The adapter accepts explicit parameter names/units/bounds, objective identity,
constraint names/units/scales, context identity, observations and evidence
references. Each complete row trains the models; failed rows retain their
reason and are excluded without imputation. Context mismatches, nonfinite data,
missing outcomes, duplicate completed points, unsafe conditioning and
out-of-box/repeated proposals are errors. Failed trials are not yet modeled
by a viability classifier. Pending evaluations remain the caller's responsibility.
The seed and all training/failed row indices accompany the proposal.

`initial_design` reuses pinned EGObox DoE's seeded classic Latin hypercube.
The same shared CLI supplies initial designs and an independent sampling control.
All backend dependencies are optional and locked; no Python physics, duplicated
robot runtime, or change to an existing live executable is introduced. The
backend's default persistence feature is retained because its disabled-feature
build fails in upstream recorder modules; the failed build log is preserved.
This adapter does not enable backend result directories or load checkpoints.

## Behavior checks

Four focused tests verify seeded stratification, exact repeated proposals from
identical inputs, exclusion of failed observations, constant zero-boundary
constraints, context/input rejection, and improvement on an independently known
constrained quadratic. The unconstrained minimizer is deliberately infeasible;
the best result is chosen only among evaluated feasible points. All 39 shared
solver tests pass with the new feature. The CLI builds successfully. A dedicated
CI workflow runs these checks; remote CI has not run here.

## First robot comparison

`run_bayesian_controller_screen.mjs` orchestrates the existing Rust selector,
`run_environment`, exact Rhai replay, and Rust lift/geometry audit. Existing
measurement reducers retain their definitions. The fixed starting motion is
`return-x25-lift-v250`, whose reference-load diagnostic failures remain explicit.
Only three controller/task parameters vary together:

| Parameter | Experimental interval |
|---|---:|
| Requested speed | 0.10–0.32 m/s |
| Position tracking gain | 0–1 |
| Velocity lead multiplier | 0.5–1.5 times the existing D/K lead |

These are experimental search bounds, not derived hardware speed/actuator
limits. Input envelopes change explicitly to admit these commands; CAD,
transmissions, contact, physical motor bounds, target bounds, world, timestep,
controller code and reference motion stay fixed. A baseline replay at the
original parameter values matches every field of all 401 source frames after
excluding only `stepping_wall_s` telemetry. An earlier comparison that included
that field stopped; its complete capture, script and error are retained.

The objective minimizes negative minimum measured forward/reverse speed.
Eight distinct residuals preserve slip, speed error, heading, tilt, settling,
late drift, planned lift failures and sampled interlink overlap. Settling keeps
the pre-existing 1e-9 s roundoff guard; an unsettled record has an explicit failed
gate indicator rather than a fabricated settling time. A run or audit failure
gets a failed observation with no numeric score. Each successful measurement
asserts exact agreement between these residuals and the independent screen gates.

The recorded pilot uses seed 42, a baseline replay and 12 common Latin-hypercube
experiments. From that identical dataset it compares eight sequential Bayesian
trials against eight independently drawn Latin-hypercube trials. Requested
counts are evaluation budgets for this comparison, not a goal time limit.
All requests, candidates, simulator captures, commands, errors, replay/audit data
and wall costs are saved in the exclusively owned output directory. Source,
model, binary and metric identities determine its context hash.

The baseline measures 0.2609470707 / 0.2492291278 m/s, 16.379093% slip, 46/46
lift checks passing, and no overlap at 401 sampled poses. It remains infeasible.
The comparison completed successfully: all 29 episodes produced measurements,
and neither arm found a feasible point. Both arms share 13 initial observations
and add eight trials. Minimum slip among the eight adaptive trials was 7.376%;
among the eight sampling-control trials it was 8.716%; the common initial design
already contained a 6.759% result. All exceed the unchanged 5% slip gate, so
these minima do not establish a walking improvement or algorithm superiority.
The [completed compact evidence](bayesian-controller-pilot-v2-completed.json)
preserves every trial record, final result and their hashes. Full captures after
the earlier six-trial snapshot remain in the run directory pending archival.
Additional seeds, other motion/contact
families, timestep refinement, sustained control, steering and browser checks
remain necessary. The physical speed maximum is still unknown.
