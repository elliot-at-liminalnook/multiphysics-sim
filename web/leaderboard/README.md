# Measured controller catalog

The viewer's Controller leaderboard loads `evaluations.json`, generated from
actual Rust environment captures and independent task audits. It currently
contains experimental recipes; none meets every declared gate. Short windows
are not shown as sustained speed and missing evidence never earns a rank.

Regenerate from the archived experiment recipes and captures:

```sh
node examples/full-robot/whole-swing/materialize_browser.mjs
node web/leaderboard/generate.mjs
node --test web/tests/leaderboard-model.mjs
WASM_BINDGEN=/path/to/wasm-bindgen node web/build-viewer.mjs runs/interactive/viewer --environment-only
node web/tests/leaderboard.mjs runs/interactive/viewer runs/interactive/leaderboard-report.json
```

Building needs only versioned evaluations, recipes and compact evidence, not
ignored native captures. The generator requires those captures to recompute
measurements. Their source recipes, seed and inputs are preserved; regenerate
each study using its documented source revision. The generator checks the
entire parsed physical scene, controller, configuration, task and acceptance
capture hash. `unretained-scene-fields.json` explicitly lists CAD export fields
the current Rust parser omits; raw scene bytes are still preserved and hashed.

Packaging rejects stale evidence or recipe hashes. Load and run verifies the
asset SHA-256 before starting a worker, loads the exact robot/world/controller/
seed, restores the tested initial command, and starts live simulation. Further
WASD commands are interactive. Replay tested inputs re-executes the full saved
sparse command sequence through the shared Rust environment. Its progress and
cancellation use the ordinary viewer path.

Speed ranks are assigned only within matching parsed model, environment,
fidelity and benchmark groups, and only when sustained walking, steering,
disturbance, terrain, numerical, realtime browser and replay gates all pass.
The display separates physical travel, native throughput and active browser
throughput/p95. Evidence cards expose failed and missing gates, endpoint error,
sampled positive shaft work and hardware metadata. Endpoint error is not
presented as physical stopping latency. Ideal observations and uncalibrated
physical properties remain explicit limitations.

Record video saves actual canvas frames as WebM where supported. Video encoding
is opt-in, affects browser load, and is excluded from performance benchmarks.
Saved input recordings are the reproducible physics artifact; video is for
viewing and sharing.

CI checks ranking eligibility, recipe packaging, actual browser loading of
every catalog recipe, tested-input replay, nonempty WebM output, filtering,
comparison and rejection of altered recipe bytes. It does not automatically
certify hardware accuracy or realtime performance on arbitrary machines.
