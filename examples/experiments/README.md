# Measured-data experiments in the Rust viewer

The first complete workflow reviews the retained Hiwonder PWM studies, replays
reconstructed inputs through shared motor/H-bridge physics, compares a fixed
baseline and editable candidate, evaluates whole held-out trials, and saves or
exports the evidence. This is offline work; it never connects to hardware.

## Open the panel

```sh
cargo run --locked -p sim-viewer -- --experiments examples/actuators/hx30hm/pwm-full-range-identification
```

Or choose **Experiments** from the native systems viewer toolbar. Switch between
pilot and full-range studies with the collection buttons. Open reviews retain
their independent drafts, histories, notes and selected trials when you switch.

1. Select a trial. Use the device, direction, absolute-duty range, tuning/held-out,
   and outcome filters to find a behavior. No-motion and failed trials remain.
2. Click **Run selected trial** to compare the fixed physical baseline with the
   current candidate. The empirical fit is a separate reference overlay.
3. Edit candidate parameters on the right. Hover a label for its meaning. Model
   units and validation come from the component registry. Use the conditions
   section for supply/temperature overrides, assumed fixture inertia/torque,
   explicit command delay, and integration step.
4. **Run filtered trials** evaluates the visible selection. **Evaluate held-out
   trials** uses all held-out trials for the selected device (or all devices),
   independently of the other filters. Cancellation retains a partial evaluation
   with unscored entries and leaves earlier evaluations available.
5. Choose an evaluation from history. See the captured parameter changes,
   per-trial scores, aggregate results, regressions, and new failures. Results
   are labelled out of date when their captured settings differ from the draft.
6. Record a decision and notes. **Open / save / export** saves an immutable JSON
   review or a standalone HTML report with plots. Supply a **new filename** for
   each saved revision; existing evidence is never overwritten. Reopen a JSON
   review using the same Open field or `--experiments` argument.

The physical baseline comes from the existing HX endpoint characterization
hypothesis in `examples/actuators/hx30hm/plan.json`, including its explicit
motor overrides. Fixture inertia starts at an explicitly assumed 0.00002 kg·m²;
load torque is zero. This is not identified hardware calibration. The recorded
source plan, resolved parameters, runtime identity, integrator, seed, measured
traces and input-file hashes travel with the review.

## Interpretation

- Reconstructed PWM edges use recorded host-command midpoints. Supply and
  temperature default to reported range midpoints, not exact time histories.
- The shared averaged H-bridge assumes zero-duty electrical braking. The
  proprietary firmware and unknown sensor sample age are not reproduced.
- Temperature is clamped for the short-pulse comparison; this is not a thermal
  validation. Fixture properties are unmeasured. A constant load torque is signed,
  opposing positive motion; it is not an automatically reversing friction load.
- Predictions are sampled at measurement times without fitting a time shift.
  Physical angle is continuous; measurements retain encoder quantization. Grey
  plot regions flag gaps larger than 2.5 times the median sample spacing.
- Original empirical limits/splits are preserved. Optional new evaluation limits
  do not rewrite historical results. Editing after viewing held-out details or
  running held-out simulations flags that validation has influenced selection.
- A preferred candidate is scoped to the retained trials and conditions. No
  parameters are automatically promoted into CAD or default models.
- Raw-source verification is performed at import. Portable saved snapshots retain
  that import-time status and source references, and validate their internal
  trace/metric consistency when reopened. They are not authenticated certificates.

## Headless counterpart

The same comparison runner can produce review files and reports without a GUI:

```sh
cargo run --locked -p sim-runtime --example review_experiments -- \
  examples/actuators/hx30hm/pwm-identification /tmp/pilot-review.json --all
```

This demonstration changes candidate output friction to 0.025 N·m. It is a test
hypothesis, not an accepted improvement. Omit `--all` for ID 4's ±10% trials.
A nonzero exit after saving means some physical trials were unscored; inspect
retained failures. A failed physical accuracy gate is an expected scientific
result and does not by itself mean the comparison tool failed.

## Verification

```sh
cargo test --locked -p sim-runtime -p sim-viewer \
  --test experiment_comparison --test experiment_study --bin sim-viewer
```

Checks include retained empirical results, exact comparison times/units, physical
candidate effects and timestep sensitivity, scheduled positive/negative pulses,
invalid candidates, cancellation, saved-evidence consistency, non-overwriting
persistence, HTML escaping, UI filters, and the background-worker review lifecycle.

The product features are specified in [experiments-panel-features.md](../../experiments-panel-features.md).
CAD migration, hardware execution, same-controller parity and automatic fitting
remain outside this first version.

A ready-made comparison is retained at
[evidence/pilot-demo.review.json](evidence/pilot-demo.review.json), with its
[HTML report](evidence/pilot-demo.review.html) and
[native panel preview](evidence/panel-preview.png). Open the JSON through
`--experiments` to start with measured, baseline and candidate curves already present.

## Controller refinement and combined evidence

The newer **Controller refinement → Fitting** view can fit one candidate to
pulse, release and recorded controller-command responses together. See the
[current acceptance status](CONTROLLER-REFINEMENT-STATUS.md) for measured failures
and the remaining physical validation work.

1. Set the measurement filters for the pulse/release trials to include. Preserve
   both original training and reserved repetitions in the selection.
2. Assign complete controller recordings to tuning or validation, with a rationale.
   These assignments and their prediction limits cannot be changed afterward.
3. Enter bounded shared parameters or device deviations. Optionally enter another
   saved review: its pulse/release trials for the selected controller device join
   the selection. Its controller recordings are not imported by this field.
4. Choose **Fit combined evidence**. Each whole trial contributes residuals scaled
   by encoder resolution and the square root of its sample count, so longer
   recordings do not automatically dominate. More repetitions still contribute
   more total weight. Reserved trials are scored after optimization only.
5. Review each trial's outcome and the frozen source snapshots. A fitted model is
   not automatically applied to the draft or CAD. Test its independently simulated
   feedback controller separately; command replay does not establish closed-loop
   predictive accuracy.

The optional additional source is read by the background worker. Duplicate trial
IDs are rejected. Cancelled/failed fitting attempts retain their datasets and
objective history. Imported, already inspected evidence is labelled as influencing
validation; a subsequent fresh confirmation is still required.

The same workflow is available through `review_controller fit-combined`:

```sh
bench_data=examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement
cargo run -p sim-runtime --example review_controller -- fit-combined \
  "$bench_data/release-fit-controller-review.json" \
  "$bench_data/combined-loss-inertia-fit-request.json" /tmp/new-combined-review.json
cargo run -p sim-runtime --example review_controller -- evaluate-combined-fit \
  /tmp/new-combined-review.json 0 /tmp/new-combined-controller-review.json
cargo run -p sim-runtime --example review_controller -- export \
  /tmp/new-combined-controller-review.json /tmp/new-combined-controller-review.html
```

The request contains `additional_studies`, immutable recording `assignments`, and
one `fit` request with explicit training/validation IDs, bounds and evaluation
budget. Source files remain unchanged; every output requires a new path. All of
these commands are offline and do not open the motor bridge.


## Low-speed loss hypothesis

`motor.loss_speed_scale` exposes the rotor-speed width of the motor's smooth
Coulomb-loss approximation, in rad/s. The historical default remains 5 rad/s.
Smaller positive values reduce creeping motion under small commands; the model
still permits creep and does not implement an exact static hold or identify a
physical breakaway threshold. Fit bounds are hypotheses, not confidence intervals.

The candidate editor can expose a registry-declared default without modifying the
baseline. Fitting can also select the parameter when a legacy study omitted it.
A nondefault accepted value can be proposed as `electrical.loss_speed_scale` in
the CAD motor definition and reaches the common robot parameter mapping. A proposal
that introduces this field uses version 2 and displays its previous value as
"not declared". Existing version-1 numeric changes remain readable. No missing
resistance, inertia or other physical measurement is inferred by this operation.


## Voltage, current, power and batteries

The controller workflow now includes **Electrical & battery**. It uses declared
sources and sensor channels in the shared Rust circuit, keeps supply and winding
current separate, and reports energy, charge, protection activity and electrical
limits independently of tracking. Calibrated electrical sidecars can be imported
and compared; raw servo-current counts remain uncalibrated. See the
[battery scenarios and measurement format](battery-scenarios/README.md) for the UI,
headless commands, explicit assumptions, and current validation limits.
