# Two-cycle crawl and live browser scheduling

The versioned `browser-crawl-startup/` preset executes eight loaded swings over
16.6 simulated seconds. It is a fixed reference with Rhai joint/body/foot feedback
and the registered effective-servo profile. It does not yet accept walking speed
or turning commands, qualify a sustained gait, or establish hardware accuracy.

The CAD mechanism, mass/inertia, gravity and contact remain active. The initial
side-foot placements advance 20 mm; subsequent placements advance 10 mm per
cycle. This accommodates the imported assembly's asymmetric mass distribution.
Body shifts and a 3 mm crouch during the +X swing are explicit planning choices.
The shared Rust planner checks 1,621 poses for mechanism closure, internal
collision and three-foot static support. Maximum planned marker error is 37.1 µm.
Hip/worm search envelopes are provisional software limits; authored CAD limits
and collision checks remain active. No CAD geometry or properties were mutated.

## Task evidence

All eight swings pass the shared Rust sampled lift check: at least 200 ms of
consecutive samples simultaneously show at least 1 mm clearance, at most 0.1 N
swing-foot force, and at least 1 N on each supporting foot. Actual qualifying
spans range from 260 to 300 ms; peak clearances are about 3.1–3.3 mm. At the end
of each transfer, all feet regain at least 8.46 N of support and remain within
0.900 mm of their reference positions. Body advance is 19.785 mm against 20 mm;
maximum tilt is 0.003543 rad (0.203°). No sampled internal contacts occur.

These are endpoint samples at 50 Hz, not bounds on between-sample impact peaks.
The 1 mm placement and 0.01 rad tilt budgets are provisional commissioning
criteria on this flat floor, not measured hardware error tolerances. Feedback
uses ideal runtime observations. Effective actuators retain the omissions and
uncalibrated gain assumptions documented in `effective-servo.md`.

## Timestep and solver evidence

A 10 ms comparison initially failed at the first liftoff after 40 Newton
iterations. Disabling cross-step Jacobian reuse and enabling guarded backtracking
both reproduced the failure. Allowing 80 iterations completes the run with the
same 1e-8 absolute and relative convergence tolerances. The refined configuration
records that larger work budget; the browser retains 20 ms / 40 iterations.

Across both cycles, the 20 ms versus 10 ms maximum sampled foot difference is
0.479 mm and body-position difference is 0.218 mm. Across both cycles,
comparison with 5 ms gives 0.762 mm maximum foot difference at 20 ms and 0.290 mm
at 10 ms. This supports refinement agreement for the tested motion; longer runs,
contact impulse comparisons, disturbance tests and hardware calibration remain.
CI enforces 1 mm foot and 0.5 mm body differences for the two-cycle comparison.

## Browser evidence

Native/WASM comparison covers all 830 transitions, with maximum numeric
difference 8.89e-11 and exact same-host replay/reset. Worker-only throughput was
1.61× realtime with 23.0 ms p95 round trips. This excludes rendering.

The shared browser comparator explicitly uses 1e-7 absolute plus 1e-8 relative
portability budgets. A Linux CI result for the detailed pendulum had differed
by 2.73e-7 rad/s at an internal rotor speed of about 37.2 rad/s, exceeding the
previous absolute-only budget. This numerical portability change is separate
from physical trajectory budgets; replay/reset remain exact on each host.
The CAD replay test now allows only the optional controller event exactly at
the episode endpoint and verifies every timestamp on the 20 ms grid.

The viewer formerly requested physics from the display animation loop. Completed
solves could wait for another display refresh. It now schedules held-action
transitions independently, paced by elapsed simulation/wall time; when behind,
it catches up without dropping physics steps. Pause/reset/cancellation stop the
scheduler. The live speed indicator includes scheduling time; its tooltip shows
processing throughput separately.

On this Intel i9-9980HK Mac, Chrome 152.0.7977.76, Intel UHD Graphics 630 via ANGLE,
a headless WebGL run improved from 20.328 s wall time to 16.592 s for 16.6 s of
simulation: 0.817× to 1.001× realtime. p95 animation-frame interval was 16.67 ms;
p95 worker transitions were 27.7 ms, still above the 20 ms target. These are one
before/after run, not replicated sustained-performance acceptance or measured
command-to-display latency. Headless drawing is not a measurement of physical
display presentation. WASD, turning and terrain interaction remain undelivered.

## Reproduce

From the repository root, build the release examples, then run:

```sh
cargo build --locked --release -p sim-runtime --example plan_marker_motion --example run_environment --example evaluate_lift
node examples/full-robot/prepare_crawl.mjs runs/crawl examples/full-robot/browser-crawl-startup/recipe.json
target/release/examples/run_environment runs/crawl/scene.json runs/crawl/config.json runs/crawl/task.json > runs/crawl/native.json
node examples/full-robot/check_crawl.mjs runs/crawl runs/crawl/native.json runs/crawl/acceptance
target/release/examples/run_environment runs/crawl/scene.json runs/crawl/refined.config.json runs/crawl/task.json > runs/crawl/refined.native.json
node examples/full-robot/compare_effective_servo.mjs runs/crawl/refined.native.json runs/crawl/native.json runs/crawl/refinement.json
```

Build the WASM module and viewer using the instructions in `web/README.md`, then:

```sh
node web/build-viewer.mjs runs/crawl/viewer --environment-only
node web/tests/environment.mjs runs/crawl/viewer robot-crawl-startup runs/crawl/native.json runs/crawl/browser.json
node web/tests/live_performance.mjs runs/crawl/viewer robot-crawl-startup runs/crawl/rendered.json
node web/serve-viewer.mjs runs/crawl/viewer 4177
```

Open `http://127.0.0.1:4177/?preset=robot-crawl-startup`. Set `CHROME_EXECUTABLE`
for a system Chrome or use Playwright's installed Chromium. `HEADED=1` enables a
visible window for the performance test. That test reports speed/latency target
failures explicitly; it only exits successfully if every physics transition
completes without page errors. CI handles functional and trajectory acceptance;
reference-hardware timing is a separate measured gate.

The next controller milestone is continuous reference generation with bounded
speed/turn requests and support-aware phase transitions, backed by the same Rust
runtime and maintained in this viewer. A fixed two-cycle reference is useful
commissioning evidence, not a substitute for that work.
