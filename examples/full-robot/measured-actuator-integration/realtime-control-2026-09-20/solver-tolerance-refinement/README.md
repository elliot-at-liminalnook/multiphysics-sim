# Solver tolerance versus timestep sensitivity

Tighter nonlinear solves do not resolve the measured 6400/12800 Hz trajectory
disagreement. All runs use the same preserved native binary, CAD/model,
controller, contact laws, seed and three-second action schedule. The mechanical
predictor is disabled. Absolute and relative Newton tolerances are varied
together; the existing physical comparison gates are unchanged.

| BE frequency | Both Newton tolerances | Wall seconds for 3 s |
| --- | ---: | ---: |
| 6400 Hz baseline | 1e-5 | 27.968132 |
| 6400 Hz | 1e-6 | 31.897566 |
| 6400 Hz | 1e-7 | 33.542364 |
| 12800 Hz | 1e-5 | 46.206577 |
| 12800 Hz | 1e-7 | 55.391170 |

At fixed 6400 Hz, changing 1e-5 to 1e-7 changes motor angles by at most
6.938e-9 rad, link positions by 3.784e-9 m, current by 5.088e-8 A and contact
force by 3.653e-5 N. At fixed 12800 Hz, corresponding differences are
1.357e-8 rad, 3.329e-9 m, 1.000e-7 A and 1.919e-4 N. Both fixed-timestep
comparisons pass all physical gates. Saved FPGA state and command arrays are
exactly equal at every 50 Hz snapshot; this does not inspect every 400 Hz sample.

With both tolerances tightened to 1e-7, halving the timestep still yields peak
differences of 0.003297065 rad, 1.550790 mm, 0.1228554 A and 27.18115 N, plus
different contact identities. These nearly match the original-tolerance
timestep comparison. Controller sample/application counts remain equal.

This supports treating timestep and hybrid/controller sensitivity as the next
numerical issue to investigate, rather than spending more runtime tightening
Newton solves. It does not identify which trajectory is closer to the
continuous-time solution, separate all sources of timestep sensitivity, or
establish measured hardware accuracy. No recipe is promoted by this study.

The fresh 12800 Hz default-tolerance run also exactly matches every saved
physical/diagnostic frame field and task transition of the historical finer BE
reference. Its timing comparison is historical, not a paired speed claim.

`analyze.mjs` verifies identical binary hashes and physical inputs, checks that
configurations differ only in numerical tolerances/timestep/reporting, and
reproduces all six comparisons without overwriting solver-optimization receipts:

```sh
node examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/solver-tolerance-refinement/analyze.mjs
```

`protocol.json` stores input and binary hashes and original gates. The binary
and source snapshots are preserved in `../mechanical-predictor/`. All four
refinement runs completed normally; sessions 84743 and 86454 need no restart.
