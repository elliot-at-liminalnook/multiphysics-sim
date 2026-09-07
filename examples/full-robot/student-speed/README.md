# Faster walking with the learned student

This study increases the maintained heading student's requested forward speed
from **1.25 to 2.5 mm/s**. This remains a slow commissioning crawl, not useful
general teleoperation speed or the real robot's maximum speed. No faster browser
controller is promoted by these results.

The versioned generator reuses the CAD-derived student scene and selected browser
solver. It changes only the declared command range, velocity-indexed posture
knots, phase timing, and selected comparison timesteps/durations. Motor and
placement bounds, network weights, feature normalization, physics, solver
tolerances and independent acceptance thresholds remain unchanged. The higher
commands are outside the network's previous training range. The inherited short
configuration includes a declared lateral push; these are development conditions,
not reserved disturbance tests.

## Screening results

`status.json` retains all 18 planned runs, including failures. Completed runs
receive an independent audit of actual sampled geometry, foot clearance, support,
body tilt and stopping. A passing endpoint or reward is insufficient.

| Change | Result |
| --- | --- |
| Increase speed without changing timing | Both 1.5× and 2× hit the planned thigh-gear/hip-pulley geometry envelope during the first lift. |
| Shorten the step cycle | Both complete 24 s; 1.5× passes all lifts but misses stopping accuracy, and 2× also misses one supported-lift duration. |
| Refine those runs from 20 to 5 ms physics | Maximum sampled foot differences are 4.38 and 3.89 mm. Controller sampling remains 50 Hz and planned phase sequences are identical. |
| Allow more time for lift, landing and return | The longer stride reaches observed first-leg interference. |
| Move the first/opposite foot stance targets back 2 mm | Clears the initial interference but reaches a front-leg reference collision at 3.58 s. |
| Reduce front/rear balance shifts | The 18/9 mm and 16/9 mm candidates pass all 13 lifts and stopping checks at both 20 and 5 ms. A 7 mm rear shift fails three lifts. |
| Sustain the 18/9 mm candidate for 60 s | Both timesteps complete 41 qualifying lifts with zero sampled overlap. Both miss the 0.005 rad heading gate; the refined run also misses the 1 mm final position gate. |

The selected development posture uses phase durations
`[0.28, 0.38, 0.38, 0.24, 0.10]` seconds (shift, raise, lower, return,
settle), +8 mm forward stance offsets for the lateral feet, −18 mm front
support shift, and +9 mm rear support shift. The reverse posture knot remains
unchanged apart from its command-speed breakpoint. Reverse walking at the new
speed has **not** been validated.

The 18/9 mm short runs stop within 0.631/0.589 mm at 20/5 ms. Their maximum
sampled foot difference is still **1.425 mm**, beyond the existing 1 mm numerical
screen; body position differs by **0.929 mm**, beyond its 0.5 mm screen. Most of
the largest foot difference is horizontal, but that does not waive either gate.
There are three contact-pair identity differences and no phase differences.
The refined run is a comparison, not a proven converged reference. See the
versioned `*-refinement.json` reports for complete sampled trajectory metrics.

Over a minute the body advances approximately 141 mm. Final heading errors are
0.00656/0.00693 rad at 20/5 ms; final body-position errors are 0.868/1.314 mm.
The maximum sampled foot difference grows to 2.116 mm, with ten contact-pair
identity differences and no phase differences. Sustained agreement therefore
also remains insufficient under the retained numerical screen.
No candidate is promoted on these short tests. Sustained heading, timestep
sensitivity, command changes, reserved disturbances, native/WASM parity and
rendered performance remain acceptance work.

## Reproduce and continue learning

From the repository root:

```sh
cargo build --locked --release -p sim-runtime --example run_environment --example evaluate_lift --example train_policy_suite
node examples/full-robot/student-speed/prepare.mjs
```

The generator creates full recipes in `runs/full-robot/learning/student-speed`.
The versioned `plan.json` records every case and hashes its versioned sources;
the source CAD artifact is already durably referenced by the retained scene.
Generated copies are not additional CAD baselines. For each case in the plan:

```sh
trial=2x-support-18-9
trial_dir=runs/full-robot/learning/student-speed
target/release/examples/run_environment "$trial_dir/$trial.scene.json" "$trial_dir/$trial.config.json" examples/full-robot/heading-task/task.json "$trial_dir/$trial.actions.json" > "$trial_dir/$trial.native.json"
node examples/full-robot/check_online_steps.mjs "$trial_dir/$trial.native.json" "$trial_dir/$trial-acceptance"
```

Early geometry failures intentionally return nonzero; audit completed captures
even if their physical acceptance then fails. After all cases finish:

```sh
node examples/full-robot/student-speed/collect.mjs
```

The collector requires every planned outcome, exact recorded controller/config/
task/command agreement, common parsed physical properties, and hashed independent
acceptance for every completed capture. It includes the initial reset transition
when checking action coverage. Runs overlapped, so their wall times are not
performance benchmarks.

`search.recipe.json` defines a bounded continuation of the existing paired random
weight search: four iterations, eight perturbations plus the baseline, seed
90724. Each network is scored on the short 2× walk, the sustained 20 ms walk,
and the sustained 5 ms walk. Selection maximizes the worst reward per simulated
second. All are development data; this is neither PPO nor a hardware-validation
claim. A better reward must still pass independent physical gates and browser
delivery.

```sh
target/release/examples/train_policy_suite examples/full-robot/student-speed/search.recipe.json runs/full-robot/learning/student-speed/search-001
node examples/full-robot/heading-task/collect_search.mjs runs/full-robot/learning/student-speed/search-001 examples/full-robot/student-speed
```

Use a fresh search directory for subsequent runs. The collector saves every
candidate's scores and errors and materializes selected configurations. Do not
replace the maintained browser preset until validation and browser checks pass.
The CAD document and detailed validation model are untouched by this study.
