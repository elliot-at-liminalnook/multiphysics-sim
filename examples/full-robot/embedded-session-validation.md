# Live quadruped through the shared Rust motor session

The lift experiment now runs live in WASM through the same incremental Rust
session as the headless runner. This delivers interactive execution of the
existing fixed-reference servo program. It does not add WASD locomotion, a
learned policy, a faster physics model, or hardware calibration.

## Extraction and numerical checks

The orchestration previously contained in `integrate_embedding.rs` moved into
`sim_runtime::embedded::EmbeddedSession`. The example now handles files and
reporting only. The shared session owns the physical state, servo and motor
memory, event scheduling state, reference clock and cached implicit workspace.
The [API and lifecycle contract](../interactive/embedded-session.md) documents
full versus bounded diagnostic capture and the controller execution modes.

The native 1.6 s quadruped lift was rerun after extraction. All 36 original
report fields checked match the previous runner exactly, excluding measured wall
time. This includes sampled frames, motor state, original closure, contact
traces/impulses and hybrid solver/event diagnostics. The new world field is
additive provenance. These single runs took 92.46 and 98.68 s respectively;
the extraction is not claimed as a performance improvement. Diagnostic storage
was subsequently restored to typed vectors to avoid JSON allocation inside steps.

Four Rust lifecycle tests pass: uneven host chunks preserve the full result,
frame inspection does not mutate it, recorded prefixes replay exactly, invalid
counts preserve state, partial runs remain incomplete, and a support-feedback
timeout is identical across chunks and latches against further advancement.
The existing 19 detailed-session tests also pass.

| Browser test | Evidence |
| --- | --- |
| Embedded motor fixture | 11 native reporting frames match exactly over 20 ms |
| Full quadruped lift | 161 native reporting frames pass the declared 1e-7 absolute entry comparison over 1.6 s |
| Largest quadruped difference | 3.432e-8 N in a sampled contact-force component |
| Browser physical replay and reset | Exact after excluding measured wall-clock timing |
| Quadruped loading | 1.85 s in the measured Chrome run |
| Quadruped stepping | 97.16 wall seconds for 1.6 simulated seconds |
| Largest 10 ms physics work chunk | 1.69 wall seconds |
| Existing detailed Rust/Rhai fixture | Native/browser comparison and exact replay still pass |

The full browser check compares every original native frame field, including
motor and servo state, joint motion, link poses, closure and contact readings.
It does not compare continuous unsampled trajectories or establish hardware
accuracy. Main-thread heartbeat checks remain active while the worker computes.
Browser replay emits progress between work chunks and can be cancelled.

After the full trajectory check, the WASM replay entry point gained a loaded
scene/controller equality guard. The final fixture test explicitly rejects a
changed recipe without mutating the current state, and the final rendered robot
test replays a real quadruped prefix through that guard. This guard does not alter
the physics step. Recorded source and executable hashes distinguish these checks.

## Rendered workflow and reproduction

Open the local workspace and choose **Quadruped · live lift experiment**. Play
runs the actual Rust servo recipe; Pause stops after the in-flight chunk; Reset
terminates the worker and reloads the original scene. Save run records the scene,
experiment config, seed and committed step count. Replay recomputes the result.
The renderer remains independent and supports selection, camera fit and contact
arrows throughout. Recorded robot runs remain available for immediate scrubbing.

Build instructions are in [web/README.md](../../web/README.md). Reproduce the
corrected CAD scene and native lift with
[lift-and-viewer-validation.md](lift-and-viewer-validation.md), then run:

```sh
node web/build-viewer.mjs runs/interactive/viewer
node web/tests/embedded.mjs runs/interactive/viewer robot-lift-live \
  runs/full-robot/learning/hip-grid-lift-5mm-execution.json \
  runs/interactive/robot-embedded-browser.json
node web/tests/viewer.mjs runs/interactive/viewer runs/interactive/viewer-report.json
node web/serve-viewer.mjs runs/interactive/viewer 4173
```

The full UI test checks recorded selection/fit/scrubbing, live robot advance,
pause, saved-recipe replay and reset, Rhai input/replay, load failures and recovery,
cancellation and desktop/mobile layout. The embedded fixture's lifecycle and UI
checks are added to browser CI. Full-robot browser checks were run locally;
no remote CI run is claimed. The status JSON records hashes and exact reports.

## Remaining objective

The physical lift remains the provisional 1.68 mm sampled-clearance result from
the preceding audit. Landing, support slip, loaded placement accuracy and active
balance are not accepted. The current full robot still runs roughly 60 times
slower than realtime. The deployed observation/action contract, WASD controller,
fast training throughput, calibration import, teacher RL, distillation and
held-out robust walking evaluation remain required. New controllers must use
this shared session path and remain runnable in the browser.
