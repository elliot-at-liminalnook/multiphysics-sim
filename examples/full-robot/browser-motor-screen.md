# Browser motor reductions on the explicit-drive baseline

After delivering the sampled teacher environment, screen the existing shared
motor reductions against the frozen v4 robot recipe. These omit winding storage,
internal rotor/gear inertia, or both. They retain the detailed firmware, driver,
mechanism, geometry, contact and controller integration. Source CAD parameters
remain unchanged and the chosen physical reduction is recorded in scene options.

All four 2.8-second runs completed. Native diagnostic wall times were 14.735 s
(detailed), 16.079 s (winding reduction), 16.446 s (rotor reduction), and 17.910 s
(both). These are single captures, not replicated benchmarks, and include the
environment's endpoint observations and frame preparation. None demonstrated
realtime; do not infer a precise regression or speedup from this timing sample.

Maximum sampled foot-position difference from the detailed run was 0.0209 mm
for the winding/combined reductions and less than 0.000001 mm for the rotor
reduction. This is encouraging for reducing those physical details, but it does
not validate currents, energy, impact behavior, a walking policy or hardware
transfer. No profile is promoted by this screen.

The next browser experiment should make a larger change: effective bounded
actuator response together with reduced internal mechanism dynamics and simpler
contact geometry. Keep the current model as the comparison reference. Merely
omitting fast motor storage inside the same coupled integration architecture is
not sufficient evidence for the new realtime browser goal.

Reproduce with a release `run_environment` binary:

```sh
cargo build --locked --release -p sim-runtime --example run_environment
node examples/full-robot/screen_browser_motors.mjs
```

The script writes captures below `runs/full-robot/learning/browser-fidelity-screen`
and records input/capture hashes and metrics in `browser-motor-screen-status.json`.
Use `--summarize-only` to examine existing complete captures. Full native/WASM
and task validation are required before delivering a changed browser profile.
