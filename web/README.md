# Robot motion workspace

The browser renders CAD-derived collision surfaces and telemetry. Physics and
Rhai control execute in Rust inside a Web Worker. Earlier commissioning presets
include live lift/servo experiments and recorded physical comparisons; newer
presets add live walking. Each uses the same Rust session and controller recipe
as its native experiment. Performance and physical acceptance are specific to
the selected preset; hardware remains uncalibrated. See
[the session contract](../examples/interactive/embedded-session.md).

Newer walking presets provide live WASD control through the same Rust environment
as native experiments. Build with `--environment-only` to include them. The
**Faster student trial** and **Move during swing** presets expose the 2.5 mm/s
development gait; they remain experimental because sustained heading, numerical
agreement and realtime processing gates are not all satisfied. See the
[current measurements](../examples/full-robot/swing-advance/RESULTS.md). Controller
descriptions distinguish these trials from the earlier commissioning presets.

## Build and open

From the repository root:

```sh
npm ci --prefix web
cargo build --locked --release -p sim-web --target wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127 --locked
node web/build-viewer.mjs runs/interactive/viewer --fixture-only
node web/serve-viewer.mjs runs/interactive/viewer 4173
```

Open <http://127.0.0.1:4173>. The fixture-only build works from repository inputs
without generated robot runs. To include the quadruped recordings, reproduce
the experiments documented in
[lift-and-viewer-validation.md](../examples/full-robot/lift-and-viewer-validation.md)
and prepare the feedback variants with
`node examples/full-robot/prepare_joint_feedback.mjs` followed by
`node examples/full-robot/prepare_task_observations.mjs` and
`node examples/full-robot/prepare_landing_checkpoint.mjs`, then run the packager
without `--fixture-only`. `web/viewer/presets.json` declares
the exact scene and capture paths. Missing or mismatched inputs fail packaging.
The forward-placement preset additionally requires the plan and prepared inputs
from [forward-placement-validation.md](../examples/full-robot/forward-placement-validation.md).
It is explicitly a failed supported-lift attempt, preserved for inspection.
The slower placement preset additionally uses `node examples/full-robot/prepare_forward_rate.mjs`.
It retimes that same geometric path without relaxing support requirements. Both
tested timesteps pass the sampled lift check, but internal hip contact and
timestep sensitivity remain under investigation; it is labeled experimental.
The corrected-sign preset uses the chassis regeneration in
`chassis-sign-correction-experiment.json`. Body-position feedback is prepared by
`node examples/full-robot/prepare_body_feedback.mjs` after that correction and
its geometric plan. It adds a bounded Rust Jacobian suggestion consumed by Rhai,
with a live body-gain input and target/actual/support diagnostics. These are
ideal teacher observations; they are not declared hardware sensors.
The solver experiment presets additionally use
`prepare_point_feedback.mjs`, `prepare_block_factor.mjs`,
`prepare_sample_reuse.mjs`, and `prepare_final_refresh.mjs` under
`examples/full-robot/`. The last preparation requires the recorded focused
Newton audit described in
[newton-convergence-validation.md](../examples/full-robot/newton-convergence-validation.md).
These preserve the same controller while comparing numerical solver options;
read each preset's readiness label before interpreting its result.
`WASM_BINDGEN=/absolute/path/to/wasm-bindgen` overrides the packaging executable.

All runtime assets, including Three.js and WASM, are local in the output
directory. Share that directory as a zip and serve it over HTTP on localhost or
HTTPS on a static host; opening `index.html` as a `file:` URL is unsupported.
`build-manifest.json` records input hashes. The provided server binds localhost;
it does not publish a public URL. Source scenes and captures remain the authority
for physical provenance, and the original CAD archive remains unchanged.

## Use it

- Drag to orbit, right-drag to pan, and scroll to zoom. Click a model surface or
  part-list row to highlight a rigid component. Fit selected, or press F, to
  inspect it; Fit model restores the whole assembly.
- Recorded runs offer play/pause, speed and timeline scrubbing. Contact arrows
  display captured forces; the right panel compares requested and actual angles.
- In the live Rhai fixture, change the target slider and press Play. Save run exports
  Rust's scene/input recording; Replay inputs re-executes it through Rust. Reset
  recreates the worker and initial state. Camera interaction remains available
  while the worker computes.
- In the live quadruped, Play advances the configured Rust servo experiment.
  The camera remains interactive while physics computes; Pause finishes the
  current small chunk, and Reset terminates the worker and reloads the model.
  Save run records the scene, controller recipe, seed and completed step count.
  Replay recomputes the physical result with progress and cancellation.
- In the joint-feedback presets, the gain slider changes the outer tracking
  correction. The inspector separates plan, motor target and actual joint
  angles. Save/replay records input changes at their original simulation steps.
  These presets use ideal state observations, not calibrated hardware sensors.
- The body/foot observation preset adds a collapsible inspector with relative
  marker position/velocity, body tilt/motion, and floor support forces. Its sample
  timestamp distinguishes controller observations from the current rendered state.
- The support-qualified return preset pauses the reference until all four feet
  sustain the configured support threshold. The inspector shows reference time,
  qualification dwell and timeout. These are provisional ideal-force conditions,
  not a validated landing or balance detector. Saved failures replay the failed
  attempt and verify its reason and timing through Rust.
- Walking presets accept W/S for forward/reverse and A/D for turning. Releasing
  keys or using Stop sends a zero motion request through the controller. Earlier
  lift/servo presets have no walking command interface.

### Driving the rover with a drive profile (compatibility path, unexecuted)

The **Two-wheel rover (drive profile)** preset (`rover-drive`) drives
`examples/wheeled-robot/baseline/` through the same three layers as the
native viewer: device bindings (`sim.drive-bindings/1`), the robot's drive
profile (`robot.drive.json`) and the kinematic adapter in its controller. The
browser cannot start the binding's Python program, so it runs the binding's
embedded Rhai adapter (`examples/wheeled-robot/drive-adapter.rhai`), which
mixes only through Rust drive functions, in the shared embedded session in
the worker (`DriveSimulation`). Rust builds the scene from the model, the
binding and the files it names (`build_drive_scene`); the page only fetches
them.

    node web/build-viewer.mjs runs/interactive/viewer --fixture-only
    node web/serve-viewer.mjs runs/interactive/viewer 4173

then open `?preset=rover-drive` (or `--preset=rover-drive` after the output
directory to package only it) and press Play.

- Bindings: the defaults come from Rust (`default_drive_bindings`: W/S
  forward, A/D turn, X stop, B halt; left stick Y forward, right stick X
  turn, South stop, East halt). An override stored in localStorage under
  `sim.drive-bindings/1` is checked by the same Rust parser
  (`validate_drive_bindings`); a refusal names the field and the page falls
  back to the defaults, saying so.
- Keys are read by physical position (`event.code`); gamepads with the
  standard layout are polled each frame. Rust turns them into normalized
  axes (deadzone, inversion and the Standard Gamepad's stick-Y sign) and the
  session scales them by the profile. The page sends requests only: every
  request raises the heartbeat in Rust, and the profile's acceleration limit
  and deadman run in Rust on simulation time. Nothing is mixed, scaled or
  limited in JavaScript.
- Stops: releasing every input sends one zero request; leaving the window,
  hiding the page, Escape (except while typing in a text box) and the stop
  and halt bindings or buttons request a stop and disarm held inputs until
  they are released; a text field taking focus disarms the keys. If requests
  stop arriving, the deadman (0.5 s of simulated time) ramps the rover down.
- The panel shows the requested and commanded twist with units, the profile
  limits, the deadman state, the heartbeat and the fidelity label with the
  drive's identity hashes. Save run downloads the embedded session's
  recording; Replay re-executes it and is refused, naming each difference,
  when the adapter, config, profile, model or CAD hashes differ.

Status: written and checked by reading only. Nothing was built or run (no
wasm build, browser or test); `node web/tests/drive_input.mjs` (the stop
rules and bindings fallback) is written but not run and not in CI. Realtime
performance is not measured. The Python controller on the native seam stays
the reference; this is a compatibility surface. Presets without a drive
profile keep their own WASD path unchanged.

The displayed simulation/wall ratio is recorded stepping cost or measured live
worker request cost, as labeled. It is not rendering FPS or a training benchmark.
Collision surfaces are a decimated physical export, not the full CAD B-rep.
Recorded poses are sampled at their captured times, without fabricated physics.

## Verification

```sh
cd web && npx playwright install chromium && cd ..
node web/tests/viewer.mjs runs/interactive/viewer runs/interactive/viewer-report.json
```

`CHROME_EXECUTABLE` can select an installed Chrome executable. The test serves
the bundle on its own ephemeral localhost port and checks real rendering, part
search/selection and fit, transport placement, playback, live Rust/Rhai inputs,
download/replay, failed-load recovery and a narrow layout. It writes desktop and
mobile screenshots. The CI fixture-only path tests the live UI; full robot
recordings receive these same local checks when available. Numerical native/WASM
parity remains the separate `web/tests/runtime.mjs` gate in browser CI.

## Swing-foot feedback experiment

`Quadruped · swing foot position feedback` adds an adjustable point-feedback gain
to the existing body/joint controller. Open **Body and foot observations** for
world foot targets, actual positions, errors, phase activation and bounded
suggestions. This is live shared Rust/WASM physics with ideal teacher state.

The experiment improves peak swing error and clearance but worsens final
placement at both tested timesteps. It remains labeled as a tradeoff alongside
the previous controller. See `examples/full-robot/point-feedback-validation.md`
for native/refined measurements, reproduction and browser validation.

## Experimental block solver

`Quadruped · swing feedback with block solver` uses the same controller with
opt-in exact independent-block closure factorization. A native pair showed a
modest 1.045× speedup. Refined motor-current differences exceed the strict
comparison gate despite nearly identical foot motion, so the original solver
remains default. One browser force reading also exceeds the strict parity
limit; the trial bundle is retained for diagnosis and does not replace the
previously passing live viewer. Browser configuration disables only native
process-global profiling. See `examples/full-robot/block-factor-validation.md` for evidence.

The **Controller leaderboard** opens the measured recipe catalog. It provides
search, validation/profile filters, side-by-side evidence, exact hash-checked
Load and run actions, Rust input replay and canvas WebM export. Experimental
controllers remain unranked until every declared gate passes. Physical travel,
native throughput and browser timing are displayed separately. The new teacher
reaches 3.755 mm/s measured travel over a minute with 41 qualified swings and
passing stopping. Its mixed steering task also passes at 2.5/1.25 ms, while
sustained numerical accuracy and browser realtime remain unresolved. Isolated
scalar and SIMD/LTO builds record compiler and source hashes; their 20 ms
student timing still misses the p95 target. See [catalog workflow](leaderboard/README.md)
and [faster gait evidence](../examples/full-robot/whole-swing/RESULTS.md).

The finer teacher is now a separate reference recipe: 3.750 mm/s measured
travel, 41 qualified swings, passing stopping and 1.25/0.625 ms trajectory
agreement. Its full 60-second native/WASM comparison and exact replay/reset
pass. It remains unranked because realtime, held-out disturbances and terrain
requirements are unresolved. The faster 20 ms secant steering profile stays
available with its separate 26.1 ms p95 result.
