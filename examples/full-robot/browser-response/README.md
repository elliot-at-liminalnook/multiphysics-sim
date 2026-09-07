# Browser command-reference latency

Realtime throughput and prompt control are different measurements. This crawl
accepts a new motion request at the next foot-transfer boundary. A fast physics
step does not remove that waiting time.

The viewer now exposes an independent read-only snapshot after an actual WebGL
submission: simulation time, drawing count, submission timestamp, and the
controller's latched motion reference. The performance harness measures keyboard
dispatch to the first received reference frame and to a drawn frame carrying that
reference. It also keeps requests that were superseded instead of silently
counting them as successful responses.

`check_command_response.mjs` checks each request against the saved input events
and the first matching reference in an independent native execution. It verifies
the reported drawn simulation time carries the requested reference. This is
**not** a measurement of monitor presentation, causal body-motion response or
physical stopping time. Those requirements remain open.

The isolated 24-second forward/turn/reverse/stop run produced these measurements:

| Request | First reference received | Reference frame drawn |
| --- | --- | --- |
| Forward from rest | 217 ms | 239 ms |
| Turn, requested at 8.4 s | 616 ms | 624 ms |
| Reverse, requested at 16.8 s | 1,012 ms | 1,022 ms |
| Stop, requested at 20 s | 24 ms | 45 ms |

The short stop delay here depends on its timing within the gait. In the separate
ten-second functional probe, stopping at 6 s took 820 ms to reach a reference
frame and 830 ms to draw it. That probe overlapped training, so its throughput
is not a performance benchmark.

The isolated run's active-transition p95 was 19.66 ms, passing the 20 ms target.
It submitted 1,020 WebGL frames. Active throughput was 0.999977 simulated seconds
per wall second, so the strict at-least-1× flag is **false**; retain that result.
The active interval exceeded its 19.8-second simulation duration by about
0.45 ms. This single pacing measurement does not establish a sustained speed
guarantee. All 35 existing viewer checks pass, including the new independent
render-snapshot assertion. Source hashes and full measurements are in
`status.json`.

The original five-second probe was rejected because its inherited push at seven
seconds lay outside the shortened episode. The rejected configuration and exact
validation error are retained in the report. The corrected ten-second probe
contains the full load schedule; no physics or acceptance tolerance changed.

## Reproduce

After building `sim-web` for WASM, from the repository root:

```sh
node examples/full-robot/browser-response/prepare.mjs
node web/build-viewer.mjs runs/interactive/command-response/viewer --environment-only
target/release/examples/run_environment examples/full-robot/student-distillation/scene.json runs/interactive/command-response/probe.config.json examples/full-robot/heading-task/task.json runs/interactive/command-response/probe.actions.json > runs/interactive/command-response/probe.native.json
node web/tests/live_performance.mjs runs/interactive/command-response/viewer robot-browser-solver runs/interactive/command-response/probe.json sustained-forward runs/interactive/command-response/probe.config.json
node web/tests/check_command_response.mjs runs/interactive/command-response/probe.json runs/interactive/command-response/probe.recording.json runs/interactive/command-response/probe.native.json runs/interactive/command-response/probe-check.json
```

Set `WASM_BINDGEN` and `CHROME_EXECUTABLE` if needed. CI runs this association
check without asserting a hardware-specific wall-time threshold. Remote CI
success is separate from the local evidence above.

For the longer isolated measurement, use scenario `turn-reverse` with
`examples/full-robot/browser-precision/guarded-short.config.json`, and replay the
versioned `browser-precision/turn-reverse.actions.json` natively for the association
check. Stop training and other test workloads before collecting timing evidence.
The controller and robot physics are unchanged by this instrumentation.
