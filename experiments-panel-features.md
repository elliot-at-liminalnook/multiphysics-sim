# Experiments panel: measured data and model refinement

Feature specification · September 13, 2026

**Agreed direction:** review existing hardware measurements and refine the
simulated model. Launching new hardware experiments is outside this first version.
**Agreed UI home:** the Rust systems viewer.
**Future direction:** the CAD viewer will also migrate to Rust. The experiments
workflow should carry forward into that experience, preserving saved comparisons,
model revisions, and links to the relevant CAD components. Its current home in the
systems viewer is the starting point, not a permanent separation from CAD.
Implementation of the first-version scope below is authorized. The complete workflow is now available in the Rust viewer; see [usage and verification](examples/experiments/README.md) and the [feature acceptance record](examples/experiments/IMPLEMENTATION.md).

## Purpose

Make it easy to answer three questions:

- Where does our model disagree with the real system?
- Does a proposed model change improve the behavior we care about?
- Does that improvement hold up on measurements we did not use to tune it?

Start with the Hiwonder HX-30HM motor recordings. The experience should also make
sense for later experiments involving other actuators, sensors, thermal behavior,
electrical systems, and coupled systems. Each experiment focuses on a measurable
aspect rather than requiring a full robot simulation.

## The first complete workflow

1. Open **Experiments** and select an existing measurement collection.
2. Choose a motor, trial, and measured behavior to investigate.
3. Compare the measurements with the current model under matching inputs and
   declared conditions.
4. Create a candidate model revision and change selected parameters.
5. Rerun the simulation and compare measurements, baseline, and candidate.
6. Evaluate the candidate on reserved validation trials, including regressions.
7. Save the comparison and a decision: retain the baseline, keep investigating,
   or mark the candidate as preferred for the tested conditions.

For example, select a low-drive trial where the real motor did not move but the
model predicted motion. Inspect the command and response, try a candidate change,
then check whether that change harms higher-drive or reverse-direction predictions.
The panel should reveal these tradeoffs without requiring manual plot generation.

## 1. Measurement library

Browse retained measurement collections and their individual trials. Show the
tested device, experiment type, command level and direction, duration, available
signals, and whether a trial is used for tuning or validation.

Filter by device, command range, direction, data role, and comparison outcome.
Keep failed and no-motion trials visible. Repeated trials remain individually
inspectable so variation is not hidden by averaging.

Each collection includes a readable account of its origin, available operating
conditions, and known limitations. Missing load, fixture, supply, temperature, or
controller information is shown as unknown; assumptions added for simulation are
visible alongside the measurements.

The initial supported collections are the retained HX-30HM PWM pulse studies:

| Collection | Recorded trials | Existing empirical-model validation |
| --- | ---: | --- |
| [Pilot pulses](examples/actuators/hx30hm/pwm-identification/README.md) | 63 | 33 of 36 held-out trials pass the recorded limits |
| [Full-range short pulses](examples/actuators/hx30hm/pwm-full-range-identification/README.md) | 216 | 81 of 162 held-out trials pass the recorded limits |

These are starting datasets, not a claim that all available recordings are
supported. Additional recording types can be added as their inputs, signals, and
comparison conditions are understood.

## 2. A clear comparison workspace

The Experiments panel lives in the Rust systems viewer. The proposed layout has a
measurement/trial browser on the left, the plots in the
center, and model settings and comparison details beside them. A collection
summary shows overall performance and the trials that need attention.

As the CAD viewer moves to Rust, the same experiment should be accessible from
the relevant component in either the schematic or CAD view. Switching views
should preserve the selected trial, candidate, and comparison context. This is a
future integration requirement; CAD migration is outside this first version.

For a selected trial, show:

- The command history, to explain what produced the response.
- Measured output, baseline prediction, and candidate prediction on shared axes.
- A residual plot showing prediction minus measurement.
- A shared time cursor, zoom, and signal toggles for inspecting events closely.
- Units, timing conventions, acquisition windows where available, and data gaps.

Start with encoder displacement from the PWM studies. Add other signals only
when they have a meaningful measured counterpart. Distinguish directly measured
signals from derived signals, such as speed calculated from encoder positions.

Timing differences are part of the evidence. Alignment must be explicit and
recorded; the panel must not silently shift curves to make a model look better.

## 3. Know what is being compared

Every comparison identifies its prediction source and how its inputs correspond
to the hardware experiment.

| Comparison | What the user can conclude |
| --- | --- |
| Recorded command replay | How well the model reproduces the measured response to the recorded input, subject to known timing and condition differences |
| Same-controller experiment | How well the modeled feedback loop reproduces hardware behavior when controller identity, configuration, and inputs are matched |
| Existing empirical response fit | How well a fitted input/output relationship predicts the measurements within its tested conditions |

**Recorded command replay is the first-version comparison mode.** Where legacy
data supplies only a reconstructed pulse, label that reconstruction and its
timing uncertainty. Do not call it a verified identical controller execution.

Keep existing empirical fits available as useful references, clearly distinguished
from physical simulation. A fit's gain or delay does not establish a physical
motor constant, friction value, or sensor latency.

When inputs or conditions cannot be matched, explain the difference. Exploratory
overlays may still be useful, but they must not receive a matched-validation label.

## 4. Understand the error

Show per-trial error and collection-level results. Initial metrics are overall
response error (RMSE), largest absolute error, and final displacement error.
Response delay, rise time, overshoot, and steady-state behavior can follow where
the experiment actually supports those measurements.

Show the acceptance limits and their units next to each scored metric. Preserve
the recorded limits for existing studies. A user can create a separate evaluation
with different limits, but the original result remains available.

Collection summaries should expose differences across motors, command levels, and
directions. Report trial counts, passes, failures, and unscored trials; an average
improvement must not conceal a new failure. The user can move directly from a
summary result to its supporting traces.

## 5. Refine a candidate model

Keep the baseline fixed while the user experiments with a candidate. Show editable
parameters with names, units, current values, and a short explanation of their
effect. Let the user change one or several parameters, reset changes, and rerun
the selected trial or a selected group of trials.

Display the changed values beside the resulting differences in error. Separate
model changes from changes to assumed experimental conditions, so a better match
has an understandable explanation.

The first version uses manual refinement. It must include a real simulation
rerun for supported motor experiments; displaying saved empirical predictions
alone completes only the measurement-review milestone.

Simulations show progress and can be cancelled. The user can continue inspecting
existing results, and an unsuccessful candidate run leaves the last successful
comparison available. Results retain the settings used to generate them; later
edits visibly mark those results as out of date.

## 6. Validate before preferring a candidate

Preserve each collection's existing tuning and held-out trial assignments. Use
whole trials for validation rather than splitting neighboring samples from the
same response into nominally independent datasets.

After tuning, compare baseline and candidate on the reserved trials. Show what
improved, what regressed, and which acceptance limits remain unmet. If validation
results guide further tuning, record that those trials have influenced selection;
fresh trials are needed for an independent confirmation.

A preferred candidate carries a statement of its tested scope: devices, command
range, direction, duration, and known operating conditions. Better agreement on
unloaded pulses does not establish loaded torque, thermal behavior, or full-robot
accuracy. The panel should support recording a need for more measurements when
the available evidence cannot distinguish competing explanations.

## 7. Save an evidence-backed decision

Save a comparison with its measurement references, baseline and candidate
revisions, changed parameters, input assumptions, tuning/validation assignments,
metrics, and notes. Reopening it restores the evidence used for the decision.

Allow a short conclusion such as “reverse response improved; low-drive behavior
still fails” and export a readable comparison summary with plots and results.
Preserve rejected candidates when they explain a tradeoff or failed hypothesis.

Marking a candidate as preferred does not overwrite the robot's CAD properties
or shared defaults. The first version retains a reviewable candidate and proposed
changes. Applying accepted physical properties back to CAD is a separate,
deliberate workflow.

## Delivery milestones

| Milestone | User-visible outcome | Completion check |
| --- | --- | --- |
| 1. Review existing evidence | Browse the two PWM collections, inspect commands and responses, and reproduce their saved empirical comparisons | Both collections retain their trial splits and reported validation outcomes; failures and timing limitations are visible |
| 2. Compare physical simulation | Replay supported recorded inputs through the current motor simulation and overlay its output | A selected trial has a reproducible simulation comparison with explicit conditions and input-matching limitations |
| 3. Refine and validate | Edit a candidate, rerun trials, compare against the baseline, and evaluate reserved trials | A candidate's improvements and regressions can be traced to saved settings and individual measurements |
| 4. Retain the decision | Save, reopen, and export a comparison and its conclusion | The complete comparison can be reviewed again without reconstructing it manually or changing CAD defaults |

Together, these milestones define the proposed first version. Milestone 1 is a
useful early preview, but does not by itself fulfill the model-refinement workflow.

## Outside the first version

- Connecting to hardware or launching new measurements from the panel.
- Automatic parameter fitting, optimizer-driven searches, or automatic adoption
  of a candidate.
- Verified same-controller hardware/simulation execution.
- Animated physical systems or synchronized 3D replay.
- Arbitrary recording-format import and automatic experiment interpretation.
- Claims about unmeasured properties or general sim-to-real accuracy.

## Initial implementation choices

- **First physical comparison:** reconstructed PWM pulses from both retained
  studies, replayed through the shared motor, gearbox and averaged H-bridge
  simulation. Zero duty assumes electrical braking; unknown firmware and fixture
  behavior keep these comparisons exploratory.
- **Initial editable parameters:** winding resistance/inductance, motor constants,
  no-load loss current, rotor and gearbox inertia, ratio, efficiency, friction,
  stiffness, damping, backlash, and driver resistance/current limit. Supply,
  temperature, load, command-delay and timestep assumptions have separate controls.
- **Baseline:** the existing endpoint-derived HX characterization hypothesis,
  captured with its source plan and explicit overrides. No candidate is promoted
  into CAD or treated as identified hardware calibration.

All four first-version milestones have been implemented. The
[acceptance record](examples/experiments/IMPLEMENTATION.md) distinguishes functional
verification from physical-model accuracy and lists the intentionally deferred
features above.
