# Composite trajectory-response search

The shared Rust optimizer can now rank candidates by expected improvement in
a cheap function of expensive measured responses. For the gait experiment,
the responses are translation, heading and body-frame velocity/turning from
the existing two prefix fits. The outer function uses the existing shared
SE(2) integration to predict net displacement per full episode duration.

`sim_solve::composite::rank_candidates` fits independent constant-mean
Matern-5/2 response GPs with pinned EGObox 0.36.3. Seeded common Gaussian
samples propagate uncertainty through the nonlinear outer function. The
selection score is Monte Carlo expected improvement, rather than improvement
evaluated only at the mean response. A deterministic hyperparameter start
avoids the backend's entropy-seeded single-restart path. All observations
retain context, units and evidence; failed observations are not imputed or
proposed again. This initial interface rejects constraint definitions rather
than ignoring them. Physical survival is evaluated externally in the unchanged
Rust runtime before a successful prefix supplies a response vector.

The method is inspired by [Bayesian Optimization of Composite Functions](https://proceedings.mlr.press/v97/astudillo19a.html).
That paper propagates a multi-output GP through a cheap outer function and
optimizes composite expected improvement. Our implementation ranks a finite
candidate set and assumes independent response outputs. It does not reproduce
the paper's acquisition-gradient optimizer or establish its convergence claims.
The two fit windows share data, including identical endpoint positions; their
posterior covariance is currently omitted. Neither uncertainty calibration nor
improved sampling efficiency is established.

## Verification and first experiment

All 46 `sim-solve` library tests pass with Bayesian support. Three focused
composite tests check analytic Gaussian EI, nonlinear uncertainty propagation,
zero-variance behavior, reproducibility, response units, context/dimension
rejection, failure identities and discovery of an unsampled minimum of an
oscillatory objective from its smooth intermediate response. The command-line
example builds and is included in the Bayesian CI build step; remote CI has not
been run in this worktree.

The first robot experiment trains on four steering seeds and candidates 000–006
from the joint nine-dimensional search, all at the same 0.3125 ms timestep and
seed 2301. Its 12 response components come from the existing 5–20 and 10–20
second fits. The Rust outer function reproduces all 11 original scalar
forecast scores with zero observed numerical difference. Later candidates
are excluded from this frozen training context.

The pool contains 1,024 seeded candidates in a normalized radius 0.025 around
the best training point and 1,024 across the declared full domain. These are
proposal regions, not physical restrictions. The selected local candidate
has speed 0.5553837458 m/s at the mean response, posterior mean speed
0.5331547244 m/s and expected improvement 0.0018832262 m/s from 1,024 posterior
samples. Those different values illustrate why applying the trajectory formula
only to posterior means is insufficient. These are model outputs, not measured
gait performance. Its physical trial is `runs/composite-planar-probe-v1/candidate20`.

That trial now completes without a sampled fall at **0.5400275693 m/s over
20 seconds**. Actual fitted turning is 0.0238530716 / 0.0235272344 rad/s,
versus predicted means -0.0010564973 / -0.0037318128 rad/s. Its new trajectory
forecasts are only 0.0647477553 / 0.0586193597 m/s over 300 seconds. The
model's mean and uncertainty therefore miss the command's turning response;
this candidate is not promoted as an improvement. The response audit retains
errors in physical units and validates exact scene/actions and all prefix steps.
The next model update should consume this counterfactual result and the now
completed scalar-search data. This failure does not establish that the composite
approach is worse generally, but there is no demonstrated robot benefit yet.

## Completed physical evidence and continuing trials

The fixed fine winner completes its 300-second test at 0.15625 ms:
160.8551528061 m, or **0.5361838427 m/s**, with no sampled fall. The check verifies
all 1,920,000 steps, 15,000 authored actions, the exact scene, and exact parity
with its earlier 1,001-frame/transition prefix, excluding wall timing.
This is 1.9579% below the same controller's 0.5468916069 m/s at 0.3125 ms.
The new gait improves on the previous neural actor at this resolution, but
timestep convergence and the physical speed limit remain unproven.

The slower ideal-heading feedback controller's unchanged 0.15625 ms full run
is `runs/heading-feedback-quarter300-v1/quarter300`. Its fine-resolution
300-second result was 0.5468435312 m/s. The new run tests whether feedback
recovers net distance under the remaining timestep-dependent turning.

The twelve-proposal scalar-GP batch has also completed without sampled prefix
falls. Candidate 010 reaches 0.5584056911 m/s over 20 seconds; its two forecasts
are 0.5557019300 and 0.5217829471 m/s. That disagreement remains relevant when
interpreting the optimistic selection score. Its unchanged full-duration
validation is running at `runs/joint-command-sustained-v2/candidate010`.
Candidate 002's full run and the original fine winner's inter-link contact
check also continue. No new sustained-speed qualification or neural-learning
gain is inferred from these forecasts.

`composite-speed-search-evidence-v1.json` durably records code, tests, the frozen
response experiment, completed scalar-search and third-timestep evidence, and
explicit immutable inputs for trials that were live when archived.

## Shared responses and additional observations

`PlanarResponseWindow` in the shared control library now binds pose and twist
to response-vector indices. Reusing position indices makes both windows use the
same sampled endpoint; the previous experiment modeled two independent copies
of that identical observation. The updated layout has ten responses instead
of twelve. Remaining response covariance is still omitted. The CLI accepts
explicit candidates for prediction audits, validates binding units, and retains
the original disjoint-block layout. Replaying the old request reproduces its
complete proposal/ranking exactly. Six planar-prediction tests pass, including
shared-anchor translation, missing/nonfinite channels and existing turn cases;
an invalid SI-unit binding is rejected before producing a proposal.

Five later scalar-search trials allow a controlled diagnostic on the same
previously missed counterfactual input. That probe remains excluded from both
the 11-row and 16-row fits. Both use the shared-position layout and identical
prediction settings. Absolute turning errors decrease from 0.0249096 to
0.0165822 rad/s for the 5–20 s window, and from 0.0272590 to 0.0230061 rad/s for
10–20 s. These are improvements on one held-out input, not a general sample
efficiency result. The errors remain 3.04 and 3.76 times their estimated standard
deviations; model uncertainty remains provisional.

The next proposal includes that probe in a 17-row training set. All 17 original
forecast scores recompose with zero numerical difference using the shared
Rust integration. A descriptive count inherited from the older parity report
is corrected in `composition-parity-note.json`; the executed check and numeric
count already cover all 17 rows. The selected candidate predicts 0.5607031075 m/s
at the mean response, with posterior mean speed 0.5282527931 m/s and expected
improvement 0.0024464371 m/s. Its new physical test is
`runs/composite-planar-probe-v2/candidate20`; no speed improvement is assumed.

The updated candidate now completes its 20-second prefix without a sampled
fall at **0.5634429022 m/s**, covering 11.2688580435 m. Its fitted body speeds are
0.5716530102 / 0.5720809083 m/s, with turning -0.0040378262 / -0.0039225005 rad/s.
The two long-run forecasts are 0.5370270765 / 0.5392802106 m/s, below the current
sustained winner, so the short-run speed is not a full-duration qualification.

Two new steering probes preserve this entire gait and change only yaw input.
Their symmetric half-span, 0.0226251191 rad/s, is the current late-window turn
divided by the magnitude of the earlier fine-gait steering gain. That prior
gain selects a measurement spacing; its accuracy is not assumed for this new
trajectory. The probes will identify the current response before a root proposal
is tested. Reducing curvature is one way to increase net distance, not a new
zero-turn acceptance constraint. Inputs are in
`runs/composite-steering-response-v2/{negative,positive}`.

## New completed sustained-speed result

Joint-search candidate 002 now completes 300 seconds at **0.5512450674 m/s**,
covering **165.3735202142 m** without a sampled fall at 0.3125 ms. This is a
0.7960% increase over the previous same-timestep winner. All 960,000 steps,
15,000 actions and the complete scene/input schedule are verified; the first
1,001 physical frames and transitions exactly reproduce its earlier prefix,
excluding wall-clock timing. This candidate came from the seeded local
identification design, selected for full validation by its trajectory forecast;
the gain is not attributed to composite acquisition or neural learning.

Its unchanged 0.15625 ms full run is active at
`runs/joint-command-quarter300-v1/candidate002`. Inter-link contact results for
the previous winner do not certify this new trajectory. The scalar search's
candidate 010 and the older heading-feedback quarter-resolution tests continue.
`joint-command-sustained-result-v1.json` records the new result;
`composite-response-update-evidence-v2.json` preserves the code, diagnostics,
completed physical evidence and immutable inputs for active trials.

## Completed steering probes of the updated composite gait

Both probes complete their 20-second intervals with exact scene/input checks,
all 64,000 steps and no sampled falls. The negative probe at yaw request
0.1031773452 rad/s reaches **0.5636470088 m/s**, with full-duration forecasts
0.5619602489 / 0.5629590591 m/s. The positive probe at 0.1484275833 rad/s reaches
0.5634560721 m/s, forecasting 0.5606156220 / 0.5616859560 m/s.

Both reduce the fitted turn magnitude relative to the center observation, so
the measured prefix response is strongly nonaffine over this interval. Shared
Rust affine fits give gains -0.00280084 / -0.00275905 and residual RMS
0.00088489 / 0.00086482 rad/s. Their zero-turn extrapolations, near -0.869 and
-0.853 rad/s, lie far outside the observed request interval and are not treated
as validated steering corrections. This directly shows why the previous gait's
gain could select a probe spacing but could not certify a new controller.

The better measured probe is now running unchanged for 300 seconds at
`runs/composite-steering-sustained-v2/negative300`. Restoring its first 20 seconds
reproduces the exact authored prefix input JSON. Full-duration performance and
physical prefix parity remain to be checked when it completes.
`composite-steering-evidence-v2.json` preserves both closed probes, calibration
diagnostics and immutable full-run inputs. These probes finished during the
preceding archival operation; that earlier manifest intentionally retained only
their then-live inputs.

## Subsequent completed full runs

Scalar-GP candidate 010 now completes 300 seconds at **0.5523818095 m/s**,
covering 165.7145428632 m without a sampled fall at 0.3125 ms. It exceeds
candidate 002's 0.5512450674 m/s. Verification covers all 960,000 steps,
15,000 actions, the exact scene/input schedule and exact parity with the earlier
1,001-frame/transition prefix, excluding wall timing. Its two earlier forecasts
were 0.5557019300 / 0.5217829471 m/s. Its unchanged third-resolution test is
`runs/joint-command-quarter300-v2/candidate010`; the new result is a fine-model
speed qualification, not a completed fidelity or physical-limit claim.

The slower heading-feedback controller also completes 300 seconds at
0.15625 ms: **0.5468228713 m/s**, versus **0.5468435312 m/s** at 0.3125 ms,
with no sampled fall at either resolution. The relative speed difference is
0.003778%. Its third-resolution scene/actions and 1,001-frame prefix match are
verified. The endpoints nevertheless differ by **3.7133 m**, so scalar speed
agreement must not be described as full-trajectory convergence. This feedback
result belongs to its own controller context and is not mixed into the constant
steering response-model training set.

`joint-command-sustained-result-v2.json` records these completed results and
`joint-sustained-late-evidence-v1.json` archives their full captures, checks and
immutable inputs for the active candidate002/candidate010 refinement runs and
the faster composite negative-probe300 trial.

## Automatic response updates and a faster completed gait

The negative steering probe now completes **300 seconds at 0.5621811105 m/s**,
covering **168.6543331506 m** without a sampled fall at 0.3125 ms. This improves
on candidate010 by **1.7740%**. The check verifies all 960,000 steps, 15,000
commands, exact scene/input events and exact parity with its earlier 1,001-frame
prefix and transitions, excluding wall timing. Its two prefix forecasts were
0.5619602489 / 0.5629590591 m/s. This candidate originates in composite acquisition
followed by measured steering probes; it does not establish a matched-budget
sample-efficiency gain or a neural-training gain.

Two unchanged-controller validations are active: the 0.15625 ms full300s test in
`runs/composite-quarter300-v2/negative300`, and the 0.3125 ms full300s test with
inter-link contact enabled in `runs/composite-contact300-v2/negative300`. Input
comparisons allow only the respective timestep/count changes or contact flag.

The earlier candidate002 also completes its third-resolution run at
**0.5361298664 m/s**, covering 160.8389599223 m without a sampled fall. It is
**2.7420%** below its 0.5512450674 m/s fine result. All 1,920,000 requested steps,
scene, commands and seed are verified, and the matched input changes only the
timestep and associated counts/indices. The remaining fidelity sensitivity
prevents treating fine-model speed rankings as converged physical rankings.
Candidate010's third-resolution full run remains active.

`run_composite_planar_speed.mjs` now closes the model/experiment loop automatically.
It begins with the preceding 17 training rows plus the updated composite gait
and its two steering probes, all from the same 20-second fine-fidelity context.
Each iteration asks shared Rust composite EI to rank 1,024 local and 1,024
full-domain candidates, tests the selected trajectory in the unchanged Rust
runtime, and adds its measured response vector before selecting again. Failed
physical trials remain failed outcomes. Each proposal checks that Rust's outer
composition reproduces every stored forecast objective within 1e-12; full300s
measured scores never enter this short-prefix prediction dataset.

The reusable `planar_speed_experiment.mjs` supplies physical evaluation and
Rust motion fitting to both scalar and composite search drivers. The new runner
exactly reproduces 101 frames and transitions from candidate002's first two
seconds, excluding wall timing. Four rejection checks cover an invalid controller
grid, a fit ending before the prefix, an unbound response and inconsistent shared
positions. Those diagnostic two-second fits are excluded from training. Source
models, binaries, configurations and seed observations are pinned; an immutable
iteration record is written after each result, and STOP cancels between trials.

The first automatic proposal completes 20 seconds at **0.5626021032 m/s**, with
forecasts **0.5693026785 / 0.5625466346 m/s**. Its mean-response forecast before
testing was 0.5494246644 m/s. The new measurement is now part of the next model;
prefix performance alone does not qualify a sustained improvement. The batch is
`runs/composite-adaptive-planar-v1`, with six requested adaptive evaluations.

`composite-sustained-result-v3.json` records the completed full-episode results.
`composite-adaptive-evidence-v1.json` preserves code, checks, closed captures,
seed data and explicitly selected immutable inputs for active work. Active
outputs are excluded. No physical speed maximum or sim-to-real accuracy is proved.

The second automatic proposal completes at **0.5709580590 m/s over 20 seconds**,
but its fitted turning lowers the full-duration forecasts to
**0.4411999644 / 0.4355318207 m/s**. It remains a measured response observation,
without being promoted on short speed alone. The third proposal is generated
from all 22 observations and again passes the complete forecast-composition
check. This records actual model feedback across multiple selections; it is
still not a demonstrated global optimum or a calibrated predictor.

## Completed automatic batch and full-episode transfer

All six adaptive trials complete their 20-second intervals without sampled
falls. The audit verifies that each response enters every later proposal's
training set: 20 initial observations grow to 26, with exact response bindings
and original Rust forecast scores. Model selection remains based on the
forecast objective, not the measured300s objective.

| Trial | Physical20s speed, m/s | Best prefix-fit300s forecast, m/s |
| --- | ---: | ---: |
| 000 | 0.5626021032 | 0.5693026785 |
| 001 | 0.5709580590 | 0.4411999644 |
| 002 | 0.5701780099 | 0.5241496955 |
| 003 | 0.5612390948 | 0.5325774002 |
| 004 | 0.5537808566 | 0.1045544367 |
| 005 | 0.5674278962 | 0.3773527697 |

For the two turning response channels, RMS error divided by each proposal's
predicted standard deviation is **2.1863 / 1.8722**, respectively. These errors
are computed before the observation enters training, avoiding an in-sample
residual claim. Six adaptively chosen points do not establish a coverage rate,
a calibrated uncertainty model or a sample-efficiency benefit. They do identify
underestimated turning uncertainty as a weakness to address in further search.

Trial000 ranks first by the declared forecast objective and is now undergoing
an unchanged full300s test at `runs/composite-adaptive-sustained-v1/candidate000`.
Its two forecasts are 0.5693026785 / 0.5625466346 m/s. No improvement over the
verified 0.5621811105 m/s winner is assumed. Shorter-run speed alone does not
promote the faster-turning alternatives, nor are they declared physically
incapable of improvement.

`examples/interactive/run_prefix_candidate_episode.mjs` provides the reusable
handoff. It extends only the requested step count and the already-authored full
command sequence, preserving the prefix's controller, world, CAD physics,
initial state, seed and pinned runtime. Its terminal check verifies the entire
full episode and exact original prefix frames/transitions, excluding wall time.
The regression reconstructs the verified0.5621811105m/s full recipe exactly and
rechecks its capture. Five negative checks reject changed prefix commands,
missing actions, an invalid step grid, incomplete output and an altered physical
prefix. Runtime failures remain failures with no invented speed.

`composite-adaptive-batch-v1.json` records the audit and selection;
`composite-episode-transfer-evidence-v1.json` archives the closed six-trial batch,
shared handoff code, regression evidence and immutable new full-run inputs.
Earlier complete seed provenance remains in `composite-adaptive-evidence-v1.json`
and its predecessor manifests. Other active full-duration fidelity/contact
trials continue; neither those inputs nor this forecast establish a physical
speed limit.

## Uncertainty rescaling and another physical candidate

The shared Rust `uncertainty::fit_standard_deviation_scales` fits one positive,
dimensionless multiplier per response. With fixed predictive means, the Gaussian
negative-log-likelihood optimum is the RMS standardized prediction error:
`s_j = sqrt(mean(((actual_j - mean_j) / std_j)^2))`. This follows the sigma-scaling
method in [Laves et al., Eq.10](https://proceedings.mlr.press/v121/laves20a/laves20a.pdf).
Their method calibrates a fixed model on separate data. Here the six predictions
come from successively updated models and adaptively selected inputs, each
excluded when its prediction was made. Applying their scales to the next model
is an experimental adaptation, not a coverage guarantee.

`composite::Config.response_std_scales` optionally rescales GP deviations before
Monte Carlo composition. It changes neither means nor the physical objective.
Empty scales preserve legacy behavior and serialization. The calibration CLI
retains response names, units and source evidence, and rejects degenerate or
nonfinite data. All **48 solver library tests pass**, including analytic NLL
minima, unit conversion, invalid data, unchanged GP means and exact posterior
rescaling. The new CLI is included in the existing Bayesian CI build; remote
CI has not been run. Both new binaries are pinned separately from active runtimes.

The complete old proposal005 report/ranking reproduces exactly with the new
binary and rescaling disabled. The controlled updated comparison uses the same
26 measured responses, 2,048 candidate points and Monte Carlo seed for both
rankings. All candidate means and mean-composed forecasts are exactly identical;
reported deviations equal raw deviations times the fitted scales. The two
rankings also compose the observed scores identically. Four original fit scores
differ from the shared outer computation by **1.11e-16**, within the existing
1e-12 experiment tolerance. An initial bit-equality assertion was too strict;
its failure and the corrected tolerance check are both retained.

The turning scales are **2.1863 / 1.8722**. Both rankings nevertheless select
exactly the same new parameter point, with mean-response forecast
**0.5715556431 m/s**. Its physical test completes 20 seconds without a sampled
fall at **0.5758768747 m/s**, but measured turning of
**-0.00894977 / -0.00882966 rad/s** lowers its new full-duration forecasts to
**0.4250479109 / 0.4290399582 m/s**. On this excluded point, turning errors change
from **-1.4110 / -0.5661** raw standard deviations to **-0.6454 / -0.3024** rescaled
standard deviations. One point cannot establish uncertainty calibration, and
identical selection provides no calibration-induced search-speed gain.

Its unchanged 0.15625 ms prefix test is
`runs/composite-uncertainty-quarter20-v1/candidate20`. This separate fidelity
outcome will not be merged into the fine training set without an explicit model
of timestep dependence.

Candidate010's full third-resolution run also completes, at
**0.5216591644 m/s over 300 seconds**, covering **156.4977493138 m** without a
sampled fall. This is **5.5618%** below its 0.5523818095 m/s fine result. Exact
matched input checks allow only timestep/count/index changes; all 1,920,000
steps, 15,000 commands, scene, seed and sampled survival are verified. This
fidelity dependence is substantial; the fine-model rankings are not converged
physical rankings. The separate 0.5468228713 m/s heading-feedback result remains
stronger at this timestep than either candidate002 or candidate010.

That matched third-resolution prefix now completes at **0.5760603777 m/s**,
within **0.031865%** of the fine short-run speed, without a sampled fall.
Verification covers the exact timestep-only input transformation, all128,000
steps, 1,001 frames, scene and command events. Fitted body speeds are
**0.5855272405 / 0.5857332251 m/s**; turning is
**-0.00654542 / -0.00645618 rad/s**, giving forecasts
**0.4957520140 / 0.4981963997 m/s**. Similar short speed does not establish
long-horizon or trajectory convergence. The faster body motion is a useful
candidate for directional-feedback experiments, with sustained net speed as
the objective and no new heading acceptance constraint.

`composite-uncertainty-result-v1.json` records the uncertainty comparison,
excluded physical probe, matched finer prefix and completed candidate010
refinement. `composite-uncertainty-evidence-v1.json` archives their source,
checks and closed evidence; current-winner full fidelity/contact and adaptive
candidate000 full-duration tests remain separate active work.

A directional-feedback transfer test is now active at
`runs/composite-heading-transfer-v1/feedback20`, using the faster gait's
0.15625 ms recipe. It reuses the successful earlier shared angular-filter
feedback algorithm and its parameters as an explicit initialization; the
response gain is not represented as identified for the new gait. The only
additions are its policy block/parameters and the declared ideal-heading
observation. CAD, world, actuators and authored commands are unchanged.
With feedback disabled, all101 frames/transitions from the first2s reproduce
exactly, excluding wall timing and the100 new heading observations. The enabled
trial will be judged by net speed and sampled survival, with no heading or
tracking acceptance threshold. No outcome is assumed before it completes.

## Further completed sustained results

Automatic trial000 now completes **300 seconds at 0.5671121795 m/s**, covering
**170.1336538489 m** without a sampled fall at 0.3125 ms. This improves on the
previous fine best by **0.8771%**. The reusable full-episode validator verifies
all960,000 steps, 15,000 commands, exact scene/input events and the original
1,001-frame/transition prefix, excluding wall timing. Its matched third-resolution
and inter-link-contact runs are active in
`runs/composite-adaptive-{quarter,contact}300-v1/candidate000`.

The previous composite negative-steering winner also completes its unchanged
0.15625 ms run, at **0.5735639103 m/s over300 seconds**, covering
**172.0691730779 m** without a sampled fall. This is **2.0248%** above its own
0.5621811105 m/s fine result. All1,920,000 steps, scene, commands and seed are
verified, and only timestep/count/index changes are allowed in the input
comparison. The endpoints differ by approximately **61.83 m** despite the
closer scalar speeds; trajectory convergence remains unproved. Its inter-link
contact test at this same finer timestep is now
`runs/composite-quarter-contact300-v2/negative300`.

The transferred heading-feedback trial completes20 seconds at **0.5761434325 m/s**,
without a sampled fall, but its mean turn becomes
**-0.00847126 / -0.00853531 rad/s**, versus
**-0.00654542 / -0.00645618 rad/s** without feedback. Its forecasts fall to
**0.4406396871 / 0.4388948990 m/s**. It is not promoted as a sustained improvement.
The shared Rust/Rhai diagnostic replay reproduces all12,000 actuator commands
exactly. The inherited yaw-offset cap is active for only12 of4,000 sampled hip
offsets; the observed filtered heading bias ranges from -0.14517 to0.02033 rad.
The old gait's feedback gains are therefore not assumed to solve the new gait's
directional response. Further identification can use this losing trial as
evidence, without imposing a new heading acceptance rule.

`composite-sustained-result-v4.json` records the two completed full runs and the
closed feedback transfer. `composite-completed-results-evidence-v1.json` preserves
their captures, checks, diagnostic state and explicitly selected immutable
validation inputs. Fine and third-resolution bests use different controllers;
no global physical maximum, timestep convergence or sim-to-real accuracy is
claimed.

The previous composite gait's matched inter-link-contact run now completes
**300 seconds at 0.5685365125 m/s**, covering **170.5609537531 m** without a
sampled fall at0.3125 ms. The input comparison changes only the contact-omission
flag; all960,000 steps, 15,000 actions, seed and the complete scene are verified,
allowing only serialization's omission of the false flag. This is a contact-enabled
result for that exact gait and timestep, not a transfer of qualification to other
controllers or resolutions. Its0.15625 ms contact test remains active.
`composite-contact-result-v3.json` records this outcome and
`composite-contact-evidence-v3.json` archives the full matched check. These
outputs completed after the preceding results archive was frozen.
