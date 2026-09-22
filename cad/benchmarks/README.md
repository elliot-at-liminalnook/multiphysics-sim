# Desktop model loading

## Implementation

File → Open, command-line `.rcad` loading and REST `POST /open` share the same
process supervisor and preparation functions. A child process owns the source
CAD, builds display/picking data, and becomes the new editor. The launcher only
handles progress and cancellation; geometry calls cannot hold its Python GIL.
The handoff disables cancellation before the child can accept edits. Closing the
launcher after handoff does not terminate the new editor.

Display arrays are cached by full archive SHA-256 plus the cache schema and OCCT
version. Cold and warm paths use the same render preparation as interactive
edits. Cached raw tessellations retain float64 coordinates, and display arrays
retain their existing float32 precision. Changing source contents invalidates the
cache. Source bytes are checked before/after loading; corrupt or missing cache
entries fall back to normal preparation. There is no CAD or physics simplification.

## Measurements on the Intel Mac, 2026-09-15

Model: `examples/full-robot/baseline/robot.rcad`, revision 1357, 261 document
items, 114 display parts. Each JSON records the archive SHA-256.

| Run | Total to window | Reused parts | UI timer p99 | Longest UI gap | 150 ms gate |
| --- | ---: | ---: | ---: | ---: | --- |
| `model-load-cold-v2.json` | 49.64 s | 0 / 114 | 17.20 ms | 145.24 ms | Pass |
| `model-load-warm-v3.json` | 14.22 s | 114 / 114 | 17.56 ms | 183.44 ms | Fail |

The warm run's long gaps occurred at initial dialog painting and final native
window creation/activation. The 150 ms limit is intentionally retained: these
runs do **not** establish a consistent worst-case guarantee. Other processes and
window-server activity affect elapsed time. Earlier trials, including failed
ones, are retained alongside the latest records. Cache population took about
168 MiB for this model. These are local desktop measurements, not CI timings.

A 16 ms Qt timer measures delivery gaps throughout loading and window handoff.
Synchronous dialog construction is reported separately (`dialog_seconds`).
`prepare_seconds` includes reading CAD and preparing/caching the display;
`window_seconds` measures final window construction in the child process.
The timer measures event-loop responsiveness, not continuous rendering FPS.

## Reproduce

From the repository root:

```sh
cad/.venv/bin/python cad/scripts/benchmark_model_load.py \
  examples/full-robot/baseline/robot.rcad --cold --out /tmp/model-load-cold.json
cad/.venv/bin/python cad/scripts/benchmark_model_load.py \
  examples/full-robot/baseline/robot.rcad --out /tmp/model-load-warm.json
```

Cold measurement uses a temporary cache. A normal open populates the user's
persistent cache. Benchmark windows disable editing and close automatically.
Default budgets are 65 s total and 150 ms maximum timer gap; a missed budget
produces a nonzero exit code and retains the measurements.

`tests/test_model_loading.py` covers cache parity/invalidation/corruption, source
preservation, UI event delivery during cancellation, real child-process handoff,
REST progress/cancellation, and protecting handed-off windows. The existing CAD
workflow automatically includes these tests. Desktop timing remains a separate
acceptance check because shared CI machines do not reproduce this Mac's window
server, graphics driver or load.
