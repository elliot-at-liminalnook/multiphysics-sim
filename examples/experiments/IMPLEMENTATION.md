# Experiments panel — feature acceptance record

September 13, 2026. All four first-version milestones in
[the feature specification](../../experiments-panel-features.md) are implemented.
This is a first usable version, with further UI refinement expected. Physical
model accuracy is a separate scientific result; the demonstration candidate is
not accepted calibration.

## Feature-by-feature evidence

| Feature | Implemented behavior | Evidence |
| --- | --- | --- |
| Measurement library | Pilot/full-range collections, independent trials, device/direction/drive/role/outcome filters; no-motion trials retained | Archive regression test reproduces 63 + 216 trials and 33/36 + 81/162 empirical held-out passes; UI filter test covers whole-trial roles and failures |
| Source and conditions | Raw file hash verification, saved extraction/model hashes and references, reported supply/temperature, explicit unknown fixture/controller behavior | Archive tests verify 16 + 5 raw sources; saved demo retains source maps and baseline plan |
| Comparison plots | Reconstructed command, measured/empirical/baseline/candidate overlays, residuals, shared cursor, time zoom/position, visibility toggles, timing windows and gaps | Native [panel screenshot](evidence/panel-preview.png), rendered from a reopened review with physical results |
| Physical simulation | Compiled shared motor/gearbox/H-bridge, supply, load and temperature components; scheduled pulse edges; no viewer physics | 279 complete baseline/candidate trial comparisons with zero unscored trials in [the full-collection record](evidence/full-collection-acceptance.json); signed pulse edge test |
| Metrics and tolerances | RMSE, maximum absolute and final errors; original limits retained; separate custom limits for new evaluations | Generic comparison tests reject incompatible units/times and invalid values; all empirical RMSE values reproduced from retained data |
| Model refinement | Fixed baseline; editable candidate with registry units/validation; separate condition edits; reset/reuse revision; captured changes and stale flags | Physical candidate test changes response; half-step test differs by less than 0.0015 rad on the representative replay; invalid settings rejected |
| Responsive execution | Background load/save/simulation, progress, cooperative cancellation, retained history and unscored partial work | UI worker lifecycle test runs, renders, edits, cancels and returns to earlier evidence; shared cancellation test |
| Validation | Preserved tuning/held-out splits; per-trial and device/direction/role summaries; improved/regressed RMSE and newly failing trials | Full-collection record retains every trial and score; UI tracks exposure to held-out detail/runs and flags subsequent edits as validation-influenced |
| Decisions and scope | Investigating, retain baseline, reject or prefer for tested conditions; notes and need for further measurements; all evaluated trial conditions retained | [Saved demo review](evidence/pilot-demo.review.json), history controls, export and round-trip test; no CAD/default write path |
| Save/reopen/export | Self-contained JSON snapshots, new-file atomic save, immutable older files, readable HTML with embedded plots and results | Persistence test rejects overwrites and tampered metrics; [demo HTML](evidence/pilot-demo.review.html) rendered and inspected in Chrome |
| Future CAD context | Stable optional component reference plus selected trial/evaluation retained in review; hiding panel or switching diagrams keeps review state | Shared review document owns these fields independently of native schematic state; CAD integration itself remains future scope |

## Verification performed

```sh
cargo build --locked -p sim-runtime -p sim-viewer \
  --example review_experiments --bin sim-viewer
cargo test --locked -p sim-runtime -p sim-viewer \
  --test experiment_comparison --test experiment_study --bin sim-viewer
```

14 tests passed: 2 comparison tests, 4 physical-study/evidence tests, and 8 native
viewer tests (including 2 new experiment-host tests). The initial headless-render
harness needed to explicitly clear unconsumed texture deltas; the corrected
harness passed. Build output retains one unrelated existing dead-code warning in
`contact_planning/joint_steps.rs`.

Both complete archives were additionally evaluated through the same runner used
by the panel. The demonstration candidate adds 0.025 N·m output friction:

| Collection | Trials scored | Candidate passes / failures | RMSE regressions versus baseline |
| --- | ---: | ---: | ---: |
| Pilot | 63/63 | 20 / 43 | 10 |
| Full range | 216/216 | 20 / 196 | 17 |

These totals include tuning and held-out trials; the retained record includes the
split on every row. They are **not** a claim of good physical agreement. All
failures were preserved, and no parameter was promoted. The three-trial saved
demo deliberately includes an improvement and a regression.

The full-collection record and demo retain the exact runtime source identity used
for their runs. Subsequent presentation-only changes do not rewrite those
historical identities. Run the headless command in the README to produce a fresh
comparison with the current build.

The native screenshot verifies the full-size panel and saved-review rendering.
UI tests exercise the background action handlers and state transitions; this is
not exhaustive manual testing of every pointer/keyboard interaction or window size.
The exported demo's table, displacement plot, residual plot and labels were
visually checked in a browser.

## Explicit scope and remaining limitations

- No hardware acquisition, automatic fitting, same-controller parity, animated
  replay, arbitrary-format import, or automatic CAD adoption. These are the
  feature document's declared later work.
- The physical model is provisional. Pulse reconstruction, host timing,
  supply/temperature midpoint assumptions, continuous-angle prediction, unknown
  sensor sample age and fixture properties limit interpretation.
- Only the two retained PWM extraction formats are supported initially. Generic
  typed traces and comparison metrics are shared; adding another measurement
  family still needs an explicit adapter and experiment definition.
- Reports preserve scored samples; plotted connecting lines are visual aids,
  not additional measurements. The UI marks large acquisition gaps explicitly.
- Save each open review separately using a new filename. Existing evidence is
  protected from overwrite, and closing with unsaved work prompts for retention.
- Parameter promotion into CAD and selecting a component directly from the
  future Rust CAD viewport remain separate future workflows. Optional references
  never infer that a hardware servo ID is a specific CAD assembly component.
