# Streamed controller verification

These are synthetic UART/RTL captures, **not physical motor measurements**.

- `case0-capture.json`: 280 streamed frames at 10 ms, IDs 10–12, 35 acknowledged batches, full-scale positive and negative synthetic controller outputs, normal terminal stop.
- `case3-capture.json`: explicit host STOP after two control writes; interrupted frame remains unscored.
- `case6-capture.json`: no refill after eight rows; frame deadline trips and modeled motors finish zero/off.
- Shared Rust `live_stream::Capture::review` verifies acknowledged references, device timestamps, controller arithmetic, PWM/torque readbacks, and terminal wire evidence. It does not mark physical stopping verified.
- `live-power2-parity.log`: 4,096 vectors compare generated tunable RTL with the shared Rust integer controller.
- `live-integration-tests.log`: capture mutation tests, upload capabilities/gain constraints, and early STOP race regression.
- `live-host-fixture-final2.log`: all 15 acquisition tests pass. Earlier retained failure logs demonstrate a synthetic heartbeat-arbitration problem; the mock now prioritizes local replies between complete records, as the device does. Actual heartbeat deadlines remain 150 ms.

The live firmware is an explicit three-axis profile (IDs 10–12), with a two-row sealed zero base and 16 future reference rows. It supports at most 1,200 frames (12 seconds), offsets ±80 encoder counts, and changes ≤32 counts per frame. Its gain capability is 27: each gain is zero or one of 64, 128, 256, 512, 1024, 2048, 4096. The same gain triple applies to all selected motors. Frame queueing adds approximately 80–160 ms to live command delivery. No unverified loaded-leg calibration is implied.

Offline audit:

```sh
cargo test --locked -p sim-runtime --test live_stream --test fpga_upload --example characterize_hx_bridge --example serve_motor_bench
cargo run --locked -p sim-runtime --example review_live_fpga -- examples/actuators/hx30hm/hardware/2026-09-15-live-leg-control/verification/case0-capture.json
```

`firmware/` in the parent directory preserves the source and fixtures used by these tests. Its generated `fixed_pd.v` belongs at `impl/fixed-pd/fixed_pd.v` when rebuilding. Rebuild with `make bridge-live`; actual 50 MHz placement/timing success and physical commissioning must be recorded separately.
