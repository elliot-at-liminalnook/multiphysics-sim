# Real-time gait: native/WASM comparison and component solve reuse

This experiment uses the exact 400 Hz bench-fit browser motor model. The outer
policy stays at 50 Hz, detailed physics at 6,400 Hz, and native capture at 200 Hz.
Every run shares CAD-derived mechanics, provisional motor parameters, initial
conditions, seed and forward command schedule. The three-second trial is a
bounded performance/accuracy probe, not a sustainable gait qualification.

## Baseline

| Execution | Three simulated seconds | Policy-step p95 | Sim/wall |
| --- | ---: | ---: | ---: |
| Native release Rust | 54.660 s | 484.9 ms | 0.055× |
| Browser release Rust/WASM | 51.735 s | 422.1 ms | 0.058× |

These sequential measurements exclude construction and final file serialization.
The browser number includes worker transport. Profiling is a separate native run;
its snapshots match the unprofiled trajectory exactly. Native/WASM observations
agree to numerical precision; `baseline.comparison.json` contains field maxima.
A local native backend is therefore not justified as the primary performance fix
by this comparison. It would still miss real time by a large margin.

The native profiler reports 15.63 s in 179,442 mechanical closure mappings,
12.73 s in dynamics preparation, and 12.39 s in 1,819,648 component evaluations.
Gait scripting takes 0.120 s across 150 commands. Newton residual/Jacobian buckets
include other buckets and must not be summed. `baseline.profile.json` retains
all tallies; `baseline.stack.txt` independently samples the process. The sample
was taken only during the diagnostic run. Timing is host-dependent.

## Candidate

`ImplicitStepConfig.reuse_auxiliary_solve` is an opt-in shared-library numerical
optimization. Within a fixed-time, fixed-step integration stage it uses the last
successfully solved component state and guarded Newton matrix as a new initial
proposal. Every mechanical trial still evaluates the actual motor/driver equations.
A failed warm attempt retries from the original cold start. No cache crosses a
step, event or committed-state boundary; no motor storage, force law, control
sample, physical timestep or acceptance tolerance is removed.

The colored numerical Newton API now supports the same existing guarded cache as
the ordinary solver. Analytic coupled motor cases exercise both cold and warm
starts with state and rate unknowns; a nonlinear independent-block test checks
reuse across changed equations. Whole-robot acceptance requires all physical
error budgets plus at least 1.2× speedup. The optional `warm-probes` recipe also
uses the previously available local mechanical derivative probes, keeping exact
accepted-endpoint checks. Prepared recipes are not automatically qualified.

`protocol.json` records the experiment and gates. Original source hashes and the
baseline executable are retained. `source-before/` preserves changed library
sources from before this experiment. No motor hardware or foreground UI is used.

## Reproduce

```sh
cargo build --locked --release -p sim-runtime --example run_environment
node examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/prepare.mjs
node web/tests/compare-native-wasm.mjs examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20 both baseline
node examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/check.mjs
```

The browser command requires the 400 Hz static viewer server on localhost:4182
and matching WASM sources. `NATIVE_BINARY`, `VIEWER_URL` and `CHROME_PATH` override
host paths; headless Chrome never foregrounds a desktop window. A changed runtime
must be rebuilt for both targets before comparing it. Retain evidence under a
new prefix when repeating measurements.

The initial nonlinear-cache test incorrectly assumed every changed problem would
converge with the old matrix. Its retained failure demonstrates a legitimate
guarded rejection. The corrected test exercises the required cold retry at the
same tolerances. All 42 solver tests then pass; the 30 robot library and 16
embedded-motor tests pass as well. Production already contained the cold retry.

## Qualified incremental result (real time still not achieved)

| Recipe / execution | 3 s trial wall time | Speedup versus matching baseline |
| --- | ---: | ---: |
| Warm component solve, native | 42.684 s | 1.281× |
| Warm solve plus existing derivative probes, native | 36.876 s | 1.482× |
| Combined recipe, Rust/WASM worker | 38.521 s | 1.343× |
| Forward/turn/reverse/stop, native baseline | 48.720 s | — |
| Same command sequence, combined recipe | 35.293 s | 1.380× |

These are sequential short measurements, not a repeated host benchmark or a
long-horizon gait guarantee. The initial `warm/concurrent.*` functional run
coincided with compilation; it is preserved but excluded from timing acceptance.
Both forward and command-sequence comparisons pass the original numerical and
minimum-speedup gates. Contact identities and sampled/applied controller counts
match exactly, and neither native sequence reports a fall. Peak motor-angle
difference is below 2e-11 rad, link-origin difference below 8e-12 m, winding-current
difference below 3e-10 A and contact-force difference below 9e-8 N. Those are
simulator agreement results, not measured hardware accuracy.

The selected profile reduces derivative builds from 192,480 to 23,159 and motor
component evaluations from 1,819,648 to 835,596. Native diagnostic time for component
equations drops from 12.39 to 5.60 s; mechanical closure mapping drops from 15.63
to 10.80 s. Dynamics preparation remains about 12.27 s and is now a larger share
of the remaining cost. Profile snapshots exactly match the unprofiled candidate;
the current native and WASM candidate trajectories agree to numerical precision.

**Real-time acceptance fails:** the browser achieves 0.078× simulated/wall time,
with policy-step p95 322 ms against a 20 ms target. It still needs roughly a 13×
throughput improvement. `accepted_as_solver_optimization` and `realtime_accepted`
are separate receipt fields. The next substantial investigation is reducing how
often the entire mechanical system must be solved, while retaining the fast motor
states and validating coupling/contact error. Removing motor dynamics or simply
raising the timestep is not qualified by this work.

## Try the staged browser

Open **http://127.0.0.1:4183/?preset=robot-measured-400hz-reuse**, press Play and use
W/S for forward/reverse or A/D for gentle arcing turns. Release or Stop motion
requests a controlled stop. It uses a 60 s interactive horizon, the same motor
model and clocks, and the qualified numerical settings. Its label explicitly
states that it is still slower than real time and calibration is provisional.
The original viewer remains on port 4182. No foreground window or audio was used.

To repeat candidate measurements after building both targets:

```sh
node web/tests/compare-native-wasm.mjs examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/warm-probes native candidate
VIEWER_URL=http://127.0.0.1:4183 node web/tests/compare-native-wasm.mjs examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/warm-probes wasm candidate
node examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/qualify.mjs warm-probes/candidate
node examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/check.mjs warm-probes/candidate
```

The `sequence-baseline` recipe uses the retained `baseline-native.bin` via
`NATIVE_BINARY`; `sequence-warm-probes` uses the current executable. Pass their
respective prefixes to `qualify.mjs` for the paired command-sequence check.
`stage-browser.mjs` requires both qualification receipts before creating the
interactive config/preset. Package it with `--preset=robot-measured-400hz-reuse`.
The rendered keyboard test accepts `VIEWER_URL`, `VIEWER_PRESET` and
`VERIFICATION_DIR` so its evidence remains separate from the original viewer.

## Continued optimization screening

`optimization-screen.json` records a fresh sequential native baseline and four
new candidates. The baseline is 30.587 s for the same 3 simulated seconds on the
current host; compare paired measurements, not earlier host timings. All four
candidates are rejected at the unchanged gates:

| Candidate | Wall time | Reason unselected |
| --- | ---: | --- |
| Direct projected inertia and shared kinematics | 30.013 s | Only 1.019x, below 1.2x gate |
| Guarded secant updates | 41.032 s | Slower |
| Temporal velocity prediction | 31.530 s | Slower |
| 3200 Hz backward-Euler physics | 19.854 s | Angle, position, current, force and contact-identity gates fail |

The first three retain physical agreement within the numerical comparison gates.
The coarser step reaches 0.246 degrees maximum motor-angle deviation, 3.109 mm
link deviation, 0.161 A winding-current deviation and 20.347 N contact-force
deviation over this three-second case. These rejected recipes and captures remain
available. No error gate was relaxed and no new browser preset was selected.

`coupled-sdirk2/` investigates second-order stiff integration of the existing
coupled motor/mechanics equations. It has separate analytic and full-robot gates;
its implementation alone is not a speed or accuracy qualification. The goal
remains realtime browser walking with measured approximation error.

## Latest checkpoint

See [STATUS.md](STATUS.md) for current results and outstanding work. Guarded
SDIRK stage reuse is 1.593x/2.235x faster against matching coarse/fine methods,
without qualifying those methods against the original fidelity reference.
Exact shared kinematics shows no measured gain; exact motor-storage reuse shows
a 3.4% single-pair gain, below the 20% gate. The selected BE recipe remains
around 0.11x realtime. A fresh SIMD/LTO browser build passes numerical parity
and rendered command checks at 0.105x realtime; it predates the final motor
storage change. No real-time profile or hardware accuracy claim is warranted.
