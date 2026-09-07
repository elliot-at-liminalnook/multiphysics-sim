# Walking speed experiments

The existing browser crawl requests 1.25 mm/s. This experiment asks whether a
larger step or a faster cadence can increase that speed while keeping the same
CAD mechanisms, motor bounds, contact model, numerical tolerances and physical
acceptance checks. **No faster controller is promoted.** The maintained browser
presets remain available and unchanged.

All experiments use the explicit terrain-contact profile: floor contact remains
dynamic, inter-link forces are omitted, and independent sampled geometry checks
remain mandatory. These are uncalibrated, ideal-observation controller trials.
The results do not establish the real robot's maximum speed.

## Findings

| Experiment | Result |
| --- | --- |
| Double speed with unchanged step timing | Front thigh gear / hip shaft-pulley reference collision at 5.02 s committed time. |
| Fourfold speed with unchanged timing | At 1 s, the -Y hip command requests 0.20061043 rad beyond its 0.2 rad software bound. |
| Double speed, smaller front support shift | Avoids that first collision but reaches a rear gear / pulley reference collision at 6.5 s. |
| Double speed and cadence | Solver fails at 16.56 s. Allowing 80 iterations completes 24 s, but every swing misses the 200 ms clearance/support-duration requirement (only 40–80 ms qualifies). |
| 1.5× speed and cadence | Completes 24 s; 120–160 ms qualifies per swing, still below the same requirement. |
| 1.5× with longer swing and shorter body return | All 13 swings pass; final body error is 1.177 mm against a 1 mm limit. |
| Same timing, standing gain increment 0.75 | All swings pass; final body error is 1.016 mm. |
| Same timing, standing gain increment 1.0 | Passes the short test: 13 swings, zero sampled overlap, final error 0.885 mm. |
| That candidate over a requested 60 s | Solver fails at 30.94 s; sustained walking is not demonstrated. |
| That candidate at 10 ms physics, 50 Hz control | Solver fails at 3.04 s, even with 80 iterations. Guarded backtracking gives the same failure. |
| That candidate at 5 ms physics, 50 Hz control | Completes and passes all swings, but final body error is 1.835 mm. Maximum foot difference from 20 ms is 3.150 mm; maximum body difference is 2.606 mm. |

The 200 ms test is the current conservative commissioning requirement, not a
universal law for faster walking. None of these experiments changes it. Faster
dynamic gaits will eventually need task-appropriate balance/contact criteria;
simply deleting the requirement would not validate them.

The 5 ms and 20 ms runs have identical sampled phase schedules and body
references. Their physical trajectory disagreement cannot be attributed to a
different command or planner wait. It exceeds the existing 1 mm foot / 0.5 mm
body numerical screen and is appreciable relative to the 5 mm requested lift.
The 5 ms run itself is not proven timestep-converged. The short coarse-step pass
is therefore insufficient evidence for controller promotion.

The solver already performs a line search. `guarded_backtracking` is an optional
early-stop heuristic for that search, not a switch enabling globalization. Its
failure to help here should not be described as evidence against line searches.

## Reproduce

From the repository root:

```sh
cargo build --locked --release -p sim-runtime --example run_environment --example evaluate_lift
node examples/full-robot/prepare_speed_envelope.mjs
```

The generated manifest enumerates every case and hashes the versioned source
scene, configuration, task and generator. For each case, use its name below;
nonzero simulator exit is an expected result for the failed trials.

```sh
trial=1.5x-stop-feedback-1
trial_dir=runs/full-robot/learning/speed-envelope
target/release/examples/run_environment "$trial_dir/$trial.scene.json" "$trial_dir/$trial.config.json" examples/full-robot/browser-reversal/task.json "$trial_dir/$trial.actions.json" > "$trial_dir/$trial.native.json"
node examples/full-robot/check_online_steps.mjs "$trial_dir/$trial.native.json" "$trial_dir/$trial-acceptance"
```

Run the acceptance tool for every completed capture; a failed physical check
still writes its report before returning nonzero. After all trials:

```sh
node examples/full-robot/summarize_speed_envelope.mjs runs/full-robot/learning/speed-envelope examples/full-robot/speed-envelope-status.json
```

The summary requires every capture and every completed run's matching hashed
acceptance report, so missing and unsuccessful trials cannot silently disappear.
Recipes and compact results are versioned; large generated captures remain local
and reproducible. Some native checks/builds overlapped, so no runtime speedup is
claimed from their wall times.

## Runtime and browser verification

The shared runtime now reports the actuator name, requested angle, allowed range
and sample time when a policy violates a command bound. It still rejects the
command; it does not clamp it or change physics. The existing rejection/replay
test verifies the diagnostic and exact failed replay. Native release and WASM
release builds pass.

The short passing candidate was loaded as an **unpromoted test preset** in a
local bundle. All 1,200 transitions agree between native and WASM, with maximum
numeric difference 4.712e-9; reset and replay are exact. This proves host parity,
not physical accuracy. The eight maintained presets pass the viewer suite,
including WASD, key release/focus loss, recording/replay, reset and narrow layout.
No rendered faster-gait performance or visible-response claim is made.

See `speed-envelope-status.json`, `speed-envelope-refinement.json` and
`speed-envelope-parity.json`. Continue by diagnosing the loaded actuator/contact
transient and numerical sensitivity of the faster motion, then retry sustained
and command-change coverage. Planning, teacher/student learning, terrain and
disturbance training remain outstanding parts of the full goal.
