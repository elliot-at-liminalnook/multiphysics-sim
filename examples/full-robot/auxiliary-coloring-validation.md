# Compressed derivatives for the local motor solve

This change compresses numerical derivative probes in the existing local
auxiliary solve. It retains the detailed equations and all solver acceptance
rules. It does not resolve the parent condensation model's discrepancies from
the simultaneous formulation, and is not a trained walking controller.

## Structural contract

The shared `sim-solve::BlockDiagonalColoring` accepts a declared partition of
residual rows and unknowns. One probe perturbs one column in each independent
block. Each column keeps exactly the same forward-difference perturbation as
ordinary numerical Newton. A 48-state bank of twelve four-state motors needs
four probes per Jacobian build instead of 48. Row scaling, line search,
correction checks, raw residual checks and iteration caps are unchanged.

`SampledMotorControl::independent_motor_boundaries` defaults to false. The
registered servo/driver adapter declares true because, at fixed trial mechanics,
time and held controller state, each bridge reads only its own motor current.
Supply voltage and winding temperature are imposed inputs in this adapter.
The declaration does not apply to a future shared current-dependent battery or
coupled thermal boundary. Such an adapter must retain ordinary probes or
provide a separately justified structure.

The opt-in `ImplicitStepConfig.color_auxiliary_jacobian` requires auxiliary
condensation. Unknown adapters fall back to ordinary derivatives. General
coupled callbacks do not acquire a sparsity assumption. No assumption is made
about the outer mechanical acceleration Jacobian, which remains coupled.

The reusable independent-column audit measures off-block derivatives at saved
states; it is a diagnostic, not proof of structural independence. Tests cover
unequal block sizes, nonlinear equations, a deliberately false declaration,
nonfinite probes, both motor directions, backlash modes, current foldback,
controller events, exact replay and unknown-adapter fallback. The focused suites
contain 37 passing tests.

## Full robot evidence

Both complete 2.8 s runs, at 0.25 and 0.125 ms, match their uncolored local-solve
references exactly: all sampled frames, terminal frames, event schedules and
accepted contact impulses. Original closure checks and the parent sampled-lift
outcomes are retained. Every successful inner solve uses the declared coloring.

| Successful-trial work | 0.25 ms ordinary | 0.25 ms colored | 0.125 ms ordinary | 0.125 ms colored |
|---|---:|---:|---:|---:|
| Inner component evaluations | 9,103,560 | 2,143,244 | 14,381,041 | 3,216,921 |
| Outer Newton iterations | 73,259 | 73,259 | 119,206 | 119,206 |
| Inner Newton iterations | 1,213,584 | 1,213,584 | 1,724,496 | 1,724,496 |

Development wall times are 126.28→106.10 s at 0.25 ms and 216.55→139.93 s at
0.125 ms. Builds and browser work overlap portions of these runs; these values
are not an isolated performance comparison. A separate sequential ABBA benchmark
uses the same frozen binary and input files with no other assistant-launched
simulation/build/browser tests running:

| Order | Method | Wall time for 2.8 simulated seconds |
|---|---|---:|
| 1 | Ordinary | 124.71 s |
| 2 | Colored | 88.04 s |
| 3 | Colored | 88.53 s |
| 4 | Ordinary | 125.85 s |

The means are 125.28 s and 88.29 s: **1.419× faster**, or 29.5% less wall time,
on the recorded Intel i9-9980HK host. All four sampled trajectories and accepted
contact traces match the uncolored reference exactly. Background host activity
is not controlled. This is two measurements per method on one workload, not a
general performance claim. Throughput is still only 0.0317 simulated seconds
per wall second, about 31.5 times slower than realtime.

The known local-solve convergence failures and different subdivisions relative
to the simultaneous solver are unchanged. Fewer component calls establish a
computational improvement, not better timestep accuracy, hardware calibration,
or realtime. The parent fast-model acceptance and learning work remain open.

## Reproduce

Prepare the point-feedback, final-refresh and auxiliary-condensation inputs,
then run:

```sh
node examples/full-robot/prepare_auxiliary_coloring.mjs
cargo test --locked -p sim-solve --test coloring -p sim-domain-robot --test embedded_motor --test embedded_step -p sim-runtime --test embedded_session
cargo build --locked --release -p sim-runtime --example integrate_embedding --example compare_embedding
target/release/examples/integrate_embedding runs/full-robot/learning/point-feedback/scene.json runs/full-robot/learning/auxiliary-coloring/base.config.json > runs/full-robot/learning/auxiliary-coloring/base.execution.json
```

Repeat with `refined.config.json`, and compare to the corresponding uncolored
capture with `compare_embedding`. The preparation manifest permits only the
numerical option to change. `benchmark_auxiliary_coloring.mjs` runs ordinary,
colored, colored, ordinary in separate sequential processes. The summary script
verifies complete trajectories and hashes captures, source snapshots, native
binary and browser evidence. Local ignored captures accompany the reproducible
recipes; they are not a replacement for durable CAD provenance.

## Browser validation

The maintained `pendulum-condensed` fixture enables compression. All 11 native/
WASM frames pass (maximum difference 1.74e-18); replay/reset and invalid-request
state preservation pass. CI checks this alongside the structural solver tests.

The full robot browser run completes all 281 frames and replays exactly, but
fails the existing strict 1e-7 absolute-entry portability diagnostic at two
force readings. Their maximum difference is 1.24865e-7 N. The threshold is not
loosened. This is a small numerical discrepancy, not evidence of a material
hardware error, but the unaccepted full-robot preset is removed from the visible
catalog. Its native/browser captures and report are retained for investigation.

Browser execution takes about 96.95 s for 2.8 simulated seconds, with a maximum
reported chunk of 1.36 s and 19,956 main-thread heartbeats across execution and
replay. These are lifecycle/performance diagnostics, not realtime acceptance.
Existing robot presets retain their original solver configurations. Full viewer
usability checks remain separate from full-trajectory numerical portability.
All 21 full-catalog UI checks pass. The maintained 19-preset bundle is
`runs/interactive/robot-lab-auxiliary-coloring-2026-09-07.zip`; the failed full
condensed candidate is excluded. Extract it, enter `viewer`, run
`node serve-viewer.mjs . 4173`, and open the printed local URL. The user's
installed viewer and CAD document are not reloaded by this experiment.

Next investigate correction scaling for internal rate coordinates at short
event intervals, with independently derived tests and unchanged original
equation checks. Shared analytic component partials remain another way to
reduce inner solve cost and numerical derivative noise. Neither is assumed
to fix the outstanding convergence or training-model accuracy gates.
