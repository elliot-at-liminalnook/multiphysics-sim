# Overlap, geometry and browser results

All 18 initial overlap cases and all eight geometry-derived stance cases ran.
Their hashed recipes and exact outcomes are in `plan.json`, `status.json`,
`workspace-plan.json` and `workspace-status.json`. The integrity reports check
all declared cases against captured controller inputs, seeds, configs and
identical parsed physical robot definitions, including failed attempts.

At 2.5 mm/s, half overlap passes all 13 supported swings and the short endpoint
gates at both 20 and 5 ms. Final position errors are 0.926 / 0.696 mm. Full
overlap passes all swings but the coarse run's final position error is 1.097 mm,
above the 1 mm gate. Half overlap's maximum timestep foot difference is
1.167 mm and its body difference is 1.004 mm, beyond the 1 mm / 0.5 mm
numerical screens. It is not promoted even though both physical audits pass.

At 3.75 mm/s every initial candidate reaches leading side-leg reference
interference before 0.8 s. At 5 mm/s either the planned or actual geometry
reaches that limit before 0.8 s. These outcomes establish a limit of the tested
posture and trajectory, not the mechanism's global speed limit.

The derived stance adjustment preserves the leading foot's forward endpoint and
clears that first obstacle. All four 3.75 mm/s variants then hit front-leg
reference interference at 3.54–3.56 s. All four 5 mm/s variants fail planned
support during the front-leg lift: one predicted supporting load is 0.472 N,
below the predeclared 0.5 N planning screen. No threshold was changed.
This exposes a tradeoff between front-leg travel and support placement. A useful
next experiment is to distribute horizontal foot travel across raise and lower,
instead of completing all horizontal travel during raise; it must still pass
the CAD trajectory and contact checks before sustained testing or learning.

The **Faster student trial** and **Move during swing** presets make these
milestones runnable through live Rust/WASM and WASD. Both are explicitly
experimental. The half-overlap 24-second native/WASM test matches all 1,200
transitions within the portability tolerance; the largest scalar difference is
5.27e-8, and reset/replay are exact. A browser forward/stop run re-executed
natively passes 15 swings, zero sampled internal overlap, 0.940 mm final body
error and 0.00422 rad final heading error. Its input-to-drawn-reference association
is checked separately; this is not a physical stopping-time measurement.

On an Intel i9-9980HK, macOS x64, Chrome 152.0.7977.76, the isolated rendered
24-second run achieves 1.00067 simulated seconds per wall second overall, but
only **0.99702× during active motion**, with **34.095 ms active p95** processing.
Both active realtime requirements fail. The maximum measured WASM call is
239.98 ms. Rendering frame-interval p95 is 16.67 ms; rendering does not explain
the physics processing limit. The stop request takes about 1.003 s to reach a
drawn stopped *reference*, including transfer completion. These are single-run
measurements on the documented host, not hardware-independent guarantees.

Recompute a saved browser episode with the shared runtime's replay validation:

```sh
target/release/examples/run_environment --replay runs/interactive/swing-advance/live-forward.recording.json > runs/interactive/swing-advance/live-forward.native.json
```

All 41 viewer checks pass, including each new preset's exact loaded neural
configuration, visible replay and reset. Preset discovery in the test now uses
the catalog instead of a fixed list, so new task presets enter the shared checks.

The CLI also supports completed-transition prefixes. It marks a replayed prefix
as `requested_steps_completed` while keeping `completed` false until the full
configured episode is finished. A short prefix cannot masquerade as a completed
benchmark. Analytic motion-metric checks and full/prefix/invalid-replay checks
pass locally; representative walking, replay, UI loading and native/WASM checks
are wired into browser CI. Remote CI execution remains unverified.

Useful sustained speed, robust forward/reverse/turn/stop control, a trustworthy
controller leaderboard, student distillation of improved policies, broader
terrain and hardware calibration remain unfinished parts of the original goal.
