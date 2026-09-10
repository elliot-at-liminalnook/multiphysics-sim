# Adaptive mixed contact search

`sim-solve::mixed_cem` now supplies persistent ask/tell selection for mixed
categorical and continuous experiments. It reuses the existing experiment
schema, explicit units/bounds, outcome definitions and seeded RNG. It is
compiled under the existing optional native `bayesian` dependency feature,
but uses no Gaussian-process surrogate or Bayesian acquisition function.

The method is an adaptation of
[CrEGOpt's mixed cross-entropy search](https://arxiv.org/abs/2410.02891).
Each population combines categorical draws with independent bounded Gaussian
draws. Elites update the category probabilities, means and diagonal variances.
The implementation adds a uniform exploration mixture, a continuous spread
floor and smoothed distribution updates. Objective and constraints have a
lexicographic ranking: every observed feasible result precedes violations;
infeasible results rank by their largest scaled violation, then objective.
Execution failures retain their reason and evidence without an invented score.
An all-failed population advances the generation but preserves its distribution.

State and pending populations are serializable. `tell` recomputes the seeded
population to reject edited or mismatched batches, and requires an outcome for
each point. The context identity belongs to the caller and includes models,
parameter meanings, gates and evaluation tools. A saved sampled-feasible result
is still subject to independent physical and controller qualification.

All 41 solver tests pass with the feature enabled. New tests approach the known
solution of a constrained mixed problem whose unconstrained winning category is
infeasible, verify serialized replay, preserve all-failed distributions, and
reject mismatched/nonfinite updates. This establishes component behavior, not
superiority on the robot.

## Robot pilot

The pilot selects among seven prepared contact-order starts and one or two
repetitions of each cycle. Repetition uses the shared Rust expansion component,
which gives each stance and step independent motion/force variables. Two
continuous initializer coordinates select period and displacement within the
existing explicit optimization boxes. A force-conic solve warms the subsequent
coupled body/feet/forces/timing solve. A conic result outside the exact force
boxes is retained but not used as a native initializer; the original valid
force start remains available. This does not relax bounds or discard a family.

The first launch exposed a configuration error: its JavaScript period edit left
body timestamps at the old period. All 24 trials failed validation and produced
no usable planning measurements. Their outcomes and original runner source are
retained; they do not train a physical failure classifier. The corrected runner
uses `JointContactMotion::with_bounded_overrides` and `set_joint_initializer`,
reusing the optimizer's decision decoder and contact-force timing. All 15
planner tests pass, including period edits that preserve force phase samples
and reject out-of-box or duplicate overrides.

This pilot uses three populations of eight and retains three elites, with
learning rate 0.7, uniform exploration probability 0.25 and normalized spread
floor 0.025. These are explicit numerical experiment choices. Each local solve
has the existing 1,200-model/five-iteration allowance. The selector's pilot
objective is sampled planning speed, not executed walking speed. Inner timing
remains free, unlike CrEGOpt's fixed-timing inner solve. Source templates have
different knot counts and different computational costs. The limited seed
orders and one/two repetitions do not cover all contact sequences.

Adaptive generation updates and planning improvements must be demonstrated by
completed results; launch alone proves neither. Passing candidates must next be
compiled and measured in the shared runtime. Local failure, exhausted budgets,
or stalled distributions cannot establish the robot's physical speed maximum.

## Concurrent measured controller search

The existing constrained LogEI selector also runs from `diagonal68-screen`,
whose baseline replays exactly. It changes command speed, tracking gain and
velocity lead while preserving CAD and physical gates. Requested speeds
0.06–0.16 m/s are experimental bounds. This remains a fixed motion family and
complements the contact search.

Initial-design trial 009 passes the eight-second screen at 0.074353/0.075074 m/s,
4.1694% slip, 30/30 lifts and zero sampled overlap. Adaptive trial 018 passes
at 0.080073/0.081901 m/s, 3.6921% slip, 28/28 lifts and zero sampled overlap.
These are measured short-screen results, not fully qualified gait promotions.
Twenty-second human/turn/stop and command-dropout cases are prepared for both,
including a 0.3125 ms step alongside 0.625 and 1.25 ms. The validation preparer
now accepts explicit command speed and constant policy inputs so optimized
tracking gains survive the historical action schedules; it preserves the
reference motion, nominal reference speed and physical model.

Trial 009's longer validation is now complete. At 0.625 ms it measures
0.075620/0.077636 m/s and passes control checks, but its 6.4573% slip exceeds
the 5% gate. The 0.3125 ms run similarly fails slip (6.5374%); its maximum body
position difference from 0.625 ms is 1.567 mm. The 1.25 ms comparison misses
the 3 mm tolerance by 0.000633 mm, which remains a failure. The dropout case
passes motion checks. Therefore trial 009 is not promoted despite passing its
eight-second screen. This demonstrates why the search needs longer-execution
feedback, not just a short-screen objective.

Audit correction: the first validation runner used partial lift windows that
omitted the turn. `turn-audit-correction.result.json` re-audits the original
captures with the historical windows [1.4,9.8] and [11,15.8]. The corrected
counts are 88/88 for all 77- and 78-command human cases, and 90/90 for all
81-command cases, with zero sampled overlap. Earlier counts below refer to
the initial partial windows; their observed failures remain additional evidence
and are not erased by the different windows. All three candidates still fail
the unchanged longer slip gate. `run_controller_validation-v1.mjs` preserves
the original runner with a hash verified against the earlier evidence manifest.

The controller pilot completed all 29 measurements. Its shared 13-point initial
dataset contains one passing trial. The next eight adaptive trials contain six
short-screen passes; the eight independent sampling-control trials contain one.
The best adaptive minimum directional speed is 0.080073 m/s, versus 0.075772 m/s
for the sampling control. This is one seeded comparison, not a general algorithm
ranking. All terminal records are in `bayesian-diagonal68-completed.json`.
The sampling-control winner (trial 025) has only 1.9818% short-screen slip and
also enters longer validation; an algorithm label does not determine promotion.

Trial 018 also fails the longer slip gate: 7.9593% at 0.625 ms and 7.7866% at
0.3125 ms. Both pass control and all 76 lift checks with zero sampled overlap;
the fine-step comparison differs by at most 1.838 mm. Its dropout case passes.
The full results are in `mixed-validation-extension.json`; it is not promoted.

`diagonal-contact-windows.result.json` partitions the existing slip integral
without changing the metric. For the worst foot (-X), trial 018 accumulates
60.7% of its loaded sliding path during the turn and 25.3% during forward
travel. Trial 009 splits 50.3%/44.9% between those phases; the 68-command source
is 85.9%/9.3%. This locates failure within the longer schedule but does not prove
its cause. Next feed the full human schedule into adaptive runtime selection;
do not optimize only the short screen or remove turning from qualification.

Trial 025 also fails the human schedule: 6.2811% slip and 71/72 lifts at
0.625 ms; the finer step retains both failures. It passes control and the
dropout case, but is not promoted. Its terminal summaries and exact preparation
settings are retained; the remaining full captures are pending archival.

The runtime runner now supports a `human20` evaluation profile. Its objective
uses measured forward/reverse speed from the full human schedule; all existing
control/contact/lift/geometry gates remain, with the turn-response residual
explicitly supplied to the selector. `bayesian-human20-pilot.spec.json` starts
from the existing 68-command human recording. It uses an experimental
0.06–0.12 m/s command box and 0–1.5 tracking-gain box, with unchanged physical
actuators and target limits. This is a new context; eight-second observations
are not treated as twenty-second measurements. The new pilot must establish
its own measured improvements, followed by the other qualification checks.

The first human20 pilot was stopped after its audit-window error was found,
before selector fitting. Its completed/partial trials are retained under the
old context and excluded from the corrected run. `bayesian-human20-pilot-v2`
uses the historical turn-inclusive lift windows. Its baseline physically
replays the 1001 source frames exactly and passes all nine measured residuals:
0.066647/0.067608 m/s, 4.5122% slip, 78/78 lifts and zero sampled overlap.
The corrected adaptive search is active. No new gait is fully qualified and
the physical maximum remains unknown.
